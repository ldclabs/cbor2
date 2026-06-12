//! The reader and writer traits used by the encoder and decoder.
//!
//! With the `std` feature (enabled by default), [`Read`] and [`Write`] are
//! implemented for every `std::io::Read` and `std::io::Write`, and
//! [`Error`] *is* `std::io::Error` — pass `std::io` types everywhere and
//! this module disappears from view.
//!
//! Without `std`, the traits are implemented for byte slices (`&[u8]` reads
//! from the front, `&mut [u8]` writes to the front) and, with the `alloc`
//! feature, for `Vec<u8>`; [`Error`] is a minimal replacement carrying an
//! [`ErrorKind`]. Implement the traits directly to read from or write to
//! anything else, such as a peripheral or a network buffer.

#[cfg(feature = "std")]
pub use std::io::{Error, ErrorKind};

#[cfg(not(feature = "std"))]
pub use no_std::{Error, ErrorKind};

/// A source of bytes.
///
/// This is the abstraction [`Decoder`](crate::core::Decoder) and the
/// deserialization functions read from. See the [module docs](self) for the
/// provided implementations.
pub trait Read {
    /// Reads exactly `buf.len()` bytes into `buf`.
    ///
    /// Reaching the end of input before `buf` is full is an error of kind
    /// [`ErrorKind::UnexpectedEof`]; the contents of `buf` are then
    /// unspecified.
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Error>;
}

/// A sink for bytes.
///
/// This is the abstraction [`Encoder`](crate::core::Encoder) and the
/// serialization functions write to. See the [module docs](self) for the
/// provided implementations.
pub trait Write {
    /// Writes all of `data`.
    fn write_all(&mut self, data: &[u8]) -> Result<(), Error>;

    /// Flushes any buffered output.
    fn flush(&mut self) -> Result<(), Error>;
}

#[cfg(feature = "std")]
impl<T: std::io::Read> Read for T {
    #[inline]
    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Error> {
        std::io::Read::read_exact(self, buf)
    }
}

#[cfg(feature = "std")]
impl<T: std::io::Write> Write for T {
    #[inline]
    fn write_all(&mut self, data: &[u8]) -> Result<(), Error> {
        std::io::Write::write_all(self, data)
    }

    #[inline]
    fn flush(&mut self) -> Result<(), Error> {
        std::io::Write::flush(self)
    }
}

#[cfg(not(feature = "std"))]
mod no_std {
    use super::{Read, Write};

    /// The error type for [`Read`] and [`Write`] operations.
    ///
    /// This is the no-`std` stand-in for `std::io::Error`: an [`ErrorKind`]
    /// and nothing else. Construct one with `From<ErrorKind>`.
    #[derive(Debug)]
    pub struct Error(ErrorKind);

    /// The kind of an I/O [`Error`].
    ///
    /// A deliberately small subset of `std::io::ErrorKind`.
    #[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
    #[non_exhaustive]
    pub enum ErrorKind {
        /// The input ended in the middle of a read.
        UnexpectedEof,

        /// The output has no room for a write.
        WriteZero,

        /// Any other error.
        Other,
    }

    impl Error {
        /// Returns the kind of this error.
        #[inline]
        pub fn kind(&self) -> ErrorKind {
            self.0
        }
    }

    impl From<ErrorKind> for Error {
        #[inline]
        fn from(kind: ErrorKind) -> Self {
            Self(kind)
        }
    }

    impl core::fmt::Display for Error {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            match self.0 {
                ErrorKind::UnexpectedEof => write!(f, "unexpected end of input"),
                ErrorKind::WriteZero => write!(f, "no room left in the output"),
                ErrorKind::Other => write!(f, "i/o error"),
            }
        }
    }

    impl serde::ser::StdError for Error {}

    impl Read for &[u8] {
        fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Error> {
            if self.len() < buf.len() {
                return Err(ErrorKind::UnexpectedEof.into());
            }

            let (head, tail) = self.split_at(buf.len());
            buf.copy_from_slice(head);
            *self = tail;
            Ok(())
        }
    }

    impl<R: Read + ?Sized> Read for &mut R {
        #[inline]
        fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), Error> {
            (**self).read_exact(buf)
        }
    }

    impl Write for &mut [u8] {
        fn write_all(&mut self, data: &[u8]) -> Result<(), Error> {
            if self.len() < data.len() {
                return Err(ErrorKind::WriteZero.into());
            }

            let (head, tail) = core::mem::take(self).split_at_mut(data.len());
            head.copy_from_slice(data);
            *self = tail;
            Ok(())
        }

        #[inline]
        fn flush(&mut self) -> Result<(), Error> {
            Ok(())
        }
    }

    #[cfg(feature = "alloc")]
    impl Write for alloc::vec::Vec<u8> {
        #[inline]
        fn write_all(&mut self, data: &[u8]) -> Result<(), Error> {
            self.extend_from_slice(data);
            Ok(())
        }

        #[inline]
        fn flush(&mut self) -> Result<(), Error> {
            Ok(())
        }
    }

    impl<W: Write + ?Sized> Write for &mut W {
        #[inline]
        fn write_all(&mut self, data: &[u8]) -> Result<(), Error> {
            (**self).write_all(data)
        }

        #[inline]
        fn flush(&mut self) -> Result<(), Error> {
            (**self).flush()
        }
    }
}

#[cfg(all(test, not(feature = "std")))]
mod tests {
    use super::*;

    #[test]
    fn slice_read() {
        let mut input: &[u8] = &[1, 2, 3];

        let mut buf = [0u8; 2];
        (&mut input).read_exact(&mut buf).unwrap();
        assert_eq!(buf, [1, 2]);
        assert_eq!(input, [3]);

        let mut buf = [0u8; 2];
        assert_eq!(
            input.read_exact(&mut buf).unwrap_err().kind(),
            ErrorKind::UnexpectedEof
        );
    }

    #[test]
    fn slice_write() {
        let mut buffer = [0u8; 3];
        let mut output: &mut [u8] = &mut buffer;

        (&mut output).write_all(&[1, 2]).unwrap();
        output.flush().unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(
            output.write_all(&[3, 4]).unwrap_err().kind(),
            ErrorKind::WriteZero
        );

        assert_eq!(buffer, [1, 2, 0]);
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn vec_write() {
        let mut buffer = alloc::vec::Vec::new();
        buffer.write_all(&[1, 2]).unwrap();
        (&mut buffer).write_all(&[3]).unwrap();
        buffer.flush().unwrap();
        assert_eq!(buffer, [1, 2, 3]);
    }
}
