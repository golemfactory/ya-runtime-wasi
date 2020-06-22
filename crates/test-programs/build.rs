fn main() {
    #[cfg(feature = "test_programs")]
    ya_wasi_tests::build_and_generate_tests()
}

#[cfg(feature = "test_programs")]
mod ya_wasi_tests {
    use std::env;
    use std::fs::{read_dir, File};
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    pub(super) fn build_and_generate_tests() {
        // Validate if any of test sources are present and if they changed
        // This should always work since there is no submodule to init anymore
        let bin_tests = std::fs::read_dir("ya-wasi-tests/src/bin").unwrap();
        for test in bin_tests {
            if let Ok(test_file) = test {
                let test_file_path = test_file
                    .path()
                    .into_os_string()
                    .into_string()
                    .expect("test file path");
                println!("cargo:rerun-if-changed={}", test_file_path);
            }
        }
        println!("cargo:rerun-if-changed=ya-wasi-tests/Cargo.toml");
        println!("cargo:rerun-if-changed=ya-wasi-tests/src/lib.rs");
        let out_dir = PathBuf::from(
            env::var("OUT_DIR").expect("The OUT_DIR environment variable must be set"),
        );
        let mut out =
            File::create(out_dir.join("ya_wasi_tests.rs")).expect("error generating test source file");
        build_tests("ya-wasi-tests", &out_dir).expect("building tests");
        // test_directory(&mut out, "ya-wasi-tests", &out_dir).expect("generating tests");
    }

    fn build_tests(testsuite: &str, out_dir: &Path) -> io::Result<()> {
        let mut cmd = Command::new("cargo");
        cmd.args(&[
            "build",
            "--release",
            "--target=wasm32-wasi",
            "--target-dir",
            out_dir.to_str().unwrap(),
        ])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .current_dir(testsuite);
        let output = cmd.output()?;

        let status = output.status;
        if !status.success() {
            panic!(
                "Building tests failed: exit code: {}",
                status.code().unwrap()
            );
        }

        Ok(())
    }
}
