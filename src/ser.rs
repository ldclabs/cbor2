//! Serde serialization support for CBOR.

#[cfg(feature = "alloc")]
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use serde::ser;

use crate::core::{simple, tag, Encoder, Header};
use crate::io::Write;
#[cfg(feature = "alloc")]
use crate::value::KeyOrder;

/// An error that occurred during serialization.
#[derive(Debug)]
pub enum Error {
    /// An error from the underlying writer.
    Io(crate::io::Error),

    /// A value cannot be represented in CBOR.
    ///
    /// Contains a description of the problem. Without the `alloc` feature
    /// only a static description can be carried, so the messages that serde
    /// composes at runtime are reduced to a generic one.
    #[cfg(feature = "alloc")]
    Value(String),

    /// A value cannot be represented in CBOR.
    ///
    /// Contains a description of the problem. Without the `alloc` feature
    /// only a static description can be carried, so the messages that serde
    /// composes at runtime are reduced to a generic one.
    #[cfg(not(feature = "alloc"))]
    Value(&'static str),
}

impl Error {
    // Composes a `Value` error from a static message in any configuration.
    #[inline]
    pub(crate) fn msg(msg: &'static str) -> Self {
        #[cfg(feature = "alloc")]
        return Self::Value(String::from(msg));
        #[cfg(not(feature = "alloc"))]
        return Self::Value(msg);
    }
}

impl From<crate::io::Error> for Error {
    #[inline]
    fn from(value: crate::io::Error) -> Self {
        Self::Io(value)
    }
}

#[cfg(feature = "alloc")]
impl From<crate::value::Error> for Error {
    fn from(value: crate::value::Error) -> Self {
        Self::Value(value.to_string())
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Io(err) => write!(f, "i/o error: {err}"),
            Error::Value(msg) => write!(f, "value error: {msg}"),
        }
    }
}

// `serde::ser::StdError` is `std::error::Error` whenever it is available,
// and an identical substitute otherwise.
impl serde::ser::StdError for Error {
    fn source(&self) -> Option<&(dyn serde::ser::StdError + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            Error::Value(..) => None,
        }
    }
}

impl ser::Error for Error {
    #[cfg(feature = "alloc")]
    fn custom<U: core::fmt::Display>(msg: U) -> Self {
        Error::Value(msg.to_string())
    }

    #[cfg(not(feature = "alloc"))]
    fn custom<U: core::fmt::Display>(_msg: U) -> Self {
        Error::Value("serialization error (message lost without alloc)")
    }
}

/// The marker prefix that carries a struct's CBOR protocol details.
///
/// CBOR protocols like COSE (RFC 9052) key their maps with integers and
/// wrap their messages in tags, which serde's data model cannot express.
/// This crate's serializers read both from a marked *container* name —
/// most conveniently produced by the `#[derive(cbor2::Cbor)]` macro —
/// of the form:
///
/// ```text
/// @@CBOR@@<tag>@@<field>=<key>;<field>=<key>@@<OriginalName>
/// @@CBOR@@<tag>@@<field>=<key>;<field>=<key>@@array@@<OriginalName>
/// ```
///
/// `<tag>` is an optional CBOR tag number and each `<field>=<key>` entry
/// maps a serde field name to an integer map key; both segments may be
/// empty. The optional `array` segment switches named structs from map
/// encoding to field-order array encoding. Because the marker renames
/// only the *container* — which formats like JSON ignore — field names
/// stay untouched and the same type serializes naturally everywhere else.
///
/// Only canonical decimals count: no leading zeros, no `-0`, no `+`,
/// within the CBOR integer range (the tag within `0..=2^64-1`). A name
/// that does not parse is an ordinary container name with no effect.
pub const STRUCT_MARKER: &str = "@@CBOR@@";

/// The container wire shape carried by a parsed [`STRUCT_MARKER`].
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum StructShape {
    /// A struct encoded as a CBOR map.
    Map,
    /// A struct encoded as a CBOR array in serde field order.
    Array,
}

