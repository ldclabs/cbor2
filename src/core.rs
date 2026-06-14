//! Low-level CBOR encoding and decoding.
//!
//! This module provides a pull/push interface over CBOR item *headers*
//! (RFC 8949 §3). A CBOR item on the wire consists of a one-byte prefix
//! (major type + additional information), an optional multi-byte argument,
//! and an optional body (for bytes, text, arrays, maps and tags).
//!
//! [`Decoder::pull`] reads the next header from the input and
//! [`Encoder::push`] writes a header to the output. Item bodies are read and
//! written directly through the underlying reader/writer; helper methods are
//! provided for (possibly segmented) byte and text strings.
//!
//! Most users should prefer the serde interface in the crate root or the
//! dynamic [`Value`](crate::Value) type; this module is for applications
//! that need precise control over the wire format.
//!
//! # Example
//!
//! Build and then inspect an indefinite-length byte string. The helper
//! methods keep the body handling explicit while still validating segmented
//! string structure for you:
//!
//! ```rust
//! use cbor2::core::{Decoder, Encoder, Header};
//!
//! let mut encoded = Vec::new();
//! let mut enc = Encoder::from(&mut encoded);
//! enc.push(Header::Bytes(None)).unwrap();
//! enc.bytes(&[0xde, 0xad]).unwrap();
//! enc.bytes(&[0xbe, 0xef]).unwrap();
//! enc.push(Header::Break).unwrap();
//!
//! let mut dec = Decoder::from(&encoded[..]);
//! let Header::Bytes(len) = dec.pull().unwrap() else { unreachable!() };
//!
//! let mut body = Vec::new();
//! dec.bytes_body(len, &mut body).unwrap();
//! assert_eq!(body, [0xde, 0xad, 0xbe, 0xef]);
//! ```

#[cfg(feature = "alloc")]
use alloc::{string::String, vec::Vec};

use crate::io::{Read, Write};

/// Simple value constants (RFC 8949 §3.3).
pub mod simple {
    /// Simple value 20: `false`.
    pub const FALSE: u8 = 20;
    /// Simple value 21: `true`.
    pub const TRUE: u8 = 21;
    /// Simple value 22: `null`.
    pub const NULL: u8 = 22;
    /// Simple value 23: `undefined`.
    pub const UNDEFINED: u8 = 23;
}

/// Well-known tag constants (RFC 8949 §3.4).
pub mod tag {
    /// Tag 2: an unsigned bignum encoded as a byte string.
    pub const BIGPOS: u64 = 2;
    /// Tag 3: a negative bignum encoded as a byte string.
    pub const BIGNEG: u64 = 3;
}

/// An error that occurred while reading or writing CBOR items.
#[derive(Debug)]
pub enum Error {
    /// An error from the underlying reader or writer.
    Io(crate::io::Error),

    /// The input is not well-formed CBOR.
    ///
    /// Contains the byte offset of the offending item.
    Syntax(usize),
}

impl From<crate::io::Error> for Error {
    #[inline]
    fn from(value: crate::io::Error) -> Self {
        Self::Io(value)
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Io(err) => write!(f, "i/o error: {err}"),
            Error::Syntax(offset) => write!(f, "syntax error at offset {offset}"),
        }
    }
}

// `serde::ser::StdError` is `std::error::Error` whenever it is available,
// and an identical substitute otherwise.
impl serde::ser::StdError for Error {
    fn source(&self) -> Option<&(dyn serde::ser::StdError + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            Error::Syntax(..) => None,
        }
    }
}

