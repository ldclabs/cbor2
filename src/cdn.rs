//! Concise Diagnostic Notation (CDN) input helpers and output re-exports.

use alloc::vec::Vec;

use serde::de::DeserializeOwned;

pub use crate::de::Error;
pub use crate::diag::{to_cdn, to_cdn_pretty, to_cdn_pretty_with_key_comments};

mod applications;
#[cfg(feature = "cdn-cri")]
mod cri;
mod datetime;
mod encode;
mod float;
#[cfg(feature = "cdn-hash")]
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
/// [CDN draft revision 29](https://datatracker.ietf.org/doc/html/draft-ietf-cbor-edn-literals-29),
/// including JSON-compatible values, CBOR byte strings
/// (`'..'`, `h'..'`, `b64'..'`), comments, optional separator commas,
/// embedded CBOR sequence literals (`<<..>>`), tags, simple values, the core
/// encoding indicators (`_i`, `_0` through `_3`, and indefinite arrays or
/// maps with `[_` / `{_`), and the application extensions implemented by this
/// crate. The built-in extensions include `dt`/`DT`, `ip`/`IP`, `b1`/`t1`,
/// `ilbs`/`ilts`, `bytes`, `same`, and `float`; enabling the `cdn` feature
/// also enables the `hash`, `cri`, and `CRI` extensions that require external
/// crates and `std`. Use `cdn-hash` alone for hash literals with `no_std` +
/// `alloc`, or `cdn-cri` for CRI alone. The default encoding is preferred serialization.
/// Errors while evaluating an application extension point to its argument's
/// opening delimiter in the source, since decoded or computed arguments may
/// no longer correspond byte for byte to the original text.
///
/// Reserved encoding indicators are ignored on items whose grammar allows
/// an indicator. Simple values have no encoding indicator. A leading plus
/// sign, extra leading decimal zeros, an ellipsis, or an unsupported prefix
/// produces an error. Use `#` for line comments and terminate them with a
/// newline. Raw string delimiters contain one to eight backticks.
///
/// The optional `cri` and `CRI` prefixes are defined in a
/// [separate CRI extension](https://www.ietf.org/archive/id/draft-bormann-core-cri-diagnostic-notation-00.html).
/// Uppercase `CRI` wraps its value in the registered tag 99.
///
/// ```rust
/// let bytes = cbor2::cdn_to_vec(r#"{ /kty/ 1: 4, "kid": h'deadbeef' }"#).unwrap();
/// assert_eq!(cbor2::to_cdn(&bytes[..]).unwrap(), r#"{1: 4, "kid": h'deadbeef'}"#);
/// ```
pub fn cdn_to_vec(input: &str) -> Result<Vec<u8>, Error> {
    parser::item_to_vec(input)
}

/// Encodes a CDN sequence as a CBOR sequence.
///
/// Top-level items may be separated by commas or by blank space/comments.
/// This is the same sequence grammar used inside CDN's `<<..>>` embedded
/// CBOR literals, but without wrapping the result as a byte string.
pub fn cdn_sequence_to_vec(input: &str) -> Result<Vec<u8>, Error> {
    parser::sequence_to_vec(input)
}

/// Encodes one CDN item, rejecting source text larger than `max_len` bytes.
///
/// The check precedes allocation and CR normalization. Use a source-size
/// budget for untrusted CDN: arbitrary-size decimal integer conversion still
/// requires more than linear work, even though binary/octal/hex input is linear.
pub fn cdn_to_vec_with_limit(input: &str, max_len: usize) -> Result<Vec<u8>, Error> {
    check_input_limit(input, max_len)?;
    cdn_to_vec(input)
}

/// Encodes a CDN sequence with the same source-byte budget as
/// [`cdn_to_vec_with_limit`].
pub fn cdn_sequence_to_vec_with_limit(input: &str, max_len: usize) -> Result<Vec<u8>, Error> {
    check_input_limit(input, max_len)?;
    cdn_sequence_to_vec(input)
}

fn check_input_limit(input: &str, max_len: usize) -> Result<(), Error> {
    if input.len() > max_len {
        return Err(Error::semantic(
            max_len,
            "CDN input exceeds source byte limit",
        ));
    }
    Ok(())
}

/// Deserializes one CDN item into a serde value.
///
/// Borrowed output fields are not supported by this helper because the CDN
/// text is first encoded into owned CBOR bytes. Use [`cdn_to_vec`] and
/// then [`from_slice`](crate::from_slice) directly when you need control over
/// the encoded bytes.
pub fn from_cdn<T>(input: &str) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    let bytes = cdn_to_vec(input)?;
    crate::from_slice(&bytes[..])
}