// A parsed `STRUCT_MARKER` container name.
pub(crate) struct StructMarker<'a> {
    pub tag: Option<u64>,
    pub keys: &'a str,
    pub shape: StructShape,
}

// Splits a marked container name into its tag number and its field key
// table. Returns `None` — no special handling — unless the marker frame
// and the tag segment are well-formed.
#[inline]
pub(crate) fn parse_struct_marker(name: &str) -> Option<StructMarker<'_>> {
    let rest = name.strip_prefix(STRUCT_MARKER)?;
    let (tag, rest) = rest.split_once("@@")?;
    let (keys, original) = rest.split_once("@@")?;

    let tag = match tag {
        "" => None,
        _ => Some(canonical_u64(tag)?),
    };

    let shape = match original.strip_prefix("array@@") {
        Some(..) => StructShape::Array,
        None => StructShape::Map,
    };

    Some(StructMarker { tag, keys, shape })
}

// The integer map key for a struct field, if the key table names it.
pub(crate) fn key_for_field(keys: &str, field: &str) -> Option<i128> {
    keys.split(';').find_map(|entry| {
        let (name, key) = entry.split_once('=')?;
        if name == field {
            canonical_int(key)
        } else {
            None
        }
    })
}

// The struct field name for an integer map key, if the key table maps it.
// Only the (alloc-gated) deserializers translate keys in this direction.
#[cfg(feature = "alloc")]
pub(crate) fn field_for_key(keys: &str, key: i128) -> Option<&str> {
    keys.split(';').find_map(|entry| {
        let (name, k) = entry.split_once('=')?;
        (canonical_int(k)? == key).then_some(name)
    })
}

// Parses a canonical decimal in the CBOR integer range: no leading
// zeros, no "-0", no sign prefix other than `-`.
fn canonical_int(decimal: &str) -> Option<i128> {
    let bytes = decimal.as_bytes();
    let digits = match bytes.first()? {
        b'-' => &bytes[1..],
        b'0'..=b'9' => bytes,
        _ => return None,
    };

    match digits {
        [] => return None,
        [b'0'] if bytes[0] == b'-' => return None,
        [b'0', _, ..] => return None,
        _ => {}
    }

    let value = decimal.parse::<i128>().ok()?;
    let in_range = value <= u64::MAX as i128 && value >= -(u64::MAX as i128) - 1;
    in_range.then_some(value)
}

// Parses a canonical decimal CBOR tag number.
fn canonical_u64(decimal: &str) -> Option<u64> {
    let bytes = decimal.as_bytes();
    match bytes {
        [] => return None,
        [b'0'] => {}
        [b'0', ..] => return None,
        _ if !bytes.iter().all(u8::is_ascii_digit) => return None,
        _ => {}
    }

    decimal.parse::<u64>().ok()
}

/// A serde serializer that writes CBOR to a [`Write`].
pub struct Serializer<W>(Encoder<W>);

impl<W: Write> From<W> for Serializer<W> {
    #[inline]
    fn from(writer: W) -> Self {
        Self(writer.into())
    }
}

impl<W: Write> From<Encoder<W>> for Serializer<W> {
    #[inline]
    fn from(encoder: Encoder<W>) -> Self {
        Self(encoder)
    }
}

impl<W: Write> Serializer<W> {
    // Writes a struct field key: an integer for fields named in the
    // container's key table (COSE-style), text otherwise.
    fn push_field_key(&mut self, keys: &str, key: &'static str) -> Result<(), Error> {
        if keys.is_empty() {
            return Ok(self.0.text(key)?);
        }

        match key_for_field(keys, key) {
            Some(n) if n >= 0 => Ok(self.0.positive(n as u64)?),
            Some(n) => Ok(self.0.negative(n as u64 ^ !0)?),
            None => Ok(self.0.text(key)?),
        }
    }
}

impl<'a, W: Write> ser::Serializer for &'a mut Serializer<W> {
    type Ok = ();
    type Error = Error;

