use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json;
use std::fs::OpenOptions;
use structopt::StructOpt;
#[cfg(feature = "aswasm")]
use ya_runtime_aswasm as aswasm;
#[cfg(feature = "wasi")]
use ya_runtime_wasi as wasi;

#[derive(Deserialize)]
enum RuntimeType {
    #[serde(rename = "wasi")]
    WASI,
    #[serde(rename = "aswasm")]
    ASWASM,
}

#[derive(Deserialize)]
struct Manifest {
    runtime: Option<RuntimeType>,
}

fn detect_runtime(task_package: &Path) -> anyhow::Result<RuntimeType> {
    let mut package = zip::ZipArchive::new(OpenOptions::new().read(true).open(task_package)?)?;
    let mut manifest_file = package.by_name("manifest.json")?;
    let m: Manifest = serde_json::from_reader(&mut manifest_file)?;
    Ok(m.runtime.unwrap_or(RuntimeType::WASI))
}

#[cfg(feature = "wasi")]
macro_rules! with_wasi {
    ($s:expr) => {{
        $s
    }};
}

#[cfg(feature = "aswasm")]
macro_rules! with_aswasm {
    ($s:expr) => {{
        $s
    }};
}

#[cfg(not(feature = "wasi"))]
macro_rules! with_wasi {
    ($s:expr) => {
        unimplemented!()
    };
}

#[cfg(not(feature = "aswasm"))]
macro_rules! with_aswasm {
    ($s:expr) => {
        unimplemented!()
    };
}

#[derive(StructOpt)]
enum Commands {
    Deploy {},
    Start {},
    Run {
        #[structopt(short = "e", long = "entrypoint")]
        entrypoint: String,
        args: Vec<String>,
    },
    Test {},
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
struct CmdArgs {
    #[structopt(short, long, required_ifs(
        &[
            ("command", "deploy"),
            ("command", "start"),
            ("command", "run")
        ])
    )]
    workdir: Option<PathBuf>,
    #[structopt(short, long, required_ifs(
        &[
            ("command", "deploy"),
            ("command", "start"),
            ("command", "run")
        ])
    )]
    task_package: Option<PathBuf>,
    #[structopt(long)]
    debug: bool,
    #[structopt(subcommand)]
    command: Commands,
}

impl CmdArgs {
    fn workdir(&self) -> anyhow::Result<PathBuf> {
        self.workdir.clone().context("No workdir arg")
    }

    fn task_package(&self) -> anyhow::Result<PathBuf> {
        self.task_package.clone().context("No task_package arg")
    }
}

fn main() -> Result<()> {
    let cmdline = CmdArgs::from_args();

    if let Commands::Test {} = cmdline.command {
        return Ok(());
    }

    env_logger::from_env("YA_WASI_LOG")
        .filter(Some("cranelift_codegen"), log::LevelFilter::Error)
        .filter(Some("cranelift_wasm"), log::LevelFilter::Error)
        .filter(
            Some("wasi_common"),
            if cmdline.debug {
                log::LevelFilter::Info
            } else {
                log::LevelFilter::Error
            },
        )
        .init();

    let runtime = detect_runtime(&cmdline.task_package()?)?;

    match cmdline.command {
        #[allow(unused_variables)]
        Commands::Run {
            ref entrypoint,
            ref args,
        } => match runtime {
            RuntimeType::WASI => with_wasi!(wasi::RuntimeOptions::from_env()?.run(
                cmdline.workdir()?,
                entrypoint,
                args.clone()
            )),
            RuntimeType::ASWASM => {
                anyhow::bail!("aswasm is blocking engine, run op is not supported.")
            }
        },
        Commands::Deploy {} => {
            let res = match runtime {
                RuntimeType::WASI => {
                    with_wasi!(wasi::deploy(&cmdline.workdir()?, cmdline.task_package()?))
                }
                RuntimeType::ASWASM => {
                    with_aswasm!(aswasm::deploy(&cmdline.workdir, cmdline.task_package()?))
                }
            }?;
            println!("{}\n", serde_json::to_string(&res)?);
            Ok(())
        }
        Commands::Start {} => match runtime {
            RuntimeType::WASI => {
                with_wasi!(wasi::RuntimeOptions::from_env()?.start(cmdline.workdir()?))
            }
            RuntimeType::ASWASM => with_aswasm!(aswasm::start(cmdline.workdir()?)),
        },
        Commands::Test {} => Ok(()),
    }
}
