//! Deterministic encoding support (RFC 8949 §4.2).

use alloc::vec::Vec;

use serde::ser::Error as _;

use super::{Error, Integer, Value};

/// The map key ordering used by deterministic encoding.
///
/// RFC 8949 defines two deterministic key orderings. They agree whenever
/// all keys encode to the same length, but differ otherwise: for example,
/// `100` (`0x1864`) sorts before `-1` (`0x20`) bytewise, but after it
/// length-first.
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum KeyOrder {
    /// Bytewise lexicographic order of the keys' deterministic encodings.
    ///
    /// This is the order required by the *core deterministic encoding
    /// requirements* (RFC 8949 §4.2.1) and is the recommended choice for
    /// new protocols; COSE (RFC 9052) and friends use it.
    #[default]
    Bytewise,

    /// Length-first order: keys with shorter encodings sort earlier, and
    /// keys of equal length sort bytewise.
    ///
    /// This is the "Canonical CBOR" ordering of RFC 7049 §3.9, kept in
    /// RFC 8949 §4.2.3 for backwards compatibility. Use it to interoperate
    /// with protocols and implementations built on the older rule (for
    /// example, `ciborium`'s canonical module).
    LengthFirst,
}

impl Value {
    /// Rewrites this value so that encoding it satisfies the *core
    /// deterministic encoding requirements* (RFC 8949 §4.2.1).
    ///
    /// This is [`canonicalize_with`](Self::canonicalize_with) using
    /// [`KeyOrder::Bytewise`].
    ///
    /// ```rust
    /// use cbor2::{cbor, Value};
    ///
    /// let mut value = cbor!({ "z": 1, "aa": 2 }).unwrap();
    /// value.canonicalize().unwrap();
    ///
    /// // "z" (0x617a) sorts before "aa" (0x626161).
    /// let keys: Vec<_> = value.as_map().unwrap().iter().map(|(k, _)| k).collect();
    /// assert_eq!(keys, [&Value::from("z"), &Value::from("aa")]);
    /// ```
    #[inline]
    pub fn canonicalize(&mut self) -> Result<(), Error> {
        self.canonicalize_with(KeyOrder::Bytewise)
    }

    /// Rewrites this value so that encoding it is deterministic, sorting
    /// map keys in the given [`KeyOrder`].
    ///
    /// The encoder already emits preferred (smallest lossless)
    /// serializations and never produces indefinite-length items, so the
    /// work left to this method is normalizing the data model:
    ///
    /// * The entries of every map are sorted in `order`. Duplicate keys
    ///   are rejected with an error, since CBOR maps holding them are
    ///   invalid (RFC 8949 §5.6) and have no unique encoding.
    /// * Bignums (tags 2 and 3) are reduced to their preferred form:
    ///   leading zeros are stripped and values that fit in major type 0
    ///   or 1 become plain integers (RFC 8949 §3.4.3).
    /// * Every NaN is replaced by the canonical quiet NaN, so a single
    ///   floating-point value cannot have multiple encodings (RFC 8949
    ///   §4.2.2).
    ///
    /// ```rust
    /// use cbor2::{cbor, KeyOrder, Value};
    ///
    /// let mut value = cbor!({ "aa": 2, 100: 1, -1: 0 }).unwrap();
    /// value.canonicalize_with(KeyOrder::LengthFirst).unwrap();
    ///
    /// // -1 (0x20, one byte) sorts before 100 (0x1864, two bytes).
    /// let keys: Vec<_> = value.as_map().unwrap().iter().map(|(k, _)| k).collect();
    /// assert_eq!(keys, [&Value::from(-1), &Value::from(100), &Value::from("aa")]);
    /// ```
    /// On error, this value is left unchanged.
    pub fn canonicalize_with(&mut self, order: KeyOrder) -> Result<(), Error> {
        let mut plans = Vec::new();
        prepare(self, order, &mut plans, crate::de::DEFAULT_RECURSION_LIMIT)
            .map_err(Error::custom)?;
        normalize(self, &mut plans.into_iter());
        Ok(())
    }
}