    type SerializeSeq = CollectionSerializer<'a, W>;
    type SerializeTuple = CollectionSerializer<'a, W>;
    type SerializeTupleStruct = CollectionSerializer<'a, W>;
    type SerializeTupleVariant = CollectionSerializer<'a, W>;
    type SerializeMap = CollectionSerializer<'a, W>;
    type SerializeStruct = CollectionSerializer<'a, W>;
    type SerializeStructVariant = CollectionSerializer<'a, W>;

    #[inline]
    fn serialize_bool(self, v: bool) -> Result<(), Error> {
        Ok(self.0.simple(match v {
            false => simple::FALSE,
            true => simple::TRUE,
        })?)
    }

    #[inline]
    fn serialize_i8(self, v: i8) -> Result<(), Error> {
        self.serialize_i64(v.into())
    }

    #[inline]
    fn serialize_i16(self, v: i16) -> Result<(), Error> {
        self.serialize_i64(v.into())
    }

    #[inline]
    fn serialize_i32(self, v: i32) -> Result<(), Error> {
        self.serialize_i64(v.into())
    }

    #[inline]
    fn serialize_i64(self, v: i64) -> Result<(), Error> {
        let _: () = match v.is_negative() {
            false => self.0.positive(v as u64)?,
            true => self.0.negative(v as u64 ^ !0)?,
        };
        Ok(())
    }

    #[inline]
    fn serialize_i128(self, v: i128) -> Result<(), Error> {
        let (tag, raw) = match v.is_negative() {
            false => (tag::BIGPOS, v as u128),
            true => (tag::BIGNEG, v as u128 ^ !0),
        };

        if let Ok(x) = u64::try_from(raw) {
            let _: () = match tag {
                tag::BIGPOS => self.0.positive(x)?,
                _ => self.0.negative(x)?,
            };
            return Ok(());
        }

        let bytes = raw.to_be_bytes();
        let first = raw.leading_zeros() as usize / 8;

        self.0.tag(tag)?;
        Ok(self.0.bytes(&bytes[first..])?)
    }

    #[inline]
    fn serialize_u8(self, v: u8) -> Result<(), Error> {
        self.serialize_u64(v.into())
    }

    #[inline]
    fn serialize_u16(self, v: u16) -> Result<(), Error> {
        self.serialize_u64(v.into())
    }

    #[inline]
    fn serialize_u32(self, v: u32) -> Result<(), Error> {
        self.serialize_u64(v.into())
    }

    #[inline]
    fn serialize_u64(self, v: u64) -> Result<(), Error> {
        Ok(self.0.positive(v)?)
    }

    #[inline]
    fn serialize_u128(self, v: u128) -> Result<(), Error> {
        if let Ok(x) = u64::try_from(v) {
            return self.serialize_u64(x);
        }

        let bytes = v.to_be_bytes();
        let first = v.leading_zeros() as usize / 8;

        self.0.tag(tag::BIGPOS)?;
        Ok(self.0.bytes(&bytes[first..])?)
    }

    #[inline]
    fn serialize_f32(self, v: f32) -> Result<(), Error> {
        self.serialize_f64(v.into())
    }

    #[inline]
    fn serialize_f64(self, v: f64) -> Result<(), Error> {
        Ok(self.0.float(v)?)
    }

    #[inline]
    fn serialize_char(self, v: char) -> Result<(), Error> {
        let mut buffer = [0u8; 4];
        self.serialize_str(v.encode_utf8(&mut buffer))
    }

    #[inline]
    fn serialize_str(self, v: &str) -> Result<(), Error> {
        Ok(self.0.text(v)?)
    }

    #[inline]
    fn serialize_bytes(self, v: &[u8]) -> Result<(), Error> {
        Ok(self.0.bytes(v)?)
    }

    #[inline]
    fn serialize_none(self) -> Result<(), Error> {
        Ok(self.0.simple(simple::NULL)?)
    }

