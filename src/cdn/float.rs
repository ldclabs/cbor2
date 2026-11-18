use crate::core::{f16_to_f64, f32_to_f64};
use crate::de::Error;

use super::types::Atom;

pub(super) fn float_atom(bytes: &[u8], offset: usize) -> Result<Atom, Error> {
    let value = match bytes {
        [a, b] => f16_to_f64(u16::from_be_bytes([*a, *b])),
        [a, b, c, d] => f32_to_f64(u32::from_be_bytes([*a, *b, *c, *d])),
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

    // A bit-pattern literal supplies the value. Its encoding indicator, if
    // present, selects the output width just as for a numeric literal.
    Ok(Atom::Float(value))
}
