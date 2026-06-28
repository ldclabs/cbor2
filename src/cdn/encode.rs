use alloc::vec::Vec;

use crate::de::Error;

use super::types::{ElidedStringPart, Indicator, ELLIPSIS_TAG};

pub(super) fn write_uint(
    out: &mut Vec<u8>,
    major: u8,
    value: u64,
    spec: Indicator<'_>,
) -> Result<(), &'static str> {
    let prefix = major << 5;
    match spec {
        Indicator::None | Indicator::Other(..) | Indicator::Indefinite => {
            write_preferred_uint(out, major, value);
        }
        Indicator::Immediate => {
            if value > 23 {
                return Err("value does not fit `_i` encoding indicator");
            }
            out.push(prefix | value as u8);
        }
        Indicator::Ai(0) => {
            let value =
                u8::try_from(value).map_err(|_| "value does not fit `_0` encoding indicator")?;
            out.extend_from_slice(&[prefix | 24, value]);
        }
        Indicator::Ai(1) => {
            let value =
                u16::try_from(value).map_err(|_| "value does not fit `_1` encoding indicator")?;
            out.push(prefix | 25);
            out.extend_from_slice(&value.to_be_bytes());
        }
        Indicator::Ai(2) => {
            let value =
                u32::try_from(value).map_err(|_| "value does not fit `_2` encoding indicator")?;
            out.push(prefix | 26);
            out.extend_from_slice(&value.to_be_bytes());
        }
        Indicator::Ai(3) => {
            out.push(prefix | 27);
            out.extend_from_slice(&value.to_be_bytes());
        }
        Indicator::Ai(_) => return Err("unsupported encoding indicator"),
    }
    Ok(())
}

fn write_preferred_uint(out: &mut Vec<u8>, major: u8, value: u64) {
    let prefix = major << 5;
    match value {
        x if x <= 23 => out.push(prefix | x as u8),
        x if x <= u8::MAX as u64 => out.extend_from_slice(&[prefix | 24, x as u8]),
        x if x <= u16::MAX as u64 => {
            out.push(prefix | 25);
            out.extend_from_slice(&(x as u16).to_be_bytes());
        }
        x if x <= u32::MAX as u64 => {
            out.push(prefix | 26);
            out.extend_from_slice(&(x as u32).to_be_bytes());
        }
        x => {
            out.push(prefix | 27);
            out.extend_from_slice(&x.to_be_bytes());
        }
    }
}

pub(super) fn write_definite_bytes(
    out: &mut Vec<u8>,
    bytes: &[u8],
    spec: Indicator<'_>,
) -> Result<(), Error> {
    write_uint(out, 2, bytes.len() as u64, spec).map_err(|msg| Error::semantic(None, msg))?;
    out.extend_from_slice(bytes);
    Ok(())
}

pub(super) fn write_definite_text(
    out: &mut Vec<u8>,
    text: &str,
    spec: Indicator<'_>,
) -> Result<(), Error> {
    write_uint(out, 3, text.len() as u64, spec).map_err(|msg| Error::semantic(None, msg))?;
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

pub(super) fn write_array_len(out: &mut Vec<u8>, len: usize) -> Result<(), Error> {
    write_uint(out, 4, len as u64, Indicator::None).map_err(|msg| Error::semantic(None, msg))
}

pub(super) fn write_tag(out: &mut Vec<u8>, tag: u64) -> Result<(), Error> {
    write_uint(out, 6, tag, Indicator::None).map_err(|msg| Error::semantic(None, msg))
}

fn write_null(out: &mut Vec<u8>) {
    out.push(0xf6);
}

pub(super) fn ellipsis_item() -> Vec<u8> {
    let mut out = Vec::new();
    // These tags are still CPA placeholders in draft-ietf-cbor-edn-literals-26.
    write_tag(&mut out, ELLIPSIS_TAG).expect("CPA tag is encodable");
    write_null(&mut out);
    out
}

pub(super) fn write_elided_string(
    out: &mut Vec<u8>,
    parts: &[ElidedStringPart],
) -> Result<(), Error> {
    write_tag(out, ELLIPSIS_TAG)?;
    write_array_len(out, parts.len())?;
    for part in parts {
        match part {
            ElidedStringPart::Bytes(bytes) => {
                write_definite_bytes(out, bytes, Indicator::None)?;
            }
            ElidedStringPart::Text(text) => {
                write_definite_text(out, text, Indicator::None)?;
            }
            ElidedStringPart::Ellipsis => {
                out.extend_from_slice(&ellipsis_item());
            }
        }
    }
    Ok(())
}
