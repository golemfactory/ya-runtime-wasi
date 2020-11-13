use crate::deploy::{Deployment, Output};
use crate::runtime::{link_eth, link_io, AsMem};
use futures::prelude::*;
use futures::FutureExt;
use std::cell::RefCell;
use std::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;
use wasmtime::{Caller, Linker, Module, Store, Trap};
use ya_runtime_api::server::{
    self, AsyncResponse, ErrorResponse, KillProcess, ProcessStatus, RunProcess, RunProcessResp,
    RuntimeEvent, RuntimeService,
};

trait OutputHandler {
    fn handle_stdout(&self, message: &[u8]);

    fn handle_stderr(&self, message: &[u8]);
}

struct NoopOutputHandler;

impl OutputHandler for NoopOutputHandler {
    fn handle_stdout(&self, _message: &[u8]) {}

    fn handle_stderr(&self, _message: &[u8]) {}
}

thread_local! {
    static OUTPUT_HADLER: RefCell<Box<dyn OutputHandler>> = RefCell::new(Box::new(NoopOutputHandler));
}

pub struct Application {
    store: Store,
    app: wasmtime::Instance,
    deployment: Deployment,
}

impl Application {
    fn new(work_dir: &Path) -> anyhow::Result<Self> {
        let deployment = Deployment::load(work_dir)?;
        let mut config = wasmtime::Config::new();
        config.static_memory_guard_size(0x1_0000);
        config.interruptable(true);
        // max: 50M
        config.static_memory_maximum_size(50 * 2 << 20);

        let engine = wasmtime::Engine::new(&config);
        let store = Store::new(&engine);
        let mut linker = Linker::new(&store);
        let wasm_binary = deployment
            .get_image()?
            .get_bytes(&deployment.main_entry().wasm_path)?;
        let module = Module::new(&engine, wasm_binary)?;
        link_io("ya", &mut linker, work_dir.to_owned(), deployment.vols())?;
        link_eth("ya", &mut linker)?;

        linker.func(
            "env",
            "abort",
            move |caller: Caller,
                  message_ptr: i32,
                  file_name: i32,
                  line: i32,
                  _column: i32|
                  -> Result<(), Trap> {
                let mem = AsMem::try_from(caller)?;
                let message = mem.decode_str(message_ptr)?;
                OUTPUT_HADLER.with(|h| {
                    let out_message =
                        format!("at [{}:{}] {}", mem.decode_str(file_name)?, line, message);
                    h.borrow().handle_stderr(out_message.as_bytes());
                    Ok::<_, Trap>(())
                })?;

                Err(Trap::new(message))
            },
        )?;
        //linker.define("ya", "context", Extern)
        linker.func(
            "ya",
            "log",
            move |caller: Caller, message: i32| -> Result<(), Trap> {
                let mem = AsMem::try_from(caller)?;
                let msg = mem.decode_str(message)?;
                OUTPUT_HADLER.with(|h| h.borrow().handle_stderr(msg.as_ref()));
                Ok::<_, Trap>(())
            },
        )?;
        let app = linker.instantiate(&module)?;

        Ok(Application {
            store,
            app,
            deployment,
        })
    }

    fn run(&self, entry_point: &str, args: Vec<String>) -> anyhow::Result<i32> {
        let ep = match self.deployment.entry_point(entry_point) {
            Some(v) => v,
            None => anyhow::bail!("unknown entrypoint: {}", entry_point),
        };
        let func = match self.app.get_func(entry_point) {
            Some(v) => v,
            None => anyhow::bail!("entrypoint {} not exported", entry_point),
        };
        let result = func.call(&ep.convert_args(&self.app, args)?)?;
        let mem = AsMem::for_instance(&self.app)?;
        match &ep.output {
            Output::ExitCode => {
                let err_code = wasmtime::Val::i32(&result[0])
                    .ok_or_else(|| anyhow::anyhow!("invalid return type, expected error code"))?;
                Ok(err_code)
            }
            Output::Void => Ok(0),
            Output::Bytes => {
                let ptr = &result[0].i32().ok_or_else(|| {
                    anyhow::anyhow!("invalid return type, expected pointer to bytes")
                })?;
                let hex = mem.decode(*ptr as i32, |bytes| Ok(hex::encode(bytes)))?;
                OUTPUT_HADLER.with(|h| h.borrow().handle_stdout(hex.as_bytes()));
                Ok(0)
            }
            Output::String => {
                let ptr = result[0].i32().ok_or_else(|| {
                    anyhow::anyhow!("invalid return type, expected pointer to bytes")
                })?;
                let output = mem.decode_str(ptr)?;
                OUTPUT_HADLER.with(|h| h.borrow().handle_stdout(output.as_bytes()));
                Ok(0)
            }
        }
    }
}

