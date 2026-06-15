//! Serde deserialization support for CBOR.

#[cfg(feature = "alloc")]
use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

use serde::de;
#[cfg(feature = "alloc")]
use serde::de::{value::BytesDeserializer, Deserializer as _};

#[cfg(feature = "alloc")]
use crate::core::{simple, tag};
use crate::core::{Decoder, Header};
use crate::io::Read;
#[cfg(feature = "alloc")]
use crate::tag::TagAccess;

/// An error that occurred during deserialization.
#[derive(Debug)]
pub enum Error {
    /// An error from the underlying reader.
    Io(crate::io::Error),

    /// The input is not well-formed CBOR.
    ///
    /// Contains the byte offset of the offending item.
    Syntax(usize),

    /// The input is well-formed CBOR but invalid for the target type.
    ///
    /// Contains a description of the error and (optionally) the byte offset
    /// of the item being processed when the error occurred. Without the
    /// `alloc` feature only a static description can be carried, so the
    /// messages that serde composes at runtime are reduced to a generic one.
    #[cfg(feature = "alloc")]
    Semantic(Option<usize>, String),

    /// The input is well-formed CBOR but invalid for the target type.
    ///
    /// Contains a description of the error and (optionally) the byte offset
    /// of the item being processed when the error occurred. Without the
    /// `alloc` feature only a static description can be carried, so the
    /// messages that serde composes at runtime are reduced to a generic one.
    #[cfg(not(feature = "alloc"))]
    Semantic(Option<usize>, &'static str),

    /// The input is nested deeper than the configured recursion limit.
    ///
    /// This error prevents stack exhaustion from adversarial input.
    RecursionLimitExceeded,
}

impl Error {
    /// A helper for composing a semantic error.
    #[cfg(feature = "alloc")]
    #[inline]
    pub fn semantic(offset: impl Into<Option<usize>>, msg: impl Into<String>) -> Self {
        Self::Semantic(offset.into(), msg.into())
    }

    /// A helper for composing a semantic error.
    #[cfg(not(feature = "alloc"))]
    #[inline]
    pub fn semantic(offset: impl Into<Option<usize>>, msg: &'static str) -> Self {
        Self::Semantic(offset.into(), msg)
    }
}

impl From<crate::io::Error> for Error {
    #[inline]
    fn from(value: crate::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<crate::core::Error> for Error {
    #[inline]
    fn from(value: crate::core::Error) -> Self {
        match value {
            crate::core::Error::Io(x) => Self::Io(x),
            crate::core::Error::Syntax(x) => Self::Syntax(x),
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Io(err) => write!(f, "i/o error: {err}"),
            Error::Syntax(offset) => write!(f, "syntax error at offset {offset}"),
            Error::Semantic(Some(offset), msg) => {
                write!(f, "semantic error at offset {offset}: {msg}")
            }
            Error::Semantic(None, msg) => write!(f, "semantic error: {msg}"),
            Error::RecursionLimitExceeded => write!(f, "recursion limit exceeded"),
        }
    }
}

// `serde::ser::StdError` is `std::error::Error` whenever it is available,
// and an identical substitute otherwise.
impl serde::ser::StdError for Error {
    fn source(&self) -> Option<&(dyn serde::ser::StdError + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl de::Error for Error {
    #[cfg(feature = "alloc")]
    #[inline]
    fn custom<U: core::fmt::Display>(msg: U) -> Self {
        Self::Semantic(None, msg.to_string())
    }

    #[cfg(not(feature = "alloc"))]
    #[inline]
    fn custom<U: core::fmt::Display>(_msg: U) -> Self {
        Self::Semantic(None, "deserialization error (message lost without alloc)")
    }
}

#[cfg(feature = "alloc")]
trait Expected {
    fn expected(self, kind: &'static str) -> Error;
}

#[cfg(feature = "alloc")]
impl Expected for Header {
    #[inline]
    fn expected(self, kind: &'static str) -> Error {
        de::Error::invalid_type(
            match self {
                Header::Positive(x) => de::Unexpected::Unsigned(x),
                Header::Negative(x) => de::Unexpected::Signed(x as i64 ^ !0),
                Header::Bytes(..) => de::Unexpected::Other("bytes"),
                Header::Text(..) => de::Unexpected::Other("string"),

                Header::Array(..) => de::Unexpected::Seq,
                Header::Map(..) => de::Unexpected::Map,

                Header::Tag(..) => de::Unexpected::Other("tag"),

                Header::Simple(simple::FALSE) => de::Unexpected::Bool(false),
                Header::Simple(simple::TRUE) => de::Unexpected::Bool(true),
                Header::Simple(simple::NULL) => de::Unexpected::Other("null"),
                Header::Simple(simple::UNDEFINED) => de::Unexpected::Other("undefined"),
                Header::Simple(..) => de::Unexpected::Other("simple"),

                Header::Float(x) => de::Unexpected::Float(x),
                Header::Break => de::Unexpected::Other("break"),
            },
            &kind,
        )
    }
}

// A parsed integer item: either a (possibly negative) integer that was
// encoded with major type 0 or 1, or a bignum (tag 2 or 3) whose payload is
// given with leading zeros stripped.
#[cfg(feature = "alloc")]
enum Num {
    Pos(u64),
    Neg(u64),
    BigPos(Vec<u8>),
    BigNeg(Vec<u8>),
}

// Interprets a stripped bignum payload as a `u128`, if it fits.
#[cfg(feature = "alloc")]
fn big_to_u128(bytes: &[u8]) -> Option<u128> {
    if bytes.len() > 16 {
        return None;
    }

    let mut buffer = [0u8; 16];
    buffer[16 - bytes.len()..].copy_from_slice(bytes);
    Some(u128::from_be_bytes(buffer))
}

// The identifier form of an integer map key that no field maps to. It can
// match no ordinary field name, so such keys are simply unknown fields.
#[cfg(feature = "alloc")]
pub(crate) const INT_KEY_PLACEHOLDER: &str = "@@KEY@@";

// A `core::fmt::Write` adapter over the scratch buffer; everything written
// through it is valid UTF-8 by construction.
#[cfg(feature = "alloc")]
struct FmtBuf<'a>(&'a mut Vec<u8>);

#[cfg(feature = "alloc")]
impl core::fmt::Write for FmtBuf<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.0.extend_from_slice(s.as_bytes());
        Ok(())
    }
}

#[cfg(feature = "alloc")]
mod sealed {
    pub trait Sealed {}
}

/// The byte source a [`Deserializer`] reads CBOR items from.
///
/// This abstracts the low-level decoder operations shared by the reader and
/// slice deserializers. It is sealed: the only implementors are
/// [`ReaderSource`] (any [`Read`]) and [`SliceSource`] (a byte slice). Use
/// [`Deserializer::from_reader`] or [`Deserializer::from_slice`] rather than
/// naming this trait directly.
#[cfg(feature = "alloc")]
pub trait Source: sealed::Sealed {
    #[doc(hidden)]
    fn pull(&mut self) -> Result<Header, crate::core::Error>;
    #[doc(hidden)]
    #[inline]
    fn integer(&mut self) -> Option<Result<(bool, u64), Error>> {
        None
    }
    #[doc(hidden)]
    #[inline]
    fn bool(&mut self) -> Option<Result<bool, Error>> {
        None
    }
    #[doc(hidden)]
    #[inline]
    fn float(&mut self) -> Option<Result<f64, Error>> {
        None
    }
    #[doc(hidden)]
    fn push(&mut self, header: Header);
    #[doc(hidden)]
    fn offset(&self) -> usize;
    #[doc(hidden)]
    fn bytes_body(
        &mut self,
        len: Option<usize>,
        out: &mut Vec<u8>,
    ) -> Result<(), crate::core::Error>;
    #[doc(hidden)]
    fn text_body(&mut self, len: Option<usize>, out: &mut String)
        -> Result<(), crate::core::Error>;
    // Captures the wire bytes of the next item, byte for byte, while
    // validating that it is well-formed (including text UTF-8). Used by
    // `RawValue`.
    #[doc(hidden)]
    fn capture(&mut self, recurse: usize) -> Result<Vec<u8>, Error>;
}

/// A [`Source`] that can borrow definite-length string bodies directly from
/// the input for the lifetime `'de`.
///
/// [`SliceSource`] hands serde `visit_borrowed_str`/`visit_borrowed_bytes`;
/// [`ReaderSource`] always copies, because a generic reader owns no input to
/// borrow from.
#[cfg(feature = "alloc")]
pub trait BorrowSource<'de>: Source {
    // Borrow `len` body bytes from the input, if this source can. `None`
    // means the caller must copy the body through `bytes_body`/`text_body`.
    #[doc(hidden)]
    fn borrow_body(&mut self, len: usize) -> Option<Result<&'de [u8], crate::io::Error>>;
}

/// A [`Source`] over any [`Read`]; always copies string bodies.
#[cfg(feature = "alloc")]
pub struct ReaderSource<R>(Decoder<R>);

#[cfg(feature = "alloc")]
impl<R> sealed::Sealed for ReaderSource<R> {}

#[cfg(feature = "alloc")]
impl<R: Read> Source for ReaderSource<R> {
    #[inline]
    fn pull(&mut self) -> Result<Header, crate::core::Error> {
        self.0.pull()
    }

