//! Process complete JSON values directly in the current input buffer. Keep
//! the original spelling available for numbers outside serde's integer model.

use super::{io, Error};
use cbor2::Value;
use serde::Deserialize;
use serde_json::{value::RawValue, Deserializer};
use std::io::{BufRead, BufReader, Read};

pub(super) fn encode(
    input: impl Read,
    mut write: impl FnMut(&[u8]) -> io::Result<()>,
) -> Result<(), Error> {
    let mut input = BufReader::with_capacity(32 * 1024, input);
    let mut encoded = Vec::new();
    loop {
        let block = match input.fill_buf() {
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if block.is_empty() {
            return Ok(());
        }
        let mut values = Deserializer::from_slice(block).into_iter::<Value>();
        let mut start = 0;
        while let Some(Ok(value)) = values.next() {
            let end = values.byte_offset();
            if end == block.len()
                && !matches!(value, Value::Array(_) | Value::Map(_) | Value::Text(_))
            {
                break; // A scalar needs a delimiter or EOF.
            }
            if needs_number_spelling(&value) {
                write(&cbor2::cdn_to_vec(std::str::from_utf8(
                    &block[start..end],
                )?)?)?;
            } else {
                encoded.clear();
                cbor2::to_writer(&value, &mut encoded)?;
                write(&encoded)?;
            }
            start = end;
        }
        let rest = &block[start..];
        let offset = rest
            .iter()
            .position(|b| !matches!(b, b' ' | b'\r' | b'\n' | b'\t'));
        let first = offset.map(|i| rest[i]);
        let consumed = start + offset.unwrap_or(rest.len());
        input.consume(consumed);
        let raw: Box<RawValue> = match first {
            None => continue,
            Some(b'-' | b'0'..=b'9' | b'n' | b't' | b'f') => {
                serde_json::from_reader(ScalarToken(&mut input))?
            }
            // Objects, arrays and strings finish at their closing delimiter.
            // serde_json reports any other starting byte as a syntax error.
            Some(_) => Box::<RawValue>::deserialize(&mut Deserializer::from_reader(&mut input))?,
        };
        write(&cbor2::cdn_to_vec(raw.get())?)?;
    }
}

// serde_json looks one byte past numbers and keywords. Give it EOF at a JSON
// delimiter so the next item can start there, even across input buffers.
struct ScalarToken<'a, R>(&'a mut R);

impl<R: BufRead> Read for ScalarToken<'_, R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let input = self.0.fill_buf()?;
        let available = &input[..input.len().min(out.len())];
        let len = available
            .iter()
            .position(|b| b" \r\n\t\"[]{},:".contains(b))
            .unwrap_or(available.len());
        out[..len].copy_from_slice(&available[..len]);
        self.0.consume(len);
        Ok(len)
    }
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
