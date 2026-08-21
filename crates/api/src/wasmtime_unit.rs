use crate::{
    entrypoint::{DirectoryMount, RuntimeOptions},
    manifest::{EntryPoint, MountAccess, WasmImage},
};

use anyhow::{Context, Result, anyhow, bail};
use log::info;
use std::collections::HashMap;
use wasmtime::component::{Component, Linker as ComponentLinker, ResourceTable};
use wasmtime::{Config, Engine, Linker, Module, OptLevel, Store, StoreLimits, StoreLimitsBuilder};
use wasmtime_wasi::p2::bindings::sync::Command;
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView, p1, p2};

#[derive(Clone)]
enum CompiledBinary {
    Preview1(Module),
    Preview2(Component),
}

struct Preview1State {
    wasi: p1::WasiP1Ctx,
    limits: StoreLimits,
}

struct Preview2State {
    wasi: WasiCtx,
    table: ResourceTable,
    limits: StoreLimits,
}

impl WasiView for Preview2State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

/// Wasmtime runtime supporting WASI Preview 1 modules and Preview 2 components.
pub(crate) struct Wasmtime {
    engine: Engine,
    mounts: Vec<DirectoryMount>,
    options: RuntimeOptions,
    binaries: HashMap<EntryPoint, CompiledBinary>,
}

impl Wasmtime {
    pub fn new(mounts: Vec<DirectoryMount>, options: RuntimeOptions) -> Result<Self> {
        let mut config = Config::new();
        config.consume_fuel(options.max_fuel.is_some());
        config.wasm_component_model(true);

        if let Some(reservation) = options.memory_reservation {
            config.memory_reservation(reservation);
            config.memory_reservation_for_growth(0);
            config.memory_may_move(false);
        }

        if let Some(optimize) = options.optimize {
            config.cranelift_opt_level(if optimize {
                OptLevel::Speed
            } else {
                OptLevel::None
            });
        }
        if options.sgx_profile.unwrap_or(false) {
            config.memory_guard_size(0x1_0000);
            config.debug_info(false);
        }

        Ok(Self {
            engine: Engine::new(&config)?,
            mounts,
            options,
            binaries: HashMap::new(),
        })
    }

    pub fn load_binaries(&mut self, image: &mut WasmImage) -> Result<()> {
        for entrypoint in &image.list_entrypoints() {
            self.load_binary(image, entrypoint)?;
        }
        Ok(())
    }

    pub fn run(&mut self, image: EntryPoint, args: Vec<String>) -> Result<()> {
        let binary = self
            .binaries
            .get(&image)
            .cloned()
            .ok_or_else(|| anyhow!("Binary not found: '{}'", image.id))?;
        let args = Self::compute_args(&args, &image);

        info!("Running wasm binary '{}'.", image.id);
        match binary {
            CompiledBinary::Preview1(module) => self.run_preview1(&image, &module, &args),
            CompiledBinary::Preview2(component) => self.run_preview2(&image, &component, &args),
        }
    }

    pub fn validate_binaries(&self) -> Result<()> {
        for (entrypoint, binary) in &self.binaries {
            match binary {
                CompiledBinary::Preview1(module) => {
                    let state = Preview1State {
                        wasi: self.wasi_builder(&[])?.build_p1(),
                        limits: self.store_limits()?,
                    };
                    let mut store = Store::new(&self.engine, state);
                    store.limiter(|state| &mut state.limits);
                    self.configure_fuel(&mut store)?;
                    let mut linker = Linker::new(&self.engine);
                    p1::add_to_linker_sync(&mut linker, |state: &mut Preview1State| {
                        &mut state.wasi
                    })?;
                    let instance = linker.instantiate(&mut store, module).map_err(|err| {
                        anyhow!("Failed to instantiate module '{}': {err:#}", entrypoint.id)
                    })?;
                    instance
                        .get_typed_func::<(), ()>(&mut store, "_start")
                        .map_err(|err| {
                            anyhow!(
                                "Module '{}' has no valid '_start' export: {err}",
                                entrypoint.id
                            )
                        })?;
                }
                CompiledBinary::Preview2(component) => {
                    let state = Preview2State {
                        wasi: self.wasi_builder(&[])?.build(),
                        table: ResourceTable::new(),
                        limits: self.store_limits()?,
                    };
                    let mut store = Store::new(&self.engine, state);
                    store.limiter(|state| &mut state.limits);
                    self.configure_fuel(&mut store)?;
                    let mut linker = ComponentLinker::new(&self.engine);
                    p2::add_to_linker_sync(&mut linker)?;
                    Command::instantiate(&mut store, component, &linker).map_err(|err| {
                        anyhow!(
                            "Failed to instantiate WASI Preview 2 component '{}': {err:#}",
                            entrypoint.id
                        )
                    })?;
                }
            }
        }
        Ok(())
    }

