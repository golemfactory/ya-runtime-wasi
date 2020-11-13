use super::{Allocator, AsMem};
use secp256k1::SharedSecret;
pub use secp256k1::{Error, Message, PublicKey, SecretKey};
use std::convert::TryInto;
use std::fmt;
use std::fmt::Debug;
use tiny_keccak::{Hasher, Keccak};
use wasmtime::{Caller, Linker, Trap};

pub struct EthHash([u8; 32]);

impl EthHash {
    pub fn parse_slice(bytes: &[u8]) -> anyhow::Result<Self> {
        Ok(EthHash(bytes.try_into()?))
    }

    pub fn personal_message(message: impl AsRef<[u8]>) -> EthHash {
        let message = message.as_ref();
        let msg_size = message.len().to_string();
        let prefix = b"\x19Ethereum Signed Message:\n";
        eth_hash_parts(&[prefix.as_ref(), msg_size.as_ref(), message])
    }

    pub fn build_with(signature: &str) -> EthHashBuilder {
        let sig = signature_hash(signature);
        let mut hasher = Keccak::v256();
        hasher.update(sig.as_ref());
        EthHashBuilder(hasher)
    }

    pub fn sign_by(&self, secret: &SecretKey) -> RecoverableSignature {
        let message = Message::parse(&self.0);
        let (signature, recovery_id) = secp256k1::sign(&message, secret);
        RecoverableSignature {
            signature,
            recovery_id,
        }
    }
}

impl AsRef<[u8]> for EthHash {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::LowerHex for EthHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

pub fn signature_hash(signature: &str) -> EthHash {
    eth_hash_parts(&[signature.as_bytes()])
}

pub struct EthHashBuilder(Keccak);

impl EthHashBuilder {
    pub fn add(mut self, content: impl AsRef<[u8]>) -> Self {
        self.0.update(content.as_ref());
        self
    }

    pub fn build(self) -> EthHash {
        let mut bytes = [0; 32];
        self.0.finalize(&mut bytes[..]);
        EthHash(bytes)
    }
}

#[derive(Eq, PartialEq, Hash)]
pub struct EthAddress([u8; 20]);

impl AsRef<[u8]> for EthAddress {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8; 20]> for EthAddress {
    fn as_ref(&self) -> &[u8; 20] {
        &self.0
    }
}

pub trait ToEthAddress {
    fn to_eth_address(&self) -> EthAddress;
}

impl fmt::LowerHex for EthAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", hex::encode(&self.0[..]))
    }
}

impl Debug for EthAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:x}", self)
    }
}

impl EthAddress {
    pub fn new(inner: [u8; 20]) -> Self {
        EthAddress(inner)
    }

    pub fn to_hex_string(&self) -> String {
        format!("{:x}", self)
    }

    pub fn from_hex(bytes: impl AsRef<[u8]>) -> Result<Self, hex::FromHexError> {
        let mut inner = [0; 20];
        hex::decode_to_slice(bytes.as_ref(), &mut inner[..])?;
        Ok(EthAddress(inner))
    }

    pub fn to_array(&self) -> [u8; 20] {
        self.0
    }
}

fn eth_hash_parts(chunks: &[impl AsRef<[u8]>]) -> EthHash {
    let mut hasher = Keccak::v256();
    for chunk in chunks {
        hasher.update(chunk.as_ref());
    }
    let mut hash_bytes = [0u8; 32];
    hasher.finalize(&mut hash_bytes[..]);
    EthHash(hash_bytes)
}

impl ToEthAddress for PublicKey {
    fn to_eth_address(&self) -> EthAddress {
        let bytes = self.serialize();
        let hash = eth_hash_parts(&[&bytes[1..]]);
        let mut address = [0; 20];
        address.copy_from_slice(&hash.0[12..]);
        EthAddress(address)
    }
}

impl ToEthAddress for SecretKey {
    fn to_eth_address(&self) -> EthAddress {
        PublicKey::from_secret_key(self).to_eth_address()
    }
}

pub struct RecoverableSignature {
    signature: secp256k1::Signature,
    recovery_id: secp256k1::RecoveryId,
}

impl RecoverableSignature {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != 65 {
            return Err(Error::InvalidInputLength);
        }

        let signature = secp256k1::Signature::parse_slice(&bytes[..64])?;
        let r = bytes[64];
        let recovery_id = if r >= 0x1b {
            secp256k1::RecoveryId::parse_rpc(bytes[64])?
        } else {
            secp256k1::RecoveryId::parse(bytes[64])?
        };

        Ok(Self {
            signature,
            recovery_id,
        })
    }

    pub fn to_hex(&self) -> String {
        let sig = self.signature.serialize();
        let r = self.recovery_id.serialize();
        format!("{}{:02x}", hex::encode(sig.as_ref()), r)
    }

    pub fn serialize_rpc(&self) -> [u8; 65] {
        let mut output = [0u8; 65];
        output[0..64].copy_from_slice(&self.signature.serialize());
        output[64] = (self.recovery_id.serialize() & 1) + 27;
        output
    }

    pub fn from_hex(mut hex: &str) -> Result<Self, Error> {
        if hex.starts_with("0x") {
            hex = &hex[2..];
        }
        Self::from_bytes(&hex::decode(hex).map_err(|_| Error::InvalidSignature)?)
    }

    pub fn recover_pub_key(&self, message_hash: &EthHash) -> Result<PublicKey, Error> {
        let message = Message::parse(&message_hash.0);

        secp256k1::recover(&message, &self.signature, &self.recovery_id)
    }
}

