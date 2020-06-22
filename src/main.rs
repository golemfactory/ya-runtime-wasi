mod entrypoint;
mod manifest;
mod wasmtime_unit;

use crate::entrypoint::{CmdArgs, ExeUnitMain};
use anyhow::Result;
use std::env;
use structopt::StructOpt;

fn main() -> Result<()> {
    let our_rust_log = "cranelift_wasm=warn,cranelift_codegen=info,wasi_common=info";
    match env::var("RUST_LOG") {
        Err(_) => env::set_var("RUST_LOG", our_rust_log),
        Ok(var) => env::set_var("RUST_LOG", format!("{},{}", var, our_rust_log)),
    };
    env_logger::init();

    let cmdargs = CmdArgs::from_args();
    ExeUnitMain::entrypoint(cmdargs)?;

    Ok(())
}
