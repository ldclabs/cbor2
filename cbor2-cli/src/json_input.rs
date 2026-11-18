//! Process complete JSON values directly in the current input buffer. Keep
//! the original spelling available for numbers outside serde's integer model.

use super::{io, Error};
use cbor2::Value;
use serde_json::{value::RawValue, Deserializer};
use std::io::{BufReader, Read};

pub(super) fn encode(
    mut input: impl Read,
    mut write: impl FnMut(&[u8]) -> io::Result<()>,
) -> Result<(), Error> {
    let mut pending = Vec::new();
    let mut block = [0; 8192];
    let mut encoded = Vec::new();
    loop {
        let n = match input.read(&mut block) {
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        let had_partial = !pending.is_empty();
        pending.extend_from_slice(&block[..n]);
        let mut values = Deserializer::from_slice(&pending).into_iter::<Value>();
        let mut start = 0;
        loop {
            match values.next() {
                Some(Ok(value)) => {
                    let end = values.byte_offset();
                    // A number can continue in the next block. Keywords also
                    // require a delimiter before another scalar.
                    if n != 0
                        && end == pending.len()
                        && !matches!(value, Value::Array(_) | Value::Map(_) | Value::Text(_))
                    {
                        break;
                    }
                    if needs_number_spelling(&value) {
                        write(&cbor2::cdn_to_vec(std::str::from_utf8(
                            &pending[start..end],
                        )?)?)?;
                    } else {
                        encoded.clear();
                        cbor2::to_writer(&value, &mut encoded)?;
                        write(&encoded)?;
                    }
                    start = end;
                }
                Some(Err(err)) if err.is_eof() && n != 0 => break,
                Some(Err(_)) => {
                    // A syntactically valid, arbitrary-width integer may be
                    // outside f64 as well. Validate its raw JSON before CDN.
                    return encode_raw(pending[start..].chain(input), write);
                }
                None => {
                    pending.clear();
                    if n == 0 {
                        return Ok(());
                    }
                    break;
                }
            }
        }
        if !pending.is_empty() {
            if had_partial && start == 0 {
                // Hand long or fragmented items to the streaming parser. A
                // growing incomplete value is never reparsed block by block.
                return encode_raw(pending.as_slice().chain(input), write);
            }
            pending.drain(..start);
        }
    }
}

fn encode_raw(
    input: impl Read,
    mut write: impl FnMut(&[u8]) -> io::Result<()>,
) -> Result<(), Error> {
    let stream = Deserializer::from_reader(BufReader::new(input));
    for value in stream.into_iter::<Box<RawValue>>() {
        write(&cbor2::cdn_to_vec(value?.get())?)?;
    }
    Ok(())
}

// With serde_json's float_roundtrip feature, decimal floats round exactly as
// in CDN. Only these floats might have come from an integer token: negative
// zero, or an integer outside i64/u64 which serde_json promoted to f64.
fn needs_number_spelling(value: &Value) -> bool {
    match value {
        Value::Float(v) => {
            v.to_bits() == (-0.0f64).to_bits() || *v >= u64::MAX as f64 || *v <= i64::MIN as f64
        }
        Value::Array(items) => items.iter().any(needs_number_spelling),
        Value::Map(items) => items.iter().any(|(_, v)| needs_number_spelling(v)),
        _ => false,
    }
}
