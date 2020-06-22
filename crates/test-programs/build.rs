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
        let mut out = File::create(out_dir.join("ya_wasi_tests.rs"))
            .expect("error generating test source file");
        build_tests("ya-wasi-tests", &out_dir).expect("building tests");
        test_directory(&mut out, "ya-wasi-tests", &out_dir).expect("generating tests");
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

    fn test_directory(out: &mut File, testsuite: &str, out_dir: &Path) -> io::Result<()> {
        let mut dir_entries: Vec<_> = read_dir(out_dir.join("wasm32-wasi/release"))
            .expect("reading testsuite directory")
            .map(|r| r.expect("reading testsuite directory entry"))
            .filter(|dir_entry| {
                let p = dir_entry.path();
                if let Some(ext) = p.extension() {
                    // Only look at wast files.
                    if ext == "wasm" {
                        // Ignore files starting with `.`, which could be editor temporary files
                        if let Some(stem) = p.file_stem() {
                            if let Some(stemstr) = stem.to_str() {
                                if !stemstr.starts_with('.') {
                                    return true;
                                }
                            }
                        }
                    }
                }
                false
            })
            .collect();

        dir_entries.sort_by_key(|dir| dir.path());

        writeln!(
            out,
            "mod {} {{",
            Path::new(testsuite)
                .file_stem()
                .expect("testsuite filename should have a stem")
                .to_str()
                .expect("testsuite filename should be representable as a string")
                .replace("-", "_")
        )?;
        writeln!(out, "    use super::{{Package}};")?;
        writeln!(out, "    use ya_runtime_wasi::{{ExeUnitMain}};")?;
        
        // Create package
        writeln!(out, "    static PKG_INIT: std::sync::Once = std::sync::Once::new();")?;
        writeln!(out, "    fn setup() {{")?;
        writeln!(out, "        PKG_INIT.call_once(|| {{")?;
        writeln!(out, "            let mut pkg = Package::new(\"{}\");", testsuite)?;
        writeln!(out, "            let zip_bytes = pkg.into_bytes();")?;
        writeln!(out, "            let dir = tempfile::tempdir().expect(\"create tempdir\");")?;
        writeln!(out, "            std::fs::write(\"{}.zip\", zip_bytes).expect(\"save zip as file\")", testsuite)?;
        writeln!(out, "        }})")?;
        writeln!(out, "     }}")?;

        // Create testcases
        for dir_entry in dir_entries {
            let test_path = dir_entry.path();
            write_testcase(out, testsuite, &test_path)?;
        }

        writeln!(out, "}}")?;
        Ok(())
    }

    fn write_testcase(out: &mut File, testsuite: &str, path: &Path) -> io::Result<()> {
        let stem = path
            .file_stem()
            .expect("file_stem")
            .to_str()
            .expect("to_str");
        writeln!(out, "    #[test]")?;

        let entrypoint = stem.replace("-", "_");
        writeln!(out, "    fn r#{}() -> anyhow::Result<()> {{", entrypoint)?;
        writeln!(out, "        setup();")?;
        writeln!(out, "        Ok(())")?;
        writeln!(out, "     }}")?;

        Ok(())
    }
}
