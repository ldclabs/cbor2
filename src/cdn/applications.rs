use alloc::{format, string::String, vec::Vec};

use crate::de::Error;
use crate::value::Value;

use super::types::{Arg, Atom};

pub(super) fn concat_strings(prefix: &str, args: Vec<Arg>, offset: usize) -> Result<Atom, Error> {
    let mut out = Vec::new();
    for arg in args {
        match arg.into_value()? {
            Value::Text(text) => out.extend_from_slice(text.as_bytes()),
            Value::Bytes(bytes) => out.extend_from_slice(&bytes),
            _ => {
                return Err(Error::semantic(
                    offset,
                    format!("{prefix} arguments must be strings"),
                ))
            }
        }
    }
    if prefix == "t1" {
        String::from_utf8(out)
            .map(Atom::Text)
            .map_err(|_| Error::semantic(offset, "t1 result is not UTF-8"))
    } else {
        Ok(Atom::Bytes(out))
    }
}

pub(super) fn same_args(args: Vec<Arg>, offset: usize) -> Result<Atom, Error> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Err(Error::semantic(
            offset,
            "same expects at least one argument",
        ));
    };

    let encoded = first.into_encoded()?;
    let first = crate::de::value_from_slice(&encoded)?;
    for arg in args {
        if !values_same(&first, &arg.into_value()?) {
            return Err(Error::semantic(offset, "same arguments are not equal"));
        }
    }

    // `same` checks data model equality but preserves the first spelling.
    Ok(Atom::Raw(encoded))
}

fn values_same(left: &Value, right: &Value) -> bool {
    values_equal(left, right, false)
}

fn values_equal(left: &Value, right: &Value, key: bool) -> bool {
    match (left, right) {
        (Value::Integer(left), Value::Integer(right)) => left == right,
        (Value::Bytes(left), Value::Bytes(right)) => left == right,
        (Value::Float(left), Value::Float(right)) => {
            if key {
                left == right
                    || (left.is_nan()
                        && right.is_nan()
                        && (left.to_bits() & 0x000f_ffff_ffff_ffff)
                            == (right.to_bits() & 0x000f_ffff_ffff_ffff))
            } else {
                left.to_bits() == right.to_bits()
            }
        }
        (Value::Text(left), Value::Text(right)) => left == right,
        (Value::Bool(left), Value::Bool(right)) => left == right,
        (Value::Null, Value::Null) => true,
        (Value::Tag(left_tag, left), Value::Tag(right_tag, right)) => {
            left_tag == right_tag && values_equal(left, right, key)
        }
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right.iter())
                    .all(|(left, right)| values_equal(left, right, key))
        }
        (Value::Map(left), Value::Map(right)) => {
            if left.len() != right.len() {
                return false;
            }
            let mut matched = alloc::vec![false; right.len()];
            left.iter().all(|(left_key, value)| {
                let found = right.iter().enumerate().position(|(i, (k, v))| {
                    !matched[i] && values_equal(left_key, k, true) && values_equal(value, v, key)
                });
                if let Some(i) = found {
                    matched[i] = true;
                    true
                } else {
                    false
                }
            })
        }
        (Value::Simple(left), Value::Simple(right)) => left == right,
        _ => false,
    }
}

pub(super) fn hex_content(mut text: &str, offset: usize) -> Result<Vec<u8>, Error> {
    let mut bytes = Vec::new();
    let mut high = None;
    while let Some(&byte) = text.as_bytes().first() {
        match byte {
            b' ' | b'\n' => text = &text[1..],
            b'#' => {
                let (comment, rest) = text.split_once('\n').unwrap_or((text, ""));
                if comment.bytes().any(|b| b < b' ') {
                    return Err(Error::Syntax(offset));
                }
                text = rest;
            }
            b'/' => {
                // Inside hex, only /text/ and # comments are available.
                let (comment, rest) = text[1..].split_once('/').ok_or(Error::Syntax(offset))?;
                if comment.is_empty()
                    || comment.starts_with('*')
                    || comment.bytes().any(|b| b < b' ' && b != b'\n')
                {
                    return Err(Error::Syntax(offset));
                }
                text = rest;
            }
            _ => {
                let digit = char::from(byte).to_digit(16).ok_or(Error::Syntax(offset))? as u8;
                text = &text[1..];
                match high.take() {
                    Some(hi) => bytes.push(hi << 4 | digit),
                    None => high = Some(digit),
                }
            }
        }
    }
    if high.is_some() {
        return Err(Error::Syntax(offset));
    }
    Ok(bytes)
}

