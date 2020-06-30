//#![cfg(feature = "integration-tests")]

use anyhow::{Context, Result};
use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::sync::Once;
use std::{env, fs};
use tempfile::{tempdir, TempDir};
use ya_runtime_wasi::{deploy, run, start, DeployFile};

static LOG_INIT: Once = Once::new();

fn log_setup() {
    LOG_INIT.call_once(|| {
        env_logger::init();
    })
}

struct TestCase {
    name: String,
    workspace: ManuallyDrop<TempDir>,
}

impl TestCase {
    fn new<S: AsRef<str>>(name: S) -> Self {
        log_setup();

        let workspace = ManuallyDrop::new(tempdir().expect("could create a temp dir"));

        Self {
            name: name.as_ref().to_owned(),
            workspace,
        }
    }

    fn with(self, logic: impl FnOnce(&Path) -> Result<()>) -> Result<()> {
        let zip_path = self.get_zip_path();
        let task_pkg = self.workspace.path().join(self.name);
        fs::copy(zip_path, &task_pkg)?;

        deploy(self.workspace.path(), &task_pkg)?;
        start(self.workspace.path())?;

        let workspace_path = self.workspace.path();
        match logic(workspace_path)
            .with_context(|| format!("while testing package at '{}'", workspace_path.display()))
        {
            Ok(()) => {
                // All assertions passed and there was no error,
                // safe to drop the workspace dir now!
                let _ = ManuallyDrop::into_inner(self.workspace);
                Ok(())
            }
            Err(err) => Err(err),
        }
    }

    fn get_zip_path(&self) -> PathBuf {
        let zip_dir = env::var("OUT_DIR").expect("OUT_DIR must be set!");
        Path::new(&zip_dir).join(format!("{}.zip", self.name))
    }
}

#[test]
fn rust_wasi_tutorial() -> Result<()> {
    TestCase::new("rust-wasi-tutorial").with(|workspace: &Path| {
        let deployment = DeployFile::load(workspace).unwrap();

        let input_vol = deployment
            .vols
            .iter()
            .find(|vol| vol.path.starts_with("/input"))
            .map(|vol| workspace.join(&vol.name))
            .unwrap();

        let output_vol = deployment
            .vols
            .iter()
            .find(|vol| vol.path.starts_with("/output"))
            .map(|vol| workspace.join(&vol.name))
            .unwrap();

        let contents = "This is it!";
        let input_file_name = "in".to_owned();
        let output_file_name = "out".to_owned();
        let input_path = input_vol.join(&input_file_name);
        let output_path = output_vol.join(&output_file_name);
        fs::write(&input_path, contents)?;
        run(
            workspace,
            "rust-wasi-tutorial",
            vec![
                ["/input/", &input_file_name].join(""),
                ["/output/", &output_file_name].join(""),
            ],
        )?;

        assert!(output_path.exists(), "expected 'out' file to be created");

        let given = fs::read_to_string(output_path)?;

        assert_eq!(
            contents, given,
            "'in' and 'out' should have matching contents"
        );

        Ok(())
    })
}
