use alloc::vec::Vec;

use crate::core::f16_to_f64;
use crate::de::Error;

use super::types::Atom;

pub(super) fn float_atom(bytes: Vec<u8>, offset: usize) -> Result<Atom, Error> {
    let value = match bytes.as_slice() {
        [a, b] => f16_to_f64(u16::from_be_bytes([*a, *b])),
        [a, b, c, d] => f32::from_bits(u32::from_be_bytes([*a, *b, *c, *d])) as f64,
        [a, b, c, d, e, f, g, h] => {
            f64::from_bits(u64::from_be_bytes([*a, *b, *c, *d, *e, *f, *g, *h]))
        }
        _ => {
            return Err(Error::semantic(
                offset,
                "float extension requires 2, 4, or 8 bytes",
            ));
        }
    };

    let mut raw = Vec::new();
    raw.push(match bytes.len() {
        2 => 0xf9,
        4 => 0xfa,
        _ => 0xfb,
    });
    raw.extend_from_slice(&bytes);
    Ok(Atom::FloatRaw { bytes: raw, value })
}
