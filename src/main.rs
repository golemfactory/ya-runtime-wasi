use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use ya_runtime_wasi as wasi;

#[derive(Subcommand)]
enum Commands {
    Deploy {},
    Start {},
    Run {
        #[arg(short = 'e', long)]
        entrypoint: String,
        args: Vec<String>,
    },
    Test {},
}

#[derive(Parser)]
#[command(rename_all = "kebab-case")]
struct CmdArgs {
    #[arg(short, long)]
    workdir: Option<PathBuf>,
    #[arg(short, long)]
    task_package: Option<PathBuf>,
    #[arg(long)]
    debug: bool,
    #[command(subcommand)]
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
    let cmdline = CmdArgs::parse();

    if matches!(&cmdline.command, Commands::Test {}) {
        return Ok(());
    }

    env_logger::Builder::from_env("YA_WASI_LOG")
        .filter(Some("cranelift_codegen"), log::LevelFilter::Error)
        .filter(Some("cranelift_wasm"), log::LevelFilter::Error)
        .filter(
            Some("wasmtime_wasi"),
            if cmdline.debug {
                log::LevelFilter::Info
            } else {
                log::LevelFilter::Error
            },
        )
        .init();

    match cmdline.command {
        #[allow(unused_variables)]
        Commands::Run {
            ref entrypoint,
            ref args,
        } => wasi::RuntimeOptions::from_env()?.run(cmdline.workdir()?, entrypoint, args.clone()),
        Commands::Deploy {} => {
            let res = wasi::deploy(&cmdline.workdir()?, cmdline.task_package()?)?;
            println!("{}\n", serde_json::to_string(&res)?);
            Ok(())
        }
        Commands::Start {} => wasi::RuntimeOptions::from_env()?.start(cmdline.workdir()?),
        Commands::Test {} => Ok(()),
    }
}
