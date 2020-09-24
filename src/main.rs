use std::path::PathBuf;

use anyhow::Result;
use structopt::StructOpt;
use ya_runtime_wasi::{deploy, RuntimeOptions};

#[derive(StructOpt)]
enum Commands {
    Deploy {},
    Start {},
    Run {
        #[structopt(short = "e", long = "entrypoint")]
        entrypoint: String,
        args: Vec<String>,
    },
}

#[derive(StructOpt)]
#[structopt(rename_all = "kebab-case")]
struct CmdArgs {
    #[structopt(short, long)]
    workdir: PathBuf,
    #[structopt(short, long)]
    task_package: PathBuf,
    #[structopt(long)]
    debug: bool,
    #[structopt(subcommand)]
    command: Commands,
}

fn main() -> Result<()> {
    let cmdline = CmdArgs::from_args();

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

    match cmdline.command {
        Commands::Run { entrypoint, args } => {
            RuntimeOptions::from_env()?.run(&cmdline.workdir, &entrypoint, args)
        }
        Commands::Deploy {} => {
            let res = deploy(&cmdline.workdir, &cmdline.task_package)?;
            println!("{}\n", serde_json::to_string(&res)?);
            Ok(())
        }
        Commands::Start {} => RuntimeOptions::from_env()?.start(&cmdline.workdir),
    }
}