pub struct Command {
    pub pid: u32,
    pub entry_point: String,
    pub args: Vec<String>,
    pub status: futures::channel::mpsc::UnboundedSender<ProcessStatus>,
}

struct SenderHandler(futures::channel::mpsc::UnboundedSender<ProcessStatus>);

impl OutputHandler for SenderHandler {
    fn handle_stdout(&self, message: &[u8]) {
        let _ = self.0.unbounded_send(ProcessStatus {
            stdout: message.into(),
            ..ProcessStatus::default()
        });
    }

    fn handle_stderr(&self, message: &[u8]) {
        let _ = self.0.unbounded_send(ProcessStatus {
            stderr: message.into(),
            ..ProcessStatus::default()
        });
    }
}

pub fn with_sender<T, F: FnOnce() -> T>(
    sender: futures::channel::mpsc::UnboundedSender<ProcessStatus>,
    f: F,
) -> T {
    let prev_handler = OUTPUT_HADLER.with(|h| h.replace(Box::new(SenderHandler(sender))));
    let ret = f();
    let _ = OUTPUT_HADLER.with(|h| h.replace(prev_handler));
    ret
}

pub type ApplicationChannel = std::sync::mpsc::Sender<Command>;

pub fn spawn_application(work_dir: PathBuf) -> ApplicationChannel {
    let (tx, rx) = std::sync::mpsc::channel::<Command>();
    let handle = tokio::task::spawn_blocking(move || {
        let app = Application::new(&work_dir)?;
        log::info!("started");
        for command in rx.iter() {
            let status = command.status.clone();
            let pid = command.pid as u64;

            log::debug!("command pid:{}, ep:{}", pid, &command.entry_point);
            match with_sender(command.status.clone(), || {
                app.run(&command.entry_point, command.args)
            }) {
                Ok(return_code) => {
                    let _ignore = status.unbounded_send(ProcessStatus {
                        pid,
                        running: false,
                        return_code,
                        ..ProcessStatus::default()
                    });
                }
                Err(e) => {
                    let stderr = format!("Fatal: {}", e);
                    let _ignore = status.unbounded_send(ProcessStatus {
                        pid,
                        running: false,
                        return_code: 1,
                        stderr: stderr.into_bytes(),
                        ..ProcessStatus::default()
                    });
                }
            }
        }
        Ok::<_, anyhow::Error>(())
    });
    tokio::spawn(async move {
        match handle.await {
            Err(e) => log::error!("crash: {}", e),
            Ok(Err(e)) => log::error!("crash: {:?}", e),
            Ok(Ok(v)) => log::info!("shutdown"),
        }
    });
    tx
}

pub struct Service<T: RuntimeEvent> {
    events: Arc<T>,
    application: ApplicationChannel,
    pid: AtomicI32,
}

impl<T: RuntimeEvent> Service<T> {
    pub fn new(events: T, work_dir: PathBuf) -> Self {
        let events = Arc::new(events);
        let pid = AtomicI32::new(1);
        let application = spawn_application(work_dir);
        Self {
            events,
            pid,
            application,
        }
    }
}

impl<T: RuntimeEvent + 'static> RuntimeService for Service<T> {
    fn hello(&self, _version: &str) -> AsyncResponse<'_, String> {
        future::ok("0.1.0".to_string()).boxed_local()
    }

    fn run_process(&self, run: RunProcess) -> AsyncResponse<'_, RunProcessResp> {
        let pid = self.pid.fetch_add(1, Ordering::SeqCst);

        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        if let Err(_e) = self.application.send(Command {
            pid: 0,
            entry_point: run.bin,
            args: run.args,
            status: tx,
        }) {
            return future::err(ErrorResponse {
                code: 0,
                message: "container failed to start".to_string(),
                context: Default::default(),
                ..ErrorResponse::default()
            })
            .boxed_local();
        }
        let events = self.events.clone();
        let _ = tokio::task::spawn_local(async move {
            while let Some(status) = rx.next().await {
                events.on_process_status(status)
            }
        });
        future::ok(RunProcessResp {
            pid: pid as u64,
            ..RunProcessResp::default()
        })
        .boxed_local()
    }

    fn kill_process(&self, _kill: KillProcess) -> AsyncResponse<'_, ()> {
        future::ok(()).boxed_local()
    }

    fn shutdown(&self) -> AsyncResponse<'_, ()> {
        future::ok(()).boxed_local()
    }
}

pub async fn start(workdir: &Path) -> anyhow::Result<()> {
    server::run(|emiter| Service::new(emiter, workdir.to_path_buf())).await;
    Ok(())
}