    #[inline]
    fn push(&mut self, header: Header) {
        self.0.push(header);
    }

    #[inline]
    fn offset(&self) -> usize {
        self.0.offset()
    }

    #[inline]
    fn bytes_body(
        &mut self,
        len: Option<usize>,
        out: &mut Vec<u8>,
    ) -> Result<(), crate::core::Error> {
        self.0.bytes_body(len, out)
    }

    #[inline]
    fn text_body(
        &mut self,
        len: Option<usize>,
        out: &mut String,
    ) -> Result<(), crate::core::Error> {
        self.0.text_body(len, out)
    }

    fn capture(&mut self, recurse: usize) -> Result<Vec<u8>, Error> {
        capture_item(&mut self.0, recurse)
    }
}

#[cfg(feature = "alloc")]
impl<'de, R: Read> BorrowSource<'de> for ReaderSource<R> {
    #[inline]
    fn borrow_body(&mut self, _len: usize) -> Option<Result<&'de [u8], crate::io::Error>> {
        None
    }
}

/// A [`Source`] over a byte slice; borrows definite-length string bodies.
#[cfg(feature = "alloc")]
pub struct SliceSource<'de>(Decoder<&'de [u8]>);

#[cfg(feature = "alloc")]
impl sealed::Sealed for SliceSource<'_> {}

#[cfg(feature = "alloc")]
impl Source for SliceSource<'_> {
    #[inline]
    fn pull(&mut self) -> Result<Header, crate::core::Error> {
        self.0.pull_slice()
    }

    #[inline]
    fn integer(&mut self) -> Option<Result<(bool, u64), Error>> {
        self.0.integer_slice().map(|res| res.map_err(Error::from))
    }

    #[inline]
    fn bool(&mut self) -> Option<Result<bool, Error>> {
        self.0.bool_slice().map(Ok)
    }

    #[inline]
    fn float(&mut self) -> Option<Result<f64, Error>> {
        self.0.float_slice().map(|res| res.map_err(Error::from))
    }

    #[inline]
    fn push(&mut self, header: Header) {
        self.0.push(header);
    }

    #[inline]
    fn offset(&self) -> usize {
        self.0.offset()
    }

    #[inline]
    fn bytes_body(
        &mut self,
        len: Option<usize>,
        out: &mut Vec<u8>,
    ) -> Result<(), crate::core::Error> {
        match len {
            Some(len) => {
                out.extend_from_slice(self.0.borrow_body(len)?);
                Ok(())
            }
            None => self.0.bytes_body(None, out),
        }
    }

    #[inline]
    fn text_body(
        &mut self,
        len: Option<usize>,
        out: &mut String,
    ) -> Result<(), crate::core::Error> {
        match len {
            Some(len) => {
                let offset = self.0.offset();
                let bytes = self.0.borrow_body(len)?;
                let text =
                    core::str::from_utf8(bytes).map_err(|_| crate::core::Error::Syntax(offset))?;
                out.push_str(text);
                Ok(())
            }
            None => self.0.text_body(None, out),
        }
    }

    fn capture(&mut self, recurse: usize) -> Result<Vec<u8>, Error> {
        capture_item(&mut self.0, recurse)
    }
}

#[cfg(feature = "alloc")]
impl<'de> BorrowSource<'de> for SliceSource<'de> {
    #[inline]
    fn borrow_body(&mut self, len: usize) -> Option<Result<&'de [u8], crate::io::Error>> {
        Some(self.0.borrow_body(len))
    }
}

// Captures one well-formed item's wire bytes from a decoder. Shared by both
// sources' `capture` implementations and used by `RawValue`.
#[cfg(feature = "alloc")]
fn capture_item<R: Read>(decoder: &mut Decoder<R>, recurse: usize) -> Result<Vec<u8>, Error> {
    decoder.start_recording();
    let result = validate_item(decoder, recurse);
    let bytes = decoder.take_recording();
    result.map(|()| bytes)
}

/// A serde deserializer that reads CBOR from a [`Source`].
///
/// Construct one with [`from_reader`](Self::from_reader) (copying, over any
/// [`Read`]) or [`from_slice`](Self::from_slice) (borrowing, over a byte
/// slice). The slice form can hand serde definite-length text and byte
/// strings borrowed directly from the input.
#[cfg(feature = "alloc")]
pub struct Deserializer<S> {
    source: S,
    scratch: Vec<u8>,
    recurse: usize,
}

/// The default recursion limit for nested CBOR items.
pub const DEFAULT_RECURSION_LIMIT: usize = 256;