pub(super) fn base64_content(content: &str, offset: usize) -> Result<Vec<u8>, Error> {
    let mut filtered = Vec::new();
    let mut chars = content.char_indices();
    while let Some((idx, ch)) = chars.next() {
        match ch {
            '\n' | ' ' => {}
            '#' => {
                for (i, next) in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                    if next < ' ' {
                        return Err(Error::Syntax(offset.saturating_sub(content.len()) + i));
                    }
                }
            }
            '=' => filtered.push(64),
            _ => {
                let Some(value) = b64_value(ch) else {
                    return Err(Error::Syntax(offset.saturating_sub(content.len()) + idx));
                };
                filtered.push(value);
            }
        }
    }

    if let Some(first_pad) = filtered.iter().position(|&value| value == 64) {
        if filtered[first_pad..].iter().any(|&value| value != 64) {
            return Err(Error::semantic(offset, "invalid base64 padding"));
        }
        let pad_count = filtered.len() - first_pad;
        if pad_count > 2 || filtered.len() % 4 != 0 {
            return Err(Error::semantic(offset, "invalid base64 padding"));
        }
        match (first_pad % 4, pad_count) {
            (2, 2) | (3, 1) => filtered.truncate(first_pad),
            _ => return Err(Error::semantic(offset, "invalid base64 padding")),
        }
    } else if filtered.len() % 4 == 1 {
        return Err(Error::semantic(offset, "invalid base64 length"));
    }

    let mut out = Vec::new();
    for chunk in filtered.chunks(4) {
        let a = chunk[0];
        let b = *chunk
            .get(1)
            .ok_or_else(|| Error::semantic(offset, "invalid base64"))?;
        if a == 64 || b == 64 {
            return Err(Error::semantic(offset, "invalid base64 padding"));
        }
        out.push((a << 2) | (b >> 4));
        if let Some(&c) = chunk.get(2) {
            if c == 64 {
                continue;
            }
            out.push((b << 4) | (c >> 2));
            if let Some(&d) = chunk.get(3) {
                if d == 64 {
                    continue;
                }
                out.push((c << 6) | d);
            }
        }
    }
    Ok(out)
}

pub(super) fn append_indefinite_string_chunk(
    out: &mut Vec<u8>,
    prefix: &str,
    want_major: u8,
    arg: Arg,
    offset: usize,
) -> Result<(), Error> {
    let encoded = arg.into_encoded()?;
    let Some(&head) = encoded.first() else {
        return Err(Error::Syntax(offset));
    };
    let source_major = head >> 5;
    if !matches!(source_major, 2 | 3) || (head & 0x1f) == 31 {
        return Err(Error::semantic(
            offset,
            format!("{prefix} arguments must be definite strings"),
        ));
    }

    if want_major == 3 {
        match crate::de::value_from_slice(&encoded)? {
            Value::Text(..) => {}
            Value::Bytes(bytes) => {
                core::str::from_utf8(&bytes)
                    .map_err(|_| Error::semantic(offset, "ilts argument is not UTF-8"))?;
            }
            _ => unreachable!("string major type decoded as non-string value"),
        }
    }

    let mut encoded = encoded;
    encoded[0] = (want_major << 5) | (head & 0x1f);
    out.extend_from_slice(&encoded);
    Ok(())
}

fn b64_value(ch: char) -> Option<u8> {
    match ch {
        'A'..='Z' => Some(ch as u8 - b'A'),
        'a'..='z' => Some(ch as u8 - b'a' + 26),
        '0'..='9' => Some(ch as u8 - b'0' + 52),
        '+' | '-' => Some(62),
        '/' | '_' => Some(63),
        _ => None,
    }
}

pub(super) fn one_text_arg(prefix: &str, args: Vec<Arg>, offset: usize) -> Result<String, Error> {
    if args.len() != 1 {
        return Err(Error::semantic(
            offset,
            format!("{prefix} expects exactly one argument"),
        ));
    }
    match args.into_iter().next().unwrap().into_value()? {
        Value::Text(s) => Ok(s),
        Value::Bytes(b) => String::from_utf8(b)
            .map_err(|_| Error::semantic(offset, format!("{prefix} argument is not UTF-8"))),
        _ => Err(Error::semantic(
            offset,
            format!("{prefix} argument must be a string"),
        )),
    }
}

pub(super) fn one_bytes_arg(prefix: &str, args: Vec<Arg>, offset: usize) -> Result<Vec<u8>, Error> {
    if args.len() != 1 {
        return Err(Error::semantic(
            offset,
            format!("{prefix} expects exactly one argument"),
        ));
    }
    match args.into_iter().next().unwrap().into_value()? {
        Value::Text(s) => hex_content(&s, offset),
        Value::Bytes(b) => Ok(b),
        _ => Err(Error::semantic(
            offset,
            format!("{prefix} argument must be a string"),
        )),
    }
}
