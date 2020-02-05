use std::path::PathBuf;
use structopt::StructOpt;

use wasi_common::preopen_dir;
use wasmtime::*;
use wasmtime_wasi::create_wasi_instance;

use anyhow::{bail, Context, Result};
use std::collections::HashMap;
use std::fs::{read, File};

#[derive(StructOpt, Debug)]
struct CmdArgs {
    wasm_binary: PathBuf,
    input_dir: PathBuf,
    output_dir: PathBuf,
}

struct DirectoryMount {
    host: PathBuf,
    guest: PathBuf,
}

fn compute_preopen_dirs(dirs: Vec<DirectoryMount>) -> Result<Vec<(String, File)>> {
    let mut preopen_dirs = Vec::new();

    for DirectoryMount { guest, host } in dirs.iter() {
        println!("Mounting: {}::{}", host.display(), guest.display());

        preopen_dirs.push((
            guest.as_os_str().to_str().unwrap().to_string(),
            preopen_dir(host)
                .with_context(|| format!("failed to open directory '{}'", host.display()))?,
        ));
    }

    Ok(preopen_dirs)
}

fn main() {
    println!("WASM-Time example.");

    pretty_env_logger::init();
    let cmdargs = CmdArgs::from_args();

    let store = Store::default();

    let mut module_registry = HashMap::new();

    let input_file = "/in/input.txt".to_string();
    let output_file = "/out/output.txt".to_string();

    let dirs_mapping = vec![
        DirectoryMount {
            host: cmdargs.input_dir,
            guest: PathBuf::from("/in/"),
        },
        DirectoryMount {
            host: cmdargs.output_dir,
            guest: PathBuf::from("/out/"),
        },
    ];

    let args = vec!["wasm-binary".to_string(), input_file, output_file];

    let preopen_dirs = compute_preopen_dirs(dirs_mapping).expect("compute_preopen_dirs failed.");
    let wasi_unstable = create_wasi_instance(&store, &preopen_dirs, &args, &vec![])
        .expect("Failed to create wasi module.");

    module_registry.insert("wasi_unstable".to_owned(), wasi_unstable);

    let wasm = read(&cmdargs.wasm_binary).expect(&format!(
        "Can't load wasm binary {}.",
        cmdargs.wasm_binary.display()
    ));
    let module = Module::new(&store, &wasm).expect("WASM module creation failed.");

    let imports = module
        .imports()
        .iter()
        .map(|import| {
            let module_name = import.module();
            if let Some(instance) = module_registry.get(module_name) {
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
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let instance =
        Instance::new(&store, &module, &imports).expect("WASM instance creation failed.");

    let answer = instance
        .find_export_by_name("_start")
        .expect("answer")
        .func()
        .expect("function");
    let _result = answer.borrow().call(&[]).expect("success");

    println!("Finished.");
}