#[cfg(feature = "alloc")]
impl<R: Read> Deserializer<ReaderSource<R>> {
    /// Creates a deserializer reading from `reader` with the default
    /// recursion limit.
    ///
    /// For repeated small reads consider wrapping the reader in a
    /// `std::io::BufReader`.
    pub fn from_reader(reader: R) -> Self {
        Self::with_recursion_limit(reader, DEFAULT_RECURSION_LIMIT)
    }

    /// Creates a deserializer reading from `reader` with the given recursion
    /// limit.
    ///
    /// Inputs nested deeper than the limit fail with
    /// [`Error::RecursionLimitExceeded`]. Set a high limit at your own risk
    /// of stack exhaustion.
    pub fn with_recursion_limit(reader: R, limit: usize) -> Self {
        Self {
            source: ReaderSource(reader.into()),
            scratch: Vec::new(),
            recurse: limit,
        }
    }

    /// Turns this deserializer into an iterator over consecutive top-level
    /// items.
    ///
    /// CBOR allows concatenating encoded items into a *sequence* (RFC 8742).
    /// The iterator yields decoded items until the input is exhausted; a
    /// clean end of input terminates the iterator, while anything else
    /// (including a truncated item) yields an error.
    ///
    /// ```rust
    /// let mut stream = Vec::new();
    /// cbor2::to_writer(&1u8, &mut stream).unwrap();
    /// cbor2::to_writer(&"two", &mut stream).unwrap();
    ///
    /// let values: Vec<cbor2::Value> = cbor2::de::Deserializer::from_reader(&stream[..])
    ///     .into_iter()
    ///     .collect::<Result<_, _>>()
    ///     .unwrap();
    ///
    /// assert_eq!(values, vec![cbor2::Value::from(1), cbor2::Value::from("two")]);
    /// ```
    // Named for symmetry with `serde_json::Deserializer::into_iter`.
    #[allow(clippy::should_implement_trait)]
    pub fn into_iter<T: de::DeserializeOwned>(self) -> Iter<T, R> {
        Iter {
            de: self,
            _marker: core::marker::PhantomData,
        }
    }
}

#[cfg(feature = "alloc")]
impl<'de> Deserializer<SliceSource<'de>> {
    /// Creates a deserializer borrowing from `slice` with the default
    /// recursion limit.
    ///
    /// Definite-length text and byte strings are handed to serde borrowed
    /// from `slice`; see [`from_slice`].
    pub fn from_slice(slice: &'de [u8]) -> Self {
        Self::from_slice_with_recursion_limit(slice, DEFAULT_RECURSION_LIMIT)
    }

    /// Creates a deserializer borrowing from `slice` with the given
    /// recursion limit.
    ///
    /// Named distinctly from the reader's
    /// [`with_recursion_limit`](Deserializer::with_recursion_limit) because a
    /// `&[u8]` argument satisfies both constructors.
    pub fn from_slice_with_recursion_limit(slice: &'de [u8], limit: usize) -> Self {
        Self {
            source: SliceSource(slice.into()),
            scratch: Vec::new(),
            recurse: limit,
        }
    }
}

#[cfg(feature = "alloc")]
impl<S: Source> Deserializer<S> {
    /// Returns the byte offset of the next item in the stream.
    #[inline]
    pub fn offset(&self) -> usize {
        self.source.offset()
    }

    #[inline]
    fn recurse<V, F: FnOnce(&mut Self) -> Result<V, Error>>(
        &mut self,
        func: F,
    ) -> Result<V, Error> {
        if self.recurse == 0 {
            return Err(Error::RecursionLimitExceeded);
        }

        self.recurse -= 1;
        let result = func(self);
        self.recurse += 1;
        result
    }

    // `#[cbor(tag = ...)]` emits a tag on encode, but tags are transparent on
    // decode. Strip any leading tag layers for marked newtype/unit/tuple
    // structs before delegating to serde's generated visitor; map/array
    // structs also skip tags in their own dispatch loop below.
    fn skip_struct_tags(&mut self, name: &'static str) -> Result<(), Error> {
        let Some(crate::ser::StructMarker { tag: Some(..), .. }) =
            crate::ser::parse_struct_marker(name)
        else {
            return Ok(());
        };

        loop {
            match self.source.pull()? {
                Header::Tag(..) => {}
                header => {
                    self.source.push(header);
                    return Ok(());
                }
            }
        }
    }

    // Captures the wire bytes of the next item; used by `RawValue`.
    fn capture_item(&mut self) -> Result<Vec<u8>, Error> {
        self.source.capture(self.recurse)
    }

    // Pulls the next integer item, skipping any tags other than the bignum
    // tags.
    fn number(&mut self) -> Result<Num, Error> {
        loop {
            let header = self.source.pull()?;

            let neg = match header {
                Header::Positive(x) => return Ok(Num::Pos(x)),
                Header::Negative(x) => return Ok(Num::Neg(x)),
                Header::Tag(tag::BIGPOS) => false,
                Header::Tag(tag::BIGNEG) => true,
                Header::Tag(..) => continue,
                header => return Err(header.expected("integer")),
            };

            let bytes = self.bignum()?;
            return Ok(match neg {
                false => Num::BigPos(bytes),
                true => Num::BigNeg(bytes),
            });
        }
    }

