use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use crate::value::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Indicator {
    None,
    Indefinite,
    Immediate,
    Ai(u8),
    Reserved,
}

#[derive(Clone, Debug)]
pub(super) enum Atom {
    // For negative integers the CBOR argument is -1 - value.
    Integer { negative: bool, argument: u64 },
    Bignum { negative: bool, argument: Vec<u8> },
    Float(f64),
    Bytes(Vec<u8>),
    Text(String),
    Raw(Vec<u8>),
}

// Keep argument bytes for extensions that preserve the original encoding.
pub(super) enum Arg {
    Encoded(Vec<u8>),
    Text(String),
}

impl Arg {
    pub(super) fn into_encoded(self) -> Result<Vec<u8>, crate::de::Error> {
        match self {
            Self::Encoded(bytes) => Ok(bytes),
            Self::Text(text) => crate::to_vec(&text)
                .map_err(|err| crate::de::Error::semantic(None, err.to_string())),
        }
    }
    pub(super) fn into_value(self) -> Result<Value, crate::de::Error> {
        match self {
            Self::Encoded(bytes) => crate::de::value_from_slice(&bytes),
            Self::Text(text) => Ok(Value::Text(text)),
        }
    }
}

#[cfg(feature = "cdn-cri")]
pub(super) const CRI_TAG: u64 = 99;