struct Entry<'a> {
    encoded: core::ops::Range<usize>,
    equivalent: core::ops::Range<usize>,
    value: &'a Value,
    original: usize,
}

// One byte arena per map, instead of a separate allocation for every key.
fn ordered<'a>(
    pairs: &'a [(Value, Value)],
    order: KeyOrder,
    zero: bool,
    depth: usize,
) -> Result<(Vec<u8>, Vec<Entry<'a>>), crate::ser::Error> {
    let mut arena = Vec::new();
    let mut entries = Vec::with_capacity(pairs.len());
    let mut same_order = true;
    for (original, (key, value)) in pairs.iter().enumerate() {
        let start = arena.len();
        encode(
            key,
            &mut crate::core::Encoder::from(&mut arena),
            order,
            zero,
            depth,
        )?;
        let encoded = start..arena.len();
        let equivalent = if !zero && has_negative_zero(key) {
            same_order = false;
            let start = arena.len();
            encode(
                key,
                &mut crate::core::Encoder::from(&mut arena),
                order,
                true,
                depth,
            )?;
            start..arena.len()
        } else {
            encoded.clone()
        };
        entries.push(Entry {
            encoded,
            equivalent,
            value,
            original,
        });
    }
    let sort_wire = |entries: &mut [Entry<'a>]| match order {
        KeyOrder::Bytewise => {
            entries.sort_unstable_by(|a, b| arena[a.encoded.clone()].cmp(&arena[b.encoded.clone()]))
        }
        KeyOrder::LengthFirst => entries.sort_unstable_by(|a, b| {
            a.encoded
                .len()
                .cmp(&b.encoded.len())
                .then_with(|| arena[a.encoded.clone()].cmp(&arena[b.encoded.clone()]))
        }),
    };
    if same_order {
        sort_wire(&mut entries);
    } else {
        // Signed-zero equivalence can put duplicates apart in wire order.
        entries
            .sort_unstable_by(|a, b| arena[a.equivalent.clone()].cmp(&arena[b.equivalent.clone()]));
    }
    if entries
        .windows(2)
        .any(|w| arena[w[0].equivalent.clone()] == arena[w[1].equivalent.clone()])
    {
        return Err(crate::ser::Error::msg("duplicate map key"));
    }
    if !same_order {
        sort_wire(&mut entries);
    }
    Ok((arena, entries))
}

fn has_negative_zero(value: &Value) -> bool {
    match value {
        Value::Float(x) => x.to_bits() == (-0.0f64).to_bits(),
        Value::Array(items) => items.iter().any(has_negative_zero),
        Value::Map(items) => items
            .iter()
            .any(|(k, v)| has_negative_zero(k) || has_negative_zero(v)),
        Value::Tag(_, inner) => has_negative_zero(inner),
        _ => false,
    }
}

// Validate before changing the value; retain only map permutations, not
// cloned payloads. Plans are consumed in the same postorder by normalize.
fn prepare(
    value: &Value,
    order: KeyOrder,
    plans: &mut Vec<Vec<usize>>,
    depth: usize,
) -> Result<(), crate::ser::Error> {
    if depth == 0 {
        return Err(crate::ser::Error::msg("recursion limit exceeded"));
    }
    match value {
        Value::Array(items) => {
            for item in items {
                prepare(item, order, plans, depth - 1)?;
            }
        }
        Value::Tag(_, inner) => prepare(inner, order, plans, depth - 1)?,
        Value::Map(pairs) => {
            for (key, value) in pairs {
                prepare(key, order, plans, depth - 1)?;
                prepare(value, order, plans, depth - 1)?;
            }
            let (_, entries) = ordered(pairs, order, false, depth - 1)?;
            let mut permutation = alloc::vec![0; entries.len()];
            for (destination, entry) in entries.iter().enumerate() {
                permutation[entry.original] = destination;
            }
            plans.push(permutation);
        }
        _ => {}
    }
    Ok(())
}

