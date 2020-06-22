use crate::entrypoint::DirectoryMount;
use crate::manifest::{EntryPoint, WasmImage};

use wasi_common::{self, preopen_dir, WasiCtxBuilder};
use wasmtime::{Linker, Module, Store, Trap};
use wasmtime_wasi::Wasi;

use anyhow::{anyhow, Context, Result, bail};
use log::info;
use std::collections::HashMap;
use std::fs::File;

pub(crate) struct Wasmtime {
    linker: Linker,
    mounts: Vec<DirectoryMount>,
    /// Modules loaded by the user.
    modules: HashMap<EntryPoint, Module>,
}

impl Wasmtime {
    pub fn new(mounts: Vec<DirectoryMount>) -> Wasmtime {
        let store = Store::default();
        let linker = Linker::new(&store);
        let modules = HashMap::new();

        Wasmtime {
            linker,
            mounts,
            modules,
        }
    }
}

impl Wasmtime {
    pub fn load_binaries(&mut self, mut image: &mut WasmImage) -> Result<()> {
        // Loading binary will validate if it can be correctly loaded by wasmtime.
        for entrypoint in &image.list_entrypoints() {
            self.load_binary(&mut image, entrypoint)?;
        }

        Ok(())
    }

    pub fn run(&mut self, image: EntryPoint, args: Vec<String>) -> Result<()> {
        let args = Wasmtime::compute_args(&args, &image);
        let preopens = self.compute_preopens()?;
        self.add_wasi_modules(&args, &preopens)?;

        info!("Running wasm binary with arguments {:?}", args);
        self.invoke(&image)?;

        Ok(())
    }

    pub fn load_binary(&mut self, image: &mut WasmImage, entrypoint: &EntryPoint) -> Result<()> {
        info!("Loading wasm binary: {}.", entrypoint.id);

        let wasm_binary = image
            .load_binary(entrypoint)
            .with_context(|| format!("Can't load wasm binary {}.", entrypoint.id))?;

        let module = Module::new(self.linker.store().engine(), wasm_binary).with_context(|| {
            format!(
                "Failed to create Wasm module for binary: '{}'",
                entrypoint.id
            )
        })?;

        if let Some(_) = self.modules.insert(entrypoint.to_owned(), module) {
            bail!("Module already defined: '{}'", entrypoint.id);
        }

        Ok(())
    }

    fn invoke(&mut self, entrypoint: &EntryPoint) -> Result<()> {
        let module = match self.modules.get(entrypoint) {
            Some(module) => module,
            None => bail!("Module not found: '{}'", entrypoint.id),
        };
        self.linker
            .module(&entrypoint.id, &module)
            .with_context(|| format!("Failed to instantiate module: '{}'", entrypoint.id))?;

        // TODO for now, we only allow invoking default Wasm export per module,
        // i.e., `_start` export. In the future, it could be useful to allow
        // invoking custom exports as well.
        let run = self
            .linker
            .get_default(&entrypoint.id)?
            .get0::<()>()
            .with_context(|| {
                format!(
                    "Failed to find '_start' export in module; did you build a library by mistake?"
                )
            })?;

        if let Err(err) =
            run().with_context(|| format!("Failed to run module: '{}'", entrypoint.id))
        {
            match err.downcast_ref::<Trap>() {
                None => return Err(err),
                Some(trap) => {
                    // TODO bubble up exit code.
                    let exit_code = match trap.i32_exit_status() {
                        Some(status) => {
                            // On Windows, exit status 3 indicates an abort (see below),
                            // so return 1 indicating a non-zero status to avoid ambiguity.
                            if cfg!(windows) && status >= 3 {
                                1
                            } else {
                                status
                            }
                        }
                        None => {
                            if cfg!(windows) {
                                // On Windows, return 3.
                                // https://docs.microsoft.com/en-us/cpp/c-runtime-library/reference/abort?view=vs-2019
                                3
                            } else {
                                128 + libc::SIGABRT
                            }
                        }
                    };

                    return Err(anyhow!("{}; exit_code={}", err, exit_code));
                }
            }
        }

        Ok(())
    }

    fn add_wasi_modules(
        &mut self,
        args: &Vec<String>,
        preopens: &Vec<(String, File)>,
    ) -> Result<()> {
        info!("Loading wasi.");

        // Add snapshot1 of WASI ABI
        let mut cx = WasiCtxBuilder::new();
        cx.inherit_stdio().args(args);

        for (name, file) in preopens {
            cx.preopened_dir(file.try_clone()?, name);
        }

        let cx = cx.build()?;
        let wasi = Wasi::new(self.linker.store(), cx);
        wasi.add_to_linker(&mut self.linker)?;

        // Add snapshot0 of WASI ABI
        let mut cx = wasi_common::old::snapshot_0::WasiCtxBuilder::new();
        cx.inherit_stdio().args(args);

        for (name, file) in preopens {
            cx.preopened_dir(file.try_clone()?, name);
        }

        let cx = cx.build()?;
        let wasi = wasmtime_wasi::old::snapshot_0::Wasi::new(self.linker.store(), cx);
        wasi.add_to_linker(&mut self.linker)?;

        Ok(())
    }

    fn compute_preopens(&self) -> Result<Vec<(String, File)>> {
        let mut preopen_dirs = Vec::new();

        for DirectoryMount { guest, host } in &self.mounts {
            info!("Mounting: {}", guest.display());

            preopen_dirs.push((
                guest
                    .to_str()
                    .ok_or_else(|| anyhow!("Invalid UTF8: guest = '{}'", guest.display()))?
                    .to_owned(),
                preopen_dir(host)
                    .with_context(|| format!("Failed to mount '{}'", guest.display()))?,
            ));
        }

        Ok(preopen_dirs)
    }

    fn compute_args(args: &Vec<String>, entrypoint: &EntryPoint) -> Vec<String> {
        let mut new_args = Vec::new();

        // Entrypoint path is relative to wasm binary package, so we don't
        // leak directory structure here.
        // TODO: What if someone uses this argument to access something in
        //       filesystem? We don't mount wasm binary image to sandbox,
        //       so he won't find expected file. Can this break code that depends
        //       on binary existance?
        new_args.push(entrypoint.wasm_path.clone());

        for arg in args {
            new_args.push(arg.clone());
        }

        return new_args;
    }
}
