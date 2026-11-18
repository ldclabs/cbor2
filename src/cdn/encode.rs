use alloc::vec::Vec;

use crate::de::Error;

use super::types::Indicator;

pub(super) fn write_uint(
    out: &mut Vec<u8>,
    major: u8,
    value: u64,
    spec: Indicator,
) -> Result<(), &'static str> {
    let prefix = major << 5;
    match spec {
        Indicator::None | Indicator::Reserved | Indicator::Indefinite => {
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
    crate::core::Encoder::from(out)
        .push_uint(major, value)
        .expect("Vec writes are infallible");
}

// Writes a small integer (major type 0 or 1) in preferred form. Callers only
// pass values well inside the CBOR integer range: epochs, flags and ports.
pub(super) fn write_int(out: &mut Vec<u8>, value: i128) {
    debug_assert!(
        (-(u64::MAX as i128) - 1..=u64::MAX as i128).contains(&value),
        "write_int value {value} is outside the CBOR integer range"
    );
    match u64::try_from(value) {
        Ok(n) => write_preferred_uint(out, 0, n),
        Err(..) => write_preferred_uint(out, 1, (-1 - value) as u64),
    }
}

pub(super) fn write_definite_bytes(
    out: &mut Vec<u8>,
    bytes: &[u8],
    spec: Indicator,
) -> Result<(), Error> {
    write_uint(out, 2, bytes.len() as u64, spec).map_err(|msg| Error::semantic(None, msg))?;
    out.extend_from_slice(bytes);
    Ok(())
}

pub(super) fn write_definite_text(
    out: &mut Vec<u8>,
    text: &str,
    spec: Indicator,
) -> Result<(), Error> {
    write_uint(out, 3, text.len() as u64, spec).map_err(|msg| Error::semantic(None, msg))?;
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

#[cfg(feature = "cdn-cri")]
pub(super) fn write_array_len(out: &mut Vec<u8>, len: usize) -> Result<(), Error> {
    write_uint(out, 4, len as u64, Indicator::None).map_err(|msg| Error::semantic(None, msg))
}

pub(super) fn write_tag(out: &mut Vec<u8>, tag: u64) -> Result<(), Error> {
    write_uint(out, 6, tag, Indicator::None).map_err(|msg| Error::semantic(None, msg))
}