    #[inline]
    fn serialize_some<U: ?Sized + ser::Serialize>(self, value: &U) -> Result<(), Error> {
        value.serialize(self)
    }

    #[inline]
    fn serialize_unit(self) -> Result<(), Error> {
        self.serialize_none()
    }

    #[inline]
    fn serialize_unit_struct(self, name: &'static str) -> Result<(), Error> {
        if let Some(StructMarker { tag: Some(tag), .. }) = parse_struct_marker(name) {
            self.0.tag(tag)?;
        }
        self.serialize_unit()
    }

    #[inline]
    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<(), Error> {
        self.serialize_str(variant)
    }

    #[inline]
    fn serialize_newtype_struct<U: ?Sized + ser::Serialize>(
        self,
        name: &'static str,
        value: &U,
    ) -> Result<(), Error> {
        // A `RawValue` splices its already-encoded bytes into the stream.
        #[cfg(feature = "alloc")]
        if name == crate::raw::NAME {
            return match value.serialize(crate::raw::RawBytesSerializer) {
                Ok(bytes) => Ok(self.0.write_all(&bytes)?),
                Err(err) => Err(Error::Value(err.to_string())),
            };
        }

        if let Some(StructMarker { tag: Some(tag), .. }) = parse_struct_marker(name) {
            self.0.tag(tag)?;
        }
        value.serialize(self)
    }

    #[inline]
    fn serialize_newtype_variant<U: ?Sized + ser::Serialize>(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        value: &U,
    ) -> Result<(), Error> {
        if name == crate::simple::NAME && variant == crate::simple::VALUE {
            let simple = value
                .serialize(crate::simple::SimpleValueSerializer)
                .map_err(|_| Error::msg("expected CBOR simple value"))?;
            return Ok(self.0.simple(simple.value())?);
        }

        if name != crate::tag::NAME || variant != crate::tag::UNTAGGED {
            self.0.map(Some(1))?;
            self.serialize_str(variant)?;
        }

        value.serialize(self)
    }

    #[inline]
    fn serialize_seq(self, length: Option<usize>) -> Result<Self::SerializeSeq, Error> {
        if let Some(length) = length {
            self.0.reserve(length.saturating_mul(4).saturating_add(9));
        }
        self.0.array(length)?;
        Ok(CollectionSerializer {
            encoder: self,
            ending: length.is_none(),
            tag: false,
            shape: StructShape::Map,
            keys: "",
        })
    }

    #[inline]
    fn serialize_tuple(self, length: usize) -> Result<Self::SerializeTuple, Error> {
        self.serialize_seq(Some(length))
    }

    #[inline]
    fn serialize_tuple_struct(
        self,
        name: &'static str,
        length: usize,
    ) -> Result<Self::SerializeTupleStruct, Error> {
        if let Some(StructMarker { tag: Some(tag), .. }) = parse_struct_marker(name) {
            self.0.tag(tag)?;
        }
        self.serialize_seq(Some(length))
    }

    #[inline]
    fn serialize_tuple_variant(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        length: usize,
    ) -> Result<Self::SerializeTupleVariant, Error> {
        if name == crate::tag::NAME && variant == crate::tag::TAGGED {
            return Ok(CollectionSerializer {
                encoder: self,
                ending: false,
                tag: true,
                shape: StructShape::Map,
                keys: "",
            });
        }

        self.0.map(Some(1))?;
        self.serialize_str(variant)?;
        self.0.array(Some(length))?;
        Ok(CollectionSerializer {
            encoder: self,
            ending: false,
            tag: false,
            shape: StructShape::Map,
            keys: "",
        })
    }

    #[inline]
    fn serialize_map(self, length: Option<usize>) -> Result<Self::SerializeMap, Error> {
        self.0.map(length)?;
        Ok(CollectionSerializer {
            encoder: self,
            ending: length.is_none(),
            tag: false,
            shape: StructShape::Map,
            keys: "",
        })
    }

