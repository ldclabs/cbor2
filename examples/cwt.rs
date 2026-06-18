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

use std::collections::BTreeMap;

use cbor2::Cbor;

/// A COSE label can be either an integer label or a text label.
///
/// The real `cose2::Label` has this same serde shape: integers serialize as
/// integer map keys, not as JSON-like strings.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Label {
    Int(i64),
    Text(String),
}

impl serde::Serialize for Label {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Int(value) => serializer.serialize_i64(*value),
            Self::Text(value) => serializer.serialize_str(value),
        }
    }
}

struct LabelVisitor;

impl<'de> serde::de::Visitor<'de> for LabelVisitor {
    type Value = Label;

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("an integer or text COSE label")
    }

    fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Ok(Label::Int(value))
    }

    fn visit_i128<E: serde::de::Error>(self, value: i128) -> Result<Self::Value, E> {
        i64::try_from(value)
            .map(Label::Int)
            .map_err(|_| E::custom("COSE integer label is out of i64 range"))
    }

    fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
        i64::try_from(value)
            .map(Label::Int)
            .map_err(|_| E::custom("COSE integer label is out of i64 range"))
    }

    fn visit_u128<E: serde::de::Error>(self, value: u128) -> Result<Self::Value, E> {
        i64::try_from(value)
            .map(Label::Int)
            .map_err(|_| E::custom("COSE integer label is out of i64 range"))
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(Label::Text(value.into()))
    }

    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(Label::Text(value))
    }
}

impl<'de> serde::Deserialize<'de> for Label {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(LabelVisitor)
    }
}

/// Extension claims keyed by COSE labels.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct CoseMap(pub BTreeMap<Label, cbor2::Value>);

impl CoseMap {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The common, typed subset of CWT claims (RFC 8392 §3).
///
/// `#[cbor(key = N)]` maps each field to its registered integer claim key and
/// `#[cbor(tag = 61)]` wraps the map in the CWT tag on encode while still
/// decoding untagged claim sets. The `#[serde(...)]` attributes keep natural
/// names (`iss`, `sub`, ...) and omit absent claims for JSON and every other
/// format, while CBOR uses the compact integer keys.
#[derive(Clone, Debug, Default, PartialEq, Cbor)]
#[cbor(tag = 61)]
pub struct Claims {
    /// Issuer (`iss`, claim 1).
    #[cbor(key = 1)]
    #[serde(rename = "iss", skip_serializing_if = "Option::is_none", default)]
    pub issuer: Option<String>,
    /// Subject (`sub`, claim 2).
    #[cbor(key = 2)]
    #[serde(rename = "sub", skip_serializing_if = "Option::is_none", default)]
    pub subject: Option<String>,
    /// Audience (`aud`, claim 3).
    #[cbor(key = 3)]
    #[serde(rename = "aud", skip_serializing_if = "Option::is_none", default)]
    pub audience: Option<String>,
    /// Expiration time, seconds since the UNIX epoch (`exp`, claim 4).
    #[cbor(key = 4)]
    #[serde(rename = "exp", skip_serializing_if = "Option::is_none", default)]
    pub expiration: Option<u64>,
    /// Not-before time, seconds since the UNIX epoch (`nbf`, claim 5).
    #[cbor(key = 5)]
    #[serde(rename = "nbf", skip_serializing_if = "Option::is_none", default)]
    pub not_before: Option<u64>,
    /// Issued-at time, seconds since the UNIX epoch (`iat`, claim 6).
    #[cbor(key = 6)]
    #[serde(rename = "iat", skip_serializing_if = "Option::is_none", default)]
    pub issued_at: Option<u64>,
    /// CWT ID (`cti`, claim 7).
    #[cbor(key = 7)]
    #[serde(
        rename = "cti",
        with = "serde_bytes",
        skip_serializing_if = "Option::is_none",
        default
    )]
    pub cwt_id: Option<Vec<u8>>,
    /// Additional CWT claims outside the typed subset above.
    ///
    /// Use this for application/private claims and registered claims that do
    /// not yet have typed fields here.
    #[serde(flatten)]
    #[serde(skip_serializing_if = "CoseMap::is_empty", default)]
    pub extra: CoseMap,
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
        extra: CoseMap::default(),
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

    println!(
        "{}",
        cbor2::diagnostic_pretty_with_key_comments(&bytes[..], Claims::KEYS)?
    );
    // 61({
    //   1: "coap://as.example.com", // "iss"
    //   2: "erikw", // "sub"
    //   3: "coap://light.example.com", // "aud"
    //   4: 1444064944, // "exp"
    //   5: 1443944944, // "nbf"
    //   6: 1443944944, // "iat"
    //   7: h'0b71' // "cti"
    // })

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
    println!(
        "{}",
        cbor2::diagnostic_pretty_with_key_comments(&minimal_bytes[..], Claims::KEYS)?
    );
    // 61({
    //   1: "me", // "iss"
    //   4: 1444064944 // "exp"
    // })
    assert_eq!(
        serde_json::to_string(&minimal)?,
        r#"{"iss":"me","exp":1444064944}"#
    );

    // Application/private claims can ride in the flattened extra map. The
    // registered fields above keep their integer CWT labels, while the business
    // field stays a normal text-keyed claim.
    let mut extended = minimal.clone();
    extended
        .extra
        .0
        .insert(Label::Text("tenant".into()), cbor2::Value::from("acme"));
    let extended_bytes = cbor2::to_canonical_vec(&extended)?;
    assert_eq!(cbor2::from_slice::<Claims>(&extended_bytes)?, extended);
    assert_eq!(
        serde_json::to_string(&extended)?,
        r#"{"iss":"me","exp":1444064944,"tenant":"acme"}"#
    );

    // CWT tokens are time-bound. cose2 ships a full `Validator` (issuer /
    // audience, nbf / iat, clock skew); the core check is the expiration:
    let now = 1444060000; // a fixed "now", just before `exp`
    assert!(claims.expiration.is_some_and(|exp| exp > now)); // still valid
    Ok(())
}