/// A semantic representation of a CBOR item header.
///
/// A header carries the major type and the argument of an item. It does
/// **not** carry the body: after pulling a [`Header::Bytes`], for example,
/// the byte string itself still has to be read from the decoder.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Header {
    /// An unsigned integer (major type 0).
    Positive(u64),

    /// A negative integer (major type 1).
    ///
    /// The value carried here is the encoded argument, i.e. the bits of the
    /// represented number `-1 - n` with all bits inverted. To recover the
    /// numeric value: `n as i128 ^ !0`.
    Negative(u64),

    /// A floating-point value (major type 7, additional information 25-27).
    Float(f64),

    /// A simple value (major type 7).
    ///
    /// Values 24 to 31 (inclusive) are reserved by RFC 8949 §3.3 and have no
    /// well-formed encoding; pushing such a header produces output that any
    /// conforming decoder (including this one) rejects.
    Simple(u8),

    /// A tag (major type 6).
    Tag(u64),

    /// The "break" stop code terminating an indefinite-length item.
    Break,

    /// A byte string (major type 2).
    ///
    /// `None` indicates an indefinite-length byte string composed of
    /// definite-length segments terminated by [`Header::Break`].
    Bytes(Option<usize>),

    /// A text string (major type 3); the length is in bytes.
    ///
    /// `None` indicates an indefinite-length text string composed of
    /// definite-length segments terminated by [`Header::Break`].
    Text(Option<usize>),

    /// An array (major type 4); the length is in items.
    ///
    /// `None` indicates an indefinite-length array terminated by
    /// [`Header::Break`].
    Array(Option<usize>),

    /// A map (major type 5); the length is in key/value *pairs*.
    ///
    /// `None` indicates an indefinite-length map terminated by
    /// [`Header::Break`].
    Map(Option<usize>),
}

/// An encoder for serializing CBOR items.
///
/// All output is written through to the wrapped writer; consider providing
/// a buffered writer for performance. [`Encoder`] only writes headers and
/// raw bodies; it does not track container balance, so callers are
/// responsible for writing the right number of array/map elements and the
/// final [`Header::Break`] for indefinite-length items.
pub struct Encoder<W>(W);

impl<W: Write> From<W> for Encoder<W> {
    #[inline]
    fn from(writer: W) -> Self {
        Self(writer)
    }
}

impl<W: Write> Encoder<W> {
    #[inline]
    pub(crate) fn reserve(&mut self, additional: usize) {
        self.0.reserve(additional);
    }

