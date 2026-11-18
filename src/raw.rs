//! The [`RawValue`] type: a valid, already-encoded CBOR item kept as raw
//! bytes.

use alloc::vec::Vec;

use serde::{de, ser};

// The marker name through which this crate's serializers splice raw
// bytes and its deserializers capture them. Not part of any public
// protocol: only `RawValue` itself uses it.
pub(crate) const NAME: &str = "@@RAW@@";

pub(crate) const BORROWED_NAME: &str = "@@RAW_BORROWED@@";

// Only used internally with the borrowing cbor2 source. Its capture path
// validates structure and UTF-8 before handing out the exact source range.
pub(crate) struct RawSlice<'a>(pub(crate) &'a [u8]);

impl<'de> de::Deserialize<'de> for RawSlice<'de> {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Borrowed;
        impl<'de> de::Visitor<'de> for Borrowed {
            type Value = RawSlice<'de>;
            fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str("a borrowed raw CBOR item")
            }
            fn visit_borrowed_bytes<E: de::Error>(
                self,
                bytes: &'de [u8],
            ) -> Result<Self::Value, E> {
                Ok(RawSlice(bytes))
            }
        }
        deserializer.deserialize_newtype_struct(BORROWED_NAME, Borrowed)
    }
}

impl ser::Serialize for RawSlice<'_> {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Bytes<'a>(&'a [u8]);
        impl ser::Serialize for Bytes<'_> {
            fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_bytes(self.0)
            }
        }
        serializer.serialize_newtype_struct(NAME, &Bytes(self.0))
    }
}

/// A valid CBOR item, kept as its raw encoded bytes.
///
/// Like [`serde_json::value::RawValue`](https://docs.rs/serde_json/latest/serde_json/value/struct.RawValue.html),
/// this defers work and preserves the wire encoding exactly: serializing
/// a `RawValue` with this crate splices the bytes into the output
/// untouched, and deserializing one captures the item's bytes without
/// decoding them — byte for byte, even for non-preferred spellings the
/// crate's own encoder would never produce. That makes it the right tool
/// for signature payloads, for passing items through unchanged, and for
/// delaying the decoding of part of a message.
///
/// Every constructor guarantees the invariant that the bytes hold
/// exactly one well-formed CBOR item (including text UTF-8 validity), so
/// re-serializing a `RawValue` cannot corrupt the surrounding stream.
///
/// ```rust
/// use serde::{Deserialize, Serialize};
/// use cbor2::RawValue;
///
/// #[derive(Debug, PartialEq, Deserialize, Serialize)]
/// struct Envelope {
///     kind: u8,
///     body: RawValue,
/// }
///
/// let envelope = Envelope {
///     kind: 1,
///     body: RawValue::serialized(&("untouched", 42))?,
/// };
///
/// let bytes = cbor2::to_vec(&envelope)?;
/// let back: Envelope = cbor2::from_slice(&bytes)?;
/// assert_eq!(back, envelope);
/// assert_eq!(back.body.deserialized::<(String, u8)>()?.1, 42);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// With the deterministic encoders (`to_canonical_*`) and through
/// [`Value`](crate::Value), the raw item *is* decoded and re-encoded —
/// canonicalization re-spells everything by design. In non-CBOR formats
/// a `RawValue` serializes as its plain bytes (a JSON array of numbers,
/// for example) and validates again on the way back in.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct RawValue(Vec<u8>);

impl RawValue {
    /// Wraps the encoding of exactly one well-formed CBOR item.
    ///
    /// The bytes are checked with [`validate_slice`](crate::validate_slice); anything
    /// else — malformed items, trailing data — is rejected, which keeps
    /// every `RawValue` safe to splice into an encoded stream.
    pub fn new(bytes: Vec<u8>) -> Result<Self, crate::de::Error> {
        crate::validate_slice(&bytes)?;
        Ok(Self(bytes))
    }

