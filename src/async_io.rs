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

use alloc::{boxed::Box, vec::Vec};
use core::{future::Future, pin::Pin};

use serde::{de, ser};

use crate::core::{f16_to_f64, Header};
use crate::de::{Error, DEFAULT_RECURSION_LIMIT};

const CHUNK: usize = 4096;

/// An async source of bytes.
///
/// Runtime-specific socket types can be adapted with a small newtype that
/// calls the runtime's own `read_exact` method.
#[allow(async_fn_in_trait)]
pub trait AsyncRead {
    /// Reads exactly `buf.len()` bytes into `buf`.
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), crate::io::Error>;
}

/// An async sink for bytes.
///
/// Runtime-specific socket types can be adapted with a small newtype that
/// calls the runtime's own `write_all` and `flush` methods.
#[allow(async_fn_in_trait)]
pub trait AsyncWrite {
    /// Writes all of `data`.
    async fn write_all(&mut self, data: &[u8]) -> Result<(), crate::io::Error>;

    /// Flushes any buffered output.
    async fn flush(&mut self) -> Result<(), crate::io::Error>;
}

impl<T: AsyncRead + ?Sized> AsyncRead for &mut T {
    #[inline]
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), crate::io::Error> {
        (**self).read_exact(buf).await
    }
}

impl<T: AsyncWrite + ?Sized> AsyncWrite for &mut T {
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
/// Enabled with the `futures` feature.
#[cfg(feature = "futures")]
pub mod futures {
    use core::{future::poll_fn, pin::Pin};

    use serde::{de, ser};

    use super::{AsyncRead, AsyncWrite};
    use crate::de::Error;

    struct Reader<'a, R: ?Sized>(&'a mut R);

    impl<R> AsyncRead for Reader<'_, R>
    where
        R: futures_io::AsyncRead + Unpin + ?Sized,
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
        W: futures_io::AsyncWrite + Unpin + ?Sized,
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
        R: futures_io::AsyncRead + Unpin + ?Sized,
    {
        let mut reader = Reader(reader);
        super::read_item(&mut reader).await
    }

    /// Reads one complete CBOR item from a futures reader and deserializes it.
    pub async fn read_value<T, R>(reader: &mut R) -> Result<T, Error>
    where
        T: de::DeserializeOwned,
        R: futures_io::AsyncRead + Unpin + ?Sized,
    {
        let mut reader = Reader(reader);
        super::read_value(&mut reader).await
    }

    /// Writes one already-encoded CBOR item to a futures writer.
    pub async fn write_item<W>(writer: &mut W, item: &[u8]) -> Result<(), Error>
    where
        W: futures_io::AsyncWrite + Unpin + ?Sized,
    {
        let mut writer = Writer(writer);
        super::write_item(&mut writer, item).await
    }

    /// Serializes a value to one CBOR item and writes it to a futures writer.
    pub async fn write_value<T, W>(writer: &mut W, value: &T) -> Result<(), crate::ser::Error>
    where
        T: ?Sized + ser::Serialize,
        W: futures_io::AsyncWrite + Unpin + ?Sized,
    {
        let mut writer = Writer(writer);
        super::write_value(&mut writer, value).await
    }
}

/// Adapters for `tokio::io::AsyncRead` and `tokio::io::AsyncWrite`.
///
/// Enabled with the `tokio` feature.
#[cfg(feature = "tokio")]
pub mod tokio {
    use serde::{de, ser};

    use super::{AsyncRead, AsyncWrite};
    use crate::de::Error;

    struct Reader<'a, R: ?Sized>(&'a mut R);

    impl<R> AsyncRead for Reader<'_, R>
    where
        R: ::tokio::io::AsyncRead + Unpin + ?Sized,
    {
        async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), crate::io::Error> {
            use ::tokio::io::AsyncReadExt as _;

            self.0.read_exact(buf).await.map(|_| ())
        }
    }

    struct Writer<'a, W: ?Sized>(&'a mut W);

    impl<W> AsyncWrite for Writer<'_, W>
    where
        W: ::tokio::io::AsyncWrite + Unpin + ?Sized,
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
        R: ::tokio::io::AsyncRead + Unpin + ?Sized,
    {
        let mut reader = Reader(reader);
        super::read_item(&mut reader).await
    }

    /// Reads one complete CBOR item from a Tokio reader and deserializes it.
    pub async fn read_value<T, R>(reader: &mut R) -> Result<T, Error>
    where
        T: de::DeserializeOwned,
        R: ::tokio::io::AsyncRead + Unpin + ?Sized,
    {
        let mut reader = Reader(reader);
        super::read_value(&mut reader).await
    }

    /// Writes one already-encoded CBOR item to a Tokio writer.
    pub async fn write_item<W>(writer: &mut W, item: &[u8]) -> Result<(), Error>
    where
        W: ::tokio::io::AsyncWrite + Unpin + ?Sized,
    {
        let mut writer = Writer(writer);
        super::write_item(&mut writer, item).await
    }

    /// Serializes a value to one CBOR item and writes it to a Tokio writer.
    pub async fn write_value<T, W>(writer: &mut W, value: &T) -> Result<(), crate::ser::Error>
    where
        T: ?Sized + ser::Serialize,
        W: ::tokio::io::AsyncWrite + Unpin + ?Sized,
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
pub async fn read_item<R: AsyncRead + ?Sized>(reader: &mut R) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    let mut offset = 0;
    read_item_inner(reader, &mut out, &mut offset, DEFAULT_RECURSION_LIMIT).await?;
    Ok(out)
}