    #[inline]
    pub(crate) fn push_uint(&mut self, major: u8, value: u64) -> Result<(), crate::io::Error> {
        let prefix = major << 5;
        match value {
            x if x <= 23 => self.0.write_all(&[prefix | x as u8]),
            x if x <= u8::MAX as u64 => self.0.write_all(&[prefix | 24, x as u8]),
            x if x <= u16::MAX as u64 => {
                let b = (x as u16).to_be_bytes();
                self.0.write_all(&[prefix | 25, b[0], b[1]])
            }
            x if x <= u32::MAX as u64 => {
                let b = (x as u32).to_be_bytes();
                self.0.write_all(&[prefix | 26, b[0], b[1], b[2], b[3]])
            }
            x => {
                let b = x.to_be_bytes();
                self.0
                    .write_all(&[prefix | 27, b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
            }
        }
    }

    #[inline]
    pub(crate) fn push_len(
        &mut self,
        major: u8,
        len: Option<usize>,
    ) -> Result<(), crate::io::Error> {
        match len {
            Some(len) => self.push_uint(major, len as u64),
            None => self.0.write_all(&[(major << 5) | 31]),
        }
    }

    #[inline]
    pub(crate) fn positive(&mut self, value: u64) -> Result<(), crate::io::Error> {
        self.push_uint(0, value)
    }

    #[inline]
    pub(crate) fn negative(&mut self, value: u64) -> Result<(), crate::io::Error> {
        self.push_uint(1, value)
    }

    #[inline]
    pub(crate) fn tag(&mut self, value: u64) -> Result<(), crate::io::Error> {
        self.push_uint(6, value)
    }

    #[inline]
    pub(crate) fn array(&mut self, len: Option<usize>) -> Result<(), crate::io::Error> {
        self.push_len(4, len)
    }

    #[inline]
    pub(crate) fn map(&mut self, len: Option<usize>) -> Result<(), crate::io::Error> {
        self.push_len(5, len)
    }

    #[inline]
    pub(crate) fn simple(&mut self, value: u8) -> Result<(), crate::io::Error> {
        match value {
            0..=23 => self.0.write_all(&[0xe0 | value]),
            value => self.0.write_all(&[0xf8, value]),
        }
    }

    #[inline]
    pub(crate) fn float(&mut self, value: f64) -> Result<(), crate::io::Error> {
        if value.is_nan() {
            if let Some(n16) = f64_to_f16(value) {
                let b = n16.to_be_bytes();
                return self.0.write_all(&[0xf9, b[0], b[1]]);
            }
        } else {
            let n32 = value as f32;
            if (n32 as f64).to_bits() == value.to_bits() {
                if let Some(n16) = f64_to_f16(value) {
                    let b = n16.to_be_bytes();
                    return self.0.write_all(&[0xf9, b[0], b[1]]);
                }

                let b = n32.to_bits().to_be_bytes();
                return self.0.write_all(&[0xfa, b[0], b[1], b[2], b[3]]);
            }
        }

        let b = value.to_bits().to_be_bytes();
        self.0
            .write_all(&[0xfb, b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    }

    /// Writes a single header to the output.
    ///
    /// The shortest well-formed argument width is chosen automatically.
    /// Floating-point values are encoded as `f16`, `f32` or `f64`, using the
    /// shortest lossless width; NaN is emitted as the canonical half-width
    /// quiet NaN when it round-trips exactly.
    #[inline]
    pub fn push(&mut self, header: Header) -> Result<(), crate::io::Error> {
        match header {
            Header::Positive(x) => self.positive(x),
            Header::Negative(x) => self.negative(x),
            Header::Bytes(x) => self.push_len(2, x),
            Header::Text(x) => self.push_len(3, x),
            Header::Array(x) => self.array(x),
            Header::Map(x) => self.map(x),
            Header::Tag(x) => self.tag(x),
            Header::Break => self.0.write_all(&[0xff]),
            Header::Simple(x) => self.simple(x),
            Header::Float(x) => self.float(x),
        }
    }

    /// Writes a definite-length byte string (header and body).
    ///
    /// When writing an indefinite-length byte string, first call
    /// [`push`](Self::push) with [`Header::Bytes`]`(None)`, then call this
    /// method for each definite-length segment, and finally push
    /// [`Header::Break`].
    #[inline]
    pub fn bytes(&mut self, value: &[u8]) -> Result<(), crate::io::Error> {
        self.reserve(value.len().saturating_add(9));
        self.push_len(2, Some(value.len()))?;
        self.0.write_all(value)
    }

    /// Writes a definite-length text string (header and body).
    ///
    /// When used as a segment inside [`Header::Text`]`(None)`, this writes one
    /// well-formed UTF-8 text segment.
    #[inline]
    pub fn text(&mut self, value: &str) -> Result<(), crate::io::Error> {
        self.reserve(value.len().saturating_add(9));
        self.push_len(3, Some(value.len()))?;
        self.0.write_all(value.as_bytes())
    }

    /// Writes raw bytes directly to the output.
    ///
    /// This is used to write item bodies after pushing the corresponding
    /// header.
    #[inline]
    pub fn write_all(&mut self, data: &[u8]) -> Result<(), crate::io::Error> {
        self.0.write_all(data)
    }

    /// Flushes the underlying writer.
    #[inline]
    pub fn flush(&mut self) -> Result<(), crate::io::Error> {
        self.0.flush()
    }
}

// Reading the body of a string item never trusts the declared length for
// allocation: memory grows as data actually arrives, in chunks of this size.
#[cfg(feature = "alloc")]
const CHUNK: usize = 16 * 1024;

/// A decoder for parsing CBOR items.
///
/// Input is read directly from the wrapped reader one item at a time;
/// consider providing a buffered reader for performance. After a string
/// header, callers must read the corresponding body before pulling the next
/// header.
pub struct Decoder<R> {
    reader: R,
    offset: usize,
    pushback: Option<(Header, usize)>,
    mark: usize,
    // The wire bytes of the most recently parsed header, so a recording
    // can be seeded when it starts behind a pushed-back header.
    #[cfg(feature = "alloc")]
    last_header: ([u8; 9], u8),
    // When active, a byte-exact copy of everything read from the wire.
    #[cfg(feature = "alloc")]
    record: Option<Vec<u8>>,
}

impl<R: Read> From<R> for Decoder<R> {
    #[inline]
    fn from(reader: R) -> Self {
        Self {
            reader,
            offset: 0,
            pushback: None,
            mark: 0,
            #[cfg(feature = "alloc")]
            last_header: ([0; 9], 0),
            #[cfg(feature = "alloc")]
            record: None,
        }
    }
}

#[inline]
fn decode_header(raw: &[u8; 9], arg: Option<u64>, start: usize) -> Result<Header, Error> {
    let major = raw[0] >> 5;
    let minor = raw[0] & 0b00011111;

    // On 64-bit targets every u64 length fits in usize; on smaller
    // targets an unrepresentable length is reported as a syntax error
    // (nothing that large could be read anyway).
    #[cfg(target_pointer_width = "64")]
    let len = |arg: Option<u64>| Ok::<_, Error>(arg.map(|x| x as usize));

    #[cfg(not(target_pointer_width = "64"))]
    let len = |arg: Option<u64>| match arg {
        Some(x) => usize::try_from(x)
            .map(Some)
            .map_err(|_| Error::Syntax(start)),
        None => Ok(None),
    };

    Ok(match major {
        0 => Header::Positive(arg.ok_or(Error::Syntax(start))?),
        1 => Header::Negative(arg.ok_or(Error::Syntax(start))?),
        2 => Header::Bytes(len(arg)?),
        3 => Header::Text(len(arg)?),
        4 => Header::Array(len(arg)?),
        5 => Header::Map(len(arg)?),
        6 => Header::Tag(arg.ok_or(Error::Syntax(start))?),
        // `major` is a three-bit value, so the only remaining case is 7.
        _ => match minor {
            x @ 0..=23 => Header::Simple(x),
            // RFC 8949 §3.3: a 0xf8 prefix followed by a byte less than
            // 0x20 is not well-formed.
            24 if raw[1] >= 32 => Header::Simple(raw[1]),
            24 => return Err(Error::Syntax(start)),
            25 => Header::Float(f16_to_f64(u16::from_be_bytes([raw[1], raw[2]]))),
            26 => Header::Float(
                f32::from_bits(u32::from_be_bytes([raw[1], raw[2], raw[3], raw[4]])) as f64,
            ),
            27 => Header::Float(f64::from_bits(u64::from_be_bytes([
                raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7], raw[8],
            ]))),
            31 => Header::Break,
            _ => return Err(Error::Syntax(start)),
        },
    })
}

impl<R: Read> Decoder<R> {
    /// Pulls the next header from the input.
    ///
    /// For byte and text strings this returns the string header only; read
    /// the body with [`bytes_body`](Self::bytes_body),
    /// [`text_body`](Self::text_body) or [`read_exact`](Self::read_exact)
    /// before continuing.
    pub fn pull(&mut self) -> Result<Header, Error> {
        if let Some((header, end)) = self.pushback.take() {
            self.mark = self.offset;
            self.offset = end;
            return Ok(header);
        }

        let start = self.offset;
        self.mark = start;

        let mut raw = [0u8; 9];
        self.read_exact(&mut raw[..1])?;

        let minor = raw[0] & 0b00011111;

        let (arg, raw_len) = match minor {
            x @ 0..=23 => (Some(u64::from(x)), 1),
            24 => {
                self.read_exact(&mut raw[1..2])?;
                (Some(u64::from(raw[1])), 2)
            }
            25 => {
                self.read_exact(&mut raw[1..3])?;
                (Some(u64::from(u16::from_be_bytes([raw[1], raw[2]]))), 3)
            }
            26 => {
                self.read_exact(&mut raw[1..5])?;
                (
                    Some(u64::from(u32::from_be_bytes([
                        raw[1], raw[2], raw[3], raw[4],
                    ]))),
                    5,
                )
            }
            27 => {
                self.read_exact(&mut raw[1..9])?;
                (
                    Some(u64::from_be_bytes([
                        raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7], raw[8],
                    ])),
                    9,
                )
            }
            31 => (None, 1),
            _ => return Err(Error::Syntax(start)),
        };

        // Remember the exact wire spelling of this header: the argument
        // width is given by the minor value, so the bytes reconstruct
        // losslessly even for non-preferred encodings.
        #[cfg(feature = "alloc")]
        {
            self.last_header.0 = raw;
            self.last_header.1 = raw_len;
        }
        #[cfg(not(feature = "alloc"))]
        let _ = raw_len;

        decode_header(&raw, arg, start)
    }

    /// Pushes a header back into the decoder, to be returned by the next
    /// [`pull`](Self::pull).
    ///
    /// # Panics
    ///
    /// Panics if a header is already buffered. Only push back the header
    /// returned by the immediately preceding `pull`.
    pub fn push(&mut self, header: Header) {
        assert!(self.pushback.is_none(), "header already buffered");
        self.pushback = Some((header, self.offset));
        self.offset = self.mark;
    }

    /// Returns the byte offset of the next item in the stream.
    ///
    /// The offset starts at zero when the decoder is created.
    #[inline]
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Reads exactly `data.len()` bytes of an item body from the input.
    ///
    /// Use this after pulling a definite-length [`Header::Bytes`] or
    /// [`Header::Text`] when you want to own body validation. The higher
    /// level [`bytes_body`](Self::bytes_body) and
    /// [`text_body`](Self::text_body) helpers also handle segmented strings.
    pub fn read_exact(&mut self, data: &mut [u8]) -> Result<(), crate::io::Error> {
        debug_assert!(self.pushback.is_none());
        self.reader.read_exact(data)?;
        self.offset += data.len();
        #[cfg(feature = "alloc")]
        if let Some(record) = &mut self.record {
            record.extend_from_slice(data);
        }
        Ok(())
    }

    // Starts a byte-exact recording of everything read from the wire.
    // A pushed-back header was consumed before the recording began, so
    // its wire bytes seed the buffer; re-pulling it reads nothing and
    // records nothing, keeping the copy aligned with the stream.
    #[cfg(feature = "alloc")]
    pub(crate) fn start_recording(&mut self) {
        let mut record = Vec::new();
        if self.pushback.is_some() {
            let (raw, raw_len) = &self.last_header;
            record.extend_from_slice(&raw[..*raw_len as usize]);
        }
        self.record = Some(record);
    }

    // Stops recording and returns the bytes read since it started.
    #[cfg(feature = "alloc")]
    pub(crate) fn take_recording(&mut self) -> Vec<u8> {
        self.record.take().unwrap_or_default()
    }

    // Appends `len` body bytes to `out`, growing the buffer as data arrives
    // so that a forged length cannot trigger a huge allocation up front.
    #[cfg(feature = "alloc")]
    fn read_body(&mut self, len: usize, out: &mut Vec<u8>) -> Result<(), Error> {
        let mut remaining = len;
        while remaining > 0 {
            let chunk = remaining.min(CHUNK);
            let used = out.len();
            out.resize(used + chunk, 0);
            self.read_exact(&mut out[used..])?;
            remaining -= chunk;
        }
        Ok(())
    }

    /// Reads the body of a byte string into `out`.
    ///
    /// Call this immediately after pulling a `Header::Bytes(len)`, passing
    /// the pulled `len`. Indefinite-length (segmented) byte strings are
    /// handled transparently.
    #[cfg(feature = "alloc")]
    pub fn bytes_body(&mut self, len: Option<usize>, out: &mut Vec<u8>) -> Result<(), Error> {
        match len {
            Some(len) => self.read_body(len, out),
            None => loop {
                let offset = self.offset;
                match self.pull()? {
                    Header::Break => return Ok(()),
                    // Segments must be definite-length strings of the same
                    // major type (RFC 8949 §3.2.3).
                    Header::Bytes(Some(len)) => self.read_body(len, out)?,
                    _ => return Err(Error::Syntax(offset)),
                }
            },
        }
    }

    /// Reads the body of a text string into `out`.
    ///
    /// Call this immediately after pulling a `Header::Text(len)`, passing
    /// the pulled `len`. Indefinite-length (segmented) text strings are
    /// handled transparently; every segment must itself be valid UTF-8.
    #[cfg(feature = "alloc")]
    pub fn text_body(&mut self, len: Option<usize>, out: &mut String) -> Result<(), Error> {
        let read_segment = |me: &mut Self, len: usize, out: &mut String| {
            let offset = me.offset;
            let mut buffer = Vec::new();
            me.read_body(len, &mut buffer)?;
            match String::from_utf8(buffer) {
                Ok(s) if out.is_empty() => {
                    *out = s;
                    Ok(())
                }
                Ok(s) => {
                    out.push_str(&s);
                    Ok(())
                }
                Err(..) => Err(Error::Syntax(offset)),
            }
        };

        match len {
            Some(len) => read_segment(self, len, out),
            None => loop {
                let offset = self.offset;
                match self.pull()? {
                    Header::Break => return Ok(()),
                    Header::Text(Some(len)) => read_segment(self, len, out)?,
                    _ => return Err(Error::Syntax(offset)),
                }
            },
        }
    }
}

#[cfg(feature = "alloc")]
impl<'de> Decoder<&'de [u8]> {
    #[inline]
    fn slice_eof_after_prefix(&mut self) -> Error {
        if let Some(record) = &mut self.record {
            record.push(self.reader[0]);
        }
        self.reader = &self.reader[1..];
        self.offset += 1;
        crate::io::Error::from(crate::io::ErrorKind::UnexpectedEof).into()
    }

    #[inline]
    fn finish_slice_header(&mut self, raw: [u8; 9], raw_len: u8) {
        self.last_header.0 = raw;
        self.last_header.1 = raw_len;
        if let Some(record) = &mut self.record {
            record.extend_from_slice(&raw[..raw_len as usize]);
        }
        self.reader = &self.reader[raw_len as usize..];
        self.offset += raw_len as usize;
    }

    /// Pulls a plain positive or negative integer from a byte slice.
    pub(crate) fn integer_slice(&mut self) -> Option<Result<(bool, u64), Error>> {
        if self.pushback.is_some() {
            return None;
        }

        let start = self.offset;
        let first = *self.reader.first()?;
        let major = first >> 5;
        if major > 1 {
            return None;
        }

        self.mark = start;

        let minor = first & 0b00011111;
        let (value, raw_len) = match minor {
            x @ 0..=23 => (u64::from(x), 1),
            24 => {
                if self.reader.len() < 2 {
                    return Some(Err(self.slice_eof_after_prefix()));
                }
                (u64::from(self.reader[1]), 2)
            }
            25 => {
                if self.reader.len() < 3 {
                    return Some(Err(self.slice_eof_after_prefix()));
                }
                (
                    u64::from(u16::from_be_bytes([self.reader[1], self.reader[2]])),
                    3,
                )
            }
            26 => {
                if self.reader.len() < 5 {
                    return Some(Err(self.slice_eof_after_prefix()));
                }
                (
                    u64::from(u32::from_be_bytes([
                        self.reader[1],
                        self.reader[2],
                        self.reader[3],
                        self.reader[4],
                    ])),
                    5,
                )
            }
            27 => {
                if self.reader.len() < 9 {
                    return Some(Err(self.slice_eof_after_prefix()));
                }
                (
                    u64::from_be_bytes([
                        self.reader[1],
                        self.reader[2],
                        self.reader[3],
                        self.reader[4],
                        self.reader[5],
                        self.reader[6],
                        self.reader[7],
                        self.reader[8],
                    ]),
                    9,
                )
            }
            _ => return Some(Err(Error::Syntax(start))),
        };

        self.reader = &self.reader[raw_len as usize..];
        self.offset += raw_len as usize;
        Some(Ok((major == 1, value)))
    }

    /// Pulls a plain boolean from a byte slice.
    pub(crate) fn bool_slice(&mut self) -> Option<bool> {
        if self.pushback.is_some() {
            return None;
        }

        let value = match *self.reader.first()? {
            0xf4 => false,
            0xf5 => true,
            _ => return None,
        };

        self.mark = self.offset;
        self.reader = &self.reader[1..];
        self.offset += 1;
        Some(value)
    }

    /// Pulls a plain floating-point number from a byte slice.
    pub(crate) fn float_slice(&mut self) -> Option<Result<f64, Error>> {
        if self.pushback.is_some() {
            return None;
        }

        let start = self.offset;
        let first = *self.reader.first()?;
        let (value, raw_len) = match first {
            0xf9 => {
                if self.reader.len() < 3 {
                    return Some(Err(self.slice_eof_after_prefix()));
                }
                (
                    f16_to_f64(u16::from_be_bytes([self.reader[1], self.reader[2]])),
                    3,
                )
            }
            0xfa => {
                if self.reader.len() < 5 {
                    return Some(Err(self.slice_eof_after_prefix()));
                }
                (
                    f32::from_bits(u32::from_be_bytes([
                        self.reader[1],
                        self.reader[2],
                        self.reader[3],
                        self.reader[4],
                    ])) as f64,
                    5,
                )
            }
            0xfb => {
                if self.reader.len() < 9 {
                    return Some(Err(self.slice_eof_after_prefix()));
                }
                (
                    f64::from_bits(u64::from_be_bytes([
                        self.reader[1],
                        self.reader[2],
                        self.reader[3],
                        self.reader[4],
                        self.reader[5],
                        self.reader[6],
                        self.reader[7],
                        self.reader[8],
                    ])),
                    9,
                )
            }
            _ => return None,
        };

        self.mark = start;
        self.reader = &self.reader[raw_len..];
        self.offset += raw_len;
        Some(Ok(value))
    }

    /// Pulls a header from a byte slice without going through `Read`.
    pub(crate) fn pull_slice(&mut self) -> Result<Header, Error> {
        if let Some((header, end)) = self.pushback.take() {
            self.mark = self.offset;
            self.offset = end;
            return Ok(header);
        }

        let start = self.offset;
        self.mark = start;

        if self.reader.is_empty() {
            return Err(crate::io::Error::from(crate::io::ErrorKind::UnexpectedEof).into());
        }

        let mut raw = [0u8; 9];
        raw[0] = self.reader[0];
        let minor = raw[0] & 0b00011111;
        let (arg, raw_len) = match minor {
            x @ 0..=23 => (Some(u64::from(x)), 1),
            24 => {
                if self.reader.len() < 2 {
                    return Err(self.slice_eof_after_prefix());
                }
                raw[1] = self.reader[1];
                (Some(u64::from(raw[1])), 2)
            }
            25 => {
                if self.reader.len() < 3 {
                    return Err(self.slice_eof_after_prefix());
                }
                raw[1] = self.reader[1];
                raw[2] = self.reader[2];
                (Some(u64::from(u16::from_be_bytes([raw[1], raw[2]]))), 3)
            }
            26 => {
                if self.reader.len() < 5 {
                    return Err(self.slice_eof_after_prefix());
                }
                raw[1] = self.reader[1];
                raw[2] = self.reader[2];
                raw[3] = self.reader[3];
                raw[4] = self.reader[4];
                (
                    Some(u64::from(u32::from_be_bytes([
                        raw[1], raw[2], raw[3], raw[4],
                    ]))),
                    5,
                )
            }
            27 => {
                if self.reader.len() < 9 {
                    return Err(self.slice_eof_after_prefix());
                }
                raw[1] = self.reader[1];
                raw[2] = self.reader[2];
                raw[3] = self.reader[3];
                raw[4] = self.reader[4];
                raw[5] = self.reader[5];
                raw[6] = self.reader[6];
                raw[7] = self.reader[7];
                raw[8] = self.reader[8];
                (
                    Some(u64::from_be_bytes([
                        raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7], raw[8],
                    ])),
                    9,
                )
            }
            31 => (None, 1),
            _ => return Err(Error::Syntax(start)),
        };

        self.finish_slice_header(raw, raw_len);

        decode_header(&raw, arg, start)
    }

    /// Borrows exactly `len` body bytes from the underlying slice.
    ///
    /// This is only valid immediately after pulling a definite-length bytes
    /// or text header. Generic readers still go through [`read_exact`];
    /// slice deserialization uses this to hand serde borrowed strings and
    /// byte strings without copying.
    pub(crate) fn borrow_body(&mut self, len: usize) -> Result<&'de [u8], crate::io::Error> {
        debug_assert!(self.pushback.is_none());
        if self.reader.len() < len {
            return Err(crate::io::ErrorKind::UnexpectedEof.into());
        }

        let (head, tail) = self.reader.split_at(len);
        self.reader = tail;
        self.offset += len;
        if let Some(record) = &mut self.record {
            record.extend_from_slice(head);
        }
        Ok(head)
    }
}