    // Reads the byte string payload following a bignum tag (2 or 3) and
    // strips its leading zeros: an empty result encodes zero (RFC 8949
    // §3.4.3). The payload is owned, so it is always copied.
    fn bignum(&mut self) -> Result<Vec<u8>, Error> {
        let mut bytes = Vec::new();
        match self.source.pull()? {
            Header::Bytes(len) => self.source.bytes_body(len, &mut bytes)?,
            header => return Err(header.expected("bytes")),
        }

        let first = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len());
        bytes.drain(..first);
        Ok(bytes)
    }

    fn unsigned(&mut self) -> Result<u128, Error> {
        match self.number()? {
            Num::Pos(x) => Ok(x.into()),
            Num::BigPos(b) => big_to_u128(&b).ok_or_else(|| de::Error::custom("bigint too large")),
            _ => Err(de::Error::custom("unexpected negative integer")),
        }
    }

    fn unsigned_u64(&mut self) -> Result<u64, Error> {
        loop {
            if let Some(res) = self.source.integer() {
                let (negative, value) = res?;
                return match negative {
                    false => Ok(value),
                    true => Err(de::Error::custom("unexpected negative integer")),
                };
            }

            return match self.source.pull()? {
                Header::Positive(x) => Ok(x),
                Header::Tag(tag::BIGPOS) => {
                    let bytes = self.bignum()?;
                    big_to_u128(&bytes)
                        .and_then(|x| u64::try_from(x).ok())
                        .ok_or_else(|| de::Error::custom("integer too large"))
                }
                Header::Tag(tag::BIGNEG) | Header::Negative(..) => {
                    Err(de::Error::custom("unexpected negative integer"))
                }
                Header::Tag(..) => continue,
                header => Err(header.expected("integer")),
            };
        }
    }

    fn signed(&mut self) -> Result<i128, Error> {
        let raw = match self.number()? {
            Num::Pos(x) => return Ok(x.into()),
            Num::Neg(x) => return Ok(x as i128 ^ !0),
            Num::BigPos(b) => {
                return big_to_u128(&b)
                    .and_then(|x| i128::try_from(x).ok())
                    .ok_or_else(|| de::Error::custom("integer too large"));
            }
            Num::BigNeg(b) => {
                big_to_u128(&b).ok_or_else(|| Error::semantic(None, "integer too large"))?
            }
        };

        match i128::try_from(raw) {
            Ok(x) => Ok(x ^ !0),
            Err(..) => Err(de::Error::custom("integer too large")),
        }
    }

    fn signed_i64(&mut self) -> Result<i64, Error> {
        loop {
            if let Some(res) = self.source.integer() {
                let (negative, value) = res?;
                return match negative {
                    false => {
                        i64::try_from(value).map_err(|_| de::Error::custom("integer too large"))
                    }
                    true => {
                        let value = -1 - i128::from(value);
                        i64::try_from(value).map_err(|_| de::Error::custom("integer too large"))
                    }
                };
            }

            return match self.source.pull()? {
                Header::Positive(x) => {
                    i64::try_from(x).map_err(|_| de::Error::custom("integer too large"))
                }
                Header::Negative(x) => {
                    let value = -1 - i128::from(x);
                    i64::try_from(value).map_err(|_| de::Error::custom("integer too large"))
                }
                Header::Tag(tag::BIGPOS) => {
                    let bytes = self.bignum()?;
                    big_to_u128(&bytes)
                        .and_then(|x| i64::try_from(x).ok())
                        .ok_or_else(|| de::Error::custom("integer too large"))
                }
                Header::Tag(tag::BIGNEG) => {
                    let bytes = self.bignum()?;
                    let raw = big_to_u128(&bytes)
                        .ok_or_else(|| Error::semantic(None, "integer too large"))?;
                    let value = -1
                        - i128::try_from(raw)
                            .map_err(|_| Error::semantic(None, "integer too large"))?;
                    i64::try_from(value).map_err(|_| de::Error::custom("integer too large"))
                }
                Header::Tag(..) => continue,
                header => Err(header.expected("integer")),
            };
        }
    }
}

// Extracts the single `char` of a one-character string, if that is what it is.
#[cfg(feature = "alloc")]
fn single_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

