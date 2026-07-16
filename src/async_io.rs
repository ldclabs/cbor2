//! Async helpers for complete CBOR item I/O.
//!
//! Serde deserialization is synchronous, so this module does not try to make
//! serde itself resumable. Instead it solves the practical async socket use
//! case: read exactly one complete CBOR item into bytes, or write one complete
//! item out, then hand those bytes to the regular [`from_slice`](crate::from_slice)
//! and [`to_vec`](crate::to_vec) APIs.
//!
//! Enable the `futures` or `tokio` crate features to use the adapters in the
//! `async_io::futures` or `async_io::tokio` modules.
//!
//! The item walk is iterative, so the futures returned here are plain state
//! machines: when the reader or writer is `Send`, so is the future, and it
//! can be driven by multi-threaded executors such as `tokio::spawn`.
//!
//! # Cancellation safety
//!
//! The read helpers are **not cancellation-safe**, for the same reason as
//! `tokio::io::AsyncReadExt::read_exact`: a read future dropped before
//! completion — a lost `select!` race against a timeout, for example — may
//! have consumed bytes that are gone with it, leaving the stream in the
//! middle of an item. Further reads on that stream would misparse the
//! remainder as fresh items, so discard the connection instead of reusing
//! it. When peers must survive a timed-out read, put the timeout around
//! the connection (closing it on expiry) rather than around an individual
//! read future.

use alloc::vec::Vec;
use core::future::Future;

use serde::{de, ser};

use crate::core::{f16_to_f64, Header};
use crate::de::{Error, DEFAULT_RECURSION_LIMIT};

const CHUNK: usize = 4096;

/// An async source of bytes.
///
/// Runtime-specific socket types can be adapted with a small newtype that
/// calls the runtime's own `read_exact` method.
///
/// The returned future is `Send`, so the [`read_item`]/[`read_value`]
/// helpers stay `Send` even through generic code and can be driven by
/// multi-threaded executors such as `tokio::spawn`. Implementors whose
/// future would not be `Send` are not supported.
pub trait AsyncRead {
    /// Reads exactly `buf.len()` bytes into `buf`.
    ///
    /// Like `tokio::io::AsyncReadExt::read_exact`, this is not expected to
    /// be cancellation-safe: a future dropped mid-read loses the bytes it
    /// already consumed. See the [module docs](self) on cancellation
    /// safety.
    fn read_exact(
        &mut self,
        buf: &mut [u8],
    ) -> impl Future<Output = Result<(), crate::io::Error>> + Send;
}

/// An async sink for bytes.
///
/// Runtime-specific socket types can be adapted with a small newtype that
/// calls the runtime's own `write_all` and `flush` methods.
///
/// The returned futures are `Send`; see [`AsyncRead`].
pub trait AsyncWrite {
    /// Writes all of `data`.
    fn write_all(
        &mut self,
        data: &[u8],
    ) -> impl Future<Output = Result<(), crate::io::Error>> + Send;

    /// Flushes any buffered output.
    fn flush(&mut self) -> impl Future<Output = Result<(), crate::io::Error>> + Send;
}

impl<T: AsyncRead + Send + ?Sized> AsyncRead for &mut T {
    #[inline]
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), crate::io::Error> {
        (**self).read_exact(buf).await
    }
}

impl<T: AsyncWrite + Send + ?Sized> AsyncWrite for &mut T {
    #[inline]
    async fn write_all(&mut self, data: &[u8]) -> Result<(), crate::io::Error> {
        (**self).write_all(data).await
    }

    #[inline]
    async fn flush(&mut self) -> Result<(), crate::io::Error> {
        (**self).flush().await
    }
}

/// Adapters for [`futures_io::AsyncRead`] and [`futures_io::AsyncWrite`].
///
/// Enabled with the `futures` feature. The read helpers are not
/// cancellation-safe; see the [module docs](super) on cancellation safety.
#[cfg(feature = "futures")]
pub mod futures {
    use core::{future::poll_fn, pin::Pin};

    use serde::{de, ser};

    use super::{AsyncRead, AsyncWrite};
    use crate::de::Error;

