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
use crate::simple::SimpleAccess;
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
                Header::Negative(x) => match i64::try_from(x) {
                    Ok(x) => de::Unexpected::Signed(-1 - x),
                    Err(..) => de::Unexpected::Other("large negative integer"),
                },
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

#[cfg(feature = "alloc")]
// Limit the total bytes scanned for a primitive, independently of its
// numeric range. Ordinary padded magnitudes can span several segments.
const PRIMITIVE_BIGNUM_BYTE_LIMIT: usize = 4096;

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
    #[doc(hidden)]
    fn read_exact(&mut self, data: &mut [u8]) -> Result<(), Error>;
    #[doc(hidden)]
    fn skip_item(&mut self, recurse: usize) -> Result<(), Error>;
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
    #[doc(hidden)]
    fn borrow_item(&mut self, _recurse: usize) -> Option<Result<&'de [u8], Error>> {
        None
    }
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

    #[inline]
    fn read_exact(&mut self, data: &mut [u8]) -> Result<(), Error> {
        self.0.read_exact(data).map_err(Error::Io)
    }

    #[inline]
    fn skip_item(&mut self, recurse: usize) -> Result<(), Error> {
        let mut scratch = [0u8; SKIP_CHUNK];
        validate_item::<_, true>(&mut self.0, recurse, &mut scratch)
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
pub struct SliceSource<'de>(Decoder<&'de [u8]>, &'de [u8]);

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
            None => loop {
                let offset = self.0.offset();
                match self.0.pull_slice()? {
                    Header::Break => return Ok(()),
                    Header::Bytes(Some(len)) => out.extend_from_slice(self.0.borrow_body(len)?),
                    _ => return Err(crate::core::Error::Syntax(offset)),
                }
            },
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
            None => loop {
                let offset = self.0.offset();
                match self.0.pull_slice()? {
                    Header::Break => return Ok(()),
                    Header::Text(Some(len)) => {
                        let body_offset = self.0.offset();
                        let body = self.0.borrow_body(len)?;
                        let text = core::str::from_utf8(body)
                            .map_err(|_| crate::core::Error::Syntax(body_offset))?;
                        out.push_str(text);
                    }
                    _ => return Err(crate::core::Error::Syntax(offset)),
                }
            },
        }
    }

    #[inline]
    fn read_exact(&mut self, data: &mut [u8]) -> Result<(), Error> {
        self.0.read_exact(data).map_err(Error::Io)
    }

    #[inline]
    fn skip_item(&mut self, recurse: usize) -> Result<(), Error> {
        validate_item_slice::<true>(&mut self.0, recurse)
    }

    fn capture(&mut self, recurse: usize) -> Result<Vec<u8>, Error> {
        let start = self.0.offset();
        validate_item_slice::<true>(&mut self.0, recurse)?;
        Ok(self.1[start..self.0.offset()].to_vec())
    }
}

#[cfg(feature = "alloc")]
impl<'de> BorrowSource<'de> for SliceSource<'de> {
    #[inline]
    fn borrow_body(&mut self, len: usize) -> Option<Result<&'de [u8], crate::io::Error>> {
        Some(self.0.borrow_body(len))
    }
    fn borrow_item(&mut self, recurse: usize) -> Option<Result<&'de [u8], Error>> {
        let start = self.0.offset();
        Some(
            validate_item_slice::<true>(&mut self.0, recurse)
                .map(|()| &self.1[start..self.0.offset()]),
        )
    }
}

