use anyhow::{bail, Context, Result};
use log::info;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::manifest::{MountPoint, WasmImage};
use crate::wasmtime_unit::Wasmtime;
use std::fs::File;
use std::io::BufReader;

pub(crate) struct DirectoryMount {
    pub host: PathBuf,
    pub guest: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct DeployFile {
    image_path: PathBuf,
}

pub struct ExeUnitMain;

impl ExeUnitMain {
    pub fn deploy(workdir: &Path, path: &Path) -> Result<()> {
        let image = WasmImage::new(&path).with_context(|| {
            format!("Can't read image file {:?}.", get_log_path(&workdir, &path))
        })?;
        write_deploy_file(workdir, &image)?;

        Ok(info!("Deploy completed."))
    }

    pub fn start(workdir: &Path) -> Result<()> {
        info!(
            "Loading deploy file: {:?}",
            get_log_path(workdir, &get_deploy_path(workdir))
        );

        let deploy_file = read_deploy_file(workdir).with_context(|| {
            format!(
                "Can't read deploy file {:?}. Did you run deploy command?",
                get_log_path(workdir, &get_deploy_path(workdir))
            )
        })?;

        info!(
            "Validating deployed image {:?}.",
            get_log_path(workdir, &deploy_file.image_path)
        );

        let mut image = WasmImage::new(&deploy_file.image_path)?;
        let mut wasmtime = ExeUnitMain::create_wasmtime(workdir, &mut image)?;

        wasmtime.load_binaries(&mut image)?;

        Ok(info!("Validation completed."))
    }

    pub fn run(workdir: &Path, entrypoint: &str, args: Vec<String>) -> Result<()> {
        info!(
            "Loading deploy file: {:?}",
            get_log_path(workdir, &get_deploy_path(workdir))
        );

        let deploy_file = read_deploy_file(workdir).with_context(|| {
            format!(
                "Can't read deploy file {:?}. Did you run deploy command?",
                get_log_path(workdir, &get_deploy_path(workdir))
            )
        })?;

        let mut image = WasmImage::new(&deploy_file.image_path)?;
        let mut wasmtime = ExeUnitMain::create_wasmtime(workdir, &mut image)?;

        info!(
            "Running image: {:?}",
            get_log_path(workdir, &deploy_file.image_path)
        );

        // Since wasmtime object doesn't live across binary executions,
        // we must deploy image for the second time, what will load binary to wasmtime.
        let entrypoint = image.find_entrypoint(entrypoint)?;
        wasmtime.load_binary(&mut image, &entrypoint)?;
        wasmtime.run(entrypoint, args)?;

        Ok(info!("Computations completed."))
    }

    fn create_wasmtime(workdir: &Path, image: &mut WasmImage) -> Result<Wasmtime> {
        let manifest = image.get_manifest();
        let mounts = directories_mounts(workdir, &manifest.mount_points)?;

        create_mount_points(&mounts)?;
        Ok(Wasmtime::new(mounts))
    }
}

fn create_mount_points(mounts: &Vec<DirectoryMount>) -> Result<()> {
    for mount in mounts.iter() {
        fs::create_dir_all(&mount.host)?
    }
    Ok(())
}

fn directories_mounts(
    workdir: &Path,
    mount_points: &Vec<MountPoint>,
) -> Result<Vec<DirectoryMount>> {
    mount_points
        .iter()
        .map(|mount_point| {
            let mut mount = PathBuf::from(mount_point.path());
            let host_path = workdir.join(&mount);

            validate_mount_path(&mount)?;

            // Requestor should see all paths as mounted to root.
            mount = PathBuf::from("/").join(mount);

            Ok(DirectoryMount {
                host: host_path,
                guest: mount,
            })
        })
        .collect()
}

fn validate_mount_path(path: &Path) -> Result<()> {
    // Protect ExeUnit from directory traversal attack.
    // Wasm can access only paths inside working directory.
    let path = PathBuf::from(path);
    for component in path.components() {
        match component {
            Component::RootDir | Component::Prefix { .. } => {
                bail!("Only relative mount paths are allowed.")
            }
            Component::ParentDir { .. } => bail!(
                "Mount path [{}] contains illegal '..' component.",
                path.display()
            ),
            Component::CurDir => bail!(
                "Mount path [{}] contains illegal '.' component.",
                path.display()
            ),
            _ => (),
        }
    }
    Ok(())
}

fn write_deploy_file(workdir: &Path, image: &WasmImage) -> Result<()> {
    let deploy_file = get_deploy_path(workdir);
    let deploy = DeployFile {
        image_path: image.path().to_owned(),
    };

    Ok(serde_json::to_writer(&File::create(deploy_file)?, &deploy)?)
}

fn read_deploy_file(workdir: &Path) -> Result<DeployFile> {
    let deploy_file = get_deploy_path(workdir);

    let reader = BufReader::new(File::open(deploy_file)?);
    let deploy = serde_json::from_reader(reader)?;
    return Ok(deploy);
}

fn get_deploy_path(workdir: &Path) -> PathBuf {
    workdir.join("deploy.json")
}

fn get_log_path<P: AsRef<Path>>(workdir: &Path, path: &P) -> PathBuf {
    let path_ref = path.as_ref();
    // try to return a relative path
    path_ref
        .strip_prefix(workdir)
        .map(|path| path.to_path_buf())
        .ok()
        // use the file name if paths do not share a common prefix
        .or_else(|| path_ref.file_name().map(PathBuf::from))
        // in an unlikely situation return an empty path
        .unwrap_or_else(|| PathBuf::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mount_path_validation() {
        assert_eq!(
            validate_mount_path(&PathBuf::from("/path/path")).is_err(),
            true
        );
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
