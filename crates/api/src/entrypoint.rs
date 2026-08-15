use crate::{
    deploy::DeployFile,
    manifest::{MountAccess, WasmImage},
    wasmtime_unit::Wasmtime,
};

use std::env;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use log::info;

const INIT_MEM_VAR: &str = "YA_RUNTIME_WASI_INIT_MEM";
const MAX_FUEL_VAR: &str = "YA_RUNTIME_WASI_MAX_FUEL";
const OPTIMIZE_VAR: &str = "YA_RUNTIME_WASI_OPT";
const SGX_VAR: &str = "YA_RUNTIME_WASI_SGX";
const DEFAULT_MAX_MEMORY: u64 = 1 << 30;

/// WASI runtime configuration.
#[derive(Clone, Debug)]
pub struct RuntimeOptions {
    pub(crate) memory_reservation: Option<u64>,
    pub(crate) max_memory: Option<u64>,
    pub(crate) max_fuel: Option<u64>,
    pub(crate) optimize: Option<bool>,
    pub(crate) sgx_profile: Option<bool>,
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        Self {
            memory_reservation: Some(DEFAULT_MAX_MEMORY),
            max_memory: Some(DEFAULT_MAX_MEMORY),
            max_fuel: None,
            optimize: None,
            sgx_profile: None,
        }
    }
}

