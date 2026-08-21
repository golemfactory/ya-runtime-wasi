use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

const MAX_MANIFEST_SIZE: u64 = 1024 * 1024;
const MAX_WASM_SIZE: u64 = 256 * 1024 * 1024;

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct Manifest {
    /// Deployment id in url like form.
    pub id: String,
    pub name: String,

    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub entry_points: Vec<EntryPoint>,

    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mount_points: Vec<MountPoint>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Hash, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct EntryPoint {
    pub id: String,
    pub wasm_path: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum MountPoint {
    Ro(String),
    Rw(String),
    Wo(String),
    Private(String),
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum MountAccess {
    ReadOnly,
    #[default]
    ReadWrite,
    WriteOnly,
}

impl MountPoint {
    pub fn path(&self) -> &str {
        match self {
            MountPoint::Ro(path) => path,
            MountPoint::Rw(path) => path,
            MountPoint::Wo(path) => path,
            MountPoint::Private(path) => path,
        }
    }

    pub fn is_private(&self) -> bool {
        matches!(self, MountPoint::Private(_))
    }

    pub fn access(&self) -> MountAccess {
        match self {
            MountPoint::Ro(_) => MountAccess::ReadOnly,
            MountPoint::Rw(_) | MountPoint::Private(_) => MountAccess::ReadWrite,
            MountPoint::Wo(_) => MountAccess::WriteOnly,
        }
    }
}

pub(crate) struct WasmImage {
    archive: ZipArchive<File>,
    pub manifest: Manifest,
    image_path: PathBuf,
}

impl WasmImage {
    pub fn new(image_path: &Path) -> Result<Self> {
        let mut archive = zip::ZipArchive::new(OpenOptions::new().read(true).open(image_path)?)?;
        let manifest = WasmImage::load_manifest(&mut archive)?;
        manifest.validate_runtime()?;

        Ok(Self {
            image_path: image_path.to_owned(),
            archive,
            manifest,
        })
    }

    fn load_manifest(archive: &mut ZipArchive<File>) -> Result<Manifest> {
        let mut entry = archive.by_name("manifest.json")?;
        if entry.size() > MAX_MANIFEST_SIZE {
            bail!(
                "Manifest is too large: {} bytes (maximum: {MAX_MANIFEST_SIZE})",
                entry.size()
            );
        }
        let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
        (&mut entry)
            .take(MAX_MANIFEST_SIZE + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_MANIFEST_SIZE {
            bail!("Manifest expands beyond the {MAX_MANIFEST_SIZE}-byte limit");
        }
        Ok(serde_json::from_slice(&bytes)?)
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
            .cloned();

        entrypoint.ok_or_else(|| anyhow!("Entrypoint {} not found.", entrypoint_id))
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

        if entry.size() > MAX_WASM_SIZE {
            bail!(
                "Wasm binary for entrypoint [{}] is too large: {} bytes (maximum: {MAX_WASM_SIZE})",
                entrypoint.id,
                entry.size()
            );
        }

        let capacity = usize::try_from(entry.size()).context("Wasm binary size exceeds usize")?;
        let mut bytes = Vec::with_capacity(capacity);
        (&mut entry)
            .take(MAX_WASM_SIZE + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_WASM_SIZE {
            bail!(
                "Wasm binary for entrypoint [{}] expands beyond the {MAX_WASM_SIZE}-byte limit",
                entrypoint.id
            );
        }

        Ok(bytes)
    }

    pub fn path(&self) -> &Path {
        &self.image_path
    }
}

impl Manifest {
    fn validate_runtime(&self) -> Result<()> {
        match self.runtime.as_deref() {
            None | Some("wasi") => Ok(()),
            Some(runtime) => bail!(
                "unsupported runtime '{runtime}'; ya-runtime-wasi only executes WASI Preview 1 modules and Preview 2 components"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    fn package(manifest: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("package.zip");
        let file = File::create(&path).unwrap();
        let mut archive = ZipWriter::new(file);
        archive
            .start_file("manifest.json", SimpleFileOptions::default())
            .unwrap();
        archive.write_all(manifest).unwrap();
        archive.finish().unwrap();
        (dir, path)
    }

    #[test]
    fn accepts_wasi_and_unspecified_runtime() {
        for runtime in [r#""runtime":"wasi","#, ""] {
            let manifest = format!(r#"{{"id":"test",{runtime}"name":"test"}}"#);
            let (_dir, path) = package(manifest.as_bytes());
            WasmImage::new(&path).unwrap();
        }
    }

    #[test]
    fn rejects_legacy_runtime() {
        let (_dir, path) = package(br#"{"id":"test","name":"test","runtime":"aswasm"}"#);
        let error = WasmImage::new(&path).err().unwrap();
        assert!(format!("{error:#}").contains("unsupported runtime 'aswasm'"));
    }

    #[test]
    fn rejects_oversized_manifest_before_parsing() {
        let manifest = vec![b' '; usize::try_from(MAX_MANIFEST_SIZE + 1).unwrap()];
        let (_dir, path) = package(&manifest);
        let error = WasmImage::new(&path).err().unwrap();
        assert!(format!("{error:#}").contains("Manifest is too large"));
    }
}