    #[inline]
    fn serialize_struct(
        self,
        name: &'static str,
        length: usize,
    ) -> Result<Self::SerializeStruct, Error> {
        let mut keys = "";
        let mut shape = StructShape::Map;
        if let Some(marker) = parse_struct_marker(name) {
            keys = marker.keys;
            shape = marker.shape;
            if let Some(tag) = marker.tag {
                self.0.tag(tag)?;
            }
        }

        self.0.reserve(length.saturating_mul(16).saturating_add(9));
        match shape {
            StructShape::Map => self.0.map(Some(length))?,
            StructShape::Array => self.0.array(Some(length))?,
        }
        Ok(CollectionSerializer {
            encoder: self,
            ending: false,
            tag: false,
            shape,
            keys,
        })
    }

    #[inline]
    fn serialize_struct_variant(
        self,
        name: &'static str,
        _index: u32,
        variant: &'static str,
        length: usize,
    ) -> Result<Self::SerializeStructVariant, Error> {
        let marker = parse_struct_marker(name);
        let keys = marker.as_ref().map_or("", |marker| marker.keys);
        let shape = marker.map_or(StructShape::Map, |marker| marker.shape);

        self.0.map(Some(1))?;
        self.serialize_str(variant)?;
        match shape {
            StructShape::Map => self.0.map(Some(length))?,
            StructShape::Array => self.0.array(Some(length))?,
        }
        Ok(CollectionSerializer {
            encoder: self,
            ending: false,
            tag: false,
            shape,
            keys,
        })
    }

    // The default implementation buffers the formatted output in a String;
    // formatting twice (once to measure the text header, once to stream the
    // body) avoids the allocation.
    fn collect_str<T: ?Sized + core::fmt::Display>(self, value: &T) -> Result<(), Error> {
        use core::fmt::Write as _;

        struct Counter(usize);

        impl core::fmt::Write for Counter {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                self.0 += s.len();
                Ok(())
            }
        }

        let mut counter = Counter(0);
        if write!(&mut counter, "{value}").is_err() {
            return Err(Error::msg("Display implementation failed"));
        }

        self.0.push_len(3, Some(counter.0))?;

        struct Body<'a, W> {
            encoder: &'a mut Encoder<W>,
            remaining: usize,
            error: Option<crate::io::Error>,
        }

        impl<W: Write> core::fmt::Write for Body<'_, W> {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                if s.len() > self.remaining {
                    return Err(core::fmt::Error);
                }

                match self.encoder.write_all(s.as_bytes()) {
                    Ok(()) => {
                        self.remaining -= s.len();
                        Ok(())
                    }
                    Err(err) => {
                        self.error = Some(err);
                        Err(core::fmt::Error)
                    }
                }
            }
        }

        let mut body = Body {
            encoder: &mut self.0,
            remaining: counter.0,
            error: None,
        };
        let result = write!(&mut body, "{value}");

        if let Some(err) = body.error {
            return Err(Error::Io(err));
        }
        if result.is_err() || body.remaining != 0 {
            return Err(Error::msg("Display implementation is not deterministic"));
        }
        Ok(())
    }

    #[inline]
    fn is_human_readable(&self) -> bool {
        false
    }
}

/// The serializer for CBOR arrays and maps.
pub struct CollectionSerializer<'a, W> {
    encoder: &'a mut Serializer<W>,
    ending: bool,
    tag: bool,
    // Structs may be marker-switched from maps to field-order arrays.
    shape: StructShape,
    // The `<field>=<key>` table of a marked struct (see [`STRUCT_MARKER`]);
    // empty for everything else.
    keys: &'static str,
}

impl<W: Write> CollectionSerializer<'_, W> {
    #[inline]
    fn end_inner(self) -> Result<(), Error> {
        if self.ending {
            self.encoder.0.push(Header::Break)?;
        }
        Ok(())
    }
}