    pub fn load_binary(&mut self, image: &mut WasmImage, entrypoint: &EntryPoint) -> Result<()> {
        info!("Loading wasm binary: {}.", entrypoint.id);
        let bytes = image
            .load_binary(entrypoint)
            .with_context(|| format!("Can't load wasm binary {}.", entrypoint.id))?;
        let binary = self.compile_binary(entrypoint, &bytes)?;

        if self
            .binaries
            .insert(entrypoint.to_owned(), binary)
            .is_some()
        {
            bail!("Binary already defined: '{}'", entrypoint.id);
        }
        Ok(())
    }

    fn compile_binary(&self, entrypoint: &EntryPoint, bytes: &[u8]) -> Result<CompiledBinary> {
        if wasmparser::Parser::is_component(bytes) {
            let component = Component::new(&self.engine, bytes).map_err(|err| {
                anyhow!(
                    "Failed to compile WASI Preview 2 component '{}': {err}",
                    entrypoint.id
                )
            })?;
            Ok(CompiledBinary::Preview2(component))
        } else {
            let module = Module::new(&self.engine, bytes).map_err(|err| {
                anyhow!(
                    "Failed to compile WASI Preview 1 module '{}': {err}",
                    entrypoint.id
                )
            })?;
            Ok(CompiledBinary::Preview1(module))
        }
    }

    fn run_preview1(&self, image: &EntryPoint, module: &Module, args: &[String]) -> Result<()> {
        let state = Preview1State {
            wasi: self.wasi_builder(args)?.build_p1(),
            limits: self.store_limits()?,
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| &mut state.limits);
        self.configure_fuel(&mut store)?;

        let mut linker = Linker::new(&self.engine);
        p1::add_to_linker_sync(&mut linker, |state: &mut Preview1State| &mut state.wasi)?;
        let instance = linker
            .instantiate(&mut store, module)
            .map_err(|err| anyhow!("Failed to instantiate module '{}': {err}", image.id))?;
        let run = instance
            .get_typed_func::<(), ()>(&mut store, "_start")
            .map_err(|err| anyhow!("Module '{}' has no valid '_start' export: {err}", image.id))?;
        run.call(&mut store, ())
            .map_err(|err| anyhow!("Failed to run module '{}': {err:#}", image.id))
    }

    fn run_preview2(
        &self,
        image: &EntryPoint,
        component: &Component,
        args: &[String],
    ) -> Result<()> {
        let state = Preview2State {
            wasi: self.wasi_builder(args)?.build(),
            table: ResourceTable::new(),
            limits: self.store_limits()?,
        };
        let mut store = Store::new(&self.engine, state);
        store.limiter(|state| &mut state.limits);
        self.configure_fuel(&mut store)?;

        let mut linker = ComponentLinker::new(&self.engine);
        p2::add_to_linker_sync(&mut linker)?;
        let command = Command::instantiate(&mut store, component, &linker).map_err(|err| {
            anyhow!(
                "Failed to instantiate WASI Preview 2 component '{}': {err}",
                image.id
            )
        })?;
        match command
            .wasi_cli_run()
            .call_run(&mut store)
            .map_err(|err| anyhow!("Failed to run component '{}': {err:#}", image.id))?
        {
            Ok(()) => Ok(()),
            Err(()) => bail!("WASI Preview 2 component '{}' returned failure", image.id),
        }
    }

    fn wasi_builder(&self, args: &[String]) -> Result<WasiCtxBuilder> {
        let mut ctx = WasiCtxBuilder::new();
        ctx.inherit_stdio();
        ctx.args(args);

        for DirectoryMount {
            guest,
            host,
            access,
        } in &self.mounts
        {
            let guest = guest
                .to_str()
                .ok_or_else(|| anyhow!("Invalid UTF8 guest path: '{}'", guest.display()))?;
            let (dir_perms, file_perms) = mount_permissions(*access);
            ctx.preopened_dir(host, guest, dir_perms, file_perms)
                .map_err(|err| anyhow!("Failed to mount '{guest}': {err}"))?;
        }

        Ok(ctx)
    }

    fn store_limits(&self) -> Result<StoreLimits> {
        let mut builder = StoreLimitsBuilder::new();
        if let Some(max_memory) = self.options.max_memory {
            builder = builder.memory_size(
                usize::try_from(max_memory)
                    .context("YA_RUNTIME_WASI_INIT_MEM exceeds the platform address space")?,
            );
        }
        Ok(builder.build())
    }

