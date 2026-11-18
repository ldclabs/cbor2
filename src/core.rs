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
//! dynamic `Value` type available with `alloc`; this module is for applications
//! that need precise control over the wire format.
//!
//! # Example
//!
//! Build and then inspect an indefinite-length byte string. The helper
//! methods keep the body handling explicit while still validating segmented
//! string structure for you:
//!
//! ```rust
//! # #[cfg(feature = "alloc")] {
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
//! # }
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
    /// well-formed encoding; pushing such a header returns an error.
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

    // Each width writes a constant-length array: the compiler turns these
    // into fixed-size stores, which beats funneling every width through one
    // variable-length buffer.
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
            24..=31 => Err(crate::io::ErrorKind::Other.into()),
            value => self.0.write_all(&[0xf8, value]),
        }
    }

    #[inline]
    pub(crate) fn float(&mut self, value: f64) -> Result<(), crate::io::Error> {
        // Preferred serialization (RFC 8949 §4.1): the shortest width that
        // preserves the value exactly. Any value representable as f16 is
        // also representable as f32, so trying the narrower width first is
        // sufficient.
        //
        // A finite value can only narrow losslessly when the fraction bits
        // beyond the target's precision are zero (f16 keeps 10 of the 52
        // bits, f32 keeps 23; subnormal targets need even more zeros), so a
        // single mask test skips the full conversion for the typical f64
        // that cannot narrow. NaN and infinity (all exponent bits set) take
        // the slow path unconditionally.
        let bits = value.to_bits();
        let special = bits & 0x7ff0_0000_0000_0000 == 0x7ff0_0000_0000_0000;

        if special || bits & ((1 << 42) - 1) == 0 {
            if let Some(n16) = f64_to_f16(value) {
                let b = n16.to_be_bytes();
                return self.0.write_all(&[0xf9, b[0], b[1]]);
            }
        }

        if special || bits & ((1 << 29) - 1) == 0 {
            if let Some(n32) = f64_to_f32(value) {
                let b = n32.to_be_bytes();
                return self.0.write_all(&[0xfa, b[0], b[1], b[2], b[3]]);
            }
        }

        let b = bits.to_be_bytes();
        self.0
            .write_all(&[0xfb, b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
    }

    /// Writes a single header to the output.
    ///
    /// The shortest well-formed argument width is chosen automatically.
    /// Floating-point values are encoded as `f16`, `f32` or `f64`, using the
    /// shortest lossless width. This applies to NaN too: the canonical quiet
    /// NaN is emitted half-width, and a NaN payload uses the narrowest width
    /// that preserves it bit for bit.
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
        // Small bodies ride on the writer's own amortized growth; calling
        // `reserve` for each of them costs more than it saves. A hint is
        // only worthwhile when the body is large enough to skip growth
        // steps.
        if value.len() >= 1024 {
            self.reserve(value.len().saturating_add(9));
        }
        self.push_len(2, Some(value.len()))?;
        self.0.write_all(value)
    }

    /// Writes a definite-length text string (header and body).
    ///
    /// When used as a segment inside [`Header::Text`]`(None)`, this writes one
    /// well-formed UTF-8 text segment.
    #[inline]
    pub fn text(&mut self, value: &str) -> Result<(), crate::io::Error> {
        // See `bytes` for why only large bodies get a capacity hint.
        if value.len() >= 1024 {
            self.reserve(value.len().saturating_add(9));
        }
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
pub(crate) fn decode_header(
    raw: &[u8; 9],
    arg: Option<u64>,
    start: usize,
) -> Result<Header, Error> {
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
            26 => Header::Float(f32_to_f64(u32::from_be_bytes([
                raw[1], raw[2], raw[3], raw[4],
            ]))),
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
    /// the body with [`read_exact`](Self::read_exact) before continuing.
    /// With `alloc`, `bytes_body` and `text_body` also handle segmented strings.
    #[inline]
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
    /// level `bytes_body` and `text_body` helpers available with `alloc`
    /// also handle segmented strings.
    #[inline]
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
        let mut buffer = Vec::new();
        if let Some(len) = len {
            let offset = self.offset;
            self.read_body(len, &mut buffer)?;
            let text = String::from_utf8(buffer).map_err(|_| Error::Syntax(offset))?;
            if out.is_empty() {
                *out = text;
            } else {
                out.push_str(&text);
            }
            return Ok(());
        }
        loop {
            let offset = self.offset;
            match self.pull()? {
                Header::Break => return Ok(()),
                Header::Text(Some(len)) => {
                    let body_offset = self.offset;
                    buffer.clear();
                    self.read_body(len, &mut buffer)?;
                    let text =
                        core::str::from_utf8(&buffer).map_err(|_| Error::Syntax(body_offset))?;
                    out.push_str(text);
                }
                _ => return Err(Error::Syntax(offset)),
            }
        }
    }
}

// The slice-specialized decoding fast paths. They are available in every
// configuration (the copy-free `validate_slice` builds on them without
// `alloc`); only the recording bookkeeping inside is `alloc`-gated.
impl Decoder<&[u8]> {
    #[inline]
    fn slice_eof_after_prefix(&mut self) -> Error {
        #[cfg(feature = "alloc")]
        if let Some(record) = &mut self.record {
            record.push(self.reader[0]);
        }
        self.reader = &self.reader[1..];
        self.offset += 1;
        crate::io::Error::from(crate::io::ErrorKind::UnexpectedEof).into()
    }

    // The `N` argument bytes that follow the prefix byte. Reading a constant
    // width keeps every arm a fixed-size load.
    #[inline(always)]
    fn slice_arg<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        match self.reader.get(1..=N) {
            Some(bytes) => Ok(bytes.try_into().expect("slice of length N")),
            None => Err(self.slice_eof_after_prefix()),
        }
    }

    #[inline]
    fn finish_slice_header(&mut self, raw: [u8; 9], raw_len: u8) {
        #[cfg(feature = "alloc")]
        {
            self.last_header.0 = raw;
            self.last_header.1 = raw_len;
            if let Some(record) = &mut self.record {
                record.extend_from_slice(&raw[..raw_len as usize]);
            }
        }
        #[cfg(not(feature = "alloc"))]
        let _ = raw;
        self.reader = &self.reader[raw_len as usize..];
        self.offset += raw_len as usize;
    }
}