/// Reads one complete CBOR item and deserializes it into an owned value.
///
/// Borrowed output cannot be returned from this helper because the temporary
/// item buffer is owned by the function. Use [`read_item`] plus
/// [`from_slice`](crate::from_slice) when the caller wants to keep the buffer
/// alive and borrow from it.
pub async fn read_value<T, R>(reader: &mut R) -> Result<T, Error>
where
    T: de::DeserializeOwned,
    R: AsyncRead + ?Sized,
{
    let item = read_item(reader).await?;
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
) -> Result<(), Error> {
    reader.read_exact(buf).await.map_err(Error::Io)?;
    *offset += buf.len();
    out.extend_from_slice(buf);
    Ok(())
}

async fn pull_header<R: AsyncRead + ?Sized>(
    reader: &mut R,
    out: &mut Vec<u8>,
    offset: &mut usize,
) -> Result<(Header, usize), Error> {
    let start = *offset;
    let mut prefix = [0u8; 1];
    read_exact_record(reader, out, offset, &mut prefix).await?;

    let major = prefix[0] >> 5;
    let minor = prefix[0] & 0b00011111;

    let arg = match minor {
        x @ 0..=23 => Arg::This(x),
        24 => {
            let mut b = [0u8; 1];
            read_exact_record(reader, out, offset, &mut b).await?;
            Arg::Next1(b[0])
        }
        25 => {
            let mut b = [0u8; 2];
            read_exact_record(reader, out, offset, &mut b).await?;
            Arg::Next2(u16::from_be_bytes(b))
        }
        26 => {
            let mut b = [0u8; 4];
            read_exact_record(reader, out, offset, &mut b).await?;
            Arg::Next4(u32::from_be_bytes(b))
        }
        27 => {
            let mut b = [0u8; 8];
            read_exact_record(reader, out, offset, &mut b).await?;
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
) -> Result<(), Error> {
    let mut buffer = [0u8; CHUNK];
    while remaining > 0 {
        let n = remaining.min(buffer.len());
        reader
            .read_exact(&mut buffer[..n])
            .await
            .map_err(Error::Io)?;
        *offset += n;
        out.extend_from_slice(&buffer[..n]);
        remaining -= n;
    }
    Ok(())
}

async fn read_text_body<R: AsyncRead + ?Sized>(
    reader: &mut R,
    out: &mut Vec<u8>,
    offset: &mut usize,
    len: usize,
) -> Result<(), Error> {
    let body_offset = *offset;
    let start = out.len();
    read_body(reader, out, offset, len).await?;
    core::str::from_utf8(&out[start..]).map_err(|_| Error::Syntax(body_offset))?;
    Ok(())
}

fn read_item_inner<'a, R: AsyncRead + ?Sized + 'a>(
    reader: &'a mut R,
    out: &'a mut Vec<u8>,
    offset: &'a mut usize,
    depth: usize,
) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'a>> {
    Box::pin(async move {
        let (header, start) = pull_header(reader, out, offset).await?;
        read_header_payload(reader, out, offset, header, start, depth).await
    })
}

fn read_header_payload<'a, R: AsyncRead + ?Sized + 'a>(
    reader: &'a mut R,
    out: &'a mut Vec<u8>,
    offset: &'a mut usize,
    header: Header,
    start: usize,
    depth: usize,
) -> Pin<Box<dyn Future<Output = Result<(), Error>> + 'a>> {
    Box::pin(async move {
        if depth == 0 {
            return Err(Error::RecursionLimitExceeded);
        }

        match header {
            Header::Positive(..)
            | Header::Negative(..)
            | Header::Float(..)
            | Header::Simple(..) => Ok(()),

            Header::Break => Err(Error::Syntax(start)),

            Header::Tag(..) => read_item_inner(reader, out, offset, depth - 1).await,

            Header::Bytes(Some(len)) => read_body(reader, out, offset, len).await,
            Header::Bytes(None) => loop {
                let (header, start) = pull_header(reader, out, offset).await?;
                match header {
                    Header::Break => return Ok(()),
                    Header::Bytes(Some(len)) => read_body(reader, out, offset, len).await?,
                    _ => return Err(Error::Syntax(start)),
                }
            },

            Header::Text(Some(len)) => read_text_body(reader, out, offset, len).await,
            Header::Text(None) => loop {
                let (header, start) = pull_header(reader, out, offset).await?;
                match header {
                    Header::Break => return Ok(()),
                    Header::Text(Some(len)) => {
                        read_text_body(reader, out, offset, len).await?;
                    }
                    _ => return Err(Error::Syntax(start)),
                }
            },

            Header::Array(Some(len)) => {
                for _ in 0..len {
                    read_item_inner(reader, out, offset, depth - 1).await?;
                }
                Ok(())
            }
            Header::Array(None) => loop {
                let (header, start) = pull_header(reader, out, offset).await?;
                if header == Header::Break {
                    return Ok(());
                }
                read_header_payload(reader, out, offset, header, start, depth - 1).await?;
            },

            Header::Map(Some(len)) => {
                for _ in 0..len {
                    read_item_inner(reader, out, offset, depth - 1).await?;
                    read_item_inner(reader, out, offset, depth - 1).await?;
                }
                Ok(())
            }
            Header::Map(None) => {
                let mut expecting_value = false;
                loop {
                    let (header, start) = pull_header(reader, out, offset).await?;
                    match header {
                        Header::Break if expecting_value => return Err(Error::Syntax(start)),
                        Header::Break => return Ok(()),
                        header => {
                            read_header_payload(reader, out, offset, header, start, depth - 1)
                                .await?;
                            expecting_value = !expecting_value;
                        }
                    }
                }
            }
        }
    })
}
