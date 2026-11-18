//! Test doubles shared by the integration-test modules.

use cbor2::io;

use serde::{Deserialize, Serialize};

/// Every serde enum variant shape, with the wire names used by the vectors.
#[derive(Debug, PartialEq, Deserialize, Serialize)]
pub enum Enum {
    Unit,
    Newtype(u32),
    Tuple(u32, u32),
    Struct { x: u32 },
}

/// A writer whose every write and flush fails.
pub struct FailWriter;

impl io::Write for FailWriter {
    fn write_all(&mut self, _: &[u8]) -> Result<(), io::Error> {
        Err(io::ErrorKind::Other.into())
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        Err(io::ErrorKind::Other.into())
    }
}

/// A writer that accepts the given number of bytes and then fails, to
/// inject failures at precise positions inside an item.
pub struct LimitedWriter(pub usize);

impl io::Write for LimitedWriter {
    fn write_all(&mut self, data: &[u8]) -> Result<(), io::Error> {
        self.0 = self
            .0
            .checked_sub(data.len())
            .ok_or(io::ErrorKind::WriteZero)?;
        Ok(())
    }

    fn flush(&mut self) -> Result<(), io::Error> {
        Ok(())
    }
}

/// A reader that fails with an I/O error after its supplied prefix.
pub struct FailReader(pub &'static [u8]);

impl io::Read for FailReader {
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), io::Error> {
        self.0
            .read_exact(buf)
            .map_err(|_| io::ErrorKind::Other.into())
    }
}

/// A formatter sink whose every write fails.
pub struct FailFmt;

impl std::fmt::Write for FailFmt {
    fn write_str(&mut self, _: &str) -> std::fmt::Result {
        Err(std::fmt::Error)
    }
}

/// `[_ 1, 2, 3]`: serde only reports a sequence length when the iterator's
/// size hint is exact, so a filtered iterator produces an indefinite array.
pub struct UnsizedSeq;

impl Serialize for UnsizedSeq {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq((1u8..=3).filter(|_| true))
    }
}

/// Polls a future once; the in-memory test readers and writers never pend.
#[cfg(all(feature = "alloc", feature = "std"))]
pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match std::pin::pin!(future).as_mut().poll(&mut cx) {
        std::task::Poll::Ready(value) => value,
        std::task::Poll::Pending => panic!("in-memory future unexpectedly pending"),
    }
}
