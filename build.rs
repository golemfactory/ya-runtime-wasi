use anyhow::{Context, Result};
use serde_json::Value;
use std::path::PathBuf;
use std::{env, fs};

const DESCRIPTOR_PATH: &str = "conf/ya-runtime-wasi.json";

#[cfg(windows)]
fn setup() {
    let mut res = winres::WindowsResource::new();
    res.set_icon("conf/webassembly.ico");
    res.compile().unwrap();
}

#[cfg(not(windows))]
fn setup() {}

fn update_descriptor() -> Result<()> {
    println!("cargo:rerun-if-changed={DESCRIPTOR_PATH}");
    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS");
    let exe_extension = if target_os == "windows" { ".exe" } else { "" };

    let mut descriptors: Value =
        serde_json::from_reader(fs::OpenOptions::new().read(true).open(DESCRIPTOR_PATH)?)?;
    for descriptor in descriptors.as_array_mut().expect("invalid descriptor") {
        if let Some(obj) = descriptor.as_object_mut() {
            obj.insert(
                "version".into(),
                env::var("CARGO_PKG_VERSION")
                    .expect("env CARGO_PKG_VERSION missing")
                    .into(),
            );
            //obj.insert("name".into(), env::var("CARGO_PKG_NAME")?.into());
            let runtime_path = obj
                .get("runtime-path")
                .and_then(|path| path.as_str().map(|path| format!("{path}{exe_extension}")));
            let supervisor_path = obj
                .get("supervisor-path")
                .and_then(|path| path.as_str().map(|path| format!("{path}{exe_extension}")));
            if let Some(runtime_path) = runtime_path {
                obj.insert("runtime-path".into(), runtime_path.into());
            }
            if let Some(supervisor_path) = supervisor_path {
                obj.insert("supervisor-path".into(), supervisor_path.into());
            }
        } else {
            panic!(
                "invalid descriptor template: {}",
                serde_json::to_string(&descriptor)?
            );
        }
    }
    let output_directory = match env::var("CARGO_BUILD_TARGET_DIR") {
        Ok(path) => PathBuf::from(path),
        Err(_) => {
            let out_directory = PathBuf::from(env::var("OUT_DIR").context("OUT_DIR is not set")?);
            out_directory
                .ancestors()
                .nth(3)
                .map(PathBuf::from)
                .context("OUT_DIR does not contain a Cargo profile directory")?
        }
    };
    fs::create_dir_all(&output_directory).with_context(|| {
        format!(
            "creating descriptor output directory {}",
            output_directory.display()
        )
    })?;

    let output_file = output_directory.join("ya-runtime-wasi.json");
    serde_json::to_writer_pretty(
        fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&output_file)
            .with_context(|| format!("writing descriptor to {}", output_file.display()))?,
        &descriptors,
    )?;

    Ok(())
}

fn main() {
    update_descriptor().unwrap();
    setup();
}
