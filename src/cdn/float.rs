use alloc::vec::Vec;

use crate::core::{f16_to_f64, f64_to_f16};
use crate::de::Error;

use super::types::Atom;

pub(super) fn float_atom(bytes: Vec<u8>, offset: usize) -> Result<Atom, Error> {
    let value = match bytes.as_slice() {
        [a, b] => f16_to_f64_preserving(u16::from_be_bytes([*a, *b])),
        [a, b, c, d] => f32_to_f64_preserving(u32::from_be_bytes([*a, *b, *c, *d])),
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

// The widening conversions below keep NaN payloads bit for bit. The decoder
// (`f16_to_f64`) and numeric casts deliberately canonicalize NaNs following
// RFC 8949 Appendix D, but the `float` extension exists precisely to express
// non-canonical NaNs, so an encoding indicator must re-encode the exact
// payload at the requested width or fail.
fn f16_to_f64_preserving(bits: u16) -> f64 {
    let exp = (bits >> 10) & 0x1f;
    let frac = bits & 0x3ff;
    if exp == 31 && frac != 0 {
        let sign = u64::from(bits >> 15) << 63;
        return f64::from_bits(sign | 0x7ff0_0000_0000_0000 | (u64::from(frac) << 42));
    }
    f16_to_f64(bits)
}

fn f32_to_f64_preserving(bits: u32) -> f64 {
    let value = f32::from_bits(bits);
    if value.is_nan() {
        let sign = u64::from(bits >> 31) << 63;
        let frac = u64::from(bits & 0x007f_ffff) << 29;
        return f64::from_bits(sign | 0x7ff0_0000_0000_0000 | frac);
    }
    f64::from(value)
}

// Narrows to half-precision like `f64_to_f16`, but keeps a NaN sign and
// payload when they fit instead of accepting only the canonical quiet NaN.
pub(super) fn f64_to_f16_preserving(value: f64) -> Option<u16> {
    if value.is_nan() {
        let bits = value.to_bits();
        if bits & ((1 << 42) - 1) != 0 {
            return None;
        }
        let sign = ((bits >> 48) & 0x8000) as u16;
        let frac = ((bits >> 42) & 0x3ff) as u16;
        return Some(sign | 0x7c00 | frac);
    }
    f64_to_f16(value)
}