#[cfg(feature = "alloc")]
impl<'de, S: BorrowSource<'de>> de::Deserializer<'de> for &mut Deserializer<S> {
    type Error = Error;

    #[inline]
    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let header = self.source.pull()?;

        // Tags are handled here directly; everything else is pushed back
        // and re-dispatched to the matching typed entry point.
        if let Header::Tag(tag) = header {
            return match tag {
                // Bignums lossy-coerce into plain integers whenever they
                // fit; otherwise they survive as a tagged byte string.
                tag::BIGPOS | tag::BIGNEG => {
                    let b = self.bignum()?;

                    let int = match big_to_u128(&b) {
                        Some(x) if tag == tag::BIGPOS => return visitor.visit_u128(x),
                        Some(x) => i128::try_from(x).ok().map(|x| x ^ !0),
                        None => None,
                    };

                    match int {
                        Some(x) => visitor.visit_i128(x),
                        None => {
                            let access = TagAccess::new(BytesDeserializer::new(&b), Some(tag));
                            visitor.visit_enum(access)
                        }
                    }
                }

                _ => self.recurse(|me| {
                    let access = TagAccess::new(me, Some(tag));
                    visitor.visit_enum(access)
                }),
            };
        }

        self.source.push(header);

        match header {
            Header::Positive(..) => self.deserialize_u64(visitor),
            Header::Negative(x) => match i64::try_from(x) {
                Ok(..) => self.deserialize_i64(visitor),
                Err(..) => self.deserialize_i128(visitor),
            },

            Header::Bytes(..) => self.deserialize_byte_buf(visitor),
            Header::Text(..) => self.deserialize_string(visitor),

            Header::Array(..) => self.deserialize_seq(visitor),
            Header::Map(..) => self.deserialize_map(visitor),

            Header::Float(..) => self.deserialize_f64(visitor),

            Header::Simple(simple::FALSE) => self.deserialize_bool(visitor),
            Header::Simple(simple::TRUE) => self.deserialize_bool(visitor),
            Header::Simple(simple::NULL) => self.deserialize_option(visitor),
            Header::Simple(simple::UNDEFINED) => self.deserialize_option(visitor),
            h @ Header::Simple(..) => Err(h.expected("known simple value")),

            // Only `Break` is left: the tag case was handled above.
            h => Err(h.expected("non-break")),
        }
    }

    #[inline]
    fn deserialize_bool<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        loop {
            if let Some(res) = self.source.bool() {
                return visitor.visit_bool(res?);
            }

            let offset = self.source.offset();

            return match self.source.pull()? {
                Header::Tag(..) => continue,
                Header::Simple(simple::FALSE) => visitor.visit_bool(false),
                Header::Simple(simple::TRUE) => visitor.visit_bool(true),
                _ => Err(Error::semantic(offset, "expected bool")),
            };
        }
    }

    #[inline]
    fn deserialize_f32<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_f64(visitor)
    }

    #[inline]
    fn deserialize_f64<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        loop {
            if let Some(res) = self.source.float() {
                return visitor.visit_f64(res?);
            }

            return match self.source.pull()? {
                Header::Tag(..) => continue,
                Header::Float(x) => visitor.visit_f64(x),
                h => Err(h.expected("float")),
            };
        }
    }

    fn deserialize_i8<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i16<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i32<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i64<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_i64(self.signed_i64()?)
    }

    fn deserialize_i128<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_i128(self.signed()?)
    }

    fn deserialize_u8<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u16<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u32<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u64<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_u64(self.unsigned_u64()?)
    }

    fn deserialize_u128<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_u128(self.unsigned()?)
    }

    fn deserialize_char<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        loop {
            let offset = self.source.offset();
            let header = self.source.pull()?;

            return match header {
                Header::Tag(..) => continue,

                Header::Text(Some(len)) if len <= 4 => match self.source.borrow_body(len) {
                    Some(res) => {
                        let s = core::str::from_utf8(res?).map_err(|_| Error::Syntax(offset))?;
                        match single_char(s) {
                            Some(c) => visitor.visit_char(c),
                            None => Err(header.expected("char")),
                        }
                    }
                    None => {
                        self.scratch.clear();
                        self.source.bytes_body(Some(len), &mut self.scratch)?;
                        match core::str::from_utf8(&self.scratch)
                            .ok()
                            .and_then(single_char)
                        {
                            Some(c) => visitor.visit_char(c),
                            None => Err(Error::Syntax(offset)),
                        }
                    }
                },

                Header::Text(None) => {
                    let mut buffer = String::new();
                    self.source.text_body(None, &mut buffer)?;
                    match single_char(&buffer) {
                        Some(c) => visitor.visit_char(c),
                        None => Err(header.expected("char")),
                    }
                }

                _ => Err(header.expected("char")),
            };
        }
    }

    fn deserialize_str<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        loop {
            return match self.source.pull()? {
                Header::Tag(..) => continue,

                Header::Text(Some(len)) => {
                    let offset = self.source.offset();
                    match self.source.borrow_body(len) {
                        Some(res) => {
                            let s =
                                core::str::from_utf8(res?).map_err(|_| Error::Syntax(offset))?;
                            visitor.visit_borrowed_str(s)
                        }
                        None => {
                            let mut buffer = String::new();
                            self.source.text_body(Some(len), &mut buffer)?;
                            visitor.visit_str(&buffer)
                        }
                    }
                }

                Header::Text(None) => {
                    let mut buffer = String::new();
                    self.source.text_body(None, &mut buffer)?;
                    visitor.visit_str(&buffer)
                }

                header => Err(header.expected("string")),
            };
        }
    }

    fn deserialize_string<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        loop {
            return match self.source.pull()? {
                Header::Tag(..) => continue,

                Header::Text(Some(len)) => {
                    let offset = self.source.offset();
                    match self.source.borrow_body(len) {
                        Some(res) => {
                            let s =
                                core::str::from_utf8(res?).map_err(|_| Error::Syntax(offset))?;
                            visitor.visit_borrowed_str(s)
                        }
                        None => {
                            let mut buffer = String::new();
                            self.source.text_body(Some(len), &mut buffer)?;
                            visitor.visit_string(buffer)
                        }
                    }
                }

                Header::Text(None) => {
                    let mut buffer = String::new();
                    self.source.text_body(None, &mut buffer)?;
                    visitor.visit_string(buffer)
                }

                header => Err(header.expected("string")),
            };
        }
    }

    fn deserialize_bytes<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        loop {
            return match self.source.pull()? {
                Header::Tag(..) => continue,

                Header::Bytes(Some(len)) => match self.source.borrow_body(len) {
                    Some(res) => visitor.visit_borrowed_bytes(res?),
                    None => {
                        self.scratch.clear();
                        self.source.bytes_body(Some(len), &mut self.scratch)?;
                        visitor.visit_bytes(&self.scratch)
                    }
                },

                Header::Bytes(None) => {
                    self.scratch.clear();
                    self.source.bytes_body(None, &mut self.scratch)?;
                    visitor.visit_bytes(&self.scratch)
                }

                // Be liberal: accept an array of integers as bytes.
                Header::Array(len) => self.recurse(|me| visitor.visit_seq(Access(me, len))),

                header => Err(header.expected("bytes")),
            };
        }
    }

    fn deserialize_byte_buf<V: de::Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        loop {
            return match self.source.pull()? {
                Header::Tag(..) => continue,

                Header::Bytes(len) => {
                    let mut buffer = Vec::new();
                    self.source.bytes_body(len, &mut buffer)?;
                    visitor.visit_byte_buf(buffer)
                }

                // Be liberal: accept an array of integers as bytes.
                Header::Array(len) => self.recurse(|me| visitor.visit_seq(Access(me, len))),

                header => Err(header.expected("bytes")),
            };
        }
    }

    fn deserialize_seq<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        loop {
            return match self.source.pull()? {
                Header::Tag(..) => continue,

                Header::Array(len) => self.recurse(|me| visitor.visit_seq(Access(me, len))),

                // Be liberal: accept a byte string as a sequence of integers.
                Header::Bytes(Some(len)) => match self.source.borrow_body(len) {
                    Some(res) => visitor.visit_seq(BorrowedBytesAccess(0, res?)),
                    None => {
                        let mut buffer = Vec::new();
                        self.source.bytes_body(Some(len), &mut buffer)?;
                        visitor.visit_seq(BytesAccess(0, buffer))
                    }
                },

                Header::Bytes(None) => {
                    let mut buffer = Vec::new();
                    self.source.bytes_body(None, &mut buffer)?;
                    visitor.visit_seq(BytesAccess(0, buffer))
                }

                header => Err(header.expected("array")),
            };
        }
    }

    fn deserialize_map<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        loop {
            return match self.source.pull()? {
                Header::Tag(..) => continue,
                Header::Map(len) => self.recurse(|me| visitor.visit_map(Access(me, len))),
                header => Err(header.expected("map")),
            };
        }
    }

    fn deserialize_struct<V: de::Visitor<'de>>(
        self,
        name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        let Some(marker) = crate::ser::parse_struct_marker(name) else {
            return self.deserialize_map(visitor);
        };

        self.skip_struct_tags(name)?;
        loop {
            return match self.source.pull()? {
                Header::Tag(..) => continue,
                Header::Map(len) if marker.shape == crate::ser::StructShape::Map => {
                    self.recurse(|me| visitor.visit_map(StructAccess(me, len, marker.keys)))
                }
                Header::Array(len) if marker.shape == crate::ser::StructShape::Array => {
                    self.recurse(|me| visitor.visit_seq(Access(me, len)))
                }
                header if marker.shape == crate::ser::StructShape::Array => {
                    Err(header.expected("array"))
                }
                header => Err(header.expected("map")),
            };
        }
    }

    fn deserialize_tuple<V: de::Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: de::Visitor<'de>>(
        self,
        name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.skip_struct_tags(name)?;
        self.deserialize_seq(visitor)
    }

    fn deserialize_identifier<V: de::Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        loop {
            let offset = self.source.offset();

            return match self.source.pull()? {
                Header::Tag(..) => continue,

                Header::Text(Some(len)) => match self.source.borrow_body(len) {
                    Some(res) => {
                        let s = core::str::from_utf8(res?).map_err(|_| Error::Syntax(offset))?;
                        visitor.visit_borrowed_str(s)
                    }
                    None => {
                        self.scratch.clear();
                        self.source.bytes_body(Some(len), &mut self.scratch)?;
                        match core::str::from_utf8(&self.scratch) {
                            Ok(s) => visitor.visit_str(s),
                            Err(..) => Err(Error::Syntax(offset)),
                        }
                    }
                },

                Header::Text(None) => {
                    let mut buffer = String::new();
                    self.source.text_body(None, &mut buffer)?;
                    visitor.visit_str(&buffer)
                }

                Header::Bytes(Some(len)) => match self.source.borrow_body(len) {
                    Some(res) => visitor.visit_borrowed_bytes(res?),
                    None => {
                        self.scratch.clear();
                        self.source.bytes_body(Some(len), &mut self.scratch)?;
                        visitor.visit_bytes(&self.scratch)
                    }
                },

                Header::Bytes(None) => {
                    self.scratch.clear();
                    self.source.bytes_body(None, &mut self.scratch)?;
                    visitor.visit_bytes(&self.scratch)
                }

                // Integer keys match struct fields through the key table
                // of a marked struct (handled in `StructAccess`); in any
                // other identifier position they take a placeholder form
                // that matches no field, so they are simply unknown.
                Header::Positive(x) => {
                    use core::fmt::Write as _;

                    self.scratch.clear();
                    let _ = write!(FmtBuf(&mut self.scratch), "{INT_KEY_PLACEHOLDER}{x}");
                    visitor.visit_str(core::str::from_utf8(&self.scratch).expect("decimal"))
                }

                Header::Negative(x) => {
                    use core::fmt::Write as _;

                    self.scratch.clear();
                    let _ = write!(
                        FmtBuf(&mut self.scratch),
                        "{INT_KEY_PLACEHOLDER}{}",
                        -1 - i128::from(x)
                    );
                    visitor.visit_str(core::str::from_utf8(&self.scratch).expect("decimal"))
                }

                header => Err(header.expected("str, bytes or an integer")),
            };
        }
    }

    fn deserialize_ignored_any<V: de::Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_any(visitor)
    }

    #[inline]
    fn deserialize_option<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.source.pull()? {
            Header::Simple(simple::UNDEFINED) => visitor.visit_none(),
            Header::Simple(simple::NULL) => visitor.visit_none(),
            header => {
                self.source.push(header);
                visitor.visit_some(self)
            }
        }
    }

    #[inline]
    fn deserialize_unit<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        loop {
            return match self.source.pull()? {
                Header::Simple(simple::UNDEFINED) => visitor.visit_unit(),
                Header::Simple(simple::NULL) => visitor.visit_unit(),
                Header::Tag(..) => continue,
                header => Err(header.expected("unit")),
            };
        }
    }

    #[inline]
    fn deserialize_unit_struct<V: de::Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.skip_struct_tags(name)?;
        self.deserialize_unit(visitor)
    }

    #[inline]
    fn deserialize_newtype_struct<V: de::Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        // A `RawValue` captures the next item's bytes without decoding.
        if name == crate::raw::NAME {
            return visitor.visit_byte_buf(self.capture_item()?);
        }

        self.skip_struct_tags(name)?;
        visitor.visit_newtype_struct(self)
    }

    #[inline]
    fn deserialize_enum<V: de::Visitor<'de>>(
        self,
        name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if name == crate::tag::NAME {
            let tag = match self.source.pull()? {
                Header::Tag(x) => Some(x),
                header => {
                    self.source.push(header);
                    None
                }
            };

            return self.recurse(|me| visitor.visit_enum(TagAccess::new(me, tag)));
        }

        let marker = crate::ser::parse_struct_marker(name);
        let keys = marker.as_ref().map_or("", |marker| marker.keys);
        let shape = marker.map_or(crate::ser::StructShape::Map, |marker| marker.shape);
        loop {
            // An enum variant is either encoded as a map with a single entry
            // (the variant name and its payload) or, for a unit variant, as
            // a bare text string.
            let map = match self.source.pull()? {
                Header::Tag(..) => continue,
                Header::Map(Some(1)) => true,
                header @ Header::Text(..) => {
                    self.source.push(header);
                    false
                }
                header => return Err(header.expected("enum")),
            };

            return self.recurse(|me| visitor.visit_enum(Enum(me, map, keys, shape)));
        }
    }

    #[inline]
    fn is_human_readable(&self) -> bool {
        false
    }
}

