use crate::{deploy::DeployFile, manifest::WasmImage, wasmtime_unit::Wasmtime};

use std::env;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Result};
use log::info;

const INIT_MEM_VAR: &str = "YA_RUNTIME_WASI_INIT_MEM";
const OPTIMIZE_VAR: &str = "YA_RUNTIME_WASI_OPT";
const SGX_VAR: &str = "YA_RUNTIME_WASI_SGX";

/// [Yagna] WASI runtime configuration.
#[derive(Default, Clone, Debug)]
pub struct RuntimeOptions {
    pub(crate) max_static_memory: Option<u64>,
    pub(crate) optimize: Option<bool>,
    pub(crate) sgx_profile: Option<bool>,
}

impl RuntimeOptions {
    /** Initializes runtime options from environment variables.
     *
     * * `YA_RUNTIME_WASI_INIT_MEM` - maximum memory size. (supported formats 250m, 1.2g)
     * * `YA_RUNTIME_WASI_OPT` - optimization. (0|no for no optimalization), (1|yes)
     * * `YA_RUNTIME_WASI_SGX` - enables sgx profiled configuration.
     */
    pub fn from_env() -> Result<Self> {
        let mut me = Self::default();

        if let Some(err_msg) = (|| {
            let mem_str = env::var(INIT_MEM_VAR).ok()?;
            let len = mem_str.as_bytes().len();
            let scale = match mem_str.as_bytes().get(len - 1) {
                Some(b'k') => 0x400,
                Some(b'm') => 0x100_000,
                Some(b'g') => 0x40_000_000,
                _ => return Some(format!("invalid max mem spec: {}", mem_str)),
            };
            let value = match mem_str[..len - 1].parse::<u64>() {
                Ok(val) => val,
                Err(e) => return Some(format!("invalid max mem spec: {} ({})", mem_str, e)),
            };
            me.max_static_memory = Some(value * scale);
            None
        })() {
            log::warn!("wasi env MAX_MEM_VAR {}", err_msg);
            return Err(anyhow::Error::msg(err_msg));
        }

        fn parse_bool(env_var: &str) -> Result<Option<bool>> {
            match env::var(env_var).as_ref().map(String::as_str) {
                Ok("1") | Ok("yes") => Ok(Some(true)),
                Ok("0") | Ok("no") => Ok(Some(false)),
                Ok(value) => anyhow::bail!(
                    "invalid value ({}) for {}, 0|1|no|yes expected",
                    value,
                    env_var
                ),
                Err(_) => Ok(None),
            }
        }
        me.optimize = parse_bool(OPTIMIZE_VAR)?;
        me.sgx_profile = parse_bool(SGX_VAR)?;
        Ok(me)
    }

    /** Configures the maximum size, in bytes, where a linear memory is
     * considered static, above which it'll be considered dynamic.
     */
    pub fn with_static_memory(mut self, max_mamory: impl Into<Option<u64>>) -> Self {
        self.max_static_memory = max_mamory.into();
        self
    }

    /**
     * Changes default optimization level.
     *
     *  * `true` - optimization for speed.
     *  * `false` - no optimization.
     *
     */
    pub fn with_optimize(mut self, optimize: bool) -> Self {
        self.optimize = Some(optimize);
        self
    }

    /** Enables configuration for Graphene-SGX.
     */
    pub fn with_sgx_profile(mut self, sgx_profile: bool) -> Self {
        self.sgx_profile = Some(sgx_profile);
        self
    }

    pub(crate) fn is_default(&self) -> bool {
        self.max_static_memory.is_none() && self.optimize.is_none() && self.sgx_profile.is_none()
    }

    /// Instantiates and executes the deployed image using Wasmtime runtime.
    pub fn run(
        self,
        workdir: impl AsRef<Path>,
        entrypoint: impl AsRef<str>,
        args: impl IntoIterator<Item = String>,
    ) -> Result<()> {
        let workdir = workdir.as_ref();
        let deploy_file = DeployFile::load(workdir)?;

        let mut image = WasmImage::new(&deploy_file.image_path())?;
        let mut wasmtime = create_wasmtime(workdir, &deploy_file, self)?;

        info!(
            "Running image: {:?}",
            get_log_path(workdir, deploy_file.image_path())
        );
        info!("Running image: {}", deploy_file.image_path().display());

        // Since wasmtime object doesn't live across binary executions,
        // we must deploy image for the second time, what will load binary to wasmtime.
        let entrypoint = image.find_entrypoint(entrypoint.as_ref())?;
        wasmtime.load_binary(&mut image, &entrypoint)?;
        wasmtime.run(entrypoint, args.into_iter().collect())?;

        info!("Computations completed.");

        Ok(())
    }

