//! Async complete-item I/O helpers.

use std::future::Future;
use std::task::{Context, Poll};

use cbor2::async_io::{self, AsyncRead, AsyncWrite};

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = std::task::Waker::noop();
    let mut cx = Context::from_waker(waker);
    let mut future = std::pin::pin!(future);

    match future.as_mut().poll(&mut cx) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("test future unexpectedly pending"),
    }
}

struct Cursor {
    data: Vec<u8>,
    pos: usize,
}

impl Cursor {
    fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }
}

impl AsyncRead for Cursor {
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), std::io::Error> {
        let end = self.pos + buf.len();
        if end > self.data.len() {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf.copy_from_slice(&self.data[self.pos..end]);
        self.pos = end;
        Ok(())
    }
}

#[derive(Default)]
struct Sink {
    data: Vec<u8>,
    flushed: bool,
}

impl AsyncWrite for Sink {
    async fn write_all(&mut self, data: &[u8]) -> Result<(), std::io::Error> {
        self.data.extend_from_slice(data);
        Ok(())
    }

    async fn flush(&mut self) -> Result<(), std::io::Error> {
        self.flushed = true;
        Ok(())
    }
}

#[test]
fn read_item_stops_at_one_complete_item() {
    let first = cbor2::to_vec(&("hi", 7u8)).unwrap();
    let second = cbor2::to_vec(&false).unwrap();
    let mut stream = first.clone();
    stream.extend_from_slice(&second);

    let mut reader = Cursor::new(stream);
    let item = block_on(async_io::read_item(&mut reader)).unwrap();
    assert_eq!(item, first);
    assert_eq!(reader.pos, first.len());

    let value: (String, u8) = cbor2::from_slice(&item).unwrap();
    assert_eq!(value, ("hi".into(), 7));
}

#[test]
fn read_value_deserializes_owned_values() {
    let bytes = cbor2::to_vec(&vec!["a".to_string(), "b".to_string()]).unwrap();
    let mut reader = Cursor::new(bytes);
    let out: Vec<String> = block_on(async_io::read_value(&mut reader)).unwrap();
    assert_eq!(out, ["a", "b"]);
}

#[test]
fn read_item_with_limit_enforces_total_item_size() {
    let mut bytes = vec![0x58, 0x20]; // h'..' with a one-byte length argument.
    bytes.extend(std::iter::repeat_n(0xab, 32));

    let mut reader = Cursor::new(bytes.clone());
    let item = block_on(async_io::read_item_with_limit(&mut reader, bytes.len())).unwrap();
    assert_eq!(item, bytes);
    assert_eq!(reader.pos, bytes.len());

    let mut reader = Cursor::new(bytes);
    let err = block_on(async_io::read_item_with_limit(&mut reader, 8)).unwrap_err();
    assert!(err.to_string().contains("exceeds size limit"), "{err}");
    assert_eq!(
        reader.pos, 2,
        "body bytes must not be read after limit failure"
    );
}

#[test]
fn read_value_with_limit_deserializes_owned_values() {
    let bytes = cbor2::to_vec(&7u8).unwrap();
    let mut reader = Cursor::new(bytes.clone());
    let out: u8 = block_on(async_io::read_value_with_limit(&mut reader, bytes.len())).unwrap();
    assert_eq!(out, 7);

    let mut reader = Cursor::new(bytes);
    assert!(block_on(async_io::read_value_with_limit::<u8, _>(&mut reader, 0)).is_err());
    assert_eq!(reader.pos, 0);
}

#[test]
fn read_item_validates_text_and_structure() {
    let mut invalid_text = Cursor::new(hex::decode("62fffe").unwrap());
    assert!(matches!(
        block_on(async_io::read_item(&mut invalid_text)),
        Err(cbor2::de::Error::Syntax(1))
    ));

    let mut dangling_key = Cursor::new(hex::decode("bf6161ff").unwrap());
    assert!(block_on(async_io::read_item(&mut dangling_key)).is_err());
}