pub fn link_eth(module: &str, linker: &mut Linker) -> anyhow::Result<()> {
    linker.func(
        module,
        "eth.newKey",
        |caller: Caller| -> Result<i32, Trap> {
            let mut a = Allocator::for_caller(&caller)?;
            let secret = secp256k1::SecretKey::random(&mut rand::thread_rng());
            let ptr = a.new_bytes(secret.serialize().as_ref())?;
            //eprintln!("heap: {}", a.size());
            Ok(a.retain(ptr)?)
            //Ok(ptr)
        },
    )?;

    linker.func(
        module,
        "eth.prvToAddress",
        |caller: Caller, ptr: i32| -> Result<i32, Trap> {
            let mem = AsMem::for_caller(&caller)?;
            let secret = mem.decode_secret(ptr)?;
            let mut a = Allocator::for_caller(&caller)?;
            let ptr = a.new_string(&secret.to_eth_address().to_hex_string())?;
            a.retain(ptr)?;
            Ok(ptr)
        },
    )?;

    linker.func(
        module,
        "eth.pubToAddress",
        |caller: Caller, ptr: i32| -> Result<i32, Trap> {
            let mem = AsMem::for_caller(&caller)?;
            let public_key = mem.decode_pubkey(ptr)?;
            let mut allocator = Allocator::for_caller(&caller)?;
            let ptr = allocator.new_string(&public_key.to_eth_address().to_hex_string())?;
            allocator.retain(ptr)?;
            Ok(ptr)
        },
    )?;

    //export function sign(pk: ArrayBuffer, messageHash: ArrayBuffer): ArrayBuffer;
    linker.func(
        module,
        "eth.sign",
        |caller: Caller, pk_ptr: i32, hash_ptr: i32| -> Result<i32, Trap> {
            let mem = AsMem::for_caller(&caller)?;
            let pk = mem.decode_secret(pk_ptr)?;
            let hash = mem.decode_hash(hash_ptr)?;
            let bytes = hash.sign_by(&pk).serialize_rpc();
            let mut allocator = Allocator::for_caller(&caller)?;
            let out_ptr = allocator.new_bytes(&bytes)?;
            allocator.retain(out_ptr)?;
            Ok(out_ptr)
        },
    )?;

    //export function keccak256(bytes : ArrayBuffer): ArrayBuffer;
    linker.func(
        module,
        "eth.keccak256",
        |caller: Caller, ptr: i32| -> Result<i32, Trap> {
            let mem = AsMem::for_caller(&caller)?;
            let hash = mem.decode(ptr, |slice| Ok(eth_hash_parts(&[slice])))?;
            let mut allocator = Allocator::for_caller(&caller)?;
            let out_ptr = allocator.new_bytes(hash.as_ref())?;
            allocator.retain(out_ptr)?;
            Ok(out_ptr)
        },
    )?;

    //
    // messageHash - 32 message hash,
    // signarure - 32 bytes R, 32 bytes S, 1 byte recovery tag.
    // returns 64 bytes public key.
    //export function ecrecover(messageHash: ArrayBuffer, signature: ArrayBuffer): ArrayBuffer;
    linker.func(
        module,
        "eth.ecrecover",
        |caller: Caller, hash_ptr: i32, sig_ptr: i32| -> Result<i32, Trap> {
            let mem = AsMem::for_caller(&caller)?;
            let hash = mem.decode_hash(hash_ptr)?;
            let sign = mem.decode(sig_ptr, |bytes| {
                RecoverableSignature::from_bytes(bytes)
                    .map_err(|e| Trap::new(format!("unable to parse signature: {}", e)))
            })?;
            let pub_key = sign
                .recover_pub_key(&hash)
                .map_err(|e| Trap::new(e.to_string()))?;
            let mut allocator = Allocator::for_caller(&caller)?;
            let out_ptr = allocator.new_bytes(pub_key.serialize().as_ref())?;
            allocator.retain(out_ptr)?;
            Ok(out_ptr)
        },
    )?;
    //export function bytesToHex(bytes: ArrayBuffer): string;
    linker.func(
        module,
        "eth.bytesToHex",
        |caller: Caller, bytes_ptr: i32| -> Result<i32, Trap> {
            let mem = AsMem::for_caller(&caller)?;
            let hex_str = mem.decode(bytes_ptr, |bytes| Ok(hex::encode(bytes)))?;
            let mut allocator = Allocator::for_caller(&caller)?;
            let out_ptr = allocator.new_string(&hex_str)?;
            allocator.retain(out_ptr)?;
            Ok(out_ptr)
        },
    )?;

    //export function sharedSecret(prvKey: ArrayBuffer, pubKey: ArrayBuffer): ArrayBuffer;
    linker.func(
        module,
        "eth.sharedSecret",
        |caller: Caller, prv_ptr: i32, pub_ptr: i32| -> Result<i32, Trap> {
            let mem = AsMem::for_caller(&caller)?;
            let secret = mem.decode_secret(prv_ptr)?;
            let pubkey = mem.decode_pubkey(pub_ptr)?;
            let shared_secret: SharedSecret<sha2::Sha256> =
                SharedSecret::new(&pubkey, &secret).map_err(|e| Trap::new(e.to_string()))?;
            let mut allocator = Allocator::for_caller(&caller)?;
            let out_ptr = allocator.new_bytes(shared_secret.as_ref())?;
            allocator.retain(out_ptr)?;
            Ok(out_ptr)
        },
    )?;

    Ok(())
}