#[cfg(feature = "alloc")]
struct Access<'a, S>(&'a mut Deserializer<S>, Option<usize>);

#[cfg(feature = "alloc")]
impl<'de, S: BorrowSource<'de>> de::SeqAccess<'de> for Access<'_, S> {
    type Error = Error;

    #[inline]
    fn next_element_seed<U: de::DeserializeSeed<'de>>(
        &mut self,
        seed: U,
    ) -> Result<Option<U::Value>, Self::Error> {
        match self.1 {
            Some(0) => return Ok(None),
            Some(x) => self.1 = Some(x - 1),
            None => match self.0.source.pull()? {
                Header::Break => return Ok(None),
                header => self.0.source.push(header),
            },
        }

        seed.deserialize(&mut *self.0).map(Some)
    }

    #[inline]
    fn size_hint(&self) -> Option<usize> {
        self.1
    }
}

#[cfg(feature = "alloc")]
impl<'de, S: BorrowSource<'de>> de::MapAccess<'de> for Access<'_, S> {
    type Error = Error;

    #[inline]
    fn next_key_seed<K: de::DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        match self.1 {
            Some(0) => return Ok(None),
            Some(x) => self.1 = Some(x - 1),
            None => match self.0.source.pull()? {
                Header::Break => return Ok(None),
                header => self.0.source.push(header),
            },
        }

        seed.deserialize(&mut *self.0).map(Some)
    }

    #[inline]
    fn next_value_seed<V: de::DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, Self::Error> {
        seed.deserialize(&mut *self.0)
    }

    #[inline]
    fn size_hint(&self) -> Option<usize> {
        self.1
    }
}

// Map access for a marked struct: integer keys translate to field names
// through the `<field>=<key>` table of the container marker (see
// [`STRUCT_MARKER`](crate::ser::STRUCT_MARKER)); everything else
// deserializes as usual.
#[cfg(feature = "alloc")]
struct StructAccess<'a, S>(&'a mut Deserializer<S>, Option<usize>, &'static str);

#[cfg(feature = "alloc")]
impl<'de, S: BorrowSource<'de>> de::MapAccess<'de> for StructAccess<'_, S> {
    type Error = Error;

    fn next_key_seed<K: de::DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        match self.1 {
            Some(0) => return Ok(None),
            Some(x) => self.1 = Some(x - 1),
            None => match self.0.source.pull()? {
                Header::Break => return Ok(None),
                header => self.0.source.push(header),
            },
        }

        loop {
            let header = self.0.source.pull()?;
            let key = match header {
                Header::Tag(..) => continue,
                Header::Positive(x) => i128::from(x),
                Header::Negative(x) => -1 - i128::from(x),
                header => {
                    self.0.source.push(header);
                    return seed.deserialize(&mut *self.0).map(Some);
                }
            };

            return match crate::ser::field_for_key(self.2, key) {
                Some(field) => seed
                    .deserialize(de::value::StrDeserializer::new(field))
                    .map(Some),
                // An unmapped integer key takes the placeholder form, so
                // it is an unknown field, exactly as in a plain struct.
                None => seed
                    .deserialize(de::value::StringDeserializer::new(format!(
                        "{INT_KEY_PLACEHOLDER}{key}"
                    )))
                    .map(Some),
            };
        }
    }

    #[inline]
    fn next_value_seed<V: de::DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, Self::Error> {
        seed.deserialize(&mut *self.0)
    }

    #[inline]
    fn size_hint(&self) -> Option<usize> {
        self.1
    }
}

// Variant access for an enum item.
//
// The boolean field indicates whether the variant was encoded as a
// single-entry map (`true`) or as a bare text string (`false`). The bare
// form only encodes a unit variant, so payload accesses in that form must
// not consume any further items from the stream. The last field is the
// key table of a marked enum (empty otherwise), applied to struct
// variants.
#[cfg(feature = "alloc")]
struct Enum<'a, S>(
    &'a mut Deserializer<S>,
    bool,
    &'static str,
    crate::ser::StructShape,
);