#[cfg(feature = "alloc")]
impl Decoder<&[u8]> {
    /// Returns true when no input and no pushed-back header remain.
    #[inline]
    pub(crate) fn is_exhausted(&self) -> bool {
        self.pushback.is_none() && self.reader.is_empty()
    }

    /// Pulls a plain positive or negative integer from a byte slice.
    #[inline]
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

        let parsed = match first & 0b00011111 {
            x @ 0..=23 => Ok((u64::from(x), 1)),
            24 => self.slice_arg::<1>().map(|b| (u64::from(b[0]), 2)),
            25 => self
                .slice_arg::<2>()
                .map(|b| (u64::from(u16::from_be_bytes(b)), 3)),
            26 => self
                .slice_arg::<4>()
                .map(|b| (u64::from(u32::from_be_bytes(b)), 5)),
            27 => self.slice_arg::<8>().map(|b| (u64::from_be_bytes(b), 9)),
            _ => Err(Error::Syntax(start)),
        };
        let (value, raw_len) = match parsed {
            Ok(parsed) => parsed,
            Err(err) => return Some(Err(err)),
        };

        self.reader = &self.reader[raw_len as usize..];
        self.offset += raw_len as usize;
        Some(Ok((major == 1, value)))
    }

    /// Pulls a plain boolean from a byte slice.
    #[inline]
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
        let parsed = match first {
            0xf9 => self
                .slice_arg::<2>()
                .map(|b| (f16_to_f64(u16::from_be_bytes(b)), 3)),
            0xfa => self
                .slice_arg::<4>()
                .map(|b| (f32_to_f64(u32::from_be_bytes(b)), 5)),
            0xfb => self
                .slice_arg::<8>()
                .map(|b| (f64::from_bits(u64::from_be_bytes(b)), 9)),
            _ => return None,
        };
        let (value, raw_len) = match parsed {
            Ok(parsed) => parsed,
            Err(err) => return Some(Err(err)),
        };

        self.mark = start;
        self.reader = &self.reader[raw_len..];
        self.offset += raw_len;
        Some(Ok(value))
    }
}

