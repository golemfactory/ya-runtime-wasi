#![cfg(feature = "integration-tests")]

use anyhow::Result;
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::{env, fs};
use tempfile::tempdir;
use ya_runtime_wasi::ExeUnitMain;

static LOG_INIT: Once = Once::new();

fn log_setup() {
    LOG_INIT.call_once(|| {
        let our_rust_log = "cranelift_wasm=warn,cranelift_codegen=info,wasi_common=trace";
        env::set_var("RUST_LOG", our_rust_log);
        env_logger::init();
    })
}

fn get_zip_path(case_name: &str) -> PathBuf {
    let zip_dir = env::var("OUT_DIR").expect("OUT_DIR must be set!");
    Path::new(&zip_dir).join(format!("{}.zip", case_name))
}

#[test]
fn rust_wasi_tutorial() -> Result<()> {
    let workspace = ManuallyDrop::new(tempdir()?);
    let zip_path = get_zip_path("rust-wasi-tutorial");
    let task_pkg = workspace.path().join("rust-wasi-tutorial.zip");
    fs::copy(zip_path, &task_pkg)?;

    ExeUnitMain::deploy(workspace.path(), &task_pkg)?;
    ExeUnitMain::start(workspace.path())?;

    let contents = "This is it!";
    let input_file_name = "input/in".to_owned();
    let output_file_name = "output/out".to_owned();
    let input_path = workspace.path().join(&input_file_name);
    let output_path = workspace.path().join(&output_file_name);
    fs::write(input_path, contents)?;

    ExeUnitMain::run(
        workspace.path(),
        "rust-wasi-tutorial",
        vec![
            ["/", &input_file_name].join(""),
            ["/", &output_file_name].join(""),
        ],
    )?;
    println!("workspace = {}", workspace.path().display());

    assert!(output_path.exists(), "expected 'out' file to be created");

    let given = fs::read_to_string(output_path)?;

    assert_eq!(
        contents, given,
        "'in' and 'out' should have matching contents"
    );

    // We drop manually so that in case of an error we can browse the temp dir
    let _ = ManuallyDrop::into_inner(workspace);
    Ok(())
}