#[test]
fn read_item_walks_nested_indefinite_and_tags() {
    // [tag(0)"x", {1: 2}, (_ "ab" "c"), (_ h'de' h'ad'), [1, 2, 3]]
    let item = hex::decode("85c06178a101027f6261626163ff5f41de41adff83010203").unwrap();
    assert!(
        cbor2::validate(&item[..]).is_ok(),
        "test vector must be one item"
    );

    let mut stream = item.clone();
    stream.extend_from_slice(&cbor2::to_vec(&99u8).unwrap());

    let mut reader = Cursor::new(stream);
    let got = block_on(async_io::read_item(&mut reader)).unwrap();
    assert_eq!(got, item);
    assert_eq!(reader.pos, item.len());
}

#[test]
fn read_item_enforces_recursion_limit() {
    // Far more nested single-element arrays than the recursion limit.
    let mut bytes = vec![0x81u8; 1000];
    bytes.push(0x00);
    let mut reader = Cursor::new(bytes);
    assert!(matches!(
        block_on(async_io::read_item(&mut reader)),
        Err(cbor2::de::Error::RecursionLimitExceeded)
    ));
}

#[test]
fn read_item_future_is_send() {
    // A concrete `Send` reader must yield a `Send` future so the helper can
    // be driven by `tokio::spawn` and other multi-threaded executors.
    fn assert_send<T: Send>(value: T) -> T {
        value
    }

    let one = cbor2::to_vec(&1u8).unwrap();
    let mut reader = Cursor::new(one.clone());
    let fut = assert_send(async_io::read_item(&mut reader));
    assert_eq!(block_on(fut).unwrap(), one);
}

#[test]
fn futures_are_send_in_generic_context() {
    // These generic bodies type-check only if the helper futures are `Send`
    // for *any* `Send` reader/writer — i.e. the `Send` guarantee survives
    // generic code, not just concrete call sites (`tokio::spawn` needs it).
    fn assert_send<T: Send>(value: T) -> T {
        value
    }

    fn check_read<R: AsyncRead + Send>(reader: &mut R) -> impl Send + '_ {
        assert_send(async_io::read_item(reader))
    }

    fn check_value<R: AsyncRead + Send>(reader: &mut R) -> impl Send + '_ {
        assert_send(async_io::read_value::<u8, R>(reader))
    }

    fn check_write<W: AsyncWrite + Send>(writer: &mut W) -> impl Send + '_ {
        assert_send(async_io::write_value(writer, &1u8))
    }

    // Reference the functions so they are instantiated and not dead code.
    let _ = (
        check_read::<Cursor>,
        check_value::<Cursor>,
        check_write::<Sink>,
    );
}

#[test]
fn write_helpers_emit_exactly_one_item() {
    let mut sink = Sink::default();
    block_on(async_io::write_value(&mut sink, &("ok", 1u8))).unwrap();
    assert!(sink.flushed);
    assert_eq!(
        cbor2::from_slice::<(String, u8)>(&sink.data).unwrap(),
        ("ok".into(), 1)
    );

    let mut item = cbor2::to_vec(&1u8).unwrap();
    item.extend_from_slice(&cbor2::to_vec(&2u8).unwrap());
    assert!(block_on(async_io::write_item(&mut Sink::default(), &item)).is_err());
}

#[cfg(feature = "futures")]
struct FuturesCursor(Cursor);

#[cfg(feature = "futures")]
impl futures_io::AsyncRead for FuturesCursor {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        let remaining = self.0.data.len().saturating_sub(self.0.pos);
        let n = remaining.min(buf.len());
        if n == 0 {
            return Poll::Ready(Ok(0));
        }

        let end = self.0.pos + n;
        buf[..n].copy_from_slice(&self.0.data[self.0.pos..end]);
        self.0.pos = end;
        Poll::Ready(Ok(n))
    }
}

#[cfg(feature = "futures")]
#[derive(Default)]
struct FuturesSink(Sink);

