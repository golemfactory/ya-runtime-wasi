#![cfg(feature = "integration-tests")]

use std::env;
use std::sync::Once;

static LOG_INIT: Once = Once::new();

fn log_setup() {
    LOG_INIT.call_once(|| {
        let our_rust_log = "cranelift_wasm=warn,cranelift_codegen=info,wasi_common=trace";
        env::set_var("RUST_LOG", our_rust_log);
        env_logger::init();
    })
}

include!(concat!(env!("OUT_DIR"), "/integration_tests.rs"));
