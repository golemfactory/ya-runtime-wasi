use anyhow::{Context, Error, Result};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct Manifest {
    /// Deployment id in url like form.
    pub id: String,
    pub name: String,

    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub entry_points: Vec<EntryPoint>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mount_points: Vec<MountPoint>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "kebab-case")]
pub struct EntryPoint {
    pub id: String,
    pub wasm_path: String,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum MountPoint {
    Ro(String),
    Rw(String),
    Wo(String),
}

impl MountPoint {
    pub fn path(&self) -> &str {
        match self {
            MountPoint::Ro(path) => path,
            MountPoint::Rw(path) => path,
            MountPoint::Wo(path) => path,
        }
    }
}

pub struct WasmImage {
    archive: ZipArchive<File>,
    manifest: Manifest,
    image_path: PathBuf,
}

impl WasmImage {
    pub fn new(image_path: &Path) -> Result<WasmImage> {
        let mut archive = zip::ZipArchive::new(OpenOptions::new().read(true).open(image_path)?)?;
        let manifest = WasmImage::load_manifest(&mut archive)?;

        Ok(WasmImage {
            image_path: image_path.to_owned(),
            archive,
            manifest,
        })
    }

    fn load_manifest(archive: &mut ZipArchive<File>) -> Result<Manifest> {
        let entry = archive.by_name("manifest.json")?;
        Ok(serde_json::from_reader(entry)?)
    }

    pub fn get_manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn list_entrypoints(&self) -> Vec<EntryPoint> {
        self.manifest.entry_points.clone()
    }

    pub fn find_entrypoint(&self, entrypoint_id: &str) -> Result<EntryPoint> {
        let entrypoint = self
            .manifest
            .entry_points
            .iter()
            .find(|entry| entry.id == entrypoint_id)
            .map(|entry| entry.clone());

        Ok(entrypoint.ok_or(Error::msg(format!(
            "Entrypoint {} not found.",
            entrypoint_id
        )))?)
    }

    pub fn load_binary(&mut self, entrypoint: &EntryPoint) -> Result<Vec<u8>> {
        let image_name = self.manifest.name.clone();
        let mut entry = self
            .archive
            .by_name(&entrypoint.wasm_path)
            .with_context(|| {
                format!(
                    "Can't find file [{}] for entrypoint [{}] in [{}] image.",
                    entrypoint.wasm_path, entrypoint.id, image_name
                )
            })?;

        let mut bytes = vec![];
        entry.read_to_end(&mut bytes)?;
        return Ok(bytes);
    }

    pub fn path(&self) -> &Path {
        &self.image_path
    }
}