#[cfg(feature = "alloc")]
impl<'de, S: BorrowSource<'de>> de::EnumAccess<'de> for Enum<'_, S> {
    type Error = Error;
    type Variant = Self;

    #[inline]
    fn variant_seed<V: de::DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), Self::Error> {
        let variant = seed.deserialize(&mut *self.0)?;
        Ok((variant, self))
    }
}

#[cfg(feature = "alloc")]
impl<'de, S: BorrowSource<'de>> de::VariantAccess<'de> for Enum<'_, S> {
    type Error = Error;

    #[inline]
    fn unit_variant(self) -> Result<(), Self::Error> {
        if self.1 {
            // The map form carries a payload; require it to be a unit.
            <() as de::Deserialize>::deserialize(&mut *self.0)?;
        }

        Ok(())
    }

    #[inline]
    fn newtype_variant_seed<U: de::DeserializeSeed<'de>>(
        self,
        seed: U,
    ) -> Result<U::Value, Self::Error> {
        if !self.1 {
            return Err(de::Error::invalid_type(
                de::Unexpected::UnitVariant,
                &"newtype variant",
            ));
        }

        seed.deserialize(&mut *self.0)
    }

    #[inline]
    fn tuple_variant<V: de::Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if !self.1 {
            return Err(de::Error::invalid_type(
                de::Unexpected::UnitVariant,
                &"tuple variant",
            ));
        }

        self.0.deserialize_seq(visitor)
    }

    #[inline]
    fn struct_variant<V: de::Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if !self.1 {
            return Err(de::Error::invalid_type(
                de::Unexpected::UnitVariant,
                &"struct variant",
            ));
        }

        let keys = self.2;
        let shape = self.3;
        loop {
            return match self.0.source.pull()? {
                Header::Tag(..) => continue,
                Header::Map(len) if shape == crate::ser::StructShape::Map => self
                    .0
                    .recurse(|me| visitor.visit_map(StructAccess(me, len, keys))),
                Header::Array(len) if shape == crate::ser::StructShape::Array => {
                    self.0.recurse(|me| visitor.visit_seq(Access(me, len)))
                }
                header if shape == crate::ser::StructShape::Array => Err(header.expected("array")),
                header => Err(header.expected("map")),
            };
        }
    }
}

// Yields the contents of a byte string as a sequence of integers.
#[cfg(feature = "alloc")]
struct BytesAccess(usize, Vec<u8>);

#[cfg(feature = "alloc")]
impl<'de> de::SeqAccess<'de> for BytesAccess {
    type Error = Error;

    #[inline]
    fn next_element_seed<U: de::DeserializeSeed<'de>>(
        &mut self,
        seed: U,
    ) -> Result<Option<U::Value>, Self::Error> {
        use de::IntoDeserializer;

        if self.0 < self.1.len() {
            let byte = self.1[self.0];
            self.0 += 1;
            seed.deserialize(byte.into_deserializer()).map(Some)
        } else {
            Ok(None)
        }
    }

    #[inline]
    fn size_hint(&self) -> Option<usize> {
        Some(self.1.len() - self.0)
    }
}

// Yields the contents of a borrowed byte string as a sequence of integers.
#[cfg(feature = "alloc")]
struct BorrowedBytesAccess<'de>(usize, &'de [u8]);

#[cfg(feature = "alloc")]
impl<'de> de::SeqAccess<'de> for BorrowedBytesAccess<'de> {
    type Error = Error;

    #[inline]
    fn next_element_seed<U: de::DeserializeSeed<'de>>(
        &mut self,
        seed: U,
    ) -> Result<Option<U::Value>, Self::Error> {
        use de::IntoDeserializer;

        if self.0 < self.1.len() {
            let byte = self.1[self.0];
            self.0 += 1;
            seed.deserialize(byte.into_deserializer()).map(Some)
        } else {
            Ok(None)
        }
    }

    #[inline]
    fn size_hint(&self) -> Option<usize> {
        Some(self.1.len() - self.0)
    }
}

/// An iterator decoding consecutive top-level items from a reader.
///
/// Created by [`Deserializer::into_iter`].
#[cfg(feature = "alloc")]
pub struct Iter<T, R> {
    de: Deserializer<ReaderSource<R>>,
    _marker: core::marker::PhantomData<T>,
}

#[cfg(feature = "alloc")]
impl<T: de::DeserializeOwned, R: Read> Iterator for Iter<T, R> {
    type Item = Result<T, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let decoder = &mut self.de.source.0;
        let start = decoder.offset();

        // Probe for a clean end of input: end-of-file before the first byte
        // of an item terminates the stream, anywhere else it is an error.
        match decoder.pull() {
            Ok(header) => decoder.push(header),
            Err(crate::core::Error::Io(err))
                if err.kind() == crate::io::ErrorKind::UnexpectedEof
                    && decoder.offset() == start =>
            {
                return None;
            }
            Err(err) => return Some(Err(err.into())),
        }

        Some(T::deserialize(&mut self.de))
    }
}

/// Checks that the input contains exactly one well-formed CBOR item.
///
/// The input is walked structurally without building any values: **no heap
/// memory is allocated**. String bodies are skipped through a fixed-size
/// stack buffer and nesting is bounded by [`DEFAULT_RECURSION_LIMIT`], so
/// adversarial input can neither exhaust memory nor the stack.
///
/// Beyond well-formedness (RFC 8949 §5.3.1) this verifies that text strings
/// are valid UTF-8 (every segment of an indefinite-length text string on
/// its own, as the RFC requires). Unassigned simple values are accepted:
/// they are well-formed, even though the serde interface has no
/// representation for them.
///
/// Trailing data after the item is an error; to handle a CBOR sequence
/// (RFC 8742), validate items one at a time from the shared reader.
///
/// ```rust
/// assert!(cbor2::validate(&b"\x83\x01\x02\x03"[..]).is_ok()); // [1, 2, 3]
/// assert!(cbor2::validate(&b"\x83\x01\x02"[..]).is_err()); // truncated
/// assert!(cbor2::validate(&b"\x62\xff\xfe"[..]).is_err()); // invalid UTF-8
/// ```
pub fn validate<R: Read>(reader: R) -> Result<(), Error> {
    let mut decoder = Decoder::from(reader);
    validate_item(&mut decoder, DEFAULT_RECURSION_LIMIT)?;
    expect_eof(&mut decoder)
}

// Requires the input to be exhausted.
pub(crate) fn expect_eof<R: Read>(decoder: &mut Decoder<R>) -> Result<(), Error> {
    let offset = decoder.offset();
    let mut probe = [0u8; 1];
    match decoder.read_exact(&mut probe) {
        Err(err) if err.kind() == crate::io::ErrorKind::UnexpectedEof => Ok(()),
        Err(err) => Err(Error::Io(err)),
        Ok(()) => Err(Error::semantic(offset, "trailing data after the item")),
    }
}