// Captures one well-formed item's wire bytes from a decoder; used by
// `RawValue` through the reader source (the slice source captures through
// its copy-free skip instead).
#[cfg(feature = "alloc")]
fn capture_item<R: Read>(decoder: &mut Decoder<R>, recurse: usize) -> Result<Vec<u8>, Error> {
    let mut scratch = [0u8; SKIP_CHUNK];
    decoder.start_recording();
    let result = validate_item::<_, true>(decoder, recurse, &mut scratch);
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
    preserve_simple: bool,
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
            preserve_simple: false,
        }
    }

    /// Turns this deserializer into an iterator over consecutive top-level
    /// items.
    ///
    /// CBOR allows concatenating encoded items into a *sequence* (RFC 8742).
    /// The iterator yields decoded items until the input is exhausted; a
    /// clean end of input terminates the iterator, while anything else
    /// (including a truncated item) yields one error and ends iteration.
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
            de: Some(self),
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
            source: SliceSource(slice.into(), slice),
            scratch: Vec::new(),
            recurse: limit,
            preserve_simple: false,
        }
    }

    /// Turns this deserializer into an iterator over consecutive top-level
    /// items of a CBOR sequence (RFC 8742) held in memory.
    ///
    /// Unlike the reader form, items may borrow from the slice. The iterator
    /// ends when the input is exhausted and stops after the first error,
    /// since the position inside a failed item is unspecified.
    ///
    /// ```rust
    /// let mut stream = Vec::new();
    /// cbor2::to_writer(&"one", &mut stream).unwrap();
    /// cbor2::to_writer(&"two", &mut stream).unwrap();
    ///
    /// let words: Vec<&str> = cbor2::de::Deserializer::from_slice(&stream)
    ///     .into_iter()
    ///     .collect::<Result<_, _>>()
    ///     .unwrap();
    ///
    /// assert_eq!(words, ["one", "two"]);
    /// ```
    // Named for symmetry with `serde_json::Deserializer::into_iter`.
    #[allow(clippy::should_implement_trait)]
    pub fn into_iter<T: de::Deserialize<'de>>(self) -> SliceIter<'de, T> {
        SliceIter {
            de: Some(self),
            _marker: core::marker::PhantomData,
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

    // Typed positions ignore wrapper tags, but their payload (including
    // nested containers) must still fit the remaining nesting budget.
    #[inline]
    fn with_header<T>(
        &mut self,
        keep_bignums: bool,
        visit: impl FnOnce(&mut Self, Header, usize) -> Result<T, Error>,
    ) -> Result<T, Error> {
        let mut remaining = self.recurse;
        loop {
            let offset = self.offset();
            let header = self.source.pull()?;
            match header {
                Header::Tag(t) if !(keep_bignums && matches!(t, tag::BIGPOS | tag::BIGNEG)) => {
                    remaining = remaining
                        .checked_sub(1)
                        .ok_or(Error::RecursionLimitExceeded)?;
                }
                _ => {
                    let saved = core::mem::replace(&mut self.recurse, remaining);
                    let result = visit(self, header, offset);
                    self.recurse = saved;
                    return result;
                }
            }
        }
    }

    // Captures the wire bytes of the next item; used by `RawValue`.
    fn capture_item(&mut self) -> Result<Vec<u8>, Error> {
        self.source.capture(self.recurse)
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

    fn bounded_bignum(&mut self, too_large: &'static str) -> Result<u128, Error> {
        if self.recurse == 0 {
            return Err(Error::RecursionLimitExceeded);
        }
        let mut budget = PRIMITIVE_BIGNUM_BYTE_LIMIT;
        let mut value = 0u128;
        let mut segment = |source: &mut S, mut len: usize| -> Result<(), Error> {
            budget = budget
                .checked_sub(len)
                .ok_or_else(|| <Error as de::Error>::custom(too_large))?;
            let mut buf = [0; 64];
            while len > 0 {
                let n = len.min(buf.len());
                source.read_exact(&mut buf[..n])?;
                for &byte in &buf[..n] {
                    value = value
                        .checked_mul(256)
                        .ok_or_else(|| <Error as de::Error>::custom(too_large))?
                        | u128::from(byte);
                }
                len -= n;
            }
            Ok(())
        };
        match self.source.pull()? {
            Header::Bytes(Some(len)) => segment(&mut self.source, len)?,
            Header::Bytes(None) => loop {
                let offset = self.offset();
                match self.source.pull()? {
                    Header::Break => break,
                    Header::Bytes(Some(len)) => segment(&mut self.source, len)?,
                    _ => return Err(Error::Syntax(offset)),
                }
            },
            header => return Err(header.expected("bytes")),
        }
        Ok(value)
    }

    fn float(&mut self) -> Result<f64, Error> {
        if let Some(result) = self.source.float() {
            return result;
        }
        self.with_header(false, |_, header, _| match header {
            Header::Float(value) => Ok(value),
            header => Err(header.expected("float")),
        })
    }

    fn unsigned(&mut self) -> Result<u128, Error> {
        self.with_header(true, |me, header, _| match header {
            Header::Positive(x) => Ok(x.into()),
            Header::Tag(tag::BIGPOS) => me.bounded_bignum("bigint too large"),
            Header::Tag(tag::BIGNEG) | Header::Negative(..) => {
                Err(de::Error::custom("unexpected negative integer"))
            }
            header => Err(header.expected("integer")),
        })
    }

    fn unsigned_u64(&mut self) -> Result<u64, Error> {
        if let Some(res) = self.source.integer() {
            let (negative, value) = res?;
            return match negative {
                false => Ok(value),
                true => Err(de::Error::custom("unexpected negative integer")),
            };
        }

        self.with_header(true, |me, header, _| match header {
            Header::Positive(x) => Ok(x),
            Header::Tag(tag::BIGPOS) => {
                let raw = me.bounded_bignum("integer too large")?;
                u64::try_from(raw).map_err(|_| de::Error::custom("integer too large"))
            }
            Header::Tag(tag::BIGNEG) | Header::Negative(..) => {
                Err(de::Error::custom("unexpected negative integer"))
            }
            header => Err(header.expected("integer")),
        })
    }

    fn signed(&mut self) -> Result<i128, Error> {
        self.with_header(true, |me, header, _| match header {
            Header::Positive(x) => Ok(x.into()),
            Header::Negative(x) => Ok(x as i128 ^ !0),
            Header::Tag(tag::BIGPOS) => me.bounded_bignum("integer too large").and_then(|x| {
                i128::try_from(x).map_err(|_| de::Error::custom("integer too large"))
            }),
            Header::Tag(tag::BIGNEG) => {
                let raw = me.bounded_bignum("integer too large")?;
                match i128::try_from(raw) {
                    Ok(x) => Ok(x ^ !0),
                    Err(..) => Err(de::Error::custom("integer too large")),
                }
            }
            header => Err(header.expected("integer")),
        })
    }

    fn signed_i64(&mut self) -> Result<i64, Error> {
        if let Some(res) = self.source.integer() {
            let (negative, value) = res?;
            return match negative {
                false => i64::try_from(value).map_err(|_| de::Error::custom("integer too large")),
                true => {
                    let value = -1 - i128::from(value);
                    i64::try_from(value).map_err(|_| de::Error::custom("integer too large"))
                }
            };
        }

        self.with_header(true, |me, header, _| match header {
            Header::Positive(x) => {
                i64::try_from(x).map_err(|_| de::Error::custom("integer too large"))
            }
            Header::Negative(x) => {
                let value = -1 - i128::from(x);
                i64::try_from(value).map_err(|_| de::Error::custom("integer too large"))
            }
            Header::Tag(tag::BIGPOS) => {
                let raw = me.bounded_bignum("integer too large")?;
                i64::try_from(raw).map_err(|_| de::Error::custom("integer too large"))
            }
            Header::Tag(tag::BIGNEG) => {
                let raw = me.bounded_bignum("integer too large")?;
                let value = -1
                    - i128::try_from(raw)
                        .map_err(|_| Error::semantic(None, "integer too large"))?;
                i64::try_from(value).map_err(|_| de::Error::custom("integer too large"))
            }
            header => Err(header.expected("integer")),
        })
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

    // Dispatches on the pulled header directly: pushing it back for a typed
    // entry point would bypass the slice fast paths and pull it twice.
    #[inline]
    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self.source.pull()? {
            Header::Positive(x) => visitor.visit_u64(x),
            Header::Negative(x) => match i64::try_from(x) {
                Ok(x) => visitor.visit_i64(-1 - x),
                Err(..) => visitor.visit_i128(x as i128 ^ !0),
            },

            Header::Bytes(len) => self.visit_byte_buf(len, visitor),
            Header::Text(len) => self.visit_string(len, visitor),

            Header::Array(len) => self.visit_array(len, visitor),
            Header::Map(len) => self.visit_object(len, "", visitor),

            Header::Float(x) => visitor.visit_f64(x),

            Header::Simple(simple::FALSE) => visitor.visit_bool(false),
            Header::Simple(simple::TRUE) => visitor.visit_bool(true),
            Header::Simple(simple::NULL) => visitor.visit_none(),
            Header::Simple(simple::UNDEFINED) if !self.preserve_simple => visitor.visit_none(),
            Header::Simple(x) => {
                let simple = crate::Simple::new(x).expect("decoder returns valid simple values");
                visitor.visit_enum(SimpleAccess::new(simple))
            }

            // Bignums lossy-coerce into plain integers whenever they fit;
            // otherwise they survive as a tagged byte string. A bignum tag
            // around anything else is still well-formed and stays a tag.
            Header::Tag(tag @ (tag::BIGPOS | tag::BIGNEG)) => {
                if self.recurse == 0 {
                    return Err(Error::RecursionLimitExceeded);
                }
                let payload = self.source.pull()?;
                let bytes = matches!(payload, Header::Bytes(..));
                self.source.push(payload);
                if !bytes {
                    return self.recurse(|me| visitor.visit_enum(TagAccess::new(me, Some(tag))));
                }

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

            Header::Tag(tag) => self.recurse(|me| {
                let access = TagAccess::new(me, Some(tag));
                visitor.visit_enum(access)
            }),

            header @ Header::Break => Err(header.expected("non-break")),
        }
    }

    #[inline]
    fn deserialize_bool<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        if let Some(res) = self.source.bool() {
            return visitor.visit_bool(res?);
        }
        self.with_header(false, |_, header, offset| match header {
            Header::Simple(simple::FALSE) => visitor.visit_bool(false),
            Header::Simple(simple::TRUE) => visitor.visit_bool(true),
            _ => Err(Error::semantic(offset, "expected bool")),
        })
    }

    #[inline]
    fn deserialize_f32<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        let value = self.float()?;
        match crate::core::f64_to_f32(value) {
            Some(bits) => visitor.visit_f32(f32::from_bits(bits)),
            None => visitor.visit_f64(value),
        }
    }

    #[inline]
    fn deserialize_f64<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        visitor.visit_f64(self.float()?)
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
        self.with_header(false, |me, header, offset| match header {
            // A char is at most four UTF-8 bytes; a reader fills a stack
            // buffer so both sources report errors the same way.
            Header::Text(Some(len)) if len <= 4 => {
                let mut buffer = [0u8; 4];
                let bytes = match me.source.borrow_body(len) {
                    Some(res) => res?,
                    None => {
                        me.source.read_exact(&mut buffer[..len])?;
                        &buffer[..len]
                    }
                };
                let s = core::str::from_utf8(bytes).map_err(|_| Error::Syntax(offset))?;
                match single_char(s) {
                    Some(c) => visitor.visit_char(c),
                    None => Err(header.expected("char")),
                }
            }

            Header::Text(None) => {
                let mut buffer = String::new();
                me.source.text_body(None, &mut buffer)?;
                match single_char(&buffer) {
                    Some(c) => visitor.visit_char(c),
                    None => Err(header.expected("char")),
                }
            }

            _ => Err(header.expected("char")),
        })
    }

    fn deserialize_str<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.with_header(false, |me, header, _| match header {
            Header::Text(len) => {
                let offset = me.source.offset();
                me.visit_str(len, offset, visitor)
            }
            header => Err(header.expected("string")),
        })
    }

    fn deserialize_string<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.with_header(false, |me, header, _| match header {
            Header::Text(len) => me.visit_string(len, visitor),
            header => Err(header.expected("string")),
        })
    }

    fn deserialize_bytes<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.with_header(false, |me, header, _| match header {
            Header::Bytes(len) => me.visit_bytes(len, visitor),
            // Be liberal: accept an array of integers as bytes.
            Header::Array(len) => me.visit_array(len, visitor),
            header => Err(header.expected("bytes")),
        })
    }

    fn deserialize_byte_buf<V: de::Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.with_header(false, |me, header, _| match header {
            Header::Bytes(len) => me.visit_byte_buf(len, visitor),
            // Be liberal: accept an array of integers as bytes.
            Header::Array(len) => me.visit_array(len, visitor),
            header => Err(header.expected("bytes")),
        })
    }

    fn deserialize_seq<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.with_header(false, |me, header, _| match header {
            Header::Array(len) => me.visit_array(len, visitor),

            // Be liberal: accept a byte string as a sequence of integers.
            Header::Bytes(len) => {
                if let Some(res) = len.and_then(|len| me.source.borrow_body(len)) {
                    return visit_byte_seq(res?, visitor);
                }
                me.scratch.clear();
                me.source.bytes_body(len, &mut me.scratch)?;
                visit_byte_seq(&me.scratch, visitor)
            }

            header => Err(header.expected("array")),
        })
    }

    fn deserialize_map<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        self.with_header(false, |me, header, _| match header {
            Header::Map(len) => me.visit_object(len, "", visitor),
            header => Err(header.expected("map")),
        })
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

        self.with_header(false, |me, header, _| match header {
            Header::Map(len) if marker.shape == crate::ser::StructShape::Map => {
                me.visit_object(len, marker.keys, visitor)
            }
            Header::Array(len) if marker.shape == crate::ser::StructShape::Array => {
                me.visit_array(len, visitor)
            }
            header if marker.shape == crate::ser::StructShape::Array => {
                Err(header.expected("array"))
            }
            header => Err(header.expected("map")),
        })
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
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_identifier<V: de::Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.with_header(false, |me, header, offset| match header {
            Header::Text(len) => me.visit_str(len, offset, visitor),
            Header::Bytes(len) => me.visit_bytes(len, visitor),

            // Integer keys match struct fields through the key table
            // of a marked struct (handled in `StructAccess`); in any
            // other identifier position they take a placeholder form
            // that matches no field, so they are simply unknown.
            Header::Positive(x) => {
                use core::fmt::Write as _;

                me.scratch.clear();
                let _ = write!(FmtBuf(&mut me.scratch), "{INT_KEY_PLACEHOLDER}{x}");
                visitor.visit_str(core::str::from_utf8(&me.scratch).expect("decimal"))
            }

            Header::Negative(x) => {
                use core::fmt::Write as _;

                me.scratch.clear();
                let _ = write!(
                    FmtBuf(&mut me.scratch),
                    "{INT_KEY_PLACEHOLDER}{}",
                    -1 - i128::from(x)
                );
                visitor.visit_str(core::str::from_utf8(&me.scratch).expect("decimal"))
            }

            header => Err(header.expected("str, bytes or an integer")),
        })
    }

    fn deserialize_ignored_any<V: de::Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        self.source.skip_item(self.recurse)?;
        visitor.visit_unit()
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
        self.with_header(false, |_, header, _| match header {
            Header::Simple(simple::UNDEFINED) => visitor.visit_unit(),
            Header::Simple(simple::NULL) => visitor.visit_unit(),
            header => Err(header.expected("unit")),
        })
    }

    #[inline]
    fn deserialize_unit_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
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
        if name == crate::flatten::fields::RAW_ITEM {
            return match self.source.borrow_item(self.recurse) {
                Some(bytes) => visitor.visit_borrowed_bytes(bytes?),
                None => visitor.visit_byte_buf(self.capture_item()?),
            };
        }
        if name == crate::raw::BORROWED_NAME {
            return match self.source.borrow_item(self.recurse) {
                Some(bytes) => visitor.visit_borrowed_bytes(bytes?),
                None => Err(Error::semantic(
                    self.offset(),
                    "raw item requires a borrowed source",
                )),
            };
        }

        if let Some(tag) = crate::ser::parse_struct_marker(name).and_then(|marker| marker.tag) {
            let header = self.source.pull()?;
            if header == Header::Tag(tag) {
                return self.recurse(|me| visitor.visit_newtype_struct(me));
            }
            self.source.push(header);
        }
        visitor.visit_newtype_struct(self)
    }

    #[inline]
    fn deserialize_enum<V: de::Visitor<'de>>(
        self,
        name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if name == crate::simple::NAME {
            return self.with_header(false, |_, header, _| match header {
                Header::Simple(x) => {
                    let simple =
                        crate::Simple::new(x).expect("decoder returns valid simple values");
                    visitor.visit_enum(SimpleAccess::new(simple))
                }
                header => Err(header.expected("simple value")),
            });
        }

        if name == crate::tag::NAME {
            return match self.source.pull()? {
                Header::Tag(tag) => {
                    self.recurse(|me| visitor.visit_enum(TagAccess::new(me, Some(tag))))
                }
                header => {
                    self.source.push(header);
                    visitor.visit_enum(TagAccess::new(self, None))
                }
            };
        }

        let marker = crate::ser::parse_struct_marker(name);
        let keys = marker.as_ref().map_or("", |marker| marker.keys);
        let shape = marker.map_or(crate::ser::StructShape::Map, |marker| marker.shape);
        self.with_header(false, |me, header, _| {
            // An enum variant is either encoded as a map with a single entry
            // (the variant name and its payload) or, for a unit variant, as
            // a bare text string. Be liberal: the map may be
            // indefinite-length, in which case the break is checked after
            // the entry.
            let form = match header {
                Header::Map(Some(1)) => EnumForm::Map,
                Header::Map(None) => EnumForm::IndefiniteMap,
                header @ Header::Text(..) => {
                    me.source.push(header);
                    EnumForm::Text
                }
                header => return Err(header.expected("enum")),
            };

            if form == EnumForm::Text {
                visitor.visit_enum(Enum(me, form, keys, shape))
            } else {
                me.recurse(|me| visitor.visit_enum(Enum(me, form, keys, shape)))
            }
        })
    }

    #[inline]
    fn is_human_readable(&self) -> bool {
        false
    }
}