impl<W: Write> ser::SerializeSeq for CollectionSerializer<'_, W> {
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_element<U: ?Sized + ser::Serialize>(&mut self, value: &U) -> Result<(), Error> {
        value.serialize(&mut *self.encoder)
    }

    #[inline]
    fn end(self) -> Result<(), Error> {
        self.end_inner()
    }
}

impl<W: Write> ser::SerializeTuple for CollectionSerializer<'_, W> {
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_element<U: ?Sized + ser::Serialize>(&mut self, value: &U) -> Result<(), Error> {
        value.serialize(&mut *self.encoder)
    }

    #[inline]
    fn end(self) -> Result<(), Error> {
        self.end_inner()
    }
}

impl<W: Write> ser::SerializeTupleStruct for CollectionSerializer<'_, W> {
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<U: ?Sized + ser::Serialize>(&mut self, value: &U) -> Result<(), Error> {
        value.serialize(&mut *self.encoder)
    }

    #[inline]
    fn end(self) -> Result<(), Error> {
        self.end_inner()
    }
}

impl<W: Write> ser::SerializeTupleVariant for CollectionSerializer<'_, W> {
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<U: ?Sized + ser::Serialize>(&mut self, value: &U) -> Result<(), Error> {
        if !self.tag {
            return value.serialize(&mut *self.encoder);
        }

        // The first field of the tag pseudo-variant is the tag number
        // itself; the second is serialized normally.
        self.tag = false;
        match value.serialize(crate::tag::TagNumberSerializer) {
            Ok(x) => Ok(self.encoder.0.tag(x)?),
            Err(..) => Err(Error::msg("expected tag")),
        }
    }

    #[inline]
    fn end(self) -> Result<(), Error> {
        self.end_inner()
    }
}

impl<W: Write> ser::SerializeMap for CollectionSerializer<'_, W> {
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_key<U: ?Sized + ser::Serialize>(&mut self, key: &U) -> Result<(), Error> {
        key.serialize(&mut *self.encoder)
    }

    #[inline]
    fn serialize_value<U: ?Sized + ser::Serialize>(&mut self, value: &U) -> Result<(), Error> {
        value.serialize(&mut *self.encoder)
    }

    #[inline]
    fn end(self) -> Result<(), Error> {
        self.end_inner()
    }
}

impl<W: Write> ser::SerializeStruct for CollectionSerializer<'_, W> {
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<U: ?Sized + ser::Serialize>(
        &mut self,
        key: &'static str,
        value: &U,
    ) -> Result<(), Error> {
        if self.shape == StructShape::Map {
            self.encoder.push_field_key(self.keys, key)?;
        }
        value.serialize(&mut *self.encoder)
    }

    #[inline]
    fn end(self) -> Result<(), Error> {
        self.end_inner()
    }
}

impl<W: Write> ser::SerializeStructVariant for CollectionSerializer<'_, W> {
    type Ok = ();
    type Error = Error;

    #[inline]
    fn serialize_field<U: ?Sized + ser::Serialize>(
        &mut self,
        key: &'static str,
        value: &U,
    ) -> Result<(), Error> {
        if self.shape == StructShape::Map {
            self.encoder.push_field_key(self.keys, key)?;
        }
        value.serialize(&mut *self.encoder)
    }

    #[inline]
    fn end(self) -> Result<(), Error> {
        self.end_inner()
    }
}

/// Serializes a value as CBOR into a [`Write`].
///
/// With the `std` feature any `std::io::Write` is accepted; for repeated
/// small writes consider wrapping the writer in a `std::io::BufWriter`.
///
/// To encode into a new `Vec<u8>`, prefer [`to_vec`]: a `&mut Vec<u8>`
/// passed here goes through the generic `std::io::Write` impl, which
/// cannot forward the capacity hints of [`Write::reserve`], while
/// `to_vec` pre-reserves.
#[inline]
pub fn to_writer<T: ?Sized + ser::Serialize, W: Write>(value: &T, writer: W) -> Result<(), Error> {
    let mut serializer = Serializer::from(writer);
    value.serialize(&mut serializer)
}