    struct Reader<'a, R: ?Sized>(&'a mut R);

    impl<R> AsyncRead for Reader<'_, R>
    where
        R: futures_io::AsyncRead + Unpin + Send + ?Sized,
    {
        async fn read_exact(&mut self, mut buf: &mut [u8]) -> Result<(), crate::io::Error> {
            while !buf.is_empty() {
                let n = poll_fn(|cx| Pin::new(&mut *self.0).poll_read(cx, buf)).await?;
                if n == 0 {
                    return Err(crate::io::ErrorKind::UnexpectedEof.into());
                }

                let rest = core::mem::take(&mut buf);
                let (_, rest) = rest.split_at_mut(n);
                buf = rest;
            }

            Ok(())
        }
    }

    struct Writer<'a, W: ?Sized>(&'a mut W);

    impl<W> AsyncWrite for Writer<'_, W>
    where
        W: futures_io::AsyncWrite + Unpin + Send + ?Sized,
    {
        async fn write_all(&mut self, mut data: &[u8]) -> Result<(), crate::io::Error> {
            while !data.is_empty() {
                let n = poll_fn(|cx| Pin::new(&mut *self.0).poll_write(cx, data)).await?;
                if n == 0 {
                    return Err(crate::io::ErrorKind::WriteZero.into());
                }
                data = &data[n..];
            }

            Ok(())
        }

        async fn flush(&mut self) -> Result<(), crate::io::Error> {
            poll_fn(|cx| Pin::new(&mut *self.0).poll_flush(cx)).await
        }
    }

    /// Reads one complete, well-formed CBOR item from a futures reader.
    pub async fn read_item<R>(reader: &mut R) -> Result<alloc::vec::Vec<u8>, Error>
    where
        R: futures_io::AsyncRead + Unpin + Send + ?Sized,
    {
        let mut reader = Reader(reader);
        super::read_item(&mut reader).await
    }

    /// Reads one complete, well-formed CBOR item from a futures reader,
    /// rejecting items larger than `max_len` bytes.
    pub async fn read_item_with_limit<R>(
        reader: &mut R,
        max_len: usize,
    ) -> Result<alloc::vec::Vec<u8>, Error>
    where
        R: futures_io::AsyncRead + Unpin + Send + ?Sized,
    {
        let mut reader = Reader(reader);
        super::read_item_with_limit(&mut reader, max_len).await
    }

    /// Reads one complete CBOR item from a futures reader and deserializes it.
    pub async fn read_value<T, R>(reader: &mut R) -> Result<T, Error>
    where
        T: de::DeserializeOwned,
        R: futures_io::AsyncRead + Unpin + Send + ?Sized,
    {
        let mut reader = Reader(reader);
        super::read_value(&mut reader).await
    }

    /// Reads one complete bounded CBOR item from a futures reader and
    /// deserializes it.
    pub async fn read_value_with_limit<T, R>(reader: &mut R, max_len: usize) -> Result<T, Error>
    where
        T: de::DeserializeOwned,
        R: futures_io::AsyncRead + Unpin + Send + ?Sized,
    {
        let mut reader = Reader(reader);
        super::read_value_with_limit(&mut reader, max_len).await
    }

    /// Writes one already-encoded CBOR item to a futures writer.
    pub async fn write_item<W>(writer: &mut W, item: &[u8]) -> Result<(), Error>
    where
        W: futures_io::AsyncWrite + Unpin + Send + ?Sized,
    {
        let mut writer = Writer(writer);
        super::write_item(&mut writer, item).await
    }

    /// Serializes a value to one CBOR item and writes it to a futures writer.
    pub async fn write_value<T, W>(writer: &mut W, value: &T) -> Result<(), crate::ser::Error>
    where
        T: ?Sized + ser::Serialize,
        W: futures_io::AsyncWrite + Unpin + Send + ?Sized,
    {
        let mut writer = Writer(writer);
        super::write_value(&mut writer, value).await
    }
}

/// Adapters for `tokio::io::AsyncRead` and `tokio::io::AsyncWrite`.
///
/// Enabled with the `tokio` feature. The read helpers are not
/// cancellation-safe; see the [module docs](super) on cancellation safety.
#[cfg(feature = "tokio")]
pub mod tokio {
    use serde::{de, ser};

    use super::{AsyncRead, AsyncWrite};
    use crate::de::Error;

