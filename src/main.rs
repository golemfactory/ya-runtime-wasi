mod entrypoint;
mod manifest;
mod wasmtime_unit;

use crate::entrypoint::{CmdArgs, ExeUnitMain};
use anyhow::Result;
use structopt::StructOpt;

fn main() -> Result<()> {
    env_logger::init();

    let cmdargs = CmdArgs::from_args();
    Ok(ExeUnitMain::entrypoint(cmdargs)?)
}
