//! This crate allows you to embed [Yagna] WASI runtime inside your application.
//!
//! [Yagna]: https://github.com/golemfactory/yagna
//!
//! ## Quick start
//!
//! The usage is pretty straightforward. In your `Cargo.toml`, put `ya-runtime-wasi`
//! as your dependency
//!
//! ```toml
//! # Cargo.toml
//! [dependencies]
//! ya-runtime-wasi = "0.2"
//! ```
//!
//! You can now embed the runtime in your app like so
//!
//! ```rust,no_run
//! use std::path::Path;
//! use ya_runtime_wasi::*;
//!
//! // In this example, we assume that `package.zip` contains a WASI binary
//! // called `hello.wasm` which also our entrypoint into the package, and
//! // maps input/output to `/workdir`
//! let workspace = Path::new("workspace");
//! let entrypoint = "hello";
//! let package = Path::new("package.zip");
//!
//! // Deploy package
//! deploy(&workspace, &package).unwrap();
//!
//! // Start the runtime
//! start(&workspace).unwrap();
//!
//! // Execute the binary
//! run(
//!     &workspace,
//!     &entrypoint,
//!     vec![
//!         "/workdir/input".into(),
//!         "/workdir/output".into(),
//!     ],
//! ).unwrap();
//! ```
//!
//! ## Obtaining handle to the deployed image
//!
//! A handle to the deployed image, i.e., the path to the image as well as a list of mapped
//! container volumes, can be obtained using [`DeployFile::load`] function invoked
//! after [`ya_runtime_wasi::deploy`] and [`ya_runtime_wasi::start`] were run.
//!
//! [`DeployFile::load`]: struct.DeployFile.html#method.load
//! [`ya_runtime_wasi::deploy`]: fn.deploy.html
//! [`ya_runtime_wasi::start`]: fn.start.html
//!
//! ## Examples
//!
//! A good example of using `ya-runtime-wasi` embedding API can be found in the [`gfaas`]
//! crate.
//!
//! [`gfaas`]: https://github.com/golemfactory/gfaas

//#![deny(missing_docs)]

mod deploy;
mod entrypoint;
mod manifest;
mod wasmtime_unit;

pub use deploy::{deploy, DeployFile};
pub use entrypoint::{run, start, RuntimeOptions};
