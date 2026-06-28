use alloc::{format, vec, vec::Vec};

use crate::de::Error;
use crate::value::Value;

use super::types::{Arg, Atom};

#[cfg(feature = "cdn")]
pub(super) fn hash_args(
    args: Vec<Arg>,
    offset: usize,
) -> Result<(Vec<u8>, Option<HashAlg>), Error> {
    if !(1..=2).contains(&args.len()) {
        return Err(Error::semantic(
            offset,
            "hash expects one string and an optional algorithm",
        ));
    }

    let mut iter = args.into_iter();
    let data = match iter.next().unwrap().value {
        Value::Text(text) => text.into_bytes(),
        Value::Bytes(bytes) => bytes,
        _ => return Err(Error::semantic(offset, "hash input must be a string")),
    };

    let alg = match iter.next() {
        Some(arg) => Some(hash_alg_from_value(arg.value, offset)?),
        None => None,
    };
    Ok((data, alg))
}

#[cfg(feature = "cdn")]
#[derive(Clone, Copy)]
pub(super) enum HashAlg {
    Sha256,
    Sha256_64,
    Sha512_256,
    Sha384,
    Sha512,
    Shake128,
    Shake256,
}

#[cfg(feature = "cdn")]
fn hash_alg_from_value(value: Value, offset: usize) -> Result<HashAlg, Error> {
    match value {
        Value::Integer(n) => hash_alg_from_id(i128::from(n), offset),
        Value::Text(name) => hash_alg_from_name(&name, offset),
        _ => Err(Error::semantic(
            offset,
            "hash algorithm must be an integer or text string",
        )),
    }
}

#[cfg(feature = "cdn")]
fn hash_alg_from_id(id: i128, offset: usize) -> Result<HashAlg, Error> {
    match id {
        -15 => Ok(HashAlg::Sha256_64),
        -16 => Ok(HashAlg::Sha256),
        -17 => Ok(HashAlg::Sha512_256),
        -18 => Ok(HashAlg::Shake128),
        -43 => Ok(HashAlg::Sha384),
        -44 => Ok(HashAlg::Sha512),
        -45 => Ok(HashAlg::Shake256),
        _ => Err(Error::semantic(
            offset,
            format!("unsupported COSE hash algorithm `{id}`"),
        )),
    }
}

#[cfg(feature = "cdn")]
fn hash_alg_from_name(name: &str, offset: usize) -> Result<HashAlg, Error> {
    match name {
        "SHA-256" => Ok(HashAlg::Sha256),
        "SHA-256/64" => Ok(HashAlg::Sha256_64),
        "SHA-512/256" => Ok(HashAlg::Sha512_256),
        "SHA-384" => Ok(HashAlg::Sha384),
        "SHA-512" => Ok(HashAlg::Sha512),
        "SHAKE128" | "SHAKE-128" => Ok(HashAlg::Shake128),
        "SHAKE256" | "SHAKE-256" => Ok(HashAlg::Shake256),
        _ => Err(Error::semantic(
            offset,
            format!("unsupported COSE hash algorithm `{name}`"),
        )),
    }
}

#[cfg(feature = "cdn")]
pub(super) fn hash_atom(data: Vec<u8>, alg: Option<HashAlg>, offset: usize) -> Result<Atom, Error> {
    use sha2::{Digest, Sha256, Sha384, Sha512, Sha512_256};
    use sha3::{
        digest::{ExtendableOutput, Update, XofReader},
        Shake128, Shake256,
    };

    let alg = alg.unwrap_or(HashAlg::Sha256);
    let bytes = match alg {
        HashAlg::Sha256 => Sha256::digest(&data).to_vec(),
        HashAlg::Sha256_64 => Sha256::digest(&data)[..8].to_vec(),
        HashAlg::Sha512_256 => Sha512_256::digest(&data).to_vec(),
        HashAlg::Sha384 => Sha384::digest(&data).to_vec(),
        HashAlg::Sha512 => Sha512::digest(&data).to_vec(),
        HashAlg::Shake128 => {
            let mut hasher = Shake128::default();
            hasher.update(&data);
            let mut reader = hasher.finalize_xof();
            let mut out = vec![0; 32];
            reader.read(&mut out);
            out
        }
        HashAlg::Shake256 => {
            let mut hasher = Shake256::default();
            hasher.update(&data);
            let mut reader = hasher.finalize_xof();
            let mut out = vec![0; 64];
            reader.read(&mut out);
            out
        }
    };

    if bytes.is_empty() {
        return Err(Error::Syntax(offset));
    }
    Ok(Atom::Bytes(bytes))
}
