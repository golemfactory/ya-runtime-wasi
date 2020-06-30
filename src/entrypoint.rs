use anyhow::{bail, Result};
use log::info;

use std::path::{Component, Path, PathBuf};
use structopt::StructOpt;

use crate::deploy::{deploy, DeployFile};
use crate::manifest::WasmImage;
use crate::wasmtime_unit::Wasmtime;

pub(crate) struct DirectoryMount {
    pub host: PathBuf,
    pub guest: PathBuf,
}

#[derive(StructOpt)]
pub enum Commands {
    Deploy {},
    Start {},
    Run {
        #[structopt(short = "e", long = "entrypoint")]
        entrypoint: String,
        args: Vec<String>,
    },
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
pub struct CmdArgs {
    #[structopt(short, long)]
    workdir: PathBuf,
    #[structopt(short, long)]
    task_package: PathBuf,
    #[structopt(subcommand)]
    command: Commands,
}

pub fn entrypoint(cmdline: CmdArgs) -> Result<()> {
    match cmdline.command {
        Commands::Run { entrypoint, args } => run(&cmdline.workdir, &entrypoint, args),
        Commands::Deploy {} => deploy(&cmdline.workdir, &cmdline.task_package),
        Commands::Start {} => start(&cmdline.workdir),
    }
}

fn start(workdir: &Path) -> Result<()> {
    let deploy_file = DeployFile::load(workdir)?;

    info!(
        "Validating deployed image {:?}.",
        get_log_path(workdir, &deploy_file.image_path)
    );

    let mut image = WasmImage::new(&deploy_file.image_path)?;
    let mut wasmtime = create_wasmtime(workdir, &mut image, &deploy_file)?;

    wasmtime.load_binaries(&mut image)?;

    Ok(info!("Validation completed."))
}

fn run(workdir: &Path, entrypoint: &str, args: Vec<String>) -> Result<()> {
    let deploy_file = DeployFile::load(workdir)?;

    let mut image = WasmImage::new(&deploy_file.image_path)?;
    let mut wasmtime = create_wasmtime(workdir, &mut image, &deploy_file)?;

    info!(
        "Running image: {:?}",
        get_log_path(workdir, &deploy_file.image_path)
    );
    info!("Running image: {}", deploy_file.image_path.display());

    // Since wasmtime object doesn't live across binary executions,
    // we must deploy image for the second time, what will load binary to wasmtime.
    let entrypoint = image.find_entrypoint(entrypoint)?;
    wasmtime.load_binary(&mut image, &entrypoint)?;
    wasmtime.run(entrypoint, args)?;

    Ok(info!("Computations completed."))
}

fn create_wasmtime(
    workdir: &Path,
    _image: &mut WasmImage,
    deploy: &DeployFile,
) -> Result<Wasmtime> {
    let mounts = deploy
        .vols
        .iter()
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

fn get_log_path<'a, P: AsRef<Path>>(workdir: &'a Path, path: &'a P) -> &'a Path {
    let path_ref = path.as_ref();
    // try to return a relative path
    path_ref
        .strip_prefix(workdir)
        .ok()
        // use the file name if paths do not share a common prefix
        .or_else(|| path_ref.file_name().map(|file_name| Path::new(file_name)))
        // in an unlikely situation return an empty path
        .unwrap_or_else(|| Path::new(""))
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
