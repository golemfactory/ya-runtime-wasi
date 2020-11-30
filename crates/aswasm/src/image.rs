use serde::de::DeserializeOwned;
use std::fs::{File, OpenOptions};
use std::io::prelude::*;
use std::path::Path;

pub struct Image<T: Read + Seek> {
    zip_file: zip::ZipArchive<T>,
}

impl Image<File> {
    pub fn from_path(path: &Path) -> std::io::Result<Self> {
        let file = OpenOptions::new().read(true).write(false).open(path)?;
        let zip_file = zip::ZipArchive::new(file)?;
        Ok(Self { zip_file })
    }
}

impl<IO: Read + Seek> Image<IO> {
    pub fn get_json<T: DeserializeOwned>(&mut self, file_name: &str) -> anyhow::Result<T> {
        let entry = self.zip_file.by_name(file_name)?;
        Ok(serde_json::from_reader(entry)?)
    }

    pub fn get_bytes(&mut self, file_name: &str) -> anyhow::Result<Vec<u8>> {
        let mut entry = self.zip_file.by_name(file_name)?;
        let size = entry.size();
        // TODO: Add MAX WASM file size check
        let mut output = Vec::with_capacity(size as usize);
        std::io::copy(&mut entry, &mut output)?;
        Ok(output)
    }
}
