use alloc::{format, vec::Vec};

use crate::de::Error;

use super::types::BigInt;

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

pub(super) fn parse_bigint_digits(
    negative: bool,
    digits: &str,
    base: u32,
    offset: usize,
) -> Result<BigInt, Error> {
    let mut magnitude = Vec::new();
    for ch in digits.chars() {
        let digit = ch.to_digit(base).ok_or(Error::Syntax(offset))?;
        mul_add(&mut magnitude, base, digit);
    }
    strip_leading_zeroes(&mut magnitude);
    Ok(BigInt {
        negative: negative && !magnitude.is_empty(),
        magnitude,
    })
}

fn mul_add(bytes: &mut Vec<u8>, base: u32, digit: u32) {
    let mut carry = digit;
    for byte in bytes.iter_mut().rev() {
        let x = u32::from(*byte) * base + carry;
        *byte = x as u8;
        carry = x >> 8;
    }
    while carry > 0 {
        bytes.insert(0, carry as u8);
        carry >>= 8;
    }
}

fn strip_leading_zeroes(bytes: &mut Vec<u8>) {
    let keep = bytes
        .iter()
        .position(|&byte| byte != 0)
        .unwrap_or(bytes.len());
    if keep > 0 {
        bytes.drain(..keep);
    }
}

pub(super) fn bytes_to_u64(bytes: &[u8]) -> Option<u64> {
    if bytes.len() > 8 {
        return None;
    }
    let mut value = 0u64;
    for &byte in bytes {
        value = (value << 8) | u64::from(byte);
    }
    Some(value)
}

pub(super) fn subtract_one(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    for byte in out.iter_mut().rev() {
        let (next, borrow) = byte.overflowing_sub(1);
        *byte = next;
        if !borrow {
            break;
        }
    }
    strip_leading_zeroes(&mut out);
    out
}

pub(super) fn parse_hex_float(lex: &str, offset: usize) -> Result<f64, Error> {
    let (negative, rest) = strip_sign(lex);
    let rest = rest
        .strip_prefix("0x")
        .or_else(|| rest.strip_prefix("0X"))
        .ok_or(Error::Syntax(offset))?;
    let (mantissa, exp) = rest.split_once(['p', 'P']).ok_or(Error::Syntax(offset))?;
    let exponent: i32 = exp
        .parse()
        .map_err(|_| Error::semantic(offset, format!("invalid hex float exponent `{exp}`")))?;

    let mut value = 0.0f64;
    let mut frac_digits = 0i32;
    let mut after_point = false;
    for ch in mantissa.chars() {
        if ch == '.' {
            if after_point {
                return Err(Error::Syntax(offset));
            }
            after_point = true;
            continue;
        }
        let digit = ch.to_digit(16).ok_or(Error::Syntax(offset))?;
        value = value * 16.0 + f64::from(digit);
        if after_point {
            frac_digits += 1;
        }
    }

    value *= exp2(exponent - 4 * frac_digits);
    if negative {
        value = -value;
    }
    Ok(value)
}

fn exp2(n: i32) -> f64 {
    if n < -1074 {
        return 0.0;
    }
    if n > 1023 {
        return f64::INFINITY;
    }
    if n >= -1022 {
        f64::from_bits(((n + 1023) as u64) << 52)
    } else {
        f64::from_bits(1u64 << (n + 1074))
    }
}

pub(super) fn f64_to_f32_bits(value: f64) -> Option<u32> {
    if value.is_nan() {
        let sign = ((value.to_bits() >> 32) & 0x8000_0000) as u32;
        return Some(sign | 0x7fc0_0000);
    }
    let f = value as f32;
    if (f as f64).to_bits() == value.to_bits() {
        Some(f.to_bits())
    } else {
        None
    }
}

impl BigInt {
    pub(super) fn from_i128(value: i128) -> Self {
        let negative = value < 0;
        let magnitude = if negative {
            value.unsigned_abs()
        } else {
            value as u128
        };
        let mut bytes = magnitude.to_be_bytes().to_vec();
        strip_leading_zeroes(&mut bytes);
        Self {
            negative: negative && !bytes.is_empty(),
            magnitude: bytes,
        }
    }
}