#[cfg(feature = "alloc")]
struct Access<'a, S> {
    de: &'a mut Deserializer<S>,
    remaining: Option<usize>,
}

#[cfg(feature = "alloc")]
impl<S: Source> Access<'_, S> {
    #[inline]
    fn next(&mut self) -> Result<bool, Error> {
        match self.remaining {
            Some(0) => return Ok(false),
            Some(n) => self.remaining = Some(n - 1),
            None => match self.de.source.pull()? {
                Header::Break => {
                    self.remaining = Some(0);
                    return Ok(false);
                }
                header => self.de.source.push(header),
            },
        }
        Ok(true)
    }

    #[inline]
    fn finish(self) -> Result<(), Error> {
        match self.remaining {
            Some(0) => Ok(()),
            None if matches!(self.de.source.pull()?, Header::Break) => Ok(()),
            _ => Err(Error::semantic(
                self.de.offset(),
                "unconsumed container elements",
            )),
        }
    }
}

#[cfg(feature = "alloc")]
impl<'de, S: BorrowSource<'de>> Deserializer<S> {
    // Visits a text body as `&str`: borrowed from a slice source, otherwise
    // staged in the scratch buffer. `offset` locates UTF-8 errors.
    #[inline]
    fn visit_str<V: de::Visitor<'de>>(
        &mut self,
        len: Option<usize>,
        offset: usize,
        visitor: V,
    ) -> Result<V::Value, Error> {
        let Some(len) = len else {
            // Segments are validated one by one, so they need a String.
            let mut buffer = String::new();
            self.source.text_body(None, &mut buffer)?;
            return visitor.visit_str(&buffer);
        };
        if let Some(res) = self.source.borrow_body(len) {
            let s = core::str::from_utf8(res?).map_err(|_| Error::Syntax(offset))?;
            return visitor.visit_borrowed_str(s);
        }
        self.scratch.clear();
        self.source.bytes_body(Some(len), &mut self.scratch)?;
        match core::str::from_utf8(&self.scratch) {
            Ok(s) => visitor.visit_str(s),
            Err(..) => Err(Error::Syntax(offset)),
        }
    }

    // Visits a text body as an owned `String`, unless it can be borrowed.
    #[inline]
    fn visit_string<V: de::Visitor<'de>>(
        &mut self,
        len: Option<usize>,
        visitor: V,
    ) -> Result<V::Value, Error> {
        if let Some(len) = len {
            let offset = self.source.offset();
            if let Some(res) = self.source.borrow_body(len) {
                let s = core::str::from_utf8(res?).map_err(|_| Error::Syntax(offset))?;
                return visitor.visit_borrowed_str(s);
            }
        }
        let mut buffer = String::new();
        self.source.text_body(len, &mut buffer)?;
        visitor.visit_string(buffer)
    }

    // Visits a byte string body as `&[u8]`: borrowed from a slice source,
    // otherwise staged in the scratch buffer.
    #[inline]
    fn visit_bytes<V: de::Visitor<'de>>(
        &mut self,
        len: Option<usize>,
        visitor: V,
    ) -> Result<V::Value, Error> {
        if let Some(res) = len.and_then(|len| self.source.borrow_body(len)) {
            return visitor.visit_borrowed_bytes(res?);
        }
        self.scratch.clear();
        self.source.bytes_body(len, &mut self.scratch)?;
        visitor.visit_bytes(&self.scratch)
    }

    // Visits a byte string body as an owned buffer.
    #[inline]
    fn visit_byte_buf<V: de::Visitor<'de>>(
        &mut self,
        len: Option<usize>,
        visitor: V,
    ) -> Result<V::Value, Error> {
        let mut buffer = Vec::new();
        self.source.bytes_body(len, &mut buffer)?;
        visitor.visit_byte_buf(buffer)
    }

    #[inline]
    fn visit_array<V: de::Visitor<'de>>(
        &mut self,
        remaining: Option<usize>,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.recurse(|me| {
            let mut access = Access { de: me, remaining };
            let value = visitor.visit_seq(&mut access)?;
            access.finish()?;
            Ok(value)
        })
    }

    #[inline]
    fn visit_object<V: de::Visitor<'de>>(
        &mut self,
        remaining: Option<usize>,
        keys: &'static str,
        visitor: V,
    ) -> Result<V::Value, Error> {
        self.recurse(|me| {
            let mut access = ObjectAccess {
                base: Access { de: me, remaining },
                pending_value: false,
                keys,
            };
            let value = visitor.visit_map(&mut access)?;
            if access.pending_value {
                return Err(Error::semantic(
                    access.base.de.offset(),
                    "missing map value",
                ));
            }
            access.base.finish()?;
            Ok(value)
        })
    }
}