    fn configure_fuel<T>(&self, store: &mut Store<T>) -> Result<()> {
        if let Some(max_fuel) = self.options.max_fuel {
            store.set_fuel(max_fuel)?;
        }
        Ok(())
    }

    fn compute_args(args: &[String], entrypoint: &EntryPoint) -> Vec<String> {
        let mut new_args = Vec::with_capacity(args.len() + 1);
        new_args.push(entrypoint.wasm_path.clone());
        new_args.extend(args.iter().cloned());
        new_args
    }
}

fn mount_permissions(access: MountAccess) -> (DirPerms, FilePerms) {
    match access {
        MountAccess::ReadOnly => (DirPerms::READ, FilePerms::READ),
        MountAccess::ReadWrite => (
            DirPerms::READ | DirPerms::MUTATE,
            FilePerms::READ | FilePerms::WRITE,
        ),
        MountAccess::WriteOnly => (DirPerms::MUTATE, FilePerms::WRITE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entrypoint() -> EntryPoint {
        EntryPoint {
            id: "test".to_owned(),
            wasm_path: "test.wasm".to_owned(),
        }
    }

    fn runtime_with_binary(wat: &str) -> Wasmtime {
        let mut runtime = Wasmtime::new(Vec::new(), RuntimeOptions::default()).unwrap();
        let entrypoint = entrypoint();
        let bytes = wat::parse_str(wat).unwrap();
        let binary = runtime.compile_binary(&entrypoint, &bytes).unwrap();
        runtime.binaries.insert(entrypoint.clone(), binary);
        runtime
    }

    #[test]
    fn mount_access_maps_to_wasi_permissions() {
        assert_eq!(
            mount_permissions(MountAccess::ReadOnly),
            (DirPerms::READ, FilePerms::READ)
        );
        assert_eq!(
            mount_permissions(MountAccess::WriteOnly),
            (DirPerms::MUTATE, FilePerms::WRITE)
        );
        assert_eq!(
            mount_permissions(MountAccess::ReadWrite),
            (
                DirPerms::READ | DirPerms::MUTATE,
                FilePerms::READ | FilePerms::WRITE
            )
        );
    }

    #[test]
    fn runs_preview1_command_module() {
        let mut runtime = runtime_with_binary(r#"(module (func (export "_start")))"#);
        runtime.run(entrypoint(), Vec::new()).unwrap();
    }

    #[test]
    fn runs_preview2_command_component() {
        let component = r#"
            (component
                (core module $module
                    (func (export "run") (result i32)
                        i32.const 0))
                (core instance $instance (instantiate $module))
                (type $result (result))
                (type $run (func (result $result)))
                (func $run (type $run) (canon lift (core func $instance "run")))
                (instance $interface
                    (export "run" (func $run)))
                (export "wasi:cli/run@0.2.0" (instance $interface)))
        "#;
        let mut runtime = runtime_with_binary(component);
        runtime.run(entrypoint(), Vec::new()).unwrap();
    }

    #[test]
    fn fuel_stops_non_terminating_preview1_module() {
        let mut runtime = Wasmtime::new(
            Vec::new(),
            RuntimeOptions::default().with_fuel_limit(10_000),
        )
        .unwrap();
        let entrypoint = entrypoint();
        let bytes =
            wat::parse_str(r#"(module (func (export "_start") (loop $loop br $loop)))"#).unwrap();
        let binary = runtime.compile_binary(&entrypoint, &bytes).unwrap();
        runtime.binaries.insert(entrypoint.clone(), binary);
        let error = runtime.run(entrypoint, Vec::new()).unwrap_err();
        assert!(format!("{error:#}").contains("fuel"), "{error:#}");
    }

    #[test]
    fn fuel_is_disabled_by_default() {
        let runtime = Wasmtime::new(Vec::new(), RuntimeOptions::default()).unwrap();
        assert_eq!(runtime.options.max_fuel, None);
    }

    #[test]
    fn validation_rejects_memory_above_static_limit() {
        let mut runtime = Wasmtime::new(
            Vec::new(),
            RuntimeOptions::default()
                .with_memory_reservation(64 * 1024)
                .with_memory_limit(64 * 1024),
        )
        .unwrap();
        let entrypoint = entrypoint();
        let bytes = wat::parse_str(r#"(module (memory 2) (func (export "_start")))"#).unwrap();
        let binary = runtime.compile_binary(&entrypoint, &bytes).unwrap();
        runtime.binaries.insert(entrypoint, binary);

        let error = runtime.validate_binaries().unwrap_err();
        assert!(format!("{error:#}").contains("memory minimum size"));
    }
}