fn validate_item<R: Read>(decoder: &mut Decoder<R>, depth: usize) -> Result<(), Error> {
    let offset = decoder.offset();
    let header = decoder.pull()?;
    validate_header(decoder, header, offset, depth)
}

fn validate_header<R: Read>(
    decoder: &mut Decoder<R>,
    header: Header,
    offset: usize,
    depth: usize,
) -> Result<(), Error> {
    if depth == 0 {
        return Err(Error::RecursionLimitExceeded);
    }

    match header {
        Header::Positive(..) | Header::Negative(..) | Header::Float(..) | Header::Simple(..) => {
            Ok(())
        }

        Header::Break => Err(Error::Syntax(offset)),

        Header::Tag(..) => validate_item(decoder, depth - 1),

        Header::Bytes(len) => match len {
            Some(len) => skip_body(decoder, len),
            None => loop {
                let offset = decoder.offset();
                match decoder.pull()? {
                    Header::Break => return Ok(()),
                    // Segments must be definite-length strings of the same
                    // major type (RFC 8949 §3.2.3).
                    Header::Bytes(Some(len)) => skip_body(decoder, len)?,
                    _ => return Err(Error::Syntax(offset)),
                }
            },
        },

        Header::Text(len) => match len {
            Some(len) => check_utf8_body(decoder, len),
            None => loop {
                let offset = decoder.offset();
                match decoder.pull()? {
                    Header::Break => return Ok(()),
                    Header::Text(Some(len)) => check_utf8_body(decoder, len)?,
                    _ => return Err(Error::Syntax(offset)),
                }
            },
        },

        Header::Array(len) => match len {
            Some(len) => {
                for _ in 0..len {
                    validate_item(decoder, depth - 1)?;
                }
                Ok(())
            }
            None => loop {
                let offset = decoder.offset();
                match decoder.pull()? {
                    Header::Break => return Ok(()),
                    header => validate_header(decoder, header, offset, depth - 1)?,
                }
            },
        },

        Header::Map(len) => match len {
            Some(len) => {
                for _ in 0..len {
                    validate_item(decoder, depth - 1)?; // key
                    validate_item(decoder, depth - 1)?; // value
                }
                Ok(())
            }
            None => {
                let mut expecting_value = false;
                loop {
                    let offset = decoder.offset();
                    match decoder.pull()? {
                        // A break in place of a value leaves a dangling key,
                        // which is not well-formed (RFC 8949 §5.3.1).
                        Header::Break if expecting_value => return Err(Error::Syntax(offset)),
                        Header::Break => return Ok(()),
                        header => {
                            validate_header(decoder, header, offset, depth - 1)?;
                            expecting_value = !expecting_value;
                        }
                    }
                }
            }
        },
    }
}

// Discards a definite-length body through a fixed-size buffer; a forged
// length cannot trigger an allocation.
fn skip_body<R: Read>(decoder: &mut Decoder<R>, mut remaining: usize) -> Result<(), Error> {
    let mut buffer = [0u8; 4096];
    while remaining > 0 {
        let n = remaining.min(buffer.len());
        decoder.read_exact(&mut buffer[..n])?;
        remaining -= n;
    }
    Ok(())
}

// Discards a definite-length text body, verifying that the whole body is
// valid UTF-8. Characters may straddle the internal chunk boundaries; up to
// three trailing bytes of an incomplete character carry over to the next
// chunk.
fn check_utf8_body<R: Read>(decoder: &mut Decoder<R>, len: usize) -> Result<(), Error> {
    let offset = decoder.offset();
    let mut buffer = [0u8; 4096];
    let mut carry = 0usize;
    let mut remaining = len;

    while remaining > 0 {
        let n = remaining.min(buffer.len() - carry);
        decoder.read_exact(&mut buffer[carry..carry + n])?;
        remaining -= n;
        let filled = carry + n;

        match core::str::from_utf8(&buffer[..filled]) {
            Ok(..) => carry = 0,
            Err(err) => {
                // An incomplete character is only acceptable while more
                // body bytes are coming.
                if err.error_len().is_some() || remaining == 0 {
                    return Err(Error::Syntax(offset));
                }

                let valid = err.valid_up_to();
                buffer.copy_within(valid..filled, 0);
                carry = filled - valid;
            }
        }
    }

    Ok(())
}

/// Deserializes a value from CBOR read out of a [`Read`].
///
/// With the `std` feature any `std::io::Read` is accepted; for repeated
/// small reads consider wrapping the reader in a `std::io::BufReader`.
///
/// This reads one leading CBOR item and leaves any following bytes unread.
/// Use [`validate`] when an input must contain exactly one well-formed item,
/// or [`Deserializer::into_iter`] to decode a CBOR sequence.
///
/// ```rust
/// let bytes = cbor2::to_vec(&("ok", 200u16)).unwrap();
/// let value: (String, u16) = cbor2::from_reader(&bytes[..]).unwrap();
/// assert_eq!(value, ("ok".to_string(), 200));
/// ```
#[cfg(feature = "alloc")]
#[inline]
pub fn from_reader<T: de::DeserializeOwned, R: Read>(reader: R) -> Result<T, Error> {
    let mut deserializer = Deserializer::from_reader(reader);
    T::deserialize(&mut deserializer)
}

/// Deserializes a value from a byte slice of CBOR.
///
/// This decodes the first complete CBOR item in `slice`. It does not report
/// trailing data; call [`validate`] first if trailing bytes should be an
/// error. Definite-length text and byte strings can be borrowed from the
/// input slice, so targets such as `&str` and `serde_bytes::Bytes` do not
/// require intermediate allocation. Indefinite-length segmented strings
/// cannot be borrowed because their logical body is not contiguous.
///
/// ```rust
/// let mut bytes = cbor2::to_vec(&1u8).unwrap();
/// bytes.extend(cbor2::to_vec(&2u8).unwrap());
///
/// assert_eq!(cbor2::from_slice::<u8>(&bytes).unwrap(), 1);
/// assert!(cbor2::validate(&bytes[..]).is_err());
/// ```
#[cfg(feature = "alloc")]
#[inline]
pub fn from_slice<'de, T: de::Deserialize<'de>>(slice: &'de [u8]) -> Result<T, Error> {
    let mut deserializer = Deserializer::from_slice(slice);
    T::deserialize(&mut deserializer)
}

#[cfg(all(test, feature = "alloc"))]
mod tests {
    use alloc::{string::String, vec, vec::Vec};

    // Round-trips through the serde entry points using only the crate's
    // own io implementations, so that `cargo test --no-default-features
    // --features alloc` exercises the no_std configuration end to end.
    #[test]
    fn slice_roundtrip() {
        let value = (1u8, "two", vec![3u32, 4]);
        let bytes = crate::ser::to_vec(&value).unwrap();

        let back: (u8, String, Vec<u32>) = super::from_slice(&bytes).unwrap();
        assert_eq!(back, (1, String::from("two"), vec![3, 4]));
        assert!(super::validate(&bytes[..]).is_ok());
        assert!(super::validate(&bytes[..bytes.len() - 1]).is_err());
    }
}
