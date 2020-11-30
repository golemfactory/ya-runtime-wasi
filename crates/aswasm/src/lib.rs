#![allow(dead_code)]
#![allow(clippy::needless_update)]

mod deploy;
pub mod image;
pub mod runtime;
pub mod service;

pub use deploy::deploy;
use std::path::Path;

pub fn start(work_dir: &Path) -> anyhow::Result<()> {
    let mut runtime = tokio::runtime::Builder::new().basic_scheduler().build()?;
    runtime.block_on(service::start(work_dir))
}
