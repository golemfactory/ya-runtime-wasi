use crate::manifest::WasmImage;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::{fs, io};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct DeployFile {
    pub image_path: PathBuf,
    pub vols: Vec<ContainerVolume>,
}

impl DeployFile {
    pub fn for_image(image: &WasmImage) -> anyhow::Result<Self> {
        let image_path = image.path().to_owned();
        let vols = image
            .manifest()
            .mount_points
            .iter()
            .map(|mount_point| ContainerVolume {
                name: format!("vol-{}", Uuid::new_v4()),
                path: mount_point.path().into(),
            })
            .collect();
        Ok(DeployFile { image_path, vols })
    }

    pub fn save(&self, work_dir: &Path) -> anyhow::Result<()> {
        let deploy_file = deploy_path(work_dir);
        fs::write(&deploy_file, serde_json::to_vec(&self)?)?;
        Ok(())
    }

    pub fn load(work_dir: &Path) -> anyhow::Result<Self> {
        let deploy_file = deploy_path(work_dir);
        let reader = io::BufReader::new(fs::File::open(&deploy_file).with_context(|| {
            format!(
                "Can't read deploy file {}. Did you run deploy command?",
                deploy_file.display()
            )
        })?);
        let deploy = serde_json::from_reader(reader)?;
        return Ok(deploy);
    }
}

fn deploy_path(work_dir: &Path) -> PathBuf {
    work_dir.join("deploy.json")
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DeployResult {
    pub valid: Result<String, String>,
    #[serde(default)]
    pub vols: Vec<ContainerVolume>,
    #[serde(default)]
    pub start_mode: StartMode,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ContainerVolume {
    pub name: String,
    pub path: String,
}

#[derive(Serialize, Deserialize, Debug, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum StartMode {
    Empty,
    Blocking,
}

impl Default for StartMode {
    fn default() -> Self {
        StartMode::Empty
    }
}

pub fn deploy(workdir: &Path, path: &Path) -> anyhow::Result<()> {
    let image = WasmImage::new(&path)
        .with_context(|| format!("Can't read image file {}.", path.display()))?;
    let deploy_file = DeployFile::for_image(&image)?;
    deploy_file.save(workdir)?;

    let result = DeployResult {
        valid: Ok(format!("Deploy completed.")),
        vols: deploy_file.vols.clone(),
        start_mode: StartMode::Empty,
    };

    eprintln!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