impl<'de> Decoder<&'de [u8]> {
    /// Pulls a header from a byte slice without going through `Read`.
    #[inline]
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
        let (arg, raw_len) = match raw[0] & 0b00011111 {
            x @ 0..=23 => (Some(u64::from(x)), 1),
            24 => {
                let b = self.slice_arg::<1>()?;
                raw[1] = b[0];
                (Some(u64::from(b[0])), 2)
            }
            25 => {
                let b = self.slice_arg::<2>()?;
                raw[1..3].copy_from_slice(&b);
                (Some(u64::from(u16::from_be_bytes(b))), 3)
            }
            26 => {
                let b = self.slice_arg::<4>()?;
                raw[1..5].copy_from_slice(&b);
                (Some(u64::from(u32::from_be_bytes(b))), 5)
            }
            27 => {
                let b = self.slice_arg::<8>()?;
                raw[1..9].copy_from_slice(&b);
                (Some(u64::from_be_bytes(b)), 9)
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
    #[inline]
    pub(crate) fn borrow_body(&mut self, len: usize) -> Result<&'de [u8], crate::io::Error> {
        debug_assert!(self.pushback.is_none());
        if self.reader.len() < len {
            return Err(crate::io::ErrorKind::UnexpectedEof.into());
        }

        let (head, tail) = self.reader.split_at(len);
        self.reader = tail;
        self.offset += len;
        #[cfg(feature = "alloc")]
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
/// This follows RFC 8949 Appendix D, additionally preserving NaN sign,
/// signaling bit and payload through a bitwise widening.
pub fn f16_to_f64(bits: u16) -> f64 {
    let exp = (bits >> 10) & 0x1f;
    let frac = (bits & 0x3ff) as f64;
    if exp == 31 && frac != 0.0 {
        return f64::from_bits(
            (u64::from(bits >> 15) << 63) | 0x7ff0_0000_0000_0000 | (u64::from(bits & 0x3ff) << 42),
        );
    }

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

// Numeric casts may quiet signaling NaNs; widening their bits avoids that.
pub(crate) fn f32_to_f64(bits: u32) -> f64 {
    if bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 {
        return f64::from_bits(
            (u64::from(bits >> 31) << 63)
                | 0x7ff0_0000_0000_0000
                | (u64::from(bits & 0x007f_ffff) << 29),
        );
    }
    f64::from(f32::from_bits(bits))
}

/// Converts an `f64` to IEEE 754 single-precision bits if (and only if) the
/// conversion is lossless.
///
/// A NaN converts when its sign and payload survive the narrowing exactly
/// (the low 29 fraction bits must be zero).
pub fn f64_to_f32(value: f64) -> Option<u32> {
    let bits = value.to_bits();

    let single = if value.is_nan() {
        // Narrow bit by bit: numeric casts may quieten or canonicalize a
        // NaN, losing the payload the caller asked to preserve.
        if bits & ((1 << 29) - 1) != 0 {
            return None;
        }
        let sign = ((bits >> 32) & 0x8000_0000) as u32;
        let frac = ((bits >> 29) & 0x007f_ffff) as u32;
        sign | 0x7f80_0000 | frac
    } else {
        (value as f32).to_bits()
    };

    // Belt and braces: only report success on an exact bit-level round trip
    // through the same widening the decoder performs.
    if f32_to_f64(single).to_bits() == bits {
        Some(single)
    } else {
        None
    }
}

/// Converts an `f64` to IEEE 754 half-precision bits if (and only if) the
/// conversion is lossless, including the sign, signaling bit and payload of NaNs.
pub fn f64_to_f16(value: f64) -> Option<u16> {
    let bits = value.to_bits();
    let sign = ((bits >> 48) & 0x8000) as u16;
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & 0x000f_ffff_ffff_ffff;

    let half = if exp == 0x7ff {
        if frac & ((1 << 42) - 1) != 0 {
            return None;
        }
        sign | 0x7c00 | (frac >> 42) as u16
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
            assert_eq!(f64_to_f16(wide), Some(bits), "bits {bits:04x}");
        }
    }

    // Every f32 bit pattern must survive widening to f64 and re-narrowing,
    // NaN payloads and signaling bits included.
    #[test]
    fn f32_narrowing_roundtrip() {
        for bits in [
            0x0000_0000u32, // 0.0
            0x8000_0000,    // -0.0
            0x3fc0_0000,    // 1.5
            0x7f7f_ffff,    // f32::MAX
            0x0000_0001,    // smallest subnormal
            0x7f80_0000,    // Infinity
            0xff80_0000,    // -Infinity
            0x7fc0_0000,    // canonical quiet NaN
            0xffc0_0000,    // -NaN
            0x7fc0_0100,    // quiet NaN with payload
            0x7f80_0001,    // signaling NaN with payload
        ] {
            let wide = f32_to_f64(bits);
            assert_eq!(f64_to_f32(wide), Some(bits), "bits {bits:08x}");
        }
    }

    // Values that are not representable as f32 must be rejected.
    #[test]
    fn f32_rejects_lossy() {
        for value in [
            0.1,                                   // fraction bits beyond 23
            f64::MAX,                              // exponent out of range
            f64::MIN_POSITIVE,                     // below the subnormal range
            f64::from_bits(0x7ff8_0000_1000_0000), // NaN payload needs 29 low bits
            f64::from_bits(0x7ff0_0000_0000_0001), // NaN payload entirely low
        ] {
            assert_eq!(f64_to_f32(value), None, "{:016x}", value.to_bits());
        }
    }

    // NaN payloads use the narrowest lossless width (RFC 8949 §4.1).
    #[cfg(feature = "alloc")]
    #[test]
    fn nan_payloads_use_preferred_widths() {
        for (bits, expected) in [
            (0x7ff8_0000_0000_0000u64, "f97e00"),          // canonical: f16
            (0xfff8_0000_0000_0000, "f9fe00"),             // -NaN: sign fits f16
            (0x7ff8_0000_2000_0000, "fa7fc00001"),         // payload fits f32
            (0x7ff8_0000_1000_0000, "fb7ff8000010000000"), // payload needs f64
        ] {
            let mut buffer = Vec::new();
            Encoder::from(&mut buffer)
                .push(Header::Float(f64::from_bits(bits)))
                .unwrap();
            assert_eq!(hex::encode(&buffer), expected, "bits {bits:016x}");
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

        // NaNs whose payload would be lost are rejected by the round-trip
        // check; the canonical quiet NaN converts with its sign preserved.
        assert_eq!(f64_to_f16(-f64::NAN), Some(0xfe00));
        assert_eq!(f64_to_f16(f64::from_bits(0x7ff8_0000_0000_0001)), None);
        assert_eq!(f64_to_f16(f64::from_bits(0xfff8_0000_0000_0001)), None);
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
