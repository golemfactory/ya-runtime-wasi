use secp256k1::{PublicKey, SecretKey};
use std::convert::TryFrom;
use wasmtime::{Caller, Func, Memory, Trap};
mod eth;
mod io;

const ARRAYBUFFER_ID: u32 = 0;
const STRING_ID: u32 = 1;

type Result<T> = std::result::Result<T, Trap>;

pub struct AsMem {
    mem: Memory,
}

impl<'a> TryFrom<Caller<'a>> for AsMem {
    type Error = Trap;

    fn try_from(value: Caller<'a>) -> Result<Self> {
        Self::for_caller(&value)
    }
}

impl AsMem {
    pub fn for_caller(caller: &Caller) -> Result<Self> {
        let mem = caller
            .get_export("memory")
            .ok_or_else(|| Trap::new("missing memory export"))?
            .into_memory()
            .ok_or_else(|| Trap::new("wrong object exported as \"memory\""))?;
        Ok(Self { mem })
    }

    pub fn decode_str(&self, ptr: u32) -> Result<String> {
        let chars: Vec<u16> = unsafe {
            let bytes = self.get_ptr(ptr)?;
            bytes
                .chunks(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .collect()
        };
        Ok(String::from_utf16_lossy(&chars))
    }

    pub fn decode_secret(&self, ptr: u32) -> Result<SecretKey> {
        unsafe { SecretKey::parse_slice(self.get_ptr(ptr)?).map_err(|e| Trap::new(e.to_string())) }
    }

    pub fn decode<T, F: FnOnce(&[u8]) -> Result<T>>(&self, ptr: u32, extractor: F) -> Result<T> {
        unsafe { extractor(self.get_ptr(ptr)?) }
    }

    pub fn decode_hash(&self, ptr: u32) -> Result<eth::EthHash> {
        unsafe {
            eth::EthHash::parse_slice(self.get_ptr(ptr)?)
                .map_err(|e| Trap::new(format!("invalid message hash: {}", e)))
        }
    }

    pub fn decode_pubkey(&self, ptr: u32) -> Result<PublicKey> {
        unsafe {
            PublicKey::parse_slice(self.get_ptr(ptr)?, None).map_err(|e| Trap::new(e.to_string()))
        }
    }

    unsafe fn get_ptr(&self, ptr: u32) -> Result<&[u8]> {
        let ptr = ptr as usize;
        let m = self.mem.data_unchecked();
        if ptr < 4 {
            return Err(Trap::new(format!("invalid allocation pointer: {}", ptr)));
        }
        let b = &m[(ptr - 4)..ptr];
        let size = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
        Ok(&m[ptr..(ptr + size)])
    }

    unsafe fn get_mut_ptr(&mut self, ptr: u32) -> Result<&mut [u8]> {
        let ptr = ptr as usize;
        let m = self.mem.data_unchecked_mut();
        if ptr < 4 {
            return Err(Trap::new(format!("invalid allocation pointer: {}", ptr)));
        }
        let b = &m[(ptr - 4)..ptr];
        let size = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
        Ok(&mut m[ptr..(ptr + size)])
    }
}

pub struct Allocator {
    mem: AsMem,
    f_new: Func,
    f_retain: Func,
}

impl Allocator {
    pub fn for_caller(caller: &Caller) -> Result<Self> {
        let mem = AsMem::for_caller(caller)?;
        let f_new = caller
            .get_export("__new")
            .ok_or_else(|| Trap::new("Missing '__new' export"))?
            .into_func()
            .ok_or_else(|| Trap::new("invalid __new"))?;
        let f_retain = caller
            .get_export("__retain")
            .ok_or_else(|| Trap::new("Missing '__retain' export"))?
            .into_func()
            .ok_or_else(|| Trap::new("invalid __retain"))?;

        Ok(Self {
            mem,
            f_new,
            f_retain,
        })
    }

    pub fn new_bytes_int(&mut self, bytes: &[u8], type_id: u32) -> Result<u32> {
        let ptr: u32 = self.f_new.get2()?(bytes.len() as u32, type_id)?;
        unsafe {
            self.mem.get_mut_ptr(ptr)?.copy_from_slice(bytes);
        }
        Ok(ptr)
    }

    pub fn new_bytes(&mut self, bytes: &[u8]) -> Result<u32> {
        self.new_bytes_int(bytes, ARRAYBUFFER_ID)
    }

    pub fn new_string(&mut self, s: &str) -> Result<u32> {
        let v: Vec<u16> = s.encode_utf16().collect();
        let ptr: u32 = self.f_new.get2()?(v.len() as u32 * 2u32, STRING_ID)?;
        unsafe {
            for (chunk, [c0, c1]) in self
                .mem
                .get_mut_ptr(ptr)?
                .chunks_exact_mut(2)
                .zip(v.into_iter().map(|v| v.to_le_bytes()))
            {
                chunk[0] = c0;
                chunk[1] = c1;
            }
        }
        Ok(ptr)
    }

    pub fn size(&self) -> usize {
        self.mem.mem.data_size() as usize
    }

    pub fn retain(&self, ptr: u32) -> Result<u32> {
        self.f_retain.get1()?(ptr)
    }
}

pub use eth::link_eth;
pub use io::link_io;