fn normalize(value: &mut Value, plans: &mut alloc::vec::IntoIter<Vec<usize>>) {
    match value {
        Value::Float(x) if x.is_nan() => *x = f64::NAN,
        Value::Array(items) => {
            for item in items {
                normalize(item, plans);
            }
        }
        Value::Tag(tag, inner) => {
            normalize(inner, plans);
            if matches!(*tag, 2 | 3) {
                if let Value::Bytes(bytes) = inner.as_mut() {
                    let first = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
                    bytes.drain(..first);
                    if let Some(integer) = small_bignum(*tag, bytes) {
                        *value = Value::Integer(integer);
                    }
                }
            }
        }
        Value::Map(pairs) => {
            for (key, value) in pairs.iter_mut() {
                normalize(key, plans);
                normalize(value, plans);
            }
            let mut permutation = plans.next().expect("validated map has a permutation");
            for i in 0..permutation.len() {
                while permutation[i] != i {
                    let destination = permutation[i];
                    pairs.swap(i, destination);
                    permutation.swap(i, destination);
                }
            }
        }
        _ => {}
    }
}

fn small_bignum(tag: u64, bytes: &[u8]) -> Option<Integer> {
    if bytes.len() > 8 {
        return None;
    }
    let mut raw = [0u8; 8];
    raw[8 - bytes.len()..].copy_from_slice(bytes);
    let raw = u64::from_be_bytes(raw);
    Some(if tag == 2 {
        Integer::from(raw)
    } else {
        Integer::try_from(-1 - i128::from(raw)).expect("CBOR negative range")
    })
}

pub(crate) fn to_writer<W: crate::io::Write>(
    value: &Value,
    writer: W,
    order: KeyOrder,
) -> Result<(), crate::ser::Error> {
    encode(
        value,
        &mut crate::core::Encoder::from(writer),
        order,
        false,
        crate::de::DEFAULT_RECURSION_LIMIT,
    )
}

fn encode<W: crate::io::Write>(
    value: &Value,
    enc: &mut crate::core::Encoder<W>,
    order: KeyOrder,
    zero: bool,
    depth: usize,
) -> Result<(), crate::ser::Error> {
    if depth == 0 {
        return Err(crate::ser::Error::msg("recursion limit exceeded"));
    }
    match value {
        Value::Integer(integer) => {
            let integer = i128::from(*integer);
            if integer < 0 {
                enc.negative((-1 - integer) as u64)?;
            } else {
                enc.positive(integer as u64)?;
            }
        }
        Value::Bytes(bytes) => enc.bytes(bytes)?,
        Value::Text(text) => enc.text(text)?,
        Value::Bool(value) => enc.simple(if *value { 21 } else { 20 })?,
        Value::Null => enc.simple(22)?,
        Value::Simple(value) => enc.simple(value.value())?,
        Value::Float(value) => enc.float(if value.is_nan() {
            f64::NAN
        } else if zero && *value == 0.0 {
            0.0
        } else {
            *value
        })?,
        Value::Tag(tag @ (2 | 3), inner) if matches!(inner.as_ref(), Value::Bytes(_)) => {
            let Value::Bytes(bytes) = inner.as_ref() else {
                unreachable!()
            };
            let first = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
            let bytes = &bytes[first..];
            if let Some(integer) = small_bignum(*tag, bytes) {
                encode(&Value::Integer(integer), enc, order, zero, depth)?;
            } else {
                enc.tag(*tag)?;
                enc.bytes(bytes)?;
            }
        }
        Value::Tag(tag, inner) => {
            enc.tag(*tag)?;
            encode(inner, enc, order, zero, depth - 1)?;
        }
        Value::Array(items) => {
            enc.array(Some(items.len()))?;
            for item in items {
                encode(item, enc, order, zero, depth - 1)?;
            }
        }
        Value::Map(pairs) => {
            let (arena, entries) = ordered(pairs, order, zero, depth - 1)?;
            enc.map(Some(entries.len()))?;
            for entry in entries {
                enc.write_all(&arena[entry.encoded])?;
                encode(entry.value, enc, order, zero, depth - 1)?;
            }
        }
    }
    Ok(())
}