#[cfg(feature = "futures")]
impl futures_io::AsyncWrite for FuturesSink {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        self.0.data.extend_from_slice(data);
        Poll::Ready(Ok(data.len()))
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        self.0.flushed = true;
        Poll::Ready(Ok(()))
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(feature = "futures")]
#[test]
fn futures_async_traits_are_supported() {
    let first = cbor2::to_vec(&("futures", 7u8)).unwrap();
    let mut reader = FuturesCursor(Cursor::new(first.clone()));
    let item = block_on(async_io::futures::read_item(&mut reader)).unwrap();
    assert_eq!(item, first);

    let mut reader = FuturesCursor(Cursor::new(first.clone()));
    let item = block_on(async_io::futures::read_item_with_limit(
        &mut reader,
        first.len(),
    ))
    .unwrap();
    assert_eq!(item, first);

    let mut reader = FuturesCursor(Cursor::new(cbor2::to_vec(&"owned").unwrap()));
    let value: String = block_on(async_io::futures::read_value(&mut reader)).unwrap();
    assert_eq!(value, "owned");

    let mut reader = FuturesCursor(Cursor::new(cbor2::to_vec(&"bounded").unwrap()));
    let value: String =
        block_on(async_io::futures::read_value_with_limit(&mut reader, 16)).unwrap();
    assert_eq!(value, "bounded");

    let mut sink = FuturesSink::default();
    block_on(async_io::futures::write_value(&mut sink, &("ok", 9u8))).unwrap();
    assert!(sink.0.flushed);
    assert_eq!(
        cbor2::from_slice::<(String, u8)>(&sink.0.data).unwrap(),
        ("ok".into(), 9)
    );

    let mut sink = FuturesSink::default();
    block_on(async_io::futures::write_item(&mut sink, &first)).unwrap();
    assert_eq!(sink.0.data, first);
}

#[cfg(feature = "tokio")]
struct TokioCursor(Cursor);

#[cfg(feature = "tokio")]
impl ::tokio::io::AsyncRead for TokioCursor {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &mut ::tokio::io::ReadBuf<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        let remaining = self.0.data.len().saturating_sub(self.0.pos);
        let n = remaining.min(buf.remaining());
        let end = self.0.pos + n;
        buf.put_slice(&self.0.data[self.0.pos..end]);
        self.0.pos = end;
        Poll::Ready(Ok(()))
    }
}

#[cfg(feature = "tokio")]
#[derive(Default)]
struct TokioSink(Sink);

#[cfg(feature = "tokio")]
impl ::tokio::io::AsyncWrite for TokioSink {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        self.0.data.extend_from_slice(data);
        Poll::Ready(Ok(data.len()))
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        self.0.flushed = true;
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }
}

#[cfg(feature = "tokio")]
#[test]
fn tokio_async_traits_are_supported() {
    let first = cbor2::to_vec(&("tokio", 7u8)).unwrap();
    let mut reader = TokioCursor(Cursor::new(first.clone()));
    let item = block_on(async_io::tokio::read_item(&mut reader)).unwrap();
    assert_eq!(item, first);

    let mut reader = TokioCursor(Cursor::new(first.clone()));
    let item = block_on(async_io::tokio::read_item_with_limit(
        &mut reader,
        first.len(),
    ))
    .unwrap();
    assert_eq!(item, first);

    let mut reader = TokioCursor(Cursor::new(cbor2::to_vec(&"owned").unwrap()));
    let value: String = block_on(async_io::tokio::read_value(&mut reader)).unwrap();
    assert_eq!(value, "owned");

    let mut reader = TokioCursor(Cursor::new(cbor2::to_vec(&"bounded").unwrap()));
    let value: String = block_on(async_io::tokio::read_value_with_limit(&mut reader, 16)).unwrap();
    assert_eq!(value, "bounded");

    let mut sink = TokioSink::default();
    block_on(async_io::tokio::write_value(&mut sink, &("ok", 9u8))).unwrap();
    assert!(sink.0.flushed);
    assert_eq!(
        cbor2::from_slice::<(String, u8)>(&sink.0.data).unwrap(),
        ("ok".into(), 9)
    );

    let mut sink = TokioSink::default();
    block_on(async_io::tokio::write_item(&mut sink, &first)).unwrap();
    assert_eq!(sink.0.data, first);
}
