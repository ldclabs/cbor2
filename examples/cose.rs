use cbor2::Cbor;

// RFC 9052, Appendix C.4.1 — Simple Encrypted Message (52 bytes):
//
// https://datatracker.ietf.org/doc/html/rfc9052#appendix-C.4
//
// 16(
//   [
//     / protected h'a1010a' / << { / alg / 1: 10 / AES-CCM-16-64-128 / } >>,
//     / unprotected / { / iv / 5: h'89f52f65a1c580933b5261a78c' },
//     / ciphertext / h'5974e1b99a3a4cc09a659aa2e9e7fff161d38ce71cb45ce460ffb569'
//   ]
// )

/// Protected header parameters (RFC 9052 §3.1). They travel as a byte
/// string holding their own CBOR encoding.
#[derive(Debug, PartialEq, Cbor)]
struct Protected {
    /// 10 = AES-CCM-16-64-128 (RFC 9053 §4.2)
    #[cbor(key = 1)]
    alg: i8,
}

/// Unprotected header parameters.
#[derive(Debug, PartialEq, Cbor)]
struct Unprotected {
    #[cbor(key = 5)]
    #[serde(with = "serde_bytes")]
    iv: Vec<u8>,
}

/// COSE_Encrypt0 (RFC 9052 §5.2): tag 16 around
/// `[protected: bstr, unprotected: map, ciphertext: bstr]`.
#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 16)]
struct CoseEncrypt0(
    #[serde(with = "serde_bytes")] Vec<u8>, // protected, already encoded
    Unprotected,
    #[serde(with = "serde_bytes")] Vec<u8>, // ciphertext
);

// cargo run --example cose --features derive
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The protected header is the encoded map {1: 10}.
    let protected = cbor2::to_canonical_vec(&Protected { alg: 10 })?;
    assert_eq!(hex::encode(&protected), "a1010a");

    let msg = CoseEncrypt0(
        protected,
        Unprotected {
            iv: hex::decode("89f52f65a1c580933b5261a78c")?,
        },
        hex::decode("5974e1b99a3a4cc09a659aa2e9e7fff161d38ce71cb45ce460ffb569")?,
    );

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
    println!("{}", cbor2::cbor!(&msg)?);
    // 16([h'a1010a', {5: h'89f52f65a1c580933b5261a78c'},
    //     h'5974e1b99a3a4cc09a659aa2e9e7fff161d38ce71cb45ce460ffb569'])
    println!("{:?}", cbor2::cbor!(&msg)?);
    // 16([
    //   h'a1010a',
    //   {
    //     5: h'89f52f65a1c580933b5261a78c'
    //   },
    //   h'5974e1b99a3a4cc09a659aa2e9e7fff161d38ce71cb45ce460ffb569'
    // ])

    // Decoding requires tag 16 and restores every layer.
    let back: CoseEncrypt0 = cbor2::from_slice(&bytes)?;
    assert_eq!(back, msg);
    let header: Protected = cbor2::from_slice(&back.0)?;
    assert_eq!(header, Protected { alg: 10 });

    // JSON stays natural — original field names, no tags, no integer keys.
    let json = serde_json::to_string(&header)?;
    assert_eq!(json, r#"{"alg":10}"#);
    println!("{}", json);
    Ok(())
}
