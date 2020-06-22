use anyhow::Result;
use std::env;
use std::path::PathBuf;
use structopt::StructOpt;
use ya_runtime_wasi::ExeUnitMain;

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

    let cmdargs = CmdArgs::from_args();
    match cmdargs.command {
        Commands::Run {
            entrypoint, args, ..
        } => ExeUnitMain::run(&cmdargs.workdir, &entrypoint, args)?,
        Commands::Deploy { .. } => ExeUnitMain::deploy(&cmdargs.workdir, &cmdargs.task_package)?,
        Commands::Start { .. } => ExeUnitMain::start(&cmdargs.workdir)?,
    }

    Ok(())
}
