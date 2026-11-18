use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use crate::value::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Indicator<'a> {
    None,
    Indefinite,
    Immediate,
    Ai(u8),
    Other(&'a str),
}

#[derive(Clone, Debug)]
pub(super) enum Atom {
    // For negative integers the CBOR argument is -1 - value.
    Integer { negative: bool, argument: u64 },
    Bignum { negative: bool, argument: Vec<u8> },
    Float(f64),
    FloatRaw { bytes: Vec<u8>, value: f64 },
    Bytes(Vec<u8>),
    Text(String),
    Simple(u8),
    Raw(Vec<u8>),
}

// Decode only when an extension needs the data model; unresolved extensions
// and byte-oriented consumers can carry the exact encoding without a Value.
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

pub(super) const ELLIPSIS_TAG: u64 = 888;
pub(super) const UNRESOLVED_APP_TAG: u64 = 999;
#[cfg(feature = "cdn-cri")]
pub(super) const CRI_TAG: u64 = 99;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ElidedStringPart {
    Bytes(Vec<u8>),
    Text(String),
    Ellipsis,
}