#[cfg(feature = "alloc")]
impl<'de, S: BorrowSource<'de>> de::SeqAccess<'de> for Access<'_, S> {
    type Error = Error;
    #[inline]
    fn next_element_seed<U: de::DeserializeSeed<'de>>(
        &mut self,
        seed: U,
    ) -> Result<Option<U::Value>, Error> {
        if !self.next()? {
            return Ok(None);
        }
        seed.deserialize(&mut *self.de).map(Some)
    }
    #[inline]
    fn size_hint(&self) -> Option<usize> {
        self.remaining
    }
}

#[cfg(feature = "alloc")]
struct ObjectAccess<'a, S> {
    base: Access<'a, S>,
    pending_value: bool,
    keys: &'static str,
}

#[cfg(feature = "alloc")]
impl<'de, S: BorrowSource<'de>> de::MapAccess<'de> for ObjectAccess<'_, S> {
    type Error = Error;
    #[inline]
    fn next_key_seed<K: de::DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Error> {
        if self.pending_value {
            return Err(Error::semantic(self.base.de.offset(), "expected map value"));
        }
        if !self.base.next()? {
            return Ok(None);
        }
        self.pending_value = true;
        if self.keys.is_empty() {
            return seed.deserialize(&mut *self.base.de).map(Some);
        }
        self.base.de.with_header(false, |me, header, _| {
            let key = match header {
                Header::Positive(x) => i128::from(x),
                Header::Negative(x) => -1 - i128::from(x),
                header => {
                    me.source.push(header);
                    return seed.deserialize(me).map(Some);
                }
            };
            match crate::ser::field_for_key(self.keys, key) {
                Some(field) => seed
                    .deserialize(de::value::StrDeserializer::new(field))
                    .map(Some),
                None => seed
                    .deserialize(de::value::StringDeserializer::new(format!(
                        "{INT_KEY_PLACEHOLDER}{key}"
                    )))
                    .map(Some),
            }
        })
    }
    #[inline]
    fn next_value_seed<V: de::DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, Error> {
        if !self.pending_value {
            return Err(Error::semantic(
                self.base.de.offset(),
                "map value requested before key",
            ));
        }
        self.pending_value = false;
        seed.deserialize(&mut *self.base.de)
    }
    #[inline]
    fn size_hint(&self) -> Option<usize> {
        self.base.remaining
    }
}