    struct Reader<'a, R: ?Sized>(&'a mut R);

    impl<R> AsyncRead for Reader<'_, R>
    where
        R: ::tokio::io::AsyncRead + Unpin + Send + ?Sized,
    {
        async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), crate::io::Error> {
            use ::tokio::io::AsyncReadExt as _;

            self.0.read_exact(buf).await.map(|_| ())
        }
    }

    struct Writer<'a, W: ?Sized>(&'a mut W);

    impl<W> AsyncWrite for Writer<'_, W>
    where
        W: ::tokio::io::AsyncWrite + Unpin + Send + ?Sized,
    {
        async fn write_all(&mut self, data: &[u8]) -> Result<(), crate::io::Error> {
            use ::tokio::io::AsyncWriteExt as _;

            self.0.write_all(data).await
        }

        async fn flush(&mut self) -> Result<(), crate::io::Error> {
            use ::tokio::io::AsyncWriteExt as _;

            self.0.flush().await
        }
    }

    /// Reads one complete, well-formed CBOR item from a Tokio reader.
    pub async fn read_item<R>(reader: &mut R) -> Result<alloc::vec::Vec<u8>, Error>
    where
        R: ::tokio::io::AsyncRead + Unpin + Send + ?Sized,
    {
        let mut reader = Reader(reader);
        super::read_item(&mut reader).await
    }

    /// Reads one complete, well-formed CBOR item from a Tokio reader,
    /// rejecting items larger than `max_len` bytes.
    pub async fn read_item_with_limit<R>(
        reader: &mut R,
        max_len: usize,
    ) -> Result<alloc::vec::Vec<u8>, Error>
    where
        R: ::tokio::io::AsyncRead + Unpin + Send + ?Sized,
    {
        let mut reader = Reader(reader);
        super::read_item_with_limit(&mut reader, max_len).await
    }

    /// Reads one complete CBOR item from a Tokio reader and deserializes it.
    pub async fn read_value<T, R>(reader: &mut R) -> Result<T, Error>
    where
        T: de::DeserializeOwned,
        R: ::tokio::io::AsyncRead + Unpin + Send + ?Sized,
    {
        let mut reader = Reader(reader);
        super::read_value(&mut reader).await
    }

    /// Reads one complete bounded CBOR item from a Tokio reader and
    /// deserializes it.
    pub async fn read_value_with_limit<T, R>(reader: &mut R, max_len: usize) -> Result<T, Error>
    where
        T: de::DeserializeOwned,
        R: ::tokio::io::AsyncRead + Unpin + Send + ?Sized,
    {
        let mut reader = Reader(reader);
        super::read_value_with_limit(&mut reader, max_len).await
    }

    /// Writes one already-encoded CBOR item to a Tokio writer.
    pub async fn write_item<W>(writer: &mut W, item: &[u8]) -> Result<(), Error>
    where
        W: ::tokio::io::AsyncWrite + Unpin + Send + ?Sized,
    {
        let mut writer = Writer(writer);
        super::write_item(&mut writer, item).await
    }

    /// Serializes a value to one CBOR item and writes it to a Tokio writer.
    pub async fn write_value<T, W>(writer: &mut W, value: &T) -> Result<(), crate::ser::Error>
    where
        T: ?Sized + ser::Serialize,
        W: ::tokio::io::AsyncWrite + Unpin + Send + ?Sized,
    {
        let mut writer = Writer(writer);
        super::write_value(&mut writer, value).await
    }
}

/// Reads one complete, well-formed CBOR item from an async reader.
///
/// The returned bytes are exactly the item read from the stream. Text strings
/// are validated as UTF-8 and nesting is bounded by the same recursion limit
/// as the synchronous deserializer.
///
/// Not cancellation-safe; see the [module docs](self).
pub async fn read_item<R: AsyncRead + ?Sized>(reader: &mut R) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    let mut offset = 0;
    read_item_inner(reader, &mut out, &mut offset, DEFAULT_RECURSION_LIMIT, None).await?;
    Ok(out)
}

/// Reads one complete, well-formed CBOR item from an async reader,
/// rejecting items larger than `max_len` bytes.
///
/// The limit is checked against the exact encoded item length, including
/// headers and string bodies. Use this for untrusted async streams when an
/// external transport or framing layer does not already impose a message
/// size limit.
///
/// Not cancellation-safe; see the [module docs](self).
pub async fn read_item_with_limit<R: AsyncRead + ?Sized>(
    reader: &mut R,
    max_len: usize,
) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    let mut offset = 0;
    read_item_inner(
        reader,
        &mut out,
        &mut offset,
        DEFAULT_RECURSION_LIMIT,
        Some(max_len),
    )
    .await?;
    Ok(out)
}

