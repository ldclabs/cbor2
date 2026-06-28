//! Concise Diagnostic Notation (CDN) input support.

use alloc::vec::Vec;

use serde::de::DeserializeOwned;

use crate::de::Error;

mod applications;
#[cfg(feature = "cdn")]
mod cri;
mod datetime;
mod encode;
mod float;
#[cfg(feature = "cdn")]
mod hash;
mod ip;
mod number;
mod parser;
#[cfg(test)]
mod tests;
mod types;

/// Encodes one Concise Diagnostic Notation (CDN) item as CBOR bytes.
///
/// This accepts the formalized diagnostic input syntax from
/// `draft-ietf-cbor-edn-literals`: JSON-compatible values, CBOR byte strings
/// (`'..'`, `h'..'`, `b64'..'`), comments, optional separator commas,
/// embedded CBOR sequence literals (`<<..>>`), tags, simple values, the core
/// encoding indicators (`_i`, `_0` through `_3`, and indefinite arrays or
/// maps with `[_` / `{_`), and the application extensions implemented by this
/// crate. Enabling the `cdn` feature also enables the `hash`, `cri`, and `CRI`
/// application extensions that require external crates. The default encoding
/// is preferred serialization.
///
/// ```rust
/// let bytes = cbor2::cdn_to_vec(r#"{ /kty/ 1: 4, "kid": h'deadbeef' }"#).unwrap();
/// assert_eq!(cbor2::to_cdn(&bytes[..]).unwrap(), r#"{1: 4, "kid": h'deadbeef'}"#);
/// ```
#[cfg(feature = "alloc")]
pub fn cdn_to_vec(input: &str) -> Result<Vec<u8>, Error> {
    parser::item_to_vec(input)
}

/// Encodes a CDN sequence as a CBOR sequence.
///
/// Top-level items may be separated by commas or by blank space/comments.
/// This is the same sequence grammar used inside CDN's `<<..>>` embedded
/// CBOR literals, but without wrapping the result as a byte string.
#[cfg(feature = "alloc")]
pub fn cdn_sequence_to_vec(input: &str) -> Result<Vec<u8>, Error> {
    parser::sequence_to_vec(input)
}

/// Deserializes one CDN item into a serde value.
///
/// Borrowed output fields are not supported by this helper because the CDN
/// text is first encoded into owned CBOR bytes. Use [`cdn_to_vec`] and
/// then [`from_slice`](crate::from_slice) directly when you need control over
/// the encoded bytes.
#[cfg(feature = "alloc")]
pub fn from_cdn<T>(input: &str) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    let bytes = cdn_to_vec(input)?;
    crate::from_slice(&bytes[..])
}