    /// Encodes any serializable value into a raw item.
    pub fn serialized<T: ?Sized + ser::Serialize>(value: &T) -> Result<Self, crate::ser::Error> {
        Self::new(crate::to_vec(value)?).map_err(ser::Error::custom)
    }

    /// Decodes the raw item into any deserializable type.
    ///
    /// Like [`from_slice`](crate::from_slice), definite-length text and byte
    /// strings can be borrowed from the raw bytes.
    ///
    /// ```rust
    /// let raw = cbor2::RawValue::serialized(&("kid", 7u8)).unwrap();
    /// let (kid, n): (&str, u8) = raw.deserialized().unwrap();
    /// assert_eq!((kid, n), ("kid", 7));
    /// ```
    pub fn deserialized<'de, T: de::Deserialize<'de>>(&'de self) -> Result<T, crate::de::Error> {
        crate::from_slice(&self.0)
    }

    /// The raw bytes of the item.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// Extracts the raw bytes of the item.
    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

impl AsRef<[u8]> for RawValue {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// Validates like [`RawValue::new`].
impl TryFrom<Vec<u8>> for RawValue {
    type Error = crate::de::Error;

    #[inline]
    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        Self::new(bytes)
    }
}

impl From<RawValue> for Vec<u8> {
    #[inline]
    fn from(value: RawValue) -> Self {
        value.0
    }
}

/// Decodes the raw item into a dynamic [`Value`](crate::Value), like
/// [`deserialized`](RawValue::deserialized).
///
/// This *decodes*: byte-level spellings the dynamic form cannot represent
/// (indefinite lengths, non-preferred widths) are not preserved.
impl TryFrom<&RawValue> for crate::Value {
    type Error = crate::de::Error;

    #[inline]
    fn try_from(raw: &RawValue) -> Result<Self, Self::Error> {
        raw.deserialized()
    }
}

/// Decodes the raw item into a dynamic [`Value`](crate::Value), like
/// [`deserialized`](RawValue::deserialized).
///
/// This *decodes*: byte-level spellings the dynamic form cannot represent
/// (indefinite lengths, non-preferred widths) are not preserved.
impl TryFrom<RawValue> for crate::Value {
    type Error = crate::de::Error;

    #[inline]
    fn try_from(raw: RawValue) -> Result<Self, Self::Error> {
        raw.deserialized()
    }
}

/// Encodes the value into a raw item, like
/// [`serialized`](RawValue::serialized).
impl TryFrom<&crate::Value> for RawValue {
    type Error = crate::ser::Error;

    #[inline]
    fn try_from(value: &crate::Value) -> Result<Self, Self::Error> {
        Self::serialized(value)
    }
}

/// Encodes the value into a raw item, like
/// [`serialized`](RawValue::serialized).
impl TryFrom<crate::Value> for RawValue {
    type Error = crate::ser::Error;

    #[inline]
    fn try_from(value: crate::Value) -> Result<Self, Self::Error> {
        Self::serialized(&value)
    }
}

/// Formats the item in CBOR diagnostic notation (RFC 8949 §8), like
/// [`Value`](crate::Value).
impl core::fmt::Display for RawValue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // The invariant guarantees a valid item, which always renders.
        match crate::diagnostic(&self.0[..]) {
            Ok(diag) => f.write_str(&diag),
            Err(..) => Err(core::fmt::Error),
        }
    }
}

impl core::fmt::Debug for RawValue {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "RawValue({self})")
    }
}

impl ser::Serialize for RawValue {
    fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RawSlice(&self.0).serialize(serializer)
    }
}

impl<'de> de::Deserialize<'de> for RawValue {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = RawValue;

            fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "a valid CBOR item")
            }

            #[inline]
            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
                self.visit_byte_buf(v.to_vec())
            }

            #[inline]
            fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
                RawValue::new(v).map_err(de::Error::custom)
            }

            // Formats without a bytes type — JSON, for one — deliver an
            // array of integers instead.
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut acc: A) -> Result<Self::Value, A::Error> {
                let mut bytes = Vec::new();
                while let Some(byte) = acc.next_element::<u8>()? {
                    bytes.push(byte);
                }
                RawValue::new(bytes).map_err(de::Error::custom)
            }

            fn visit_newtype_struct<D: de::Deserializer<'de>>(
                self,
                deserializer: D,
            ) -> Result<Self::Value, D::Error> {
                let bytes: Vec<u8> = de::Deserialize::deserialize(deserializer)?;
                RawValue::new(bytes).map_err(de::Error::custom)
            }
        }

        deserializer.deserialize_newtype_struct(NAME, Visitor)
    }
}

// `RawValue::serialize` wraps its bytes behind the `NAME` marker; this
// serializer takes them back out, like `tag::TagNumberSerializer`. Every
// other input is an error.
#[derive(Debug)]
pub(crate) struct NotRawBytes;

impl core::fmt::Display for NotRawBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "expected raw bytes")
    }
}

impl ser::StdError for NotRawBytes {}

impl ser::Error for NotRawBytes {
    fn custom<U: core::fmt::Display>(_msg: U) -> Self {
        NotRawBytes
    }
}

pub(crate) struct RawBytesSerializer<F> {
    pub(crate) write: F,
}

#[cfg(test)]
fn bytes_serializer() -> RawBytesSerializer<impl FnOnce(&[u8]) -> Result<Vec<u8>, NotRawBytes>> {
    RawBytesSerializer {
        write: |bytes: &[u8]| Ok(bytes.to_vec()),
    }
}

impl<F, T, E: ser::Error> ser::Serializer for RawBytesSerializer<F>
where
    F: FnOnce(&[u8]) -> Result<T, E>,
{
    type Ok = T;
    type Error = E;

    #[inline]
    fn serialize_bytes(self, v: &[u8]) -> Result<T, E> {
        (self.write)(v)
    }

    crate::ser::reject_serializer! {
        E::custom(NotRawBytes);
        serialize_bool(bool),
        serialize_i8(i8),
        serialize_i16(i16),
        serialize_i32(i32),
        serialize_i64(i64),
        serialize_i128(i128),
        serialize_u8(u8),
        serialize_u16(u16),
        serialize_u32(u32),
        serialize_u64(u64),
        serialize_u128(u128),
        serialize_f32(f32),
        serialize_f64(f64),
        serialize_char(char),
        serialize_str(&str),
    }
}

#[cfg(test)]
mod tests {
    use alloc::{
        format,
        string::{String, ToString},
        vec,
    };

    use serde::de::value::{BytesDeserializer, Error as DeError, SeqDeserializer, U32Deserializer};
    use serde::de::{Deserialize as _, IntoDeserializer as _};
    use serde::ser::{Error as _, Serializer as _};

    use super::*;

    #[test]
    fn accessors_expose_the_bytes() {
        let raw = RawValue::new(vec![0x01]).unwrap();
        assert_eq!(raw.as_ref(), [0x01]);
        assert_eq!(raw.clone().into_bytes(), vec![0x01]);
        assert_eq!(Vec::from(raw), vec![0x01]);
    }

    // The CBOR deserializer always hands the visitor an owned buffer;
    // borrowed bytes and integer sequences arrive from other formats.
    #[test]
    fn visitor_accepts_bytes_and_sequences() {
        let de = BytesDeserializer::<DeError>::new(&[0x01]);
        assert_eq!(RawValue::deserialize(de).unwrap().as_bytes(), [0x01]);

        let de = SeqDeserializer::<_, DeError>::new(vec![0x01u8].into_iter());
        assert_eq!(RawValue::deserialize(de).unwrap().as_bytes(), [0x01]);

        // A type the visitor cannot absorb reports what it expected.
        let err = RawValue::deserialize(7u32.into_deserializer() as U32Deserializer<DeError>)
            .unwrap_err()
            .to_string();
        assert!(err.contains("a valid CBOR item"), "{err}");
    }