/// Reads one complete CBOR item and deserializes it into an owned value.
///
/// Borrowed output cannot be returned from this helper because the temporary
/// item buffer is owned by the function. Use [`read_item`] plus
/// [`from_slice`](crate::from_slice) when the caller wants to keep the buffer
/// alive and borrow from it.
///
/// Not cancellation-safe; see the [module docs](self).
pub async fn read_value<T, R>(reader: &mut R) -> Result<T, Error>
where
    T: de::DeserializeOwned,
    R: AsyncRead + ?Sized,
{
    let item = read_item(reader).await?;
    crate::from_slice(&item)
}

/// Reads one bounded CBOR item and deserializes it into an owned value.
///
/// This is the bounded counterpart of [`read_value`]; see
/// [`read_item_with_limit`] for the limit semantics.
///
/// Not cancellation-safe; see the [module docs](self).
pub async fn read_value_with_limit<T, R>(reader: &mut R, max_len: usize) -> Result<T, Error>
where
    T: de::DeserializeOwned,
    R: AsyncRead + ?Sized,
{
    let item = read_item_with_limit(reader, max_len).await?;
    crate::from_slice(&item)
}

/// Writes one already-encoded CBOR item to an async writer.
///
/// The bytes are validated before writing so a caller cannot accidentally
/// send a partial item or a CBOR sequence through this exact-one-item helper.
pub async fn write_item<W: AsyncWrite + ?Sized>(writer: &mut W, item: &[u8]) -> Result<(), Error> {
    crate::validate(item)?;
    writer.write_all(item).await.map_err(Error::Io)?;
    writer.flush().await.map_err(Error::Io)
}

/// Serializes a value to one CBOR item and writes it to an async writer.
pub async fn write_value<T, W>(writer: &mut W, value: &T) -> Result<(), crate::ser::Error>
where
    T: ?Sized + ser::Serialize,
    W: AsyncWrite + ?Sized,
{
    let item = crate::to_vec(value)?;
    writer
        .write_all(&item)
        .await
        .map_err(crate::ser::Error::Io)?;
    writer.flush().await.map_err(crate::ser::Error::Io)
}

#[derive(Copy, Clone)]
enum Arg {
    This(u8),
    Next1(u8),
    Next2(u16),
    Next4(u32),
    Next8(u64),
    Indefinite,
}

fn int_arg(arg: Arg) -> Option<u64> {
    match arg {
        Arg::This(x) => Some(x as u64),
        Arg::Next1(x) => Some(x as u64),
        Arg::Next2(x) => Some(x as u64),
        Arg::Next4(x) => Some(x as u64),
        Arg::Next8(x) => Some(x),
        Arg::Indefinite => None,
    }
}

#[cfg(target_pointer_width = "64")]
fn len_arg(arg: Arg, _start: usize) -> Result<Option<usize>, Error> {
    Ok(int_arg(arg).map(|x| x as usize))
}

#[cfg(not(target_pointer_width = "64"))]
fn len_arg(arg: Arg, start: usize) -> Result<Option<usize>, Error> {
    match int_arg(arg) {
        Some(x) => usize::try_from(x)
            .map(Some)
            .map_err(|_| Error::Syntax(start)),
        None => Ok(None),
    }
}

async fn read_exact_record<R: AsyncRead + ?Sized>(
    reader: &mut R,
    out: &mut Vec<u8>,
    offset: &mut usize,
    buf: &mut [u8],
    max_len: Option<usize>,
) -> Result<(), Error> {
    check_size_limit(*offset, buf.len(), max_len)?;
    reader.read_exact(buf).await.map_err(Error::Io)?;
    *offset += buf.len();
    out.extend_from_slice(buf);
    Ok(())
}

fn check_size_limit(offset: usize, additional: usize, max_len: Option<usize>) -> Result<(), Error> {
    if let Some(max_len) = max_len {
        if additional > max_len.saturating_sub(offset) {
            return Err(Error::semantic(offset, "CBOR item exceeds size limit"));
        }
    }
    Ok(())
}