// The wire form an enum item was encoded in.
#[cfg(feature = "alloc")]
#[derive(Copy, Clone, Eq, PartialEq)]
enum EnumForm {
    // A bare text string: a unit variant with no payload item, so payload
    // accesses must not consume any further items from the stream.
    Text,
    // A definite-length map with a single entry.
    Map,
    // An indefinite-length map; its break follows the single entry.
    IndefiniteMap,
}

// Variant access for an enum item.
//
// The second field carries the wire form. The last field is the key table
// of a marked enum (empty otherwise), applied to struct variants.
#[cfg(feature = "alloc")]
struct Enum<'a, S>(
    &'a mut Deserializer<S>,
    EnumForm,
    &'static str,
    crate::ser::StructShape,
);

// After the payload of the indefinite-length map form, the entry must have
// been the only one: the next header has to be the break.
#[cfg(feature = "alloc")]
fn finish_enum_map<S: Source>(de: &mut Deserializer<S>, form: EnumForm) -> Result<(), Error> {
    if form != EnumForm::IndefiniteMap {
        return Ok(());
    }

    let offset = de.source.offset();
    match de.source.pull()? {
        Header::Break => Ok(()),
        _ => Err(Error::semantic(
            offset,
            "expected a single-entry map for an enum",
        )),
    }
}

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
        if self.1 != EnumForm::Text {
            // The map form carries a payload; require it to be a unit.
            <() as de::Deserialize>::deserialize(&mut *self.0)?;
            finish_enum_map(self.0, self.1)?;
        }

        Ok(())
    }

    #[inline]
    fn newtype_variant_seed<U: de::DeserializeSeed<'de>>(
        self,
        seed: U,
    ) -> Result<U::Value, Self::Error> {
        if self.1 == EnumForm::Text {
            return Err(de::Error::invalid_type(
                de::Unexpected::UnitVariant,
                &"newtype variant",
            ));
        }

        let value = seed.deserialize(&mut *self.0)?;
        finish_enum_map(self.0, self.1)?;
        Ok(value)
    }

    #[inline]
    fn tuple_variant<V: de::Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if self.1 == EnumForm::Text {
            return Err(de::Error::invalid_type(
                de::Unexpected::UnitVariant,
                &"tuple variant",
            ));
        }

        let value = self.0.deserialize_seq(visitor)?;
        finish_enum_map(self.0, self.1)?;
        Ok(value)
    }

    #[inline]
    fn struct_variant<V: de::Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        if self.1 == EnumForm::Text {
            return Err(de::Error::invalid_type(
                de::Unexpected::UnitVariant,
                &"struct variant",
            ));
        }

        let keys = self.2;
        let shape = self.3;
        let value = self.0.with_header(false, |me, header, _| match header {
            Header::Map(len) if shape == crate::ser::StructShape::Map => {
                me.visit_object(len, keys, visitor)
            }
            Header::Array(len) if shape == crate::ser::StructShape::Array => {
                me.visit_array(len, visitor)
            }
            header if shape == crate::ser::StructShape::Array => Err(header.expected("array")),
            header => Err(header.expected("map")),
        })?;
        finish_enum_map(self.0, self.1)?;
        Ok(value)
    }
}