    #[test]
    fn not_raw_bytes_error() {
        assert_eq!(NotRawBytes.to_string(), "expected raw bytes");
        assert_eq!(
            NotRawBytes::custom("ignored").to_string(),
            "expected raw bytes"
        );
        assert!(format!("{NotRawBytes:?}").contains("NotRawBytes"));
    }

    #[test]
    fn raw_bytes_serializer_accepts_only_bytes() {
        assert_eq!(
            bytes_serializer().serialize_bytes(b"\x01").unwrap(),
            vec![0x01]
        );
        assert!(!bytes_serializer().is_human_readable());

        assert!(bytes_serializer().serialize_bool(true).is_err());
        assert!(bytes_serializer().serialize_i8(1).is_err());
        assert!(bytes_serializer().serialize_i16(1).is_err());
        assert!(bytes_serializer().serialize_i32(1).is_err());
        assert!(bytes_serializer().serialize_i64(1).is_err());
        assert!(bytes_serializer().serialize_i128(1).is_err());
        assert!(bytes_serializer().serialize_u8(1).is_err());
        assert!(bytes_serializer().serialize_u16(1).is_err());
        assert!(bytes_serializer().serialize_u32(1).is_err());
        assert!(bytes_serializer().serialize_u64(1).is_err());
        assert!(bytes_serializer().serialize_u128(1).is_err());
        assert!(bytes_serializer().serialize_f32(1.0).is_err());
        assert!(bytes_serializer().serialize_f64(1.0).is_err());
        assert!(bytes_serializer().serialize_char('a').is_err());
        assert!(bytes_serializer().serialize_str("a").is_err());
        assert!(bytes_serializer().serialize_none().is_err());
        assert!(bytes_serializer().serialize_some(&1u8).is_err());
        assert!(bytes_serializer().serialize_unit().is_err());
        assert!(bytes_serializer().serialize_unit_struct("x").is_err());
        assert!(bytes_serializer()
            .serialize_unit_variant("x", 0, "y")
            .is_err());
        assert!(bytes_serializer()
            .serialize_newtype_struct("x", &1u8)
            .is_err());
        assert!(bytes_serializer()
            .serialize_newtype_variant("x", 0, "y", &1u8)
            .is_err());
        assert!(bytes_serializer().serialize_seq(None).is_err());
        assert!(bytes_serializer().serialize_tuple(0).is_err());
        assert!(bytes_serializer().serialize_tuple_struct("x", 0).is_err());
        assert!(bytes_serializer()
            .serialize_tuple_variant("x", 0, "y", 0)
            .is_err());
        assert!(bytes_serializer().serialize_map(None).is_err());
        assert!(bytes_serializer().serialize_struct("x", 0).is_err());
        assert!(bytes_serializer()
            .serialize_struct_variant("x", 0, "y", 0)
            .is_err());
    }

    // Something that is not a `RawValue` hiding behind the raw marker
    // name is rejected by both the streaming and the `Value` serializer.
    #[test]
    fn forged_raw_marker_is_rejected() {
        struct Forged;

        impl ser::Serialize for Forged {
            fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_newtype_struct(NAME, &7u8)
            }
        }

        let err = crate::to_vec(&Forged).unwrap_err().to_string();
        assert!(err.contains("expected raw bytes"), "{err}");

        let err = crate::Value::serialized(&Forged).unwrap_err().to_string();
        assert!(err.contains("expected raw bytes"), "{err}");
    }

    // String-keyed test: the unused import lint keeps `String` honest.
    #[test]
    fn display_uses_diagnostic_notation() {
        let raw = RawValue::new(vec![0x01]).unwrap();
        assert_eq!(raw.to_string(), String::from("1"));
    }
}
