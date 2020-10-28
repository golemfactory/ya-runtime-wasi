#![cfg(feature = "integration-tests")]
use std::ffi::OsStr;
use std::path::Path;

use std::{env, process};
use tempfile::tempdir;

const NO_ARGS: &[&str] = &[];

fn exe_runtime(
    package: &str,
    work_dir: &Path,
    command: &str,
    args: impl IntoIterator<Item = impl AsRef<OsStr>>,
) -> anyhow::Result<process::Output> {
    let app = env!("CARGO_BIN_EXE_ya-runtime-wasi");
    let output = process::Command::new(app)
        .stderr(process::Stdio::piped())
        .stdout(process::Stdio::piped())
        .args(&["--task-package", package])
        .arg("--workdir")
        .arg(work_dir)
        .arg(command)
        .args(args)
        .output()?;
    Ok(output)
}

#[test]
fn test_outputs() -> anyhow::Result<()> {
    let package = "tests/trusted-voting-mgr-66d7ce8208f4da9d7cbd.ywasi";
    let dir = tempdir()?;
    let output = exe_runtime(package, dir.path(), "deploy", NO_ARGS)?;
    assert!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).is_ok(),
        "deploy output should by json.\nstdout: [{}]\nstderr: [{}]",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let output = exe_runtime(package, dir.path(), "start", NO_ARGS)?;
    assert_eq!(output.stdout.len(), 0, "start expected empty stdout");
    assert_eq!(
        output.stderr.len(),
        0,
        "start expected empty stderr\nwas: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = exe_runtime(
        package,
        dir.path(),
        "run",
        &[
            "-e",
            "trusted-voting-mgr",
            "--",
            "init",
            "aea5db67524e02a263b9339fe6667d6b577f3d4c",
            "1",
        ],
    )?;
    assert_eq!(
        output.stderr.len(),
        0,
        "run expected empty stderr\nwas: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = exe_runtime(
        package,
        dir.path(),
        "run",
        &["-e", "trusted-voting-mgr", "--", "debug"],
    )?;
    assert_eq!(
        output.stderr.len(),
        0,
        "run expected empty stderr\nwas: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    Ok(())
}
