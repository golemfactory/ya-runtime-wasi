use serde_json::Value;
use std::error::Error;
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

fn update_descriptor() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=conf/{}", DESCRIPTOR_PATH);
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
                .and_then(|path| Some(format!("{}{}", path.as_str()?, exe_extension)));
            let supervisor_path = obj
                .get("supervisor-path")
                .and_then(|path| Some(format!("{}{}", path.as_str()?, exe_extension)));
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
    println!("cargo:warning={}", serde_json::to_string(&descriptors)?);
    let output_directory: PathBuf = env::var("CARGO_BUILD_TARGET_DIR")
        .ok()
        .unwrap_or_else(|| "target/release".to_string())
        .into();
    serde_json::to_writer_pretty(
        fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(output_directory.join("ya-runtime-wasi.json"))?,
        &descriptors,
    )?;

    Ok(())
}

fn main() {
    update_descriptor().unwrap();
    setup();
}