// 2^n for a small exponent range, built directly from the IEEE 754 bit
// layout because `f64::powi` is not available in core. Exact for any
// normal exponent (-1022..=1023).
fn exp2(n: i32) -> f64 {
    f64::from_bits(((n + 1023) as u64) << 52)
}

/// Converts IEEE 754 half-precision bits to an `f64`.
///
/// This follows the decoding algorithm given in RFC 8949 Appendix D.
pub fn f16_to_f64(bits: u16) -> f64 {
    let exp = (bits >> 10) & 0x1f;
    let frac = (bits & 0x3ff) as f64;

    let value = match exp {
        0 => frac * exp2(-24),
        31 if frac == 0.0 => f64::INFINITY,
        31 => f64::NAN,
        _ => (1024.0 + frac) * exp2(exp as i32 - 25),
    };

    if bits & 0x8000 == 0 {
        value
    } else {
        -value
    }
}

/// Converts an `f64` to IEEE 754 half-precision bits if (and only if) the
/// conversion is lossless. NaN converts to the canonical quiet NaN.
pub fn f64_to_f16(value: f64) -> Option<u16> {
    let bits = value.to_bits();
    let sign = ((bits >> 48) & 0x8000) as u16;
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & 0x000f_ffff_ffff_ffff;

    let half = if exp == 0x7ff {
        // Infinity or NaN. Any NaN becomes the canonical quiet NaN; the
        // round-trip check below rejects NaNs whose payload would be lost.
        match frac {
            0 => sign | 0x7c00,
            _ => 0x7e00,
        }
    } else {
        let unbiased = exp - 1023;
        if exp == 0 && frac == 0 {
            sign // ±0.0
        } else if (-14..=15).contains(&unbiased) {
            // Candidate for an f16 normal: the low 42 fraction bits must
            // be zero for the conversion to be exact.
            if frac & ((1 << 42) - 1) != 0 {
                return None;
            }
            sign | (((unbiased + 15) as u16) << 10) | (frac >> 42) as u16
        } else if (-24..-14).contains(&unbiased) {
            // Candidate for an f16 subnormal.
            let mantissa = (1u64 << 52) | frac;
            let shift = 42 + (-14 - unbiased);
            if mantissa & ((1 << shift) - 1) != 0 {
                return None;
            }
            sign | (mantissa >> shift) as u16
        } else {
            return None;
        }
    };

    // Belt and braces: only report success on an exact bit-level round trip.
    if f16_to_f64(half).to_bits() == bits {
        Some(half)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every f16 bit pattern must survive decoding to f64 and re-encoding.
    #[test]
    fn f16_exhaustive_roundtrip() {
        for bits in 0..=u16::MAX {
            let wide = f16_to_f64(bits);

            if wide.is_nan() {
                // NaN payloads are not preserved; the canonical NaN is.
                assert!(f64_to_f16(f64::NAN) == Some(0x7e00));
                continue;
            }

            assert_eq!(f64_to_f16(wide), Some(bits), "bits {bits:04x} ({wide})");
        }
    }

    // Values that are not representable as f16 must be rejected.
    #[test]
    fn f16_rejects_lossy() {
        for value in [
            f64::MIN_POSITIVE,          // far below the subnormal range
            65504.0 + 32.0,             // above f16::MAX
            65536.0,                    // 2^16, exponent out of range
            1.1,                        // fraction bits beyond 10
            5.960464477539063e-8 / 2.0, // below the smallest subnormal
            1.5 * 5.960464477539063e-8, // subnormal range, dropped bits
        ] {
            assert_eq!(f64_to_f16(value), None, "{value}");
        }

        // NaNs whose sign or payload would be lost are rejected by the
        // round-trip check; only the canonical quiet NaN converts.
        assert_eq!(f64_to_f16(-f64::NAN), None);
        assert_eq!(f64_to_f16(f64::from_bits(0x7ff8_0000_0000_0001)), None);
    }

    // Headers round-trip through encode and decode.
    #[cfg(feature = "alloc")]
    #[test]
    fn header_roundtrip() {
        let headers = [
            Header::Positive(0),
            Header::Positive(23),
            Header::Positive(24),
            Header::Positive(u64::MAX),
            Header::Negative(0),
            Header::Negative(u64::MAX),
            Header::Float(1.5),
            Header::Float(f64::MAX),
            Header::Simple(simple::FALSE),
            Header::Simple(simple::UNDEFINED),
            Header::Simple(255),
            Header::Tag(0),
            Header::Tag(u64::MAX),
            Header::Break,
            Header::Bytes(Some(0)),
            Header::Bytes(Some(usize::MAX)),
            Header::Bytes(None),
            Header::Text(Some(64)),
            Header::Text(None),
            Header::Array(Some(1)),
            Header::Array(None),
            Header::Map(Some(1)),
            Header::Map(None),
        ];

        for header in headers {
            let mut buffer = Vec::new();
            Encoder::from(&mut buffer).push(header).unwrap();

            let mut decoder = Decoder::from(&buffer[..]);
            assert_eq!(decoder.pull().unwrap(), header, "{header:?}");
            assert_eq!(decoder.offset(), buffer.len());
        }
    }

    // Pushback rewinds the offset and replays the header.
    #[test]
    fn pushback() {
        let bytes = [0x19, 0x01, 0x00, 0x01]; // 256, 1
        let mut decoder = Decoder::from(&bytes[..]);

        let first = decoder.pull().unwrap();
        assert_eq!(first, Header::Positive(256));
        assert_eq!(decoder.offset(), 3);

        decoder.push(first);
        assert_eq!(decoder.offset(), 0);

        assert_eq!(decoder.pull().unwrap(), Header::Positive(256));
        assert_eq!(decoder.offset(), 3);
        assert_eq!(decoder.pull().unwrap(), Header::Positive(1));
        assert_eq!(decoder.offset(), 4);
    }
}