impl RuntimeOptions {
    /// Initializes runtime options from environment variables.
    ///
    /// * `YA_RUNTIME_WASI_INIT_MEM` - maximum size of each guest linear memory
    ///   (for example `250m` or `1g`; defaults to `1g`).
    /// * `YA_RUNTIME_WASI_MAX_FUEL` - optional Wasmtime fuel limit. An unset
    ///   value or `0` disables the limit.
    /// * `YA_RUNTIME_WASI_OPT` - optimization. (0|no for no optimalization), (1|yes)
    /// * `YA_RUNTIME_WASI_SGX` - enables sgx profiled configuration.
    ///
    pub fn from_env() -> Result<Self> {
        let mut me = Self::default();

        if let Ok(memory) = env::var(INIT_MEM_VAR) {
            let memory = parse_memory_spec(&memory)?;
            me.memory_reservation = Some(memory);
            me.max_memory = Some(memory);
        }
        if let Ok(max_fuel) = env::var(MAX_FUEL_VAR) {
            me.max_fuel = parse_fuel_limit(&max_fuel)?;
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

    /// Configures a non-moving reservation and hard limit for each guest linear
    /// memory, in bytes.
    ///
    /// Wasmtime reserves this virtual address space when a memory is
    /// instantiated. This does not eagerly commit the same amount of physical
    /// RAM.
    pub fn with_static_memory(mut self, max_memory: impl Into<Option<u64>>) -> Self {
        let max_memory = max_memory.into();
        self.memory_reservation = max_memory;
        self.max_memory = max_memory;
        self
    }

    /// Configures the maximum size of each guest linear memory, in bytes, and
    /// aligns its non-moving reservation to the same value.
    pub fn with_memory_limit(mut self, max_memory: impl Into<Option<u64>>) -> Self {
        let max_memory = max_memory.into();
        self.memory_reservation = max_memory;
        self.max_memory = max_memory;
        self
    }

    /// Configures non-moving virtual address space reserved for each guest
    /// linear memory when the instance starts.
    pub fn with_memory_reservation(mut self, memory_reservation: impl Into<Option<u64>>) -> Self {
        self.memory_reservation = memory_reservation.into();
        self
    }

    /// Configures an optional Wasmtime fuel limit for each invocation.
    ///
    /// Fuel measures guest computation rather than wall-clock time. `None`
    /// disables fuel accounting, which is the default and is suitable for
    /// workloads billed by actual execution time.
    pub fn with_fuel_limit(mut self, max_fuel: impl Into<Option<u64>>) -> Self {
        self.max_fuel = max_fuel.into().filter(|fuel| *fuel > 0);
        self
    }

    /// Changes default optimization level.
    ///
    /// * `true` - optimization for speed.
    /// * `false` - no optimization.
    ///
    pub fn with_optimize(mut self, optimize: bool) -> Self {
        self.optimize = Some(optimize);
        self
    }

    /// Enables configuration for Graphene-SGX.
    pub fn with_sgx_profile(mut self, sgx_profile: bool) -> Self {
        self.sgx_profile = Some(sgx_profile);
        self
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

        let mut image = WasmImage::new(deploy_file.image_path())?;
        let mut wasmtime = create_wasmtime(workdir, &deploy_file, self)?;

        info!(
            "Running image: {:?}",
            get_log_path(workdir, deploy_file.image_path())
        );
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

        let mut image = WasmImage::new(deploy_file.image_path())?;
        let mut wasmtime = create_wasmtime(workdir, &deploy_file, self)?;

        wasmtime.load_binaries(&mut image)?;
        wasmtime.validate_binaries()?;

        info!("Validation completed.");

        Ok(())
    }
}

fn parse_memory_spec(spec: &str) -> Result<u64> {
    let suffix_index = spec
        .len()
        .checked_sub(1)
        .ok_or_else(|| anyhow::anyhow!("invalid max mem spec: value is empty"))?;
    let (value, suffix) = spec.split_at(suffix_index);
    let scale = match suffix {
        "k" | "K" => 1_u64 << 10,
        "m" | "M" => 1_u64 << 20,
        "g" | "G" => 1_u64 << 30,
        _ => anyhow::bail!("invalid max mem spec: {spec}"),
    };
    value
        .parse::<u64>()
        .with_context(|| format!("invalid max mem spec: {spec}"))?
        .checked_mul(scale)
        .ok_or_else(|| anyhow::anyhow!("max mem spec overflows: {spec}"))
}

fn parse_fuel_limit(spec: &str) -> Result<Option<u64>> {
    let fuel = spec
        .parse::<u64>()
        .with_context(|| format!("invalid fuel limit: {spec}"))?;
    Ok((fuel > 0).then_some(fuel))
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
    pub access: MountAccess,
}

fn create_wasmtime(
    workdir: &Path,
    deploy: &DeployFile,
    options: RuntimeOptions,
) -> Result<Wasmtime> {
    let mounts = deploy
        .mounts()
        .map(|(access, v)| {
            let host = workdir.join(&v.name);
            let guest = PathBuf::from(&v.path);
            validate_mount_path(&guest)?;
            Ok(DirectoryMount {
                host,
                guest,
                access,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Wasmtime::new(mounts, options)
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
        .or_else(|| path.file_name().map(Path::new))
        // in an unlikely situation return an empty path
        .unwrap_or_else(|| Path::new(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_path_validation() {
        assert!(validate_mount_path(&PathBuf::from("path/path/path")).is_ok());
        assert!(validate_mount_path(&PathBuf::from("path/../path")).is_err());
        assert!(validate_mount_path(&PathBuf::from("./path/../path")).is_err());
        assert!(validate_mount_path(&PathBuf::from("./path/path")).is_err());
    }

    #[test]
    fn test_memory_spec() {
        assert_eq!(parse_memory_spec("250m").unwrap(), 250 * (1 << 20));
        assert_eq!(parse_memory_spec("1G").unwrap(), 1 << 30);
        assert!(parse_memory_spec("").is_err());
        assert!(parse_memory_spec("1.2g").is_err());
        assert_eq!(RuntimeOptions::default().max_memory, Some(1 << 30));
        assert_eq!(RuntimeOptions::default().memory_reservation, Some(1 << 30));
        assert_eq!(RuntimeOptions::default().max_fuel, None);
        assert_eq!(parse_fuel_limit("0").unwrap(), None);
        assert_eq!(parse_fuel_limit("100000").unwrap(), Some(100_000));
        assert!(parse_fuel_limit("1m").is_err());
    }
}
