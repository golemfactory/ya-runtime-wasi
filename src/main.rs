use std::{env, path::PathBuf};

use anyhow::Result;
use structopt::StructOpt;
use ya_runtime_wasi::{deploy, run, start, RuntimeOptions};

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
    #[structopt(subcommand)]
    command: Commands,
}

fn main() -> Result<()> {
    let our_rust_log = "cranelift_wasm=warn,cranelift_codegen=info,wasi_common=info";
    match env::var("RUST_LOG") {
        Err(_) => env::set_var("RUST_LOG", our_rust_log),
        Ok(var) => env::set_var("RUST_LOG", format!("{},{}", var, our_rust_log)),
    };
    env_logger::init();

    let cmdline = CmdArgs::from_args();
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
