use secp256k1::PublicKey;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fs;
use std::path::PathBuf;
use structopt::StructOpt;
use wasmtime::{Caller, ExportType, ImportType, Linker, Module, Store, Trap};
use ya_runtime_aswasm::{link_eth, Allocator, AsMem};

#[derive(StructOpt)]
enum Commands {
    Run(RunCommand),
}

#[derive(StructOpt)]
pub struct RunCommand {
    wasm: PathBuf,
    command: String,
}

impl RunCommand {
    fn exec(self) -> anyhow::Result<()> {
        let store = Store::default();
        let mut linker = Linker::new(&store);

        let wasm_binary = fs::read(self.wasm)?;
        let module = Module::new(linker.store().engine(), wasm_binary)?;
        eprintln!("exports");
        for e in module.exports() {
            let e: ExportType = e;
            eprintln!("name={}, ty={:?}", e.name(), e.ty());
        }
        eprintln!("\nimports\n");
        for i in module.imports() {
            let i: ImportType = i;
            eprintln!("module={}, name={}, ty={:?}", i.module(), i.name(), i.ty());
        }
        linker.func(
            "env",
            "abort",
            move |caller: Caller,
                  message_ptr: u32,
                  file_name: u32,
                  line: u32,
                  _column: u32|
                  -> Result<(), Trap> {
                let mem = AsMem::try_from(caller)?;
                let message = mem.decode_str(message_ptr)?;
                eprintln!("at [{}:{}] {}", mem.decode_str(file_name)?, line, message);
                Err(Trap::new(message))
            },
        )?;
        //linker.define("ya", "context", Extern)
        linker.func(
            "ya",
            "context",
            move |caller: Caller| -> Result<u32, Trap> {
                let mut a = Allocator::for_caller(&caller)?;
                let string_ptr = a.new_string("reqc1")?;
                let s = a.retain(string_ptr)?.to_le_bytes();
                let ptr = a.new_bytes(&[0, 0, 0, 0, 0, 0, 0, 0, s[0], s[1], s[2], s[3]])?;
                Ok(ptr)
            },
        )?;
        linker.func(
            "ya",
            "log",
            move |caller: Caller, message: u32| -> Result<(), Trap> {
                let mem = AsMem::try_from(caller)?;
                eprintln!("log: {}", mem.decode_str(message)?);
                Ok::<_, Trap>(())
            },
        )?;
        link_eth("ya", &mut linker)?;

        linker.func(
            "ya",
            "eth.toPubKey",
            |caller: Caller, ptr: u32| -> Result<u32, Trap> {
                let mem = AsMem::for_caller(&caller)?;
                let secret = mem.decode_secret(ptr)?;
                let mut a = Allocator::for_caller(&caller)?;
                let out_ptr =
                    a.new_bytes(PublicKey::from_secret_key(&secret).serialize().as_ref())?;
                a.retain(out_ptr)
            },
        )?;

        let app = linker.instantiate(&module).unwrap();
        let f = app.get_func(self.command.as_str()).unwrap();
        let rv = f.call(&[])?;
        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    match Commands::from_args() {
        Commands::Run(r) => r.exec()?,
    };
    Ok(())
}
