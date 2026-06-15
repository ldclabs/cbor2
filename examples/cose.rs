//! COSE_Encrypt0 (RFC 9052 §5.2) with `#[derive(cbor2::Cbor)]`.
//!
//! This is the wire layer of the [`cose2`](https://github.com/ldclabs/cose2)
//! crate — a full RFC 9052/9053 COSE and RFC 8392 CWT implementation built on
//! cbor2. The `Encrypt0` struct below is its `Encrypt0Wire` type; COSE messages
//! travel both tagged and untagged, and `#[cbor(tag = 16)]` handles both with
//! one type (the tag is written on encode, transparent on decode).
//!
//! It reproduces the Simple Encrypted Message of RFC 9052, Appendix C.4.1
//! byte for byte (52 bytes):
//!
//! https://datatracker.ietf.org/doc/html/rfc9052#appendix-C.4
//!
//! 16(
//!   [
//!     / protected h'a1010a' / << { / alg / 1: 10 / AES-CCM-16-64-128 / } >>,
//!     / unprotected / { / iv / 5: h'89f52f65a1c580933b5261a78c' },
//!     / ciphertext / h'5974e1b99a3a4cc09a659aa2e9e7fff161d38ce71cb45ce460ffb569'
//!   ]
//! )

use cbor2::Cbor;

/// Protected header parameters (RFC 9052 §3.1). They travel as a byte string
/// holding their own CBOR encoding, so this map is encoded once and then
/// carried as a `bstr` inside the message.
#[derive(Clone, Debug, PartialEq, Cbor)]
struct Protected {
    /// 10 = AES-CCM-16-64-128 (RFC 9053 §4.2)
    #[cbor(key = 1)]
    alg: i8,
}

/// Unprotected header parameters: a plain integer-keyed CBOR map.
#[derive(Clone, Debug, PartialEq, Cbor)]
struct Unprotected {
    #[cbor(key = 5)]
    #[serde(with = "serde_bytes")]
    iv: Vec<u8>,
}

/// The on-the-wire COSE_Encrypt0 message: tag 16 around the array
/// `[protected: bstr, unprotected: map, ciphertext: bstr / nil]`.
///
/// `#[cbor(array)]` keeps the readable Rust field names but encodes the struct
/// as a field-order array, exactly as RFC 9052 requires. The tag is emitted on
/// encode yet transparent on decode, so tag-less transports work with the same
/// type. `ciphertext` is an `Option`: `None` becomes `nil` for a detached
/// ciphertext carried out of band.
#[derive(Clone, Debug, PartialEq, Cbor)]
#[cbor(tag = 16, array)]
struct Encrypt0 {
    #[serde(with = "serde_bytes")]
    protected: Vec<u8>,
    unprotected: Unprotected,
    #[serde(with = "serde_bytes")]
    ciphertext: Option<Vec<u8>>,
}

// cargo run --features derive --example cose
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The protected header is the encoded map {1: 10}; canonical so the bytes
    // are stable and reproducible (they double as the encryption AAD in COSE).
    let protected = cbor2::to_canonical_vec(&Protected { alg: 10 })?;
    assert_eq!(hex::encode(&protected), "a1010a");

    let msg = Encrypt0 {
        protected,
        unprotected: Unprotected {
            iv: hex::decode("89f52f65a1c580933b5261a78c")?,
        },
        ciphertext: Some(hex::decode(
            "5974e1b99a3a4cc09a659aa2e9e7fff161d38ce71cb45ce460ffb569",
        )?),
    };

    // The RFC's 52-byte message, byte for byte.
    let bytes = cbor2::to_canonical_vec(&msg)?;
    assert_eq!(bytes.len(), 52);
    assert_eq!(
        hex::encode(&bytes),
        "d08343a1010aa1054d89f52f65a1c580933b5261a78c581c\
         5974e1b99a3a4cc09a659aa2e9e7fff161d38ce71cb45ce460ffb569"
    );

    println!("{}", cbor2::diagnostic(&bytes[..])?);
    // 16([h'a1010a', {5: h'89f52f65a1c580933b5261a78c'},
    //     h'5974e1b99a3a4cc09a659aa2e9e7fff161d38ce71cb45ce460ffb569'])

    // Decoding restores every layer, including the nested protected header.
    let back: Encrypt0 = cbor2::from_slice(&bytes)?;
    assert_eq!(back, msg);
    let header: Protected = cbor2::from_slice(&back.protected)?;
    assert_eq!(header, Protected { alg: 10 });

    // The derive exposes the wire shape as constants — checkable at runtime
    // or, since they are `const`, at compile time.
    assert_eq!(Protected::KEYS, &[("alg", 1)]);
    assert_eq!(Encrypt0::TAG, Some(16));
    const { assert!(Encrypt0::ARRAY) }; // the message encodes as a CBOR array

    // The same `Encrypt0` decodes an untagged message (the tag-16 byte
    // dropped) — no separate "bare" type and `From` impl.
    let untagged = &bytes[1..]; // drop the 0xd0 tag-16 byte
    let from_untagged: Encrypt0 = cbor2::from_slice(untagged)?;
    assert_eq!(from_untagged, msg);

    // A detached ciphertext is `nil` on the wire; the bytes travel separately.
    let detached = Encrypt0 {
        ciphertext: None,
        ..msg.clone()
    };
    let detached_bytes = cbor2::to_canonical_vec(&detached)?;
    assert_eq!(*detached_bytes.last().unwrap(), 0xf6); // nil
    println!("{}", cbor2::diagnostic(&detached_bytes[..])?);
    // 16([h'a1010a', {5: h'89f52f65a1c580933b5261a78c'}, null])

    // JSON stays natural — original field names, no tags, no integer keys.
    let json = serde_json::to_string(&header)?;
    assert_eq!(json, r#"{"alg":10}"#);
    println!("{}", json);
    Ok(())
}