async fn pull_header<R: AsyncRead + ?Sized>(
    reader: &mut R,
    out: &mut Vec<u8>,
    offset: &mut usize,
    max_len: Option<usize>,
) -> Result<(Header, usize), Error> {
    let start = *offset;
    let mut prefix = [0u8; 1];
    read_exact_record(reader, out, offset, &mut prefix, max_len).await?;

    let major = prefix[0] >> 5;
    let minor = prefix[0] & 0b00011111;

    let arg = match minor {
        x @ 0..=23 => Arg::This(x),
        24 => {
            let mut b = [0u8; 1];
            read_exact_record(reader, out, offset, &mut b, max_len).await?;
            Arg::Next1(b[0])
        }
        25 => {
            let mut b = [0u8; 2];
            read_exact_record(reader, out, offset, &mut b, max_len).await?;
            Arg::Next2(u16::from_be_bytes(b))
        }
        26 => {
            let mut b = [0u8; 4];
            read_exact_record(reader, out, offset, &mut b, max_len).await?;
            Arg::Next4(u32::from_be_bytes(b))
        }
        27 => {
            let mut b = [0u8; 8];
            read_exact_record(reader, out, offset, &mut b, max_len).await?;
            Arg::Next8(u64::from_be_bytes(b))
        }
        31 => Arg::Indefinite,
        _ => return Err(Error::Syntax(start)),
    };

    let header = match major {
        0 => Header::Positive(int_arg(arg).ok_or(Error::Syntax(start))?),
        1 => Header::Negative(int_arg(arg).ok_or(Error::Syntax(start))?),
        2 => Header::Bytes(len_arg(arg, start)?),
        3 => Header::Text(len_arg(arg, start)?),
        4 => Header::Array(len_arg(arg, start)?),
        5 => Header::Map(len_arg(arg, start)?),
        6 => Header::Tag(int_arg(arg).ok_or(Error::Syntax(start))?),
        _ => match arg {
            Arg::This(x) => Header::Simple(x),
            Arg::Next1(x) if x >= 32 => Header::Simple(x),
            Arg::Next1(..) => return Err(Error::Syntax(start)),
            Arg::Next2(x) => Header::Float(f16_to_f64(x)),
            Arg::Next4(x) => Header::Float(f32::from_bits(x) as f64),
            Arg::Next8(x) => Header::Float(f64::from_bits(x)),
            Arg::Indefinite => Header::Break,
        },
    };

    Ok((header, start))
}

async fn read_body<R: AsyncRead + ?Sized>(
    reader: &mut R,
    out: &mut Vec<u8>,
    offset: &mut usize,
    mut remaining: usize,
    max_len: Option<usize>,
) -> Result<(), Error> {
    // Grow `out` chunk by chunk and read straight into it: memory still
    // only grows as data actually arrives, without staging each chunk
    // through a separate buffer first.
    while remaining > 0 {
        let n = remaining.min(CHUNK);
        check_size_limit(*offset, n, max_len)?;
        let used = out.len();
        out.resize(used + n, 0);
        reader
            .read_exact(&mut out[used..])
            .await
            .map_err(Error::Io)?;
        *offset += n;
        remaining -= n;
    }
    Ok(())
}

async fn read_text_body<R: AsyncRead + ?Sized>(
    reader: &mut R,
    out: &mut Vec<u8>,
    offset: &mut usize,
    len: usize,
    max_len: Option<usize>,
) -> Result<(), Error> {
    let body_offset = *offset;
    let start = out.len();
    read_body(reader, out, offset, len, max_len).await?;
    core::str::from_utf8(&out[start..]).map_err(|_| Error::Syntax(body_offset))?;
    Ok(())
}

// One open container while walking an item. The walk is iterative rather
// than recursive so the resulting future is a plain state machine (no boxed
// `dyn Future`), which keeps `read_item` `Send` for `Send` readers — a
// requirement for `tokio::spawn`. Recursion depth becomes the stack length.
enum Frame {
    // A definite-length array: this many items remain.
    Array(usize),
    // A definite-length map: this many key/value pairs remain. `value` is
    // true when the next item read completes a pair (a value rather than a
    // key).
    Map { pairs: usize, value: bool },
    // An indefinite-length array, read until `Break`.
    IndefArray,
    // An indefinite-length map, read until `Break`. `value` as above; a
    // `Break` is only well-formed between pairs (when `value` is false).
    IndefMap { value: bool },
}

