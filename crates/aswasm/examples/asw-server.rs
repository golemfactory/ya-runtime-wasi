use secp256k1::PublicKey;

use actix_web::{web, App, HttpServer};
use actix_web::{HttpResponse, Responder};
use futures::prelude::*;
use std::convert::TryFrom;
use std::fs;
use std::path::PathBuf;
use structopt::StructOpt;
use wasmtime::{Caller, ExportType, ImportType, Linker, Module, Store, Trap};
use ya_runtime_aswasm::runtime::{link_eth, link_io, Allocator, AsMem};
use ya_runtime_aswasm::service::{ApplicationChannel, Command};

#[derive(StructOpt)]
enum Commands {
    Run(RunCommand),
    Server(ServerCommand),
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
                  message_ptr: i32,
                  file_name: i32,
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
            move |caller: Caller| -> Result<i32, Trap> {
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
            move |caller: Caller, message: i32| -> Result<(), Trap> {
                let mem = AsMem::try_from(caller)?;
                eprintln!("log: {}", mem.decode_str(message)?);
                Ok::<_, Trap>(())
            },
        )?;
        link_eth("ya", &mut linker)?;
        link_io("ya", &mut linker, PathBuf::from("/tmp/w"), Vec::new())?;

        linker.func(
            "ya",
            "eth.toPubKey",
            |caller: Caller, ptr: i32| -> Result<i32, Trap> {
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
        let _rv = f.call(&[])?;
        Ok(())
    }
}

#[derive(StructOpt)]
struct ServerCommand {
    #[structopt(long, short)]
    workdir: Option<PathBuf>,
    package: PathBuf,
}

#[actix_web::post("/run/{entryPoint}")]
async fn do_run(
    p: web::Path<(String,)>,
    app: web::Data<ApplicationChannel>,
    body: web::Json<Vec<String>>,
) -> impl Responder {
    let entry_point = p.into_inner().0;
    let (rx, mut tx) = futures::channel::mpsc::unbounded();
    let _ = app.send(Command {
        pid: 0,
        entry_point,
        args: body.0,
        status: rx,
    });
    let mut stdout = Vec::<u8>::new();
    let mut stderr = Vec::<u8>::new();
    while let Some(e) = tx.next().await {
        stdout.extend(&e.stdout);
        stderr.extend(&e.stderr);
    }
    let stdout = String::from_utf8_lossy(&stdout);
    let stderr = String::from_utf8_lossy(&stderr);
    HttpResponse::Ok().json(serde_json::json!({
           "stdout": stdout,
           "stderr": stderr
    }))
}

impl ServerCommand {
    async fn exec(&self) -> anyhow::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let work_dir = self
            .workdir
            .as_ref()
            .map(AsRef::as_ref)
            .unwrap_or_else(|| temp_dir.as_ref());
        let deploy = ya_runtime_aswasm::deploy(work_dir, &self.package)?;
        eprintln!("{:?}", deploy);
        let app = ya_runtime_aswasm::service::spawn_application(work_dir.to_owned());
        HttpServer::new(move || {
            let app = app.clone();
            App::new().data(app).service(do_run)
        })
        .bind("127.0.0.1:8080")?
        .run()
        .await?;

        Ok(())
    }
}

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    match Commands::from_args() {
        Commands::Run(r) => r.exec()?,
        Commands::Server(s) => s.exec().await?,
    };
    Ok(())
}