    /// Validates the deployed image.
    pub fn start(self, workdir: impl AsRef<Path>) -> Result<()> {
        let workdir = workdir.as_ref();
        let deploy_file = DeployFile::load(workdir)?;

        info!(
            "Validating deployed image {:?}.",
            get_log_path(workdir, deploy_file.image_path())
        );

        let mut image = WasmImage::new(&deploy_file.image_path())?;
        let mut wasmtime = create_wasmtime(workdir, &deploy_file, self)?;

        wasmtime.load_binaries(&mut image)?;

        info!("Validation completed.");

        Ok(())
    }
}

/// Validates the deployed image.
///
/// Takes path to the workdir as an argument.
pub fn start(workdir: impl AsRef<Path>) -> Result<()> {
    RuntimeOptions::default().start(workdir)
}

/// Instantiates and executes the deployed image using Wasmtime runtime.
///
/// Takes path to the workdir, an entrypoint (name of WASI binary), and input arguments as arguments.
///
/// ## Example
///
/// ```rust,no_run
/// use std::path::Path;
/// use ya_runtime_wasi::run;
///
/// run(
///     Path::new("workspace"),
///     "hello",
///     vec![
///         "/workdir/input".into(),
///         "/workdir/output".into(),
///     ],
/// ).unwrap();
/// ```
pub fn run(
    workdir: impl AsRef<Path>,
    entrypoint: impl AsRef<str>,
    args: impl IntoIterator<Item = String>,
) -> Result<()> {
    RuntimeOptions::default().run(workdir, entrypoint, args)
}

pub(crate) struct DirectoryMount {
    pub host: PathBuf,
    pub guest: PathBuf,
}

fn create_wasmtime(
    workdir: &Path,
    deploy: &DeployFile,
    options: RuntimeOptions,
) -> Result<Wasmtime> {
    let mounts = deploy
        .container_vols()
        .map(|v| {
            let host = workdir.join(&v.name);
            let guest = PathBuf::from(&v.path);
            validate_mount_path(&guest)?;
            Ok(DirectoryMount { host, guest })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(Wasmtime::new(mounts, options))
}

fn validate_mount_path(path: &Path) -> Result<()> {
    // Protect ExeUnit from directory traversal attack.
    // Wasm can access only paths inside working directory.
    let path = PathBuf::from(path);
    for component in path.components() {
        match component {
            Component::Prefix { .. } => {
                bail!("Expected unix path instead of [{}].", path.display())
            }
            Component::ParentDir { .. } => {
                bail!("Path [{}] contains illegal '..' component.", path.display())
            }
            Component::CurDir => bail!("Path [{}] contains illegal '.' component.", path.display()),
            _ => (),
        }
    }
    Ok(())
}

fn get_log_path<'a>(workdir: &'a Path, path: &'a Path) -> &'a Path {
    // try to return a relative path
    path.strip_prefix(workdir)
        .ok()
        // use the file name if paths do not share a common prefix
        .or_else(|| path.file_name().map(|file_name| Path::new(file_name)))
        // in an unlikely situation return an empty path
        .unwrap_or_else(|| Path::new(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_path_validation() {
        assert_eq!(
            validate_mount_path(&PathBuf::from("path/path/path")).is_err(),
            false
        );
        assert_eq!(
            validate_mount_path(&PathBuf::from("path/../path")).is_err(),
            true
        );
        assert_eq!(
            validate_mount_path(&PathBuf::from("./path/../path")).is_err(),
            true
        );
        assert_eq!(
            validate_mount_path(&PathBuf::from("./path/path")).is_err(),
            true
        );
    }

    #[test]
    fn test_options() {
        env::set_var(INIT_MEM_VAR, "250m");
        let options = RuntimeOptions::from_env().unwrap();

        assert_eq!(options.max_static_memory, Some(250 * 0x100_000));
    }
}
