fn main() {
    #[cfg(feature = "integration-tests")]
    integration_tests::build_and_generate_tests()
}

#[cfg(feature = "integration-tests")]
mod integration_tests {
    use anyhow::{anyhow, Result};
    use std::collections::HashMap;
    use std::env;
    use std::fs::{self, read_dir, File};
    use std::io::{self, Cursor, Write};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use zip::{write::FileOptions, CompressionMethod, ZipWriter};

    pub(super) fn build_and_generate_tests() {
        let out_dir = PathBuf::from(
            env::var("OUT_DIR").expect("The OUT_DIR environment variable must be set"),
        );

        let mut testcases = HashMap::new();
        let packages = fs::read_dir("integration-tests").unwrap();
        for pkg in packages {
            let pkg_path = pkg.expect("valid package path").path();
            println!(
                "cargo:rerun-if-changed={}",
                pkg_path.join("Cargo.toml").display()
            );
            let (testcase_name, zip_path) =
                build_package(&pkg_path, &out_dir).expect("building package");
            testcases.insert(testcase_name, zip_path);
        }

        let mut out = File::create(out_dir.join("integration_tests.rs"))
            .expect("error generating test source file");
        generate_tests(&mut out, testcases).expect("generating tests");
    }

    fn build_package(pkg_path: &Path, out_dir: &Path) -> Result<(String, PathBuf)> {
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
                "Building WASI binary failed: exit code: {}",
                status.code().unwrap()
            );
        }

        let pkg_name = pkg_path
            .file_stem()
            .ok_or_else(|| anyhow!("missing file stem in pkg path?: '{:?}'", pkg_path))?
            .to_str()
            .ok_or_else(|| anyhow!("invalid UTF8 in path: '{:?}'", pkg_path))?;
        let manifest = fs::read(pkg_path.join("manifest.json"))?;
        let wasm_binary = fs::read(
            out_dir
                .join("wasm32-wasi/release")
                .join(format!("{}.wasm", pkg_name)),
        )?;

        let w = Cursor::new(Vec::new());
        let mut zip = ZipWriter::new(w);
        let options = FileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("manifest.json", options)?;
        zip.write(&manifest)?;
        zip.start_file(format!("{}.wasm", pkg_name), options)?;
        zip.write(&wasm_binary)?;
        let w = zip.finish()?;
        fs::write(out_dir.join(format!("{}.zip", pkg_name)), w.into_inner())?;

        Ok((
            pkg_name.to_owned(),
            out_dir.join(format!("{}.zip", pkg_name)),
        ))
    }

    fn generate_tests(out: &mut File, cases: HashMap<String, PathBuf>) -> Result<()> {
        writeln!(out, "mod integration_tests {{")?;

        for (case_name, zip_path) in cases {
            writeln!(out, "    #[test]")?;
            writeln!(out, "    fn {}() -> anyhow::Result<()> {{", case_name.replace("-", "_"))?;
            writeln!(out, "        ExeUnitMain::deploy()?;")?;
            writeln!(out, "        ExeUnitMain::start()?;")?;

            add_test_logic(out, case_name)?;

            writeln!(out, "    }}")?;
        }

        writeln!(out, "}}")?;
        Ok(())
    }

    fn add_test_logic(out: &mut File, case_name: &str) -> Result<()> {

    }
}