#[cfg(feature = "alloc")]
struct VecWriter<'a>(&'a mut Vec<u8>);

#[cfg(feature = "alloc")]
impl Write for VecWriter<'_> {
    // `inline(always)`: header writes arrive as constant-length arrays, and
    // only full inlining lets those constants reach `extend_from_slice`,
    // turning it into fixed-size stores instead of a memcpy call.
    #[inline(always)]
    fn write_all(&mut self, data: &[u8]) -> Result<(), crate::io::Error> {
        self.0.extend_from_slice(data);
        Ok(())
    }

    #[inline]
    fn reserve(&mut self, additional: usize) {
        self.0.reserve(additional);
    }

    #[inline]
    fn flush(&mut self) -> Result<(), crate::io::Error> {
        Ok(())
    }
}

struct SliceWriter<'a> {
    buffer: &'a mut [u8],
    written: usize,
}

impl Write for SliceWriter<'_> {
    #[inline]
    fn write_all(&mut self, data: &[u8]) -> Result<(), crate::io::Error> {
        if self.buffer.len() - self.written < data.len() {
            return Err(crate::io::Error::from(crate::io::ErrorKind::WriteZero));
        }

        let end = self.written + data.len();
        self.buffer[self.written..end].copy_from_slice(data);
        self.written = end;
        Ok(())
    }

    #[inline]
    fn flush(&mut self) -> Result<(), crate::io::Error> {
        Ok(())
    }
}

#[cfg(feature = "std")]
impl Write for &mut SliceWriter<'_> {
    #[inline]
    fn write_all(&mut self, data: &[u8]) -> Result<(), crate::io::Error> {
        (**self).write_all(data)
    }

    #[inline]
    fn flush(&mut self) -> Result<(), crate::io::Error> {
        (**self).flush()
    }
}

/// Serializes a value as CBOR into a new `Vec<u8>`.
#[cfg(feature = "alloc")]
#[inline]
pub fn to_vec<T: ?Sized + ser::Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    let mut buffer = Vec::new();
    to_writer(value, VecWriter(&mut buffer))?;
    Ok(buffer)
}

/// Serializes a value as CBOR into the front of `buffer`, returning the
/// written prefix.
///
/// This never allocates, which makes it the natural encoding function
/// without the `alloc` feature; a buffer too small for the value fails
/// with an [`Error::Io`] of kind
/// [`WriteZero`](crate::io::ErrorKind::WriteZero). Use [`serialized_size`]
/// to size the buffer in advance.
///
/// ```rust
/// let mut buffer = [0u8; 64];
/// let item = cbor2::to_slice(&("id", 42u8), &mut buffer).unwrap();
/// assert_eq!(item, &[0x82, 0x62, b'i', b'd', 0x18, 42]);
/// ```
pub fn to_slice<'a, T: ?Sized + ser::Serialize>(
    value: &T,
    buffer: &'a mut [u8],
) -> Result<&'a mut [u8], Error> {
    let mut writer = SliceWriter { buffer, written: 0 };
    to_writer(value, &mut writer)?;
    let written = writer.written;
    Ok(&mut buffer[..written])
}

/// Computes the exact number of bytes that [`to_writer`] would produce for
/// a value, without writing or buffering anything.
///
/// The value is serialized through the regular serializer into a counting
/// sink, so the result is exact by construction (including preferred float
/// widths, bignums, tags and indefinite-length containers) and no memory is
/// allocated.
///
/// ```rust
/// let value = ("hello", 42u64, vec![1u8, 2, 3]);
/// let size = cbor2::serialized_size(&value).unwrap();
/// assert_eq!(size as usize, cbor2::to_vec(&value).unwrap().len());
/// ```
pub fn serialized_size<T: ?Sized + ser::Serialize>(value: &T) -> Result<u64, Error> {
    let mut counter = ByteCounter(0);
    to_writer(value, &mut counter)?;
    Ok(counter.0)
}

