use super::AsMem;
use std::cell::RefCell;
use std::collections::BTreeMap;

use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::ops::Not;
use std::path::PathBuf;
use std::rc::Rc;
use wasmtime::{Caller, Linker, Trap};
use ya_runtime_api::deploy::ContainerVolume;

const MAX_FDS: usize = 4096;
const MIN_FD: i32 = 256;

pub struct Fd(std::fs::File);

impl Fd {
    fn read(&mut self, slice: &mut [u8]) -> std::io::Result<i32> {
        assert!(slice.len() < 1 << 32);
        let s = self.0.read(slice)?;
        Ok(s as i32)
    }

    fn write(&mut self, slice: &[u8]) -> std::io::Result<i32> {
        assert!(slice.len() < 1 << 32);
        self.0.write_all(slice)?;
        Ok(slice.len() as i32)
    }
}

pub struct FdStateInner {
    vols: Vec<ContainerVolume>,
    base_dir: PathBuf,
    fds: BTreeMap<i32, Fd>,
    n_fds: i32,
}

impl FdStateInner {
    fn add_fd(&mut self, fd: Fd) -> i32 {
        if self.fds.len() > 4096 {
            return -1;
        }
        loop {
            let ifd = self.n_fds;
            self.n_fds = (self.n_fds + 1) & 0xfff_ffff;
            if self.n_fds < MIN_FD {
                self.n_fds = MIN_FD
            }

            if self.fds.contains_key(&ifd).not() {
                let _ = self.fds.insert(ifd, fd);
                return ifd;
            }
        }
    }

    fn remove_fd(&mut self, fd: i32) -> bool {
        self.fds.remove(&fd).is_some()
    }

    fn exec<Operation: FnOnce(&mut Fd) -> std::io::Result<i32>>(
        &mut self,
        fd: i32,
        operation: Operation,
    ) -> std::io::Result<i32> {
        if let Some(fd) = self.fds.get_mut(&fd) {
            operation(fd)
        } else {
            Ok(-1)
        }
    }

    fn find_path(&self, path: &str) -> std::io::Result<Option<PathBuf>> {
        if path.chars().any(|ch| ch == '.' || ch == ':') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "invalid path char",
            ));
        }
        for vol in &self.vols {
            assert!(vol.path.ends_with('/'));
            if path.starts_with(&vol.path) {
                let tail = &path[vol.path.len()..];
                return Ok(Some(self.base_dir.join(&vol.name).join(tail)));
            }
        }
        Ok(None)
    }
}

#[derive(Clone)]
pub struct FdState {
    inner: Rc<RefCell<FdStateInner>>,
}

/*
export declare namespace io {
  type Fd=i32;
  export function wopen(path: string) : Fd;
  export function ropen(path: string) : Fd;
  export function write(fd: Fd, bytes : ArrayBuffer) : i32;
  export function read(fd: Fd, bytes : ArrayBuffer) : i32;
  export function close(fd: Fd) : void;
}
*/

impl FdState {
    fn new(base_dir: PathBuf, vols: Vec<ContainerVolume>) -> Self {
        let inner = Rc::new(RefCell::new(FdStateInner {
            vols,
            base_dir,
            fds: Default::default(),
            n_fds: MIN_FD,
        }));

        FdState { inner }
    }

    fn open_write(&self, path: &str) -> std::io::Result<i32> {
        let mut b = (*self.inner).borrow_mut();
        if let Some(resolved_path) = b.find_path(path)? {
            let f = OpenOptions::new()
                .write(true)
                .create(true)
                .open(resolved_path)?;
            Ok(b.add_fd(Fd(f)))
        } else {
            Ok(-1)
        }
    }

    fn open_read(&self, path: &str) -> std::io::Result<i32> {
        let mut b = (*self.inner).borrow_mut();
        if let Some(resolved_path) = b.find_path(path)? {
            let f = OpenOptions::new().read(true).open(resolved_path)?;
            Ok(b.add_fd(Fd(f)))
        } else {
            Ok(-1)
        }
    }

    fn write(&self, fd: i32, buf: &[u8]) -> std::io::Result<i32> {
        (*self.inner).borrow_mut().exec(fd, |f| f.write(buf))
    }

    fn read(&self, fd: i32, buf: &mut [u8]) -> std::io::Result<i32> {
        (*self.inner).borrow_mut().exec(fd, |f| f.read(buf))
    }

    fn close(&self, fd: i32) {
        let mut b = (*self.inner).borrow_mut();
        let _ = b.remove_fd(fd);
    }
}

pub fn link_io(
    module: &str,
    linker: &mut Linker,
    base_dir: PathBuf,
    vols: Vec<ContainerVolume>,
) -> anyhow::Result<()> {
    let state = FdState::new(base_dir, vols);

    fn decode_result(r: std::io::Result<i32>) -> Result<i32, Trap> {
        match r {
            Ok(v) => Ok(v),
            Err(_) => Ok(-1),
        }
    }

    {
        let state = state.clone();
        linker.func(
            module,
            "io.wopen",
            move |caller: Caller, path_ptr: i32| -> Result<i32, Trap> {
                let mem = AsMem::for_caller(&caller)?;
                let path = mem.decode_str(path_ptr)?;
                decode_result(state.open_write(&path))
            },
        )?;
    }

    {
        let state = state.clone();
        linker.func(
            module,
            "io.ropen",
            move |caller: Caller, path_ptr: i32| -> Result<i32, Trap> {
                let mem = AsMem::for_caller(&caller)?;
                let path = mem.decode_str(path_ptr)?;
                decode_result(state.open_read(&path))
            },
        )?;
    }

    {
        let state = state.clone();
        linker.func(
            module,
            "io.read",
            move |caller: Caller, fd: i32, buffer: i32| -> Result<i32, Trap> {
                let mut mem = AsMem::for_caller(&caller)?;
                unsafe { decode_result(state.read(fd, mem.get_mut_ptr(buffer)?)) }
            },
        )?;
    }

    {
        let state = state.clone();
        linker.func(
            module,
            "io.write",
            move |caller: Caller, fd: i32, buffer: i32| -> Result<i32, Trap> {
                let mem = AsMem::for_caller(&caller)?;
                unsafe { decode_result(state.write(fd, mem.get_ptr(buffer)?)) }
            },
        )?;
    }

    {
        // let state = state.clone();
        linker.func(
            module,
            "io.close",
            move |_caller: Caller, fd: i32| -> Result<(), Trap> {
                state.close(fd);
                Ok(())
            },
        )?;
    }
    Ok(())
}
