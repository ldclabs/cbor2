use alloc::{string::String, vec::Vec};

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
pub(super) struct BigInt {
    pub(super) negative: bool,
    pub(super) magnitude: Vec<u8>,
}

#[derive(Clone, Debug)]
pub(super) enum Atom {
    Integer(BigInt),
    Float(f64),
    FloatRaw { bytes: Vec<u8>, value: f64 },
    Bytes(Vec<u8>),
    Text(String),
    Simple(u8),
    Raw(Vec<u8>),
}

pub(super) struct Arg {
    pub(super) encoded: Vec<u8>,
    pub(super) value: Value,
}

pub(super) const ELLIPSIS_TAG: u64 = 888;
pub(super) const UNRESOLVED_APP_TAG: u64 = 999;
#[cfg(feature = "cdn")]
pub(super) const CRI_TAG: u64 = 99;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ElidedStringPart {
    Bytes(Vec<u8>),
    Text(String),
    Ellipsis,
}