// Byte strings accepted as sequences must obey the target's length too.
#[cfg(feature = "alloc")]
pub(crate) fn visit_byte_seq<'de, V: de::Visitor<'de>>(
    bytes: &[u8],
    visitor: V,
) -> Result<V::Value, Error> {
    let mut access = BorrowedBytesAccess(0, bytes);
    let value = visitor.visit_seq(&mut access)?;
    if access.0 != bytes.len() {
        return Err(Error::semantic(None, "unconsumed byte string elements"));
    }
    Ok(value)
}

// Yields the contents of a borrowed byte string as a sequence of integers.
#[cfg(feature = "alloc")]
struct BorrowedBytesAccess<'de>(usize, &'de [u8]);

#[cfg(feature = "alloc")]
impl<'de> de::SeqAccess<'de> for BorrowedBytesAccess<'_> {
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
    de: Option<Deserializer<ReaderSource<R>>>,
    _marker: core::marker::PhantomData<T>,
}

#[cfg(feature = "alloc")]
impl<T: de::DeserializeOwned, R: Read> Iterator for Iter<T, R> {
    type Item = Result<T, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let de = self.de.as_mut()?;
        let decoder = &mut de.source.0;
        let start = decoder.offset();

        // Probe for a clean end of input: end-of-file before the first byte
        // of an item terminates the stream, anywhere else it is an error.
        let item = match decoder.pull() {
            Ok(header) => {
                decoder.push(header);
                T::deserialize(de)
            }
            Err(crate::core::Error::Io(err))
                if err.kind() == crate::io::ErrorKind::UnexpectedEof
                    && decoder.offset() == start =>
            {
                self.de = None;
                return None;
            }
            Err(err) => Err(err.into()),
        };
        if item.is_err() {
            self.de = None;
        }
        Some(item)
    }
}

/// An iterator decoding consecutive top-level items from a byte slice.
///
/// Created by [`Deserializer::into_iter`] on a slice deserializer.
#[cfg(feature = "alloc")]
pub struct SliceIter<'de, T> {
    // `None` once the input is exhausted or an item failed.
    de: Option<Deserializer<SliceSource<'de>>>,
    _marker: core::marker::PhantomData<T>,
}

#[cfg(feature = "alloc")]
impl<'de, T: de::Deserialize<'de>> Iterator for SliceIter<'de, T> {
    type Item = Result<T, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        let de = self.de.as_mut()?;
        if de.source.0.is_exhausted() {
            self.de = None;
            return None;
        }

        let item = T::deserialize(de);
        if item.is_err() {
            self.de = None;
        }
        Some(item)
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
/// they are well-formed and can be represented by [`crate::Simple`].
///
/// Trailing data after the item is an error; to handle a CBOR sequence
/// (RFC 8742), validate items one at a time from the shared reader.
///
/// For input that is already a byte slice, [`validate_slice`] performs the
/// same checks without copying string bodies.
///
/// ```rust
/// assert!(cbor2::validate(&b"\x83\x01\x02\x03"[..]).is_ok()); // [1, 2, 3]
/// assert!(cbor2::validate(&b"\x83\x01\x02"[..]).is_err()); // truncated
/// assert!(cbor2::validate(&b"\x62\xff\xfe"[..]).is_err()); // invalid UTF-8
/// ```
pub fn validate<R: Read>(reader: R) -> Result<(), Error> {
    let mut decoder = Decoder::from(reader);
    let mut scratch = [0u8; SKIP_CHUNK];
    validate_item::<_, false>(&mut decoder, DEFAULT_RECURSION_LIMIT, &mut scratch)?;
    expect_eof(&mut decoder)
}

