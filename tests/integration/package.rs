use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Write};
use std::path::Path;
use ya_runtime_wasi::{EntryPoint, Manifest, MountPoint};
use zip::{write::FileOptions, CompressionMethod, ZipWriter};

pub(super) struct Package {
    manifest: Manifest,
    buffer: Cursor<Vec<u8>>,
    modules: HashMap<String, Vec<u8>>,
}

impl Package {
    pub fn new<S: AsRef<str>>(name: S) -> Self {
        let name = name.as_ref();
        let manifest = Manifest {
            id: name.to_owned(),
            name: name.to_owned(),
            entry_points: Vec::new(),
            mount_points: Vec::new(),
        };
        let buffer = Cursor::new(Vec::new());
        let modules = HashMap::new();
        Self {
            manifest,
            buffer,
            modules,
        }
    }

    pub fn add_module<S: AsRef<str>, B: AsRef<[u8]>, M: AsRef<[MountPoint]>>(
        &mut self,
        name: S,
        module: B,
        mount_points: M,
    ) -> &mut Self {
        let name = name.as_ref();
        let entry_point = EntryPoint {
            id: name.to_string(),
            wasm_path: [name, "wasm"].join("."),
        };
        self.manifest.entry_points.push(entry_point);
        self.manifest
            .mount_points
            .extend_from_slice(mount_points.as_ref());
        self.modules
            .insert(name.to_owned(), module.as_ref().to_owned());
        self
    }

    pub fn add_module_from_file<P: AsRef<Path>, M: AsRef<[MountPoint]>>(
        &mut self,
        path: P,
        mount_points: M,
    ) -> &mut Self {
        let path = path.as_ref();
        let stem = path
            .file_stem()
            .expect("file_stem")
            .to_str()
            .expect("to_str");
        let bytes = fs::read(path).expect("fs::read");
        self.add_module(stem, bytes, mount_points);
        self
    }

    pub fn into_bytes(self) -> Vec<u8> {
        let mut zip = ZipWriter::new(self.buffer);
        let options = FileOptions::default().compression_method(CompressionMethod::Stored);

        // Write manifest
        zip.start_file("manifest.json", options)
            .expect("add file to zip");
        zip.write(&serde_json::to_vec(&self.manifest).expect("manifest to JSON"))
            .expect("write JSON to zip");

        // Write Wasm modules
        for (name, module) in self.modules {
            zip.start_file(name, options).expect("add file to zip");
            zip.write(&module).expect("write Wasm to zip");
        }

        let out = zip.finish().expect("finalize zip");
        out.into_inner()
    }
}
