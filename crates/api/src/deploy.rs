use crate::manifest::WasmImage;

use std::{
    borrow::Cow,
    path::{Path, PathBuf},
    {fs, io},
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Represents deployed Yagna Wasm image with set up volumes inside the
/// container.
///
/// A handle to the deployed image can be obtained after [`ya_runtime_wasi::deploy`]
/// command was executed, however, then the image will not have been yet validated. To
/// obtain a handle to a validated image you must run [`ya_runtime_wasi::start`] first.
///
/// [`ya_runtime_wasi::deploy`]: fn.deploy.html
/// [`ya_runtime_wasi::start`]: fn.start.html
///
/// ## Example
///
/// ```rust,no_run
/// use std::path::Path;
/// use ya_runtime_wasi::{deploy, DeployFile, start};
///
/// deploy(Path::new("workspace"), Path::new("package.zip")).unwrap();
/// let not_validated = DeployFile::load(Path::new("workspace")).unwrap();
///
/// start(Path::new("workspace")).unwrap();
/// let validated = DeployFile::load(Path::new("workspace")).unwrap();
/// ```
#[derive(Serialize, Deserialize)]
pub struct DeployFile {
    image_path: PathBuf,
    vols: Vec<ContainerVolume>,
}

impl DeployFile {
    fn for_image(image: &WasmImage) -> Result<Self> {
        let image_path = image.path().to_owned();
        let vols = image
            .manifest
            .mount_points
            .iter()
            .map(|mount_point| ContainerVolume {
                name: format!("vol-{}", Uuid::new_v4()),
                path: absolute_path(mount_point.path()).into(),
            })
            .collect();
        Ok(DeployFile { image_path, vols })
    }

    /// Loads deployed image from workspace where [`ya_runtime_wasi::deploy`] was executed.
    ///
    /// [`ya_runtime_wasi::deploy`]: fn.deploy.html
    pub fn load(work_dir: impl AsRef<Path>) -> Result<Self> {
        let deploy_file = deploy_path(work_dir.as_ref());
        let reader = io::BufReader::new(fs::File::open(&deploy_file).with_context(|| {
            format!(
                "Can't read deploy file {}. Did you run deploy command?",
                deploy_file.display()
            )
        })?);
        let deploy = serde_json::from_reader(reader)?;

        Ok(deploy)
    }

    pub(crate) fn save(&self, work_dir: impl AsRef<Path>) -> Result<()> {
        let deploy_file = deploy_path(work_dir.as_ref());
        fs::write(&deploy_file, serde_json::to_vec(&self)?)?;
        Ok(())
    }

    pub(crate) fn create_dirs(&self, work_dir: impl AsRef<Path>) -> Result<()> {
        let work_dir = work_dir.as_ref();
        for vol in &self.vols {
            fs::create_dir(work_dir.join(&vol.name))?;
        }
        Ok(())
    }

    /// Returns path to the deployed image.
    pub fn image_path(&self) -> &Path {
        &self.image_path
    }

    /// Returns an iterator over mapped container volumes.
    pub fn vols(&self) -> impl Iterator<Item = &ContainerVolume> {
        self.vols.iter()
    }
}

fn deploy_path(work_dir: &Path) -> PathBuf {
    work_dir.join("deploy.json")
}

fn absolute_path(path: &str) -> Cow<'_, str> {
    if path.starts_with('/') {
        Cow::Borrowed(path)
    } else {
        Cow::Owned(format!("/{}", path))
    }
}

/// Represents name and path to the mapped volume in the container.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContainerVolume {
    /// Volume name
    pub name: String,

    /// Path to the volume inside the container
    pub path: String,
}

/// Deploys the Wasm image into the workspace.
///
/// Takes path to workdir and path to the Wasm image as arguments.
///
/// ## Example
///
/// ```rust,no_run
/// use std::path::Path;
/// use ya_runtime_wasi::deploy;
///
/// deploy(Path::new("workspace"), Path::new("package.zig")).unwrap();
/// ```
pub fn deploy(workdir: impl AsRef<Path>, path: impl AsRef<Path>) -> Result<()> {
    let workdir = workdir.as_ref();
    let path = path.as_ref();

    let image = WasmImage::new(&path)
        .with_context(|| format!("Can't read image file {}.", path.display()))?;
    let deploy_file = DeployFile::for_image(&image)?;
    deploy_file.save(workdir)?;
    deploy_file.create_dirs(workdir)?;

    log::info!("Deploy completed");
    log::info!("Volumes = {:#?}", deploy_file.vols);

    Ok(())
}