/// Checks that a byte slice contains exactly one well-formed CBOR item.
///
/// The slice counterpart of [`validate`], performing exactly the same
/// checks: well-formedness (RFC 8949 §5.3.1), text UTF-8 validity (every
/// segment of an indefinite-length text string on its own), nesting bounded
/// by [`DEFAULT_RECURSION_LIMIT`] and no trailing data. Because the whole
/// input is already in memory, string bodies are checked in place instead
/// of being copied through a scratch buffer, which makes this the faster
/// entry point for buffers. **No heap memory is allocated.**
///
/// ```rust
/// assert!(cbor2::validate_slice(b"\x83\x01\x02\x03").is_ok()); // [1, 2, 3]
/// assert!(cbor2::validate_slice(b"\x83\x01\x02").is_err()); // truncated
/// assert!(cbor2::validate_slice(b"\x62\xff\xfe").is_err()); // invalid UTF-8
/// ```
pub fn validate_slice(slice: &[u8]) -> Result<(), Error> {
    let mut decoder = Decoder::from(slice);
    validate_item_slice::<false>(&mut decoder, DEFAULT_RECURSION_LIMIT)?;
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

// The scratch buffer used to discard string bodies while walking an item.
// It is created once per walk and shared by every body along the way, so
// its (zero-initialization) cost is paid once, not per string.
const SKIP_CHUNK: usize = 1024;

// Typed deserialization budgets wrappers: after the final array, map or
// tag, a scalar is still permitted. Exact-item validation retains its existing
// item-depth boundary. Both modes walk skipped and captured data identically.
fn validate_item<R: Read, const LEAF_AT_LIMIT: bool>(
    decoder: &mut Decoder<R>,
    depth: usize,
    scratch: &mut [u8; SKIP_CHUNK],
) -> Result<(), Error> {
    let offset = decoder.offset();
    let header = decoder.pull()?;
    validate_header::<_, LEAF_AT_LIMIT>(decoder, header, offset, depth, scratch)
}

fn validate_header<R: Read, const LEAF_AT_LIMIT: bool>(
    decoder: &mut Decoder<R>,
    header: Header,
    offset: usize,
    depth: usize,
    scratch: &mut [u8; SKIP_CHUNK],
) -> Result<(), Error> {
    if depth == 0
        && (!LEAF_AT_LIMIT || matches!(header, Header::Array(_) | Header::Map(_) | Header::Tag(_)))
    {
        return Err(Error::RecursionLimitExceeded);
    }

    match header {
        Header::Positive(..) | Header::Negative(..) | Header::Float(..) | Header::Simple(..) => {
            Ok(())
        }

        Header::Break => Err(Error::Syntax(offset)),

        Header::Tag(..) => validate_item::<_, LEAF_AT_LIMIT>(decoder, depth - 1, scratch),

        Header::Bytes(len) => match len {
            Some(len) => skip_body(decoder, len, scratch),
            None => loop {
                let offset = decoder.offset();
                match decoder.pull()? {
                    Header::Break => return Ok(()),
                    // Segments must be definite-length strings of the same
                    // major type (RFC 8949 §3.2.3).
                    Header::Bytes(Some(len)) => skip_body(decoder, len, scratch)?,
                    _ => return Err(Error::Syntax(offset)),
                }
            },
        },

        Header::Text(len) => match len {
            Some(len) => check_utf8_body(decoder, len, scratch),
            None => loop {
                let offset = decoder.offset();
                match decoder.pull()? {
                    Header::Break => return Ok(()),
                    Header::Text(Some(len)) => check_utf8_body(decoder, len, scratch)?,
                    _ => return Err(Error::Syntax(offset)),
                }
            },
        },

        Header::Array(len) => match len {
            Some(len) => {
                for _ in 0..len {
                    validate_item::<_, LEAF_AT_LIMIT>(decoder, depth - 1, scratch)?;
                }
                Ok(())
            }
            None => loop {
                let offset = decoder.offset();
                match decoder.pull()? {
                    Header::Break => return Ok(()),
                    header => validate_header::<_, LEAF_AT_LIMIT>(
                        decoder,
                        header,
                        offset,
                        depth - 1,
                        scratch,
                    )?,
                }
            },
        },

        Header::Map(len) => match len {
            Some(len) => {
                for _ in 0..len {
                    validate_item::<_, LEAF_AT_LIMIT>(decoder, depth - 1, scratch)?; // key
                    validate_item::<_, LEAF_AT_LIMIT>(decoder, depth - 1, scratch)?;
                    // value
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
                            validate_header::<_, LEAF_AT_LIMIT>(
                                decoder,
                                header,
                                offset,
                                depth - 1,
                                scratch,
                            )?;
                            expecting_value = !expecting_value;
                        }
                    }
                }
            }
        },
    }
}

// Walks one item directly over a byte slice: string bodies are borrowed
// (and text checked) in place, with no intermediate buffer at all.
fn validate_item_slice<const LEAF_AT_LIMIT: bool>(
    decoder: &mut Decoder<&[u8]>,
    depth: usize,
) -> Result<(), Error> {
    let offset = decoder.offset();
    let header = decoder.pull_slice()?;
    validate_header_slice::<LEAF_AT_LIMIT>(decoder, header, offset, depth)
}

