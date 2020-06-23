fn main() {
    #[cfg(feature = "integration-tests")]
    integration_tests::build_and_generate_tests()
}

#[cfg(feature = "integration-tests")]
mod integration_tests {
    use std::env;
    use std::fs::{self, read_dir, File};
    use std::io::{self, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    pub(super) fn build_and_generate_tests() {
        const INTEGRATION_TESTS: &str = "integration-tests";
        let out_dir = PathBuf::from(
            env::var("OUT_DIR").expect("The OUT_DIR environment variable must be set"),
        );
        let packages = fs::read_dir(INTEGRATION_TESTS).unwrap();
        for pkg in packages {
            let pkg_path = pkg.expect("valid package path").path();
            println!("cargo:rerun-if-changed={}", pkg_path.join("Cargo.toml").display());
            build_package(&pkg_path, &out_dir).expect("building package");
        }
    }

    fn build_package(pkg_path: &Path, out_dir: &Path) -> io::Result<()> {
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
        .current_dir(pkg_path);
        let output = cmd.output()?;

        let status = output.status;
        if !status.success() {
            panic!(
                "Building package failed: exit code: {}",
                status.code().unwrap()
            );
        }

        Ok(())
    }

    // fn test_directory(out: &mut File, testsuite: &str, out_dir: &Path) -> io::Result<()> {
    //     let mut dir_entries: Vec<_> = read_dir(out_dir.join("wasm32-wasi/release"))
    //         .expect("reading testsuite directory")
    //         .map(|r| r.expect("reading testsuite directory entry"))
    //         .filter(|dir_entry| {
    //             let p = dir_entry.path();
    //             if let Some(ext) = p.extension() {
    //                 // Only look at wast files.
    //                 if ext == "wasm" {
    //                     // Ignore files starting with `.`, which could be editor temporary files
    //                     if let Some(stem) = p.file_stem() {
    //                         if let Some(stemstr) = stem.to_str() {
    //                             if !stemstr.starts_with('.') {
    //                                 return true;
    //                             }
    //                         }
    //                     }
    //                 }
    //             }
    //             false
    //         })
    //         .collect();

    //     dir_entries.sort_by_key(|dir| dir.path());

    //     writeln!(
    //         out,
    //         "mod {} {{",
    //         Path::new(testsuite)
    //             .file_stem()
    //             .expect("testsuite filename should have a stem")
    //             .to_str()
    //             .expect("testsuite filename should be representable as a string")
    //             .replace("-", "_")
    //     )?;
    //     writeln!(out, "    use super::{{Package}};")?;
    //     writeln!(out, "    use ya_runtime_wasi::{{ExeUnitMain}};")?;
        
    //     // Create package
    //     writeln!(out, "    static PKG_INIT: std::sync::Once = std::sync::Once::new();")?;
    //     writeln!(out, "    fn setup() {{")?;
    //     writeln!(out, "        PKG_INIT.call_once(|| {{")?;
    //     writeln!(out, "            let mut pkg = Package::new(\"{}\");", testsuite)?;
    //     writeln!(out, "            let zip_bytes = pkg.into_bytes();")?;
    //     writeln!(out, "            let dir = tempfile::tempdir().expect(\"create tempdir\");")?;
    //     writeln!(out, "            std::fs::write(\"{}.zip\", zip_bytes).expect(\"save zip as file\")", testsuite)?;
    //     writeln!(out, "        }})")?;
    //     writeln!(out, "     }}")?;

    //     // Create testcases
    //     for dir_entry in dir_entries {
    //         let test_path = dir_entry.path();
    //         write_testcase(out, testsuite, &test_path)?;
    //     }

    //     writeln!(out, "}}")?;
    //     Ok(())
    // }

    // fn write_testcase(out: &mut File, testsuite: &str, path: &Path) -> io::Result<()> {
    //     let stem = path
    //         .file_stem()
    //         .expect("file_stem")
    //         .to_str()
    //         .expect("to_str");
    //     writeln!(out, "    #[test]")?;

    //     let entrypoint = stem.replace("-", "_");
    //     writeln!(out, "    fn r#{}() -> anyhow::Result<()> {{", entrypoint)?;
    //     writeln!(out, "        setup();")?;
    //     writeln!(out, "        Ok(())")?;
    //     writeln!(out, "     }}")?;

    //     Ok(())
    // }
}
