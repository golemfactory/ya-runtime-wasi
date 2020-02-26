use crate::entrypoint::DirectoryMount;
use crate::manifest::{EntryPoint, WasmImage};

use wasi_common::preopen_dir;
use wasmtime::*;
use wasmtime_wasi::old::snapshot_0::create_wasi_instance as create_wasi_instance_snapshot_0;

use anyhow::{bail, Context, Error, Result};
use log::info;
use std::collections::HashMap;
use std::fs::File;

pub struct Wasmtime {
    store: Store,
    mounts: Vec<DirectoryMount>,
    /// Wasi modules instances and other dependencies, that we can add in future.
    dependencies: HashMap<String, Instance>,
    /// Modules loaded by user.
    modules: HashMap<String, Module>,
}

impl Wasmtime {
    pub fn new(mounts: Vec<DirectoryMount>) -> Wasmtime {
        let wasmtime = Wasmtime {
            store: Store::default(),
            mounts,
            dependencies: HashMap::<String, Instance>::new(),
            modules: HashMap::<String, Module>::new(),
        };

        wasmtime
    }
}

impl Wasmtime {
    pub fn load_binaries(&mut self, mut image: &mut WasmImage) -> Result<()> {
        // Loading binary will validate if it can be correctly loaded by wasmtime.
        for entrypoint in image.list_entrypoints().iter() {
            self.load_binary(&mut image, entrypoint)?;
        }
        Ok(())
    }

    pub fn run(&mut self, image: EntryPoint, args: Vec<String>) -> Result<()> {
        let args = Wasmtime::prepare_args(&args, &image);

        self.create_wasi_module(&args)?;
        let instance = self.create_instance(&image.id)?;

        info!("Running wasm binary with arguments {:?}", args);
        Ok(Wasmtime::run_instance(&instance, "_start")?)
    }

    pub fn load_binary(&mut self, image: &mut WasmImage, entrypoint: &EntryPoint) -> Result<()> {
        info!("Loading wasm binary: {}.", entrypoint.id);

        let wasm_binary = image
            .load_binary(entrypoint)
            .with_context(|| format!("Can't load wasm binary {}.", entrypoint.id))?;

        let module = Module::new(&self.store, &wasm_binary).with_context(|| {
            format!("WASM module creation failed for binary: {}.", entrypoint.id)
        })?;

        self.modules.insert(entrypoint.id.clone(), module);
        Ok(())
    }

    fn run_instance(instance: &Instance, entrypoint: &str) -> Result<()> {
        info!("Running wasm binary entrypoint {}", entrypoint);

        let function = instance
            .find_export_by_name("_start")
            .with_context(|| format!("Can't find {} entrypoint.", entrypoint))?
            .func()
            .with_context(|| format!("Can't find {} entrypoint.", entrypoint))?;
        //TODO: Return error code from execution.
        let _result = function.borrow().call(&[])?;
        Ok(())
    }

    fn create_wasi_module(&mut self, args: &Vec<String>) -> Result<()> {
        info!("Loading wasi.");

        let preopen_dirs = Wasmtime::compute_preopen_dirs(&self.mounts)?;

        // Create and instantiate snapshot0 of WASI ABI (FWIW, this is one *can* still
        // be targeted when using an older Rust toolchain)
        let snapshot0 = create_wasi_instance_snapshot_0(&self.store, &preopen_dirs, args, &vec![])
            .with_context(|| format!("Failed to create snapshot0 WASI module."))?;

        // Create and instantiate snapshot1 of WASI ABI, aka the "current stable"
        let snapshot1 =
            wasmtime_wasi::create_wasi_instance(&self.store, &preopen_dirs, args, &vec![])
                .with_context(|| format!("Failed to create snapshot1 WASI module."))?;

        self.dependencies
            .insert("wasi_unstable".to_owned(), snapshot0);
        self.dependencies
            .insert("wasi_snapshot_preview1".to_owned(), snapshot1);
        Ok(())
    }

    fn create_instance(&mut self, module_name: &str) -> Result<Instance> {
        info!("Resolving [{}] module's dependencies.", module_name);

        match self.modules.get_mut(module_name) {
            Some(mut module) => {
                let imports = Wasmtime::resolve_imports(&self.dependencies, &mut module)?;
                let instance = Instance::new(&self.store, &module, &imports)
                    .with_context(|| format!("WASM instance creation failed."))?;
                Ok(instance)
            }
            None => {
                return Err(Error::msg(format!(
                    "Module {} is not loaded. Did you forgot to run deploy step.",
                    module_name
                )))
            }
        }
    }

    fn resolve_imports(
        dependencies: &HashMap<String, Instance>,
        module: &mut Module,
    ) -> Result<Vec<Extern>> {
        Ok(module
            .imports()
            .iter()
            .map(|import| {
                let module_name = import.module();
                if let Some(instance) = dependencies.get(module_name) {
                    let field_name = import.name();
                    if let Some(export) = instance.find_export_by_name(field_name) {
                        Ok(export.clone())
                    } else {
                        bail!(
                            "Import {} was not found in module {}",
                            field_name,
                            module_name
                        )
                    }
                } else {
                    bail!("Import module {} was not found", module_name)
                }
            })
            .collect::<Result<Vec<_>, _>>()?)
    }

    fn compute_preopen_dirs(dirs: &Vec<DirectoryMount>) -> Result<Vec<(String, File)>> {
        let mut preopen_dirs = Vec::new();

        for DirectoryMount { guest, host } in dirs.iter() {
            info!("Mounting: {}::{}", host.display(), guest.display());

            preopen_dirs.push((
                guest.as_os_str().to_str().unwrap().to_string(),
                preopen_dir(host)
                    .with_context(|| format!("Failed to open directory '{}'", host.display()))?,
            ));
        }

        Ok(preopen_dirs)
    }

    fn prepare_args(args: &Vec<String>, entrypoint: &EntryPoint) -> Vec<String> {
        let mut new_args = Vec::new();

        // Entrypoint path is relative to wasm binary package, so we don't
        // leak directory structure here.
        // TODO: What if someone uses this argument to access something in
        //       filesystem? We don't mount wasm binary image to sandbox,
        //       so he won't find expected file. Can this break code that depends
        //       on binary existance?
        new_args.push(entrypoint.wasm_path.clone());

        for arg in args.iter() {
            new_args.push(arg.clone());
        }

        return new_args;
    }
}