fn validate_header_slice<const LEAF_AT_LIMIT: bool>(
    decoder: &mut Decoder<&[u8]>,
    header: Header,
    offset: usize,
    depth: usize,
) -> Result<(), Error> {
    if depth == 0
        && (!LEAF_AT_LIMIT || matches!(header, Header::Array(_) | Header::Map(_) | Header::Tag(_)))
    {
        return Err(Error::RecursionLimitExceeded);
    }

    let check_utf8 = |decoder: &mut Decoder<&[u8]>, len: usize| -> Result<(), Error> {
        let offset = decoder.offset();
        let body = decoder.borrow_body(len)?;
        core::str::from_utf8(body)
            .map(|_| ())
            .map_err(|_| Error::Syntax(offset))
    };

    match header {
        Header::Positive(..) | Header::Negative(..) | Header::Float(..) | Header::Simple(..) => {
            Ok(())
        }

        Header::Break => Err(Error::Syntax(offset)),

        Header::Tag(..) => validate_item_slice::<LEAF_AT_LIMIT>(decoder, depth - 1),

        Header::Bytes(len) => match len {
            Some(len) => decoder.borrow_body(len).map(|_| ()).map_err(Error::Io),
            None => loop {
                let offset = decoder.offset();
                match decoder.pull_slice()? {
                    Header::Break => return Ok(()),
                    // Segments must be definite-length strings of the same
                    // major type (RFC 8949 §3.2.3).
                    Header::Bytes(Some(len)) => {
                        decoder.borrow_body(len)?;
                    }
                    _ => return Err(Error::Syntax(offset)),
                }
            },
        },

        Header::Text(len) => match len {
            Some(len) => check_utf8(decoder, len),
            None => loop {
                let offset = decoder.offset();
                match decoder.pull_slice()? {
                    Header::Break => return Ok(()),
                    Header::Text(Some(len)) => check_utf8(decoder, len)?,
                    _ => return Err(Error::Syntax(offset)),
                }
            },
        },

        Header::Array(len) => match len {
            Some(len) => {
                for _ in 0..len {
                    validate_item_slice::<LEAF_AT_LIMIT>(decoder, depth - 1)?;
                }
                Ok(())
            }
            None => loop {
                let offset = decoder.offset();
                match decoder.pull_slice()? {
                    Header::Break => return Ok(()),
                    header => {
                        validate_header_slice::<LEAF_AT_LIMIT>(decoder, header, offset, depth - 1)?
                    }
                }
            },
        },

        Header::Map(len) => match len {
            Some(len) => {
                for _ in 0..len {
                    validate_item_slice::<LEAF_AT_LIMIT>(decoder, depth - 1)?; // key
                    validate_item_slice::<LEAF_AT_LIMIT>(decoder, depth - 1)?; // value
                }
                Ok(())
            }
            None => {
                let mut expecting_value = false;
                loop {
                    let offset = decoder.offset();
                    match decoder.pull_slice()? {
                        // A break in place of a value leaves a dangling key,
                        // which is not well-formed (RFC 8949 §5.3.1).
                        Header::Break if expecting_value => return Err(Error::Syntax(offset)),
                        Header::Break => return Ok(()),
                        header => {
                            validate_header_slice::<LEAF_AT_LIMIT>(
                                decoder,
                                header,
                                offset,
                                depth - 1,
                            )?;
                            expecting_value = !expecting_value;
                        }
                    }
                }
            }
        },
    }
}

// Discards a definite-length body through the shared scratch buffer; a
// forged length cannot trigger an allocation.
fn skip_body<R: Read>(
    decoder: &mut Decoder<R>,
    mut remaining: usize,
    scratch: &mut [u8; SKIP_CHUNK],
) -> Result<(), Error> {
    while remaining > 0 {
        let n = remaining.min(scratch.len());
        decoder.read_exact(&mut scratch[..n])?;
        remaining -= n;
    }
    Ok(())
}

// Discards a definite-length text body, verifying that the whole body is
// valid UTF-8. Characters may straddle the internal chunk boundaries; up to
// three trailing bytes of an incomplete character carry over to the next
// chunk.
fn check_utf8_body<R: Read>(
    decoder: &mut Decoder<R>,
    len: usize,
    scratch: &mut [u8; SKIP_CHUNK],
) -> Result<(), Error> {
    let offset = decoder.offset();
    let mut carry = 0usize;
    let mut remaining = len;

    while remaining > 0 {
        let n = remaining.min(scratch.len() - carry);
        decoder.read_exact(&mut scratch[carry..carry + n])?;
        remaining -= n;
        let filled = carry + n;

        match core::str::from_utf8(&scratch[..filled]) {
            Ok(..) => carry = 0,
            Err(err) => {
                // An incomplete character is only acceptable while more
                // body bytes are coming.
                if err.error_len().is_some() || remaining == 0 {
                    return Err(Error::Syntax(offset));
                }

                let valid = err.valid_up_to();
                scratch.copy_within(valid..filled, 0);
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

// Internal data-model consumers (CDN, deterministic encoding) must retain
// undefined even though the public serde-visible Value decoder maps it to null.
#[cfg(feature = "alloc")]
pub(crate) fn value_from_slice(slice: &[u8]) -> Result<crate::Value, Error> {
    let mut deserializer = Deserializer::from_slice(slice);
    deserializer.preserve_simple = true;
    de::Deserialize::deserialize(&mut deserializer)
}

/// Deserializes a value from a byte slice of CBOR.
///
/// This decodes the first complete CBOR item in `slice`. It does not report
/// trailing data; call [`validate_slice`] first if trailing bytes should be
/// an error. Definite-length text and byte strings can be borrowed from the
/// input slice, so targets such as `&str` and `serde_bytes::Bytes` do not
/// require intermediate allocation. Indefinite-length segmented strings
/// cannot be borrowed because their logical body is not contiguous.
///
/// ```rust
/// let mut bytes = cbor2::to_vec(&1u8).unwrap();
/// bytes.extend(cbor2::to_vec(&2u8).unwrap());
///
/// assert_eq!(cbor2::from_slice::<u8>(&bytes).unwrap(), 1);
/// assert!(cbor2::validate_slice(&bytes).is_err());
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
