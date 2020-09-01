use crate::{deploy::DeployFile, manifest::WasmImage, wasmtime_unit::Wasmtime};

use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Result};
use log::info;

/// Validates the deployed image.
///
/// Takes path to the workdir as an argument.
pub fn start(workdir: impl AsRef<Path>) -> Result<()> {
    let workdir = workdir.as_ref();
    let deploy_file = DeployFile::load(workdir)?;

    info!(
        "Validating deployed image {:?}.",
        get_log_path(workdir, deploy_file.image_path())
    );

    let mut image = WasmImage::new(&deploy_file.image_path())?;
    let mut wasmtime = create_wasmtime(workdir, &mut image, &deploy_file)?;

    wasmtime.load_binaries(&mut image)?;

    info!("Validation completed.");

    Ok(())
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
    let workdir = workdir.as_ref();
    let deploy_file = DeployFile::load(workdir)?;

    let mut image = WasmImage::new(&deploy_file.image_path())?;
    let mut wasmtime = create_wasmtime(workdir, &mut image, &deploy_file)?;

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

pub(crate) struct DirectoryMount {
    pub host: PathBuf,
    pub guest: PathBuf,
}

fn create_wasmtime(
    workdir: &Path,
    _image: &mut WasmImage,
    deploy: &DeployFile,
) -> Result<Wasmtime> {
    let mounts = deploy
        .vols()
        .map(|v| {
            let host = workdir.join(&v.name);
            let guest = PathBuf::from(&v.path);
            validate_mount_path(&guest)?;
            Ok(DirectoryMount { host, guest })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(Wasmtime::new(mounts))
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
}
