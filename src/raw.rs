//! The [`RawValue`] type: a valid, already-encoded CBOR item kept as raw
//! bytes.

use serde::{de, ser};

// The marker name through which this crate's serializers splice raw
// bytes and its deserializers capture them. Not part of any public
// protocol: only `RawValue` itself uses it.
pub(crate) const NAME: &str = "@@RAW@@";

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
    /// The bytes are checked with [`validate`](crate::validate); anything
    /// else — malformed items, trailing data — is rejected, which keeps
    /// every `RawValue` safe to splice into an encoded stream.
    pub fn new(bytes: Vec<u8>) -> Result<Self, crate::de::Error> {
        crate::validate(&bytes[..])?;
        Ok(Self(bytes))
    }

    /// Encodes any serializable value into a raw item.
    pub fn serialized<T: ?Sized + ser::Serialize>(value: &T) -> Result<Self, crate::ser::Error> {
        Ok(Self(crate::to_vec(value)?))
    }

    /// Decodes the raw item into any deserializable type.
    pub fn deserialized<T: de::DeserializeOwned>(&self) -> Result<T, crate::de::Error> {
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
        struct Bytes<'a>(&'a [u8]);

        impl ser::Serialize for Bytes<'_> {
            #[inline]
            fn serialize<S: ser::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_bytes(self.0)
            }
        }

        serializer.serialize_newtype_struct(NAME, &Bytes(&self.0))
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

pub(crate) struct RawBytesSerializer;

macro_rules! not_raw_bytes {
    ($($f:ident($($t:ty),*);)+) => {$(
        fn $f(self, $(_: $t),*) -> Result<Vec<u8>, NotRawBytes> {
            Err(NotRawBytes)
        }
    )+};
}

impl ser::Serializer for RawBytesSerializer {
    type Ok = Vec<u8>;
    type Error = NotRawBytes;

    type SerializeSeq = ser::Impossible<Vec<u8>, NotRawBytes>;
    type SerializeTuple = ser::Impossible<Vec<u8>, NotRawBytes>;
    type SerializeTupleStruct = ser::Impossible<Vec<u8>, NotRawBytes>;
    type SerializeTupleVariant = ser::Impossible<Vec<u8>, NotRawBytes>;
    type SerializeMap = ser::Impossible<Vec<u8>, NotRawBytes>;
    type SerializeStruct = ser::Impossible<Vec<u8>, NotRawBytes>;
    type SerializeStructVariant = ser::Impossible<Vec<u8>, NotRawBytes>;

    #[inline]
    fn serialize_bytes(self, v: &[u8]) -> Result<Vec<u8>, NotRawBytes> {
        Ok(v.to_vec())
    }

    not_raw_bytes! {
        serialize_bool(bool);
        serialize_i8(i8);
        serialize_i16(i16);
        serialize_i32(i32);
        serialize_i64(i64);
        serialize_i128(i128);
        serialize_u8(u8);
        serialize_u16(u16);
        serialize_u32(u32);
        serialize_u64(u64);
        serialize_u128(u128);
        serialize_f32(f32);
        serialize_f64(f64);
        serialize_char(char);
        serialize_str(&str);
        serialize_none();
        serialize_unit();
        serialize_unit_struct(&'static str);
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
    ) -> Result<Vec<u8>, NotRawBytes> {
        Err(NotRawBytes)
    }

    fn serialize_some<U: ?Sized + ser::Serialize>(self, _: &U) -> Result<Vec<u8>, NotRawBytes> {
        Err(NotRawBytes)
    }

    fn serialize_newtype_struct<U: ?Sized + ser::Serialize>(
        self,
        _: &'static str,
        _: &U,
    ) -> Result<Vec<u8>, NotRawBytes> {
        Err(NotRawBytes)
    }

    fn serialize_newtype_variant<U: ?Sized + ser::Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &U,
    ) -> Result<Vec<u8>, NotRawBytes> {
        Err(NotRawBytes)
    }

    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, NotRawBytes> {
        Err(NotRawBytes)
    }

    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, NotRawBytes> {
        Err(NotRawBytes)
    }

    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, NotRawBytes> {
        Err(NotRawBytes)
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, NotRawBytes> {
        Err(NotRawBytes)
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, NotRawBytes> {
        Err(NotRawBytes)
    }

    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, NotRawBytes> {
        Err(NotRawBytes)
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, NotRawBytes> {
        Err(NotRawBytes)
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}