// Reads one complete item, appending its exact wire bytes to `out`. A tag is
// treated as a one-item container so its content is walked at the next level.
async fn read_item_inner<R: AsyncRead + ?Sized>(
    reader: &mut R,
    out: &mut Vec<u8>,
    offset: &mut usize,
    limit: usize,
    max_len: Option<usize>,
) -> Result<(), Error> {
    // The initial one-item frame is the budget for the single top-level item.
    let mut stack = Vec::with_capacity(8);
    stack.push(Frame::Array(1));

    while !stack.is_empty() {
        // Close any finished definite container, bubbling up through parents.
        match stack.last().expect("non-empty") {
            Frame::Array(0)
            | Frame::Map {
                pairs: 0,
                value: false,
            } => {
                stack.pop();
                continue;
            }
            _ => {}
        }

        let (header, start) = pull_header(reader, out, offset, max_len).await?;

        if header == Header::Break {
            match stack.last().expect("non-empty") {
                Frame::IndefArray | Frame::IndefMap { value: false } => {
                    stack.pop();
                    continue;
                }
                // A break inside a definite container, between a map key and
                // its value, or at the top level is not well-formed.
                _ => return Err(Error::Syntax(start)),
            }
        }

        // Count this item against the current frame.
        match stack.last_mut().expect("non-empty") {
            Frame::Array(n) => *n -= 1,
            Frame::Map { pairs, value } => {
                if *value {
                    *pairs -= 1;
                }
                *value = !*value;
            }
            Frame::IndefArray => {}
            Frame::IndefMap { value } => *value = !*value,
        }

        // Walk the item's own body: read string bodies inline, and push a
        // frame for each container (a tag wraps exactly one item).
        match header {
            Header::Positive(..)
            | Header::Negative(..)
            | Header::Float(..)
            | Header::Simple(..) => {}

            Header::Break => unreachable!("handled above"),

            Header::Bytes(Some(len)) => read_body(reader, out, offset, len, max_len).await?,
            Header::Text(Some(len)) => read_text_body(reader, out, offset, len, max_len).await?,
            Header::Bytes(None) => read_indef_string(reader, out, offset, false, max_len).await?,
            Header::Text(None) => read_indef_string(reader, out, offset, true, max_len).await?,

            Header::Tag(..) => push(&mut stack, Frame::Array(1), limit)?,
            Header::Array(Some(len)) => push(&mut stack, Frame::Array(len), limit)?,
            Header::Array(None) => push(&mut stack, Frame::IndefArray, limit)?,
            Header::Map(Some(pairs)) => push(
                &mut stack,
                Frame::Map {
                    pairs,
                    value: false,
                },
                limit,
            )?,
            Header::Map(None) => push(&mut stack, Frame::IndefMap { value: false }, limit)?,
        }
    }

    Ok(())
}

// Pushes a nested container, enforcing the recursion limit on nesting depth.
fn push(stack: &mut Vec<Frame>, frame: Frame, limit: usize) -> Result<(), Error> {
    if stack.len() >= limit {
        return Err(Error::RecursionLimitExceeded);
    }
    stack.push(frame);
    Ok(())
}

// Reads the segments of an indefinite-length byte or text string until the
// terminating `Break`. Segments must be definite-length strings of the same
// major type (RFC 8949 §3.2.3); text segments are individually UTF-8 checked.
async fn read_indef_string<R: AsyncRead + ?Sized>(
    reader: &mut R,
    out: &mut Vec<u8>,
    offset: &mut usize,
    text: bool,
    max_len: Option<usize>,
) -> Result<(), Error> {
    loop {
        let (header, start) = pull_header(reader, out, offset, max_len).await?;
        match header {
            Header::Break => return Ok(()),
            Header::Text(Some(len)) if text => {
                read_text_body(reader, out, offset, len, max_len).await?
            }
            Header::Bytes(Some(len)) if !text => {
                read_body(reader, out, offset, len, max_len).await?
            }
            _ => return Err(Error::Syntax(start)),
        }
    }
}
