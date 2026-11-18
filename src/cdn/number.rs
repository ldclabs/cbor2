use alloc::{format, vec::Vec};

use crate::de::Error;

use super::types::Atom;

pub(super) fn is_app_char_any(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '-'
}

pub(super) fn is_tag_uint(s: &str) -> bool {
    if s == "0" {
        return true;
    }
    let mut chars = s.chars();
    matches!(chars.next(), Some('1'..='9')) && chars.all(|ch| ch.is_ascii_digit())
}

pub(super) fn strip_sign(s: &str) -> (bool, &str) {
    if let Some(rest) = s.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = s.strip_prefix('+') {
        (false, rest)
    } else {
        (false, s)
    }
}

pub(super) fn parse_integer(
    negative: bool,
    digits: &str,
    base: u32,
    offset: usize,
) -> Result<Atom, Error> {
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Ok(Atom::Integer {
            negative: false,
            argument: 0,
        });
    }
    if let Ok(value) = u128::from_str_radix(digits, base) {
        let argument = value - u128::from(negative);
        if let Ok(argument) = u64::try_from(argument) {
            return Ok(Atom::Integer { negative, argument });
        }
        let bytes = argument.to_be_bytes();
        return Ok(Atom::Bignum {
            negative,
            argument: bytes[argument.leading_zeros() as usize / 8..].to_vec(),
        });
    }

    let mut magnitude = Vec::new();
    if matches!(base, 2 | 8 | 16) {
        let width = base.trailing_zeros();
        let mut bits = 0;
        let mut acc = 0u16;
        for ch in digits.chars().rev() {
            let digit = ch.to_digit(base).ok_or(Error::Syntax(offset))? as u16;
            acc |= digit << bits;
            bits += width;
            if bits >= 8 {
                magnitude.push(acc as u8);
                acc >>= 8;
                bits -= 8;
            }
        }
        if acc != 0 {
            magnitude.push(acc as u8);
        }
        magnitude.reverse();
    } else {
        // Base-2^32 limbs, nine decimal digits at a time. No front insertion.
        let mut limbs = Vec::<u32>::new();
        let mut pos = 0;
        let first = match digits.len() % 9 {
            0 => 9,
            n => n,
        };
        while pos < digits.len() {
            let len = if pos == 0 { first } else { 9 };
            let chunk = digits.get(pos..pos + len).ok_or(Error::Syntax(offset))?;
            let mut carry = u64::from(chunk.parse::<u32>().map_err(|_| Error::Syntax(offset))?);
            for limb in &mut limbs {
                let value = u64::from(*limb) * 1_000_000_000 + carry;
                *limb = value as u32;
                carry = value >> 32;
            }
            if carry != 0 {
                limbs.push(carry as u32);
            }
            pos += len;
        }
        for limb in limbs.into_iter().rev() {
            magnitude.extend_from_slice(&limb.to_be_bytes());
        }
    }
    if negative {
        for byte in magnitude.iter_mut().rev() {
            let (next, borrow) = byte.overflowing_sub(1);
            *byte = next;
            if !borrow {
                break;
            }
        }
    }
    let first = magnitude
        .iter()
        .position(|&b| b != 0)
        .unwrap_or(magnitude.len());
    magnitude.drain(..first);
    Ok(Atom::Bignum {
        negative,
        argument: magnitude,
    })
}

pub(super) fn parse_hex_float(lex: &str, offset: usize) -> Result<f64, Error> {
    let (negative, rest) = strip_sign(lex);
    let rest = rest
        .strip_prefix("0x")
        .or_else(|| rest.strip_prefix("0X"))
        .ok_or(Error::Syntax(offset))?;
    let (mantissa, exp) = rest.split_once(['p', 'P']).ok_or(Error::Syntax(offset))?;
    let (exp_negative, exp_digits) = strip_sign(exp);
    let mut exponent = 0i64;
    for byte in exp_digits.bytes() {
        if !byte.is_ascii_digit() {
            return Err(Error::Syntax(offset));
        }
        exponent = exponent
            .saturating_mul(10)
            .saturating_add(i64::from(byte - b'0'));
    }
    if exp_negative {
        exponent = -exponent;
    }
    let frac_digits = mantissa.split_once('.').map_or(0, |(_, frac)| frac.len());
    let scale = exponent.saturating_sub((frac_digits as i64).saturating_mul(4));
    // Keep 53 significant bits, one rounding bit and a sticky bit. Never
    // perform floating arithmetic before the final IEEE 754 representation.
    let mut significand = 0u64;
    let mut length = 0i64;
    let mut kept = 0u32;
    let mut sticky = false;
    for ch in mantissa.chars().filter(|&ch| ch != '.') {
        let digit = ch.to_digit(16).ok_or(Error::Syntax(offset))?;
        for bit in (0..4).rev() {
            let one = (digit >> bit) & 1;
            if length == 0 && one == 0 {
                continue;
            }
            length += 1;
            if kept < 54 {
                significand = (significand << 1) | u64::from(one);
                kept += 1;
            } else {
                sticky |= one != 0;
            }
        }
    }
    let sign = u64::from(negative) << 63;
    if length == 0 {
        return Ok(f64::from_bits(sign));
    }
    let mut exponent = scale.saturating_add(length - 1);
    let overflow = || Error::semantic(offset, format!("hex float `{lex}` overflows the f64 range"));
    if exponent > 1023 {
        return Err(overflow());
    }
    if exponent < -1075 {
        return Ok(f64::from_bits(sign));
    }
    let precision = if exponent >= -1022 {
        53
    } else {
        (exponent + 1075) as u32
    };
    let mut rounded = if kept <= precision {
        significand << (precision - kept)
    } else {
        let shift = kept - precision;
        let head = significand >> shift;
        let guard = (significand >> (shift - 1)) & 1;
        let tail = significand & ((1u64 << (shift - 1)) - 1);
        head + u64::from(guard != 0 && (sticky || tail != 0 || head & 1 != 0))
    };
    if exponent < -1022 {
        return Ok(f64::from_bits(sign | rounded));
    }
    if rounded == 1 << 53 {
        rounded >>= 1;
        exponent += 1;
        if exponent > 1023 {
            return Err(overflow());
        }
    }
    Ok(f64::from_bits(
        sign | (((exponent + 1023) as u64) << 52) | (rounded & ((1 << 52) - 1)),
    ))
}
