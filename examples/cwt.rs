//! CBOR Web Token (CWT) claims (RFC 8392) with `#[derive(cbor2::Cbor)]`.
//!
//! This is the claims layer of the [`cose2`](https://github.com/ldclabs/cose2)
//! crate — a complete RFC 9052 COSE and RFC 8392 CWT library built on cbor2.
//!
//! CWT claim sets travel both tagged (CBOR tag 61) and untagged, so the type
//! must decode either form. `#[cbor(tag = 61)]` does exactly that with a
//! single struct: the tag is written on encode and transparent on decode — no
//! separate "bare" type and `From` impl.
//!
//! Where COSE_Encrypt0 (see `examples/cose.rs`) is a tagged *array*, a CWT
//! claims set is a tagged *map*: registered integer claim keys on the wire,
//! natural field names in JSON, and absent optional claims omitted entirely
//! via `#[serde(skip_serializing_if = ...)]`.
//!
//! It reproduces the example CWT Claims Set of RFC 8392, Appendix A.1:
//!
//! https://datatracker.ietf.org/doc/html/rfc8392#appendix-A.1

use cbor2::Cbor;

/// The common, typed subset of CWT claims (RFC 8392 §3).
///
/// `#[cbor(key = N)]` maps each field to its registered integer claim key and
/// `#[cbor(tag = 61)]` wraps the map in the CWT tag on encode while still
/// decoding untagged claim sets. The `#[serde(...)]` attributes keep natural
/// names (`iss`, `sub`, ...) and omit absent claims for JSON and every other
/// format, while CBOR uses the compact integer keys.
#[derive(Clone, Debug, Default, PartialEq, Cbor)]
#[cbor(tag = 61)]
struct Claims {
    /// Issuer (`iss`, claim 1).
    #[cbor(key = 1)]
    #[serde(rename = "iss", skip_serializing_if = "Option::is_none", default)]
    issuer: Option<String>,
    /// Subject (`sub`, claim 2).
    #[cbor(key = 2)]
    #[serde(rename = "sub", skip_serializing_if = "Option::is_none", default)]
    subject: Option<String>,
    /// Audience (`aud`, claim 3).
    #[cbor(key = 3)]
    #[serde(rename = "aud", skip_serializing_if = "Option::is_none", default)]
    audience: Option<String>,
    /// Expiration time, seconds since the UNIX epoch (`exp`, claim 4).
    #[cbor(key = 4)]
    #[serde(rename = "exp", skip_serializing_if = "Option::is_none", default)]
    expiration: Option<u64>,
    /// Not-before time, seconds since the UNIX epoch (`nbf`, claim 5).
    #[cbor(key = 5)]
    #[serde(rename = "nbf", skip_serializing_if = "Option::is_none", default)]
    not_before: Option<u64>,
    /// Issued-at time, seconds since the UNIX epoch (`iat`, claim 6).
    #[cbor(key = 6)]
    #[serde(rename = "iat", skip_serializing_if = "Option::is_none", default)]
    issued_at: Option<u64>,
    /// CWT ID (`cti`, claim 7).
    #[cbor(key = 7)]
    #[serde(
        rename = "cti",
        with = "serde_bytes",
        skip_serializing_if = "Option::is_none",
        default
    )]
    cwt_id: Option<Vec<u8>>,
}

// cargo run --features derive --example cwt
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The example CWT Claims Set of RFC 8392, Appendix A.1.
    let claims = Claims {
        issuer: Some("coap://as.example.com".into()),
        subject: Some("erikw".into()),
        audience: Some("coap://light.example.com".into()),
        expiration: Some(1444064944),
        not_before: Some(1443944944),
        issued_at: Some(1443944944),
        cwt_id: Some(vec![0x0b, 0x71]),
    };

    // Canonical CBOR: tag 61 around the integer-keyed claim map, byte for byte
    // as the RFC prints it.
    let bytes = cbor2::to_canonical_vec(&claims)?;
    assert_eq!(&bytes[..2], &[0xd8, 0x3d]); // tag 61 (CWT)
    assert_eq!(
        hex::encode(&bytes),
        "d83da70175636f61703a2f2f61732e6578616d706c652e636f6d02656572696b77\
         037818636f61703a2f2f6c696768742e6578616d706c652e636f6d041a5612aeb0\
         051a5610d9f0061a5610d9f007420b71"
    );

    println!("{}", cbor2::diagnostic(&bytes[..])?);
    // 61({1: "coap://as.example.com", 2: "erikw",
    //     3: "coap://light.example.com", 4: 1444064944, 5: 1443944944,
    //     6: 1443944944, 7: h'0b71'})

    // The same type decodes both the tagged claim set and an untagged one
    // (the tag-61 bytes dropped) — no second struct.
    let from_tagged: Claims = cbor2::from_slice(&bytes)?;
    let from_untagged: Claims = cbor2::from_slice(&bytes[2..])?;
    assert_eq!(from_tagged, claims);
    assert_eq!(from_untagged, claims);

    // The derive surfaces the declared tag and the claim-key table.
    assert_eq!(Claims::TAG, Some(61));
    assert_eq!(
        Claims::KEYS,
        &[
            ("iss", 1),
            ("sub", 2),
            ("aud", 3),
            ("exp", 4),
            ("nbf", 5),
            ("iat", 6),
            ("cti", 7),
        ]
    );

    // The same type serializes to natural JSON and round-trips back.
    let json = serde_json::to_string(&claims)?;
    let from_json: Claims = serde_json::from_str(&json)?;
    assert_eq!(from_json, claims);
    println!("{json}");

    // `skip_serializing_if` omits absent claims from *both* CBOR and JSON, so a
    // sparse token stays compact — only the keys you set appear on the wire.
    let minimal = Claims {
        issuer: Some("me".into()),
        expiration: Some(1444064944),
        ..Default::default()
    };
    let minimal_bytes = cbor2::to_canonical_vec(&minimal)?;
    println!("{}", cbor2::diagnostic(&minimal_bytes[..])?);
    // 61({1: "me", 4: 1444064944})
    assert_eq!(
        serde_json::to_string(&minimal)?,
        r#"{"iss":"me","exp":1444064944}"#
    );

    // CWT tokens are time-bound. cose2 ships a full `Validator` (issuer /
    // audience, nbf / iat, clock skew); the core check is the expiration:
    let now = 1444060000; // a fixed "now", just before `exp`
    assert!(claims.expiration.is_some_and(|exp| exp > now)); // still valid
    Ok(())
}