// A sink that discards everything written to it, keeping only the count.
struct ByteCounter(u64);

// With std, the blanket implementation over std::io::Write provides the
// crate's Write trait; without it, the trait is implemented directly.
#[cfg(feature = "std")]
impl std::io::Write for ByteCounter {
    #[inline]
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.0 += data.len() as u64;
        Ok(data.len())
    }

    #[inline]
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(not(feature = "std"))]
impl Write for ByteCounter {
    #[inline]
    fn write_all(&mut self, data: &[u8]) -> Result<(), crate::io::Error> {
        self.0 += data.len() as u64;
        Ok(())
    }

    #[inline]
    fn flush(&mut self) -> Result<(), crate::io::Error> {
        Ok(())
    }
}

/// Serializes a value as deterministically encoded CBOR into a
/// [`Write`], satisfying the core deterministic encoding
/// requirements of RFC 8949 §4.2.1.
///
/// This is [`to_canonical_writer_with`] using [`KeyOrder::Bytewise`].
#[cfg(feature = "alloc")]
pub fn to_canonical_writer<T: ?Sized + ser::Serialize, W: Write>(
    value: &T,
    writer: W,
) -> Result<(), Error> {
    to_canonical_writer_with(value, writer, KeyOrder::Bytewise)
}

/// Serializes a value as deterministically encoded CBOR into a new
/// `Vec<u8>`, satisfying the core deterministic encoding requirements of
/// RFC 8949 §4.2.1.
///
/// This is [`to_canonical_vec_with`] using [`KeyOrder::Bytewise`].
#[cfg(feature = "alloc")]
pub fn to_canonical_vec<T: ?Sized + ser::Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    to_canonical_vec_with(value, KeyOrder::Bytewise)
}

/// Serializes a value as deterministically encoded CBOR into a
/// [`Write`], sorting map keys in the given [`KeyOrder`].
///
/// See [`Value::canonicalize_with`](crate::Value::canonicalize_with) for
/// the exact normalization rules. The value is buffered as a
/// [`Value`](crate::Value) in order to sort map keys, so this is more
/// expensive than [`to_writer`].
///
/// Maps with duplicate keys (after normalization) are rejected.
#[cfg(feature = "alloc")]
pub fn to_canonical_writer_with<T: ?Sized + ser::Serialize, W: Write>(
    value: &T,
    writer: W,
    order: KeyOrder,
) -> Result<(), Error> {
    let mut value = crate::value::Value::serialized(value)?;
    value.canonicalize_with(order)?;
    to_writer(&value, writer)
}

/// Serializes a value as deterministically encoded CBOR into a new
/// `Vec<u8>`, sorting map keys in the given [`KeyOrder`].
///
/// See [`to_canonical_writer_with`] for details.
#[cfg(feature = "alloc")]
pub fn to_canonical_vec_with<T: ?Sized + ser::Serialize>(
    value: &T,
    order: KeyOrder,
) -> Result<Vec<u8>, Error> {
    let mut buffer = Vec::new();
    to_canonical_writer_with(value, VecWriter(&mut buffer), order)?;
    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_counter_is_a_well_behaved_sink() {
        let mut counter = ByteCounter(0);
        Write::write_all(&mut counter, b"12345").unwrap();
        Write::flush(&mut counter).unwrap();
        assert_eq!(counter.0, 5);
    }

    // `to_slice` works without any allocation, returning the written
    // prefix and rejecting a buffer that is too small.
    #[test]
    fn to_slice_returns_the_written_prefix() {
        let mut buffer = [0xffu8; 8];
        let item = to_slice(&(1u8, "ab"), &mut buffer).unwrap();
        assert_eq!(item, &[0x82, 0x01, 0x62, b'a', b'b']);

        let mut buffer = [0u8; 2];
        assert!(matches!(
            to_slice(&(1u8, "ab"), &mut buffer),
            Err(Error::Io(..))
        ));
    }
}
