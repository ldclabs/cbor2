//! End-to-end tests for `#[derive(cbor2::Cbor)]` (`derive` feature).

use std::collections::BTreeMap;

use cbor2::{Cbor, Value};

// A COSE_Key-shaped structure (RFC 9052 §7): all map keys are integers.
#[derive(Debug, PartialEq, Cbor)]
struct CoseKey {
    #[cbor(key = 1)]
    kty: u8,
    #[cbor(key = 3)]
    alg: i8,
    #[cbor(key = -1)]
    crv: u8,
    #[cbor(key = -2)]
    x: serde_bytes::ByteBuf,
    // Fields without a key keep their textual name.
    note: Option<String>,
}

fn sample() -> CoseKey {
    CoseKey {
        kty: 2,  // EC2
        alg: -7, // ES256
        crv: 1,  // P-256
        x: serde_bytes::ByteBuf::from(vec![0x11, 0x22, 0x33, 0x44]),
        note: None,
    }
}

#[test]
fn cose_key_round_trip() {
    // {1: 2, 3: -7, -1: 1, -2: h'11223344', "note": null}
    let bytes = cbor2::to_vec(&sample()).unwrap();
    assert_eq!(
        hex::encode(&bytes),
        "a5010203262001214411223344646e6f7465f6"
    );
    assert_eq!(cbor2::from_slice::<CoseKey>(&bytes).unwrap(), sample());

    // Through Value, and decoding accepts text keys (the field names)
    // alongside the integers.
    let value = Value::serialized(&sample()).unwrap();
    assert_eq!(value.deserialized::<CoseKey>().unwrap(), sample());

    let textual = cbor2::cbor!({
        1 => 2,
        "alg" => -7,
        -1 => 1,
        -2 => cbor2::Value::Bytes(vec![0x11, 0x22, 0x33, 0x44]),
        "note" => null,
    })
    .unwrap();
    assert_eq!(textual.deserialized::<CoseKey>().unwrap(), sample());
}

#[test]
fn json_just_works() {
    // The derive leaves field names untouched, so plain serde_json uses
    // them — no integer keys, no tag, no wrappers.
    let json = serde_json::to_string(&sample()).unwrap();
    assert_eq!(
        json,
        r#"{"kty":2,"alg":-7,"crv":1,"x":[17,34,51,68],"note":null}"#
    );
    assert_eq!(serde_json::from_str::<CoseKey>(&json).unwrap(), sample());
}

#[derive(Debug, PartialEq, Cbor)]
struct LifetimeNames<'a, '__de> {
    #[cbor(key = 1)]
    value: u8,

    #[serde(skip)]
    marker: core::marker::PhantomData<(&'a (), &'__de ())>,
}

#[test]
fn derive_avoids_internal_lifetime_name_collisions() {
    let value = LifetimeNames {
        value: 7,
        marker: core::marker::PhantomData,
    };

    let bytes = cbor2::to_vec(&value).unwrap();
    assert_eq!(hex::encode(&bytes), "a10107");
}

#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 123)]
struct ProtectedHeader {
    #[cbor(key = 1)]
    alg: i8,

    #[cbor(key = 4)]
    #[serde(with = "serde_bytes")]
    kid: Vec<u8>,
}

fn header() -> ProtectedHeader {
    ProtectedHeader {
        alg: -7,
        kid: b"kid".to_vec(),
    }
}

#[test]
fn tagged_structs_wrap_and_decode_transparently() {
    // 123({1: -7, 4: h'6b6964'})
    let bytes = cbor2::to_vec(&header()).unwrap();
    assert_eq!(hex::encode(&bytes), "d87ba2012604436b6964");
    assert_eq!(
        cbor2::diagnostic(&bytes[..]).unwrap(),
        "123({1: -7, 4: h'6b6964'})"
    );
    assert_eq!(
        cbor2::from_slice::<ProtectedHeader>(&bytes).unwrap(),
        header()
    );

    // Canonical encoding keeps the tag.
    assert_eq!(cbor2::to_canonical_vec(&header()).unwrap(), bytes);

    // The declared tag is transparent on decode, so tag-less transports work.
    let untagged = hex::decode("a2012604436b6964").unwrap();
    assert_eq!(
        cbor2::from_slice::<ProtectedHeader>(&untagged).unwrap(),
        header()
    );

    // Other wrapper tags are also transparent, as in the rest of the
    // deserializer.
    let wrong = hex::decode("d87ca2012604436b6964").unwrap(); // 124(...)
    assert_eq!(
        cbor2::from_slice::<ProtectedHeader>(&wrong).unwrap(),
        header()
    );

    // Extra foreign tags around it stay transparent too.
    let wrapped = hex::decode("d9d9f7d87ba2012604436b6964").unwrap(); // 55799(123(...))
    assert_eq!(
        cbor2::from_slice::<ProtectedHeader>(&wrapped).unwrap(),
        header()
    );

    // The same rules apply through Value.
    let value = Value::serialized(&header()).unwrap();
    assert_eq!(
        value,
        Value::Tag(
            123,
            Box::new(cbor2::cbor!({ 1 => -7, 4 => Value::Bytes(b"kid".to_vec()) }).unwrap())
        )
    );
    assert_eq!(value.deserialized::<ProtectedHeader>().unwrap(), header());
    let untagged = cbor2::cbor!({ 1 => -7, 4 => Value::Bytes(b"kid".to_vec()) }).unwrap();
    assert_eq!(
        untagged.deserialized::<ProtectedHeader>().unwrap(),
        header()
    );

    // JSON carries neither the tag nor the integer keys.
    let json = serde_json::to_string(&header()).unwrap();
    assert_eq!(json, r#"{"alg":-7,"kid":[107,105,100]}"#);
    assert_eq!(
        serde_json::from_str::<ProtectedHeader>(&json).unwrap(),
        header()
    );
}

#[test]
fn tagged_tuple_structs_work_too() {
    // COSE_Sign1-shaped: a tagged array (RFC 9052 §4.2).
    #[derive(Debug, PartialEq, Cbor)]
    #[cbor(tag = 18)]
    struct Sign1(
        #[serde(with = "serde_bytes")] Vec<u8>,
        u8,
        #[serde(with = "serde_bytes")] Vec<u8>,
        #[serde(with = "serde_bytes")] Vec<u8>,
    );

    let msg = Sign1(vec![0xa0], 0, vec![], vec![0xff]);
    let bytes = cbor2::to_vec(&msg).unwrap();
    // 18([h'a0', 0, h'', h'ff'])
    assert_eq!(hex::encode(&bytes), "d28441a0004041ff");
    assert_eq!(cbor2::from_slice::<Sign1>(&bytes).unwrap(), msg);

    assert_eq!(
        cbor2::from_slice::<Sign1>(&hex::decode("8441a0004041ff").unwrap()).unwrap(),
        msg
    );
}

#[test]
fn named_structs_can_use_cose_array_shape() {
    #[derive(Debug, PartialEq, Cbor)]
    #[cbor(tag = 18, array)]
    struct Sign1 {
        #[serde(with = "serde_bytes")]
        protected: Vec<u8>,
        unprotected: u8,
        #[serde(with = "serde_bytes")]
        payload: Vec<u8>,
        #[serde(with = "serde_bytes")]
        signature: Vec<u8>,
    }

    let msg = Sign1 {
        protected: vec![0xa0],
        unprotected: 0,
        payload: vec![],
        signature: vec![0xff],
    };

    assert_eq!(Sign1::KEYS, &[]);
    assert_eq!(Sign1::TAG, Some(18));
    const {
        assert!(Sign1::ARRAY);
    }

    let bytes = cbor2::to_vec(&msg).unwrap();
    assert_eq!(hex::encode(&bytes), "d28441a0004041ff");
    assert_eq!(cbor2::from_slice::<Sign1>(&bytes).unwrap(), msg);

    let json = serde_json::to_string(&msg).unwrap();
    assert_eq!(
        json,
        r#"{"protected":[160],"unprotected":0,"payload":[],"signature":[255]}"#
    );
    assert_eq!(serde_json::from_str::<Sign1>(&json).unwrap(), msg);
}

#[test]
fn enums_and_generics_work_too() {
    #[derive(Debug, PartialEq, Cbor)]
    enum Message {
        Signed {
            #[cbor(key = 1)]
            payload: u8,
            label: bool,
        },
        Unit,
    }

    let bytes = cbor2::to_vec(&Message::Signed {
        payload: 7,
        label: true,
    })
    .unwrap();
    // {"Signed": {1: 7, "label": true}}
    assert_eq!(hex::encode(&bytes), "a1665369676e6564a20107656c6162656cf5");
    assert_eq!(
        cbor2::from_slice::<Message>(&bytes).unwrap(),
        Message::Signed {
            payload: 7,
            label: true
        }
    );
    assert_eq!(cbor2::to_vec(&Message::Unit).unwrap(), b"\x64Unit");

    let json = serde_json::to_string(&Message::Signed {
        payload: 7,
        label: true,
    })
    .unwrap();
    assert_eq!(json, r#"{"Signed":{"payload":7,"label":true}}"#);

    #[derive(Debug, PartialEq, Cbor)]
    #[cbor(tag = 7)]
    struct Wrap<T> {
        #[cbor(key = 1)]
        inner: T,
    }

    let wrapped = Wrap { inner: 5u8 };
    let bytes = cbor2::to_vec(&wrapped).unwrap();
    assert_eq!(hex::encode(&bytes), "c7a10105"); // 7({1: 5})
    assert_eq!(cbor2::from_slice::<Wrap<u8>>(&bytes).unwrap(), wrapped);
}

#[test]
fn serde_attributes_combine() {
    // An explicit field rename carries over to the key table and to JSON.
    #[derive(Debug, PartialEq, Cbor)]
    struct Renamed {
        #[cbor(key = 3)]
        #[serde(rename = "alg", alias = "algorithm")]
        algorithm: i8,
        #[serde(default)]
        note: String,
    }

    let value = Renamed {
        algorithm: -7,
        note: String::new(),
    };
    let bytes = cbor2::to_vec(&value).unwrap();
    // {3: -7, "note": ""}
    assert_eq!(hex::encode(&bytes), "a20326646e6f746560");
    assert_eq!(cbor2::from_slice::<Renamed>(&bytes).unwrap(), value);

    assert_eq!(
        serde_json::to_string(&value).unwrap(),
        r#"{"alg":-7,"note":""}"#
    );
    let parsed: Renamed = serde_json::from_str(r#"{"algorithm":-7}"#).unwrap();
    assert_eq!(parsed, value);
}

#[test]
fn container_serde_bounds_are_honored() {
    struct NotSerde;

    // The classic marker-field idiom: `bound = ""` lifts the inferred
    // `T: Serialize` / `T: Deserialize` bounds off the generated impls.
    #[derive(Cbor)]
    #[serde(bound = "")]
    struct Wrap<T> {
        #[cbor(key = 1)]
        value: u8,
        #[serde(skip)]
        _marker: std::marker::PhantomData<T>,
    }

    let wrap = Wrap::<NotSerde> {
        value: 7,
        _marker: std::marker::PhantomData,
    };
    let bytes = cbor2::to_vec(&wrap).unwrap();
    // {1: 7}
    assert_eq!(hex::encode(&bytes), "a10107");
    let back: Wrap<NotSerde> = cbor2::from_slice(&bytes).unwrap();
    assert_eq!(back.value, 7);

    // Split bounds combine with a tag; `'de` names the impl's
    // deserializer lifetime like in serde's own derive.
    #[derive(Debug, PartialEq, Cbor)]
    #[cbor(tag = 90)]
    #[serde(bound(
        serialize = "T: serde::Serialize",
        deserialize = "T: serde::Deserialize<'de> + Default"
    ))]
    struct Tagged<T> {
        #[cbor(key = 1)]
        inner: T,
    }

    let tagged = Tagged { inner: 5u8 };
    let bytes = cbor2::to_vec(&tagged).unwrap();
    // 90({1: 5})
    assert_eq!(hex::encode(&bytes), "d85aa10105");
    assert_eq!(cbor2::from_slice::<Tagged<u8>>(&bytes).unwrap(), tagged);
}

#[test]
fn shared_enum_keys_keep_variant_field_order_and_plain_names() {
    #[derive(Debug, PartialEq, Cbor)]
    enum Message {
        First {
            #[cbor(key = 1)]
            a: u8,
            #[cbor(key = 2)]
            b: u8,
        },
        Second {
            #[cbor(key = 2)]
            b: u8,
            note: u8,
            #[cbor(key = 1)]
            a: u8,
            #[cbor(key = 3)]
            #[serde(default, skip_serializing_if = "Option::is_none")]
            extra: Option<u8>,
        },
    }
    let message = Message::Second {
        b: 2,
        note: 3,
        a: 1,
        extra: None,
    };
    let bytes = cbor2::to_vec(&message).unwrap();
    assert_eq!(
        cbor2::diagnostic(&bytes[..]).unwrap(),
        r#"{"Second": {2: 2, "note": 3, 1: 1}}"#
    );
    assert_eq!(cbor2::from_slice::<Message>(&bytes).unwrap(), message);
    assert_eq!(
        Value::serialized(&message)
            .unwrap()
            .deserialized::<Message>()
            .unwrap(),
        message
    );

    // An untagged variant remains supported when no CBOR key table is used.
    #[derive(Debug, PartialEq, Cbor)]
    enum Plain {
        Unit,
        #[serde(untagged)]
        Number(u8),
    }
    assert_eq!(cbor2::from_slice::<Plain>(&[7]).unwrap(), Plain::Number(7));
}

#[test]
fn plain_struct_tags_use_the_original_type_name() {
    #[derive(Debug, PartialEq, Cbor)]
    #[serde(tag = "kind")]
    struct Message {
        value: u8,
    }
    let message = Message { value: 7 };
    assert_eq!(
        serde_json::to_string(&message).unwrap(),
        r#"{"kind":"Message","value":7}"#
    );
    let bytes = cbor2::to_vec(&message).unwrap();
    assert_eq!(cbor2::from_slice::<Message>(&bytes).unwrap(), message);
}

#[test]
fn flattening_inner_structs_uses_serde_names() {
    #[derive(Debug, PartialEq, Cbor)]
    struct Inner {
        #[cbor(key = 1)]
        value: u8,
    }
    #[derive(Debug, PartialEq, Cbor)]
    struct Outer {
        #[serde(flatten)]
        inner: Inner,
    }
    let value = Outer {
        inner: Inner { value: 7 },
    };
    let bytes = cbor2::to_vec(&value).unwrap();
    assert_eq!(cbor2::diagnostic(&bytes[..]).unwrap(), r#"{"value": 7}"#);
    assert_eq!(cbor2::from_slice::<Outer>(&bytes).unwrap(), value);
    assert_eq!(
        Value::serialized(&value)
            .unwrap()
            .deserialized::<Outer>()
            .unwrap(),
        value
    );
    assert!(cbor2::from_slice::<Outer>(&[0xa1, 1, 7])
        .unwrap_err()
        .to_string()
        .contains("missing field"));
}

#[test]
fn full_key_range() {
    #[derive(Debug, PartialEq, Cbor)]
    #[cbor(tag = 18446744073709551615)]
    struct Edges {
        #[cbor(key = 0)]
        zero: u8,
        #[cbor(key = 18446744073709551615)]
        hi: u8,
        #[cbor(key = -18446744073709551616)]
        lo: u8,
    }

    let edges = Edges {
        zero: 0,
        hi: 1,
        lo: 2,
    };
    let bytes = cbor2::to_vec(&edges).unwrap();
    // 18446744073709551615({0: 0, 18446744073709551615: 1, -18446744073709551616: 2})
    assert_eq!(
        hex::encode(&bytes),
        "dbffffffffffffffffa300001bffffffffffffffff013bffffffffffffffff02"
    );
    assert_eq!(cbor2::from_slice::<Edges>(&bytes).unwrap(), edges);
}

// The derive also implements the `cbor2::Cbor` trait, exposing the
// declared protocol details for runtime inspection.
#[test]
fn derive_exposes_keys_and_tag() {
    assert_eq!(
        CoseKey::KEYS,
        &[("kty", 1), ("alg", 3), ("crv", -1), ("x", -2)]
    );
    assert_eq!(CoseKey::TAG, None);

    assert_eq!(ProtectedHeader::KEYS, &[("alg", 1), ("kid", 4)]);
    assert_eq!(ProtectedHeader::TAG, Some(123));

    // The convenience method collects the table into a map.
    let keys = sample().keys();
    assert_eq!(keys.len(), 4);
    assert_eq!(keys["kty"], 1);
    assert_eq!(keys["x"], -2);
    assert!(!keys.contains_key("note")); // unkeyed fields are not listed

    // A plain derive without #[cbor] attributes declares nothing.
    #[derive(Cbor)]
    struct Plain {
        #[allow(dead_code)]
        a: u8,
    }

    assert_eq!(Plain::KEYS, &[]);
    assert_eq!(Plain::TAG, None);
    assert!(Plain { a: 0 }.keys().is_empty());
}

// A CWT-claims-shaped struct (RFC 8392): tag 61 on encode, but decode accepts
// either the tagged or the untagged form — no separate "bare" struct and
// `From` impl.
#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 61)]
struct Claims {
    #[cbor(key = 1)]
    #[serde(rename = "iss")]
    issuer: String,
    #[cbor(key = 4)]
    #[serde(rename = "exp")]
    expiration: u64,
}

#[test]
fn tagged_claims_decode_tagged_or_untagged() {
    let claims = Claims {
        issuer: "me".into(),
        expiration: 9,
    };

    // Encode writes the tag, canonically.
    let bytes = cbor2::to_canonical_vec(&claims).unwrap();
    assert_eq!(hex::encode(&bytes), "d83da201626d650409"); // 61({1: "me", 4: 9})
    assert_eq!(&bytes[..2], [0xd8, 0x3d]);

    // Decode accepts the tagged form and the untagged form alike.
    assert_eq!(cbor2::from_slice::<Claims>(&bytes).unwrap(), claims);
    assert_eq!(cbor2::from_slice::<Claims>(&bytes[2..]).unwrap(), claims);

    // The trait surfaces the declared tag.
    assert_eq!(Claims::TAG, Some(61));

    // The Value paths agree on both forms.
    let value = Value::serialized(&claims).unwrap();
    assert_eq!(value.deserialized::<Claims>().unwrap(), claims);
    assert_eq!(
        cbor2::cbor!({ 1 => "me", 4 => 9 })
            .unwrap()
            .deserialized::<Claims>()
            .unwrap(),
        claims
    );
}

#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 61)]
struct OpenClaims {
    #[cbor(key = 1)]
    #[serde(rename = "iss")]
    issuer: String,
    #[cbor(key = 4)]
    #[serde(rename = "exp")]
    expiration: u64,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    extra: BTreeMap<String, Value>,
}

#[test]
fn tagged_claims_support_flattened_extra_fields() {
    let mut extra = BTreeMap::new();
    extra.insert("scope".into(), Value::Array(vec![Value::from("read")]));
    extra.insert("tenant".into(), Value::from("acme"));
    let claims = OpenClaims {
        issuer: "me".into(),
        expiration: 9,
        extra,
    };

    let value = Value::serialized(&claims).unwrap();
    assert_eq!(
        value,
        Value::Tag(
            61,
            Box::new(
                cbor2::cbor!({
                    1 => "me",
                    4 => 9,
                    "scope" => ["read"],
                    "tenant" => "acme",
                })
                .unwrap()
            )
        )
    );

    let bytes = cbor2::to_canonical_vec(&claims).unwrap();
    assert_eq!(cbor2::from_slice::<OpenClaims>(&bytes).unwrap(), claims);
    assert_eq!(
        cbor2::from_slice::<OpenClaims>(&bytes[2..]).unwrap(),
        claims
    );
    assert_eq!(value.deserialized::<OpenClaims>().unwrap(), claims);
    assert_eq!(
        cbor2::cbor!({
            1 => "me",
            4 => 9,
            "scope" => ["read"],
            "tenant" => "acme",
        })
        .unwrap()
        .deserialized::<OpenClaims>()
        .unwrap(),
        claims
    );

    assert_eq!(OpenClaims::KEYS, &[("iss", 1), ("exp", 4)]);
    assert_eq!(OpenClaims::TAG, Some(61));

    let json = serde_json::to_string(&claims).unwrap();
    assert_eq!(
        json,
        r#"{"iss":"me","exp":9,"scope":["read"],"tenant":"acme"}"#
    );
    assert_eq!(serde_json::from_str::<OpenClaims>(&json).unwrap(), claims);
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Label {
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

    fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
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

#[derive(Clone, Debug, Default, PartialEq)]
struct CoseMap(BTreeMap<Label, Value>);

impl CoseMap {
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl serde::Serialize for CoseMap {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap as _;

        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

struct CoseMapVisitor;

impl<'de> serde::de::Visitor<'de> for CoseMapVisitor {
    type Value = CoseMap;

    fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("a COSE label map")
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(self, mut acc: A) -> Result<Self::Value, A::Error> {
        let mut map = BTreeMap::new();
        while let Some((key, value)) = acc.next_entry::<Label, Value>()? {
            map.insert(key, value);
        }
        Ok(CoseMap(map))
    }
}

impl<'de> serde::Deserialize<'de> for CoseMap {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(CoseMapVisitor)
    }
}

#[derive(Debug, PartialEq, Cbor)]
#[cbor(tag = 61)]
struct LabelClaims {
    #[cbor(key = 1)]
    #[serde(rename = "iss")]
    issuer: String,
    #[cbor(key = 4)]
    #[serde(rename = "exp")]
    expiration: u64,
    #[serde(flatten, default, skip_serializing_if = "CoseMap::is_empty")]
    extra: CoseMap,
}

#[test]
fn tagged_claims_support_flattened_cose_label_map() {
    let mut extra = CoseMap::default();
    extra.0.insert(Label::Int(-65537), Value::from("private"));
    extra.0.insert(Label::Int(42), Value::from("answer"));
    extra
        .0
        .insert(Label::Text("tenant".into()), Value::from("acme"));
    let claims = LabelClaims {
        issuer: "me".into(),
        expiration: 9,
        extra,
    };

    let untagged = cbor2::cbor!({
        1 => "me",
        4 => 9,
        -65537 => "private",
        42 => "answer",
        "tenant" => "acme",
    })
    .unwrap();
    let value = Value::serialized(&claims).unwrap();
    assert_eq!(value, Value::Tag(61, Box::new(untagged.clone())));

    let bytes = cbor2::to_canonical_vec(&claims).unwrap();
    assert_eq!(cbor2::from_slice::<LabelClaims>(&bytes).unwrap(), claims);
    assert_eq!(
        cbor2::from_slice::<LabelClaims>(&bytes[2..]).unwrap(),
        claims
    );
    assert_eq!(value.deserialized::<LabelClaims>().unwrap(), claims);
    assert_eq!(untagged.deserialized::<LabelClaims>().unwrap(), claims);
}

#[test]
fn tagged_newtypes_keep_the_contents_own_tag() {
    use cbor2::tag::RequireExact;
    #[derive(Debug, PartialEq, Cbor)]
    #[cbor(tag = 42)]
    struct Big(u128);
    #[derive(Debug, PartialEq, Cbor)]
    #[cbor(tag = 42)]
    struct Date(RequireExact<u64, 1>);
    #[derive(Debug, PartialEq, Cbor)]
    #[cbor(tag = 42)]
    struct Dynamic(Value);

    fn check<T: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug + PartialEq>(
        item: T,
    ) {
        let bytes = cbor2::to_vec(&item).unwrap();
        assert_eq!(&bytes[..2], &[0xd8, 42]);
        for bytes in [&bytes[..], &bytes[2..]] {
            assert_eq!(cbor2::from_slice::<T>(bytes).unwrap(), item);
            assert_eq!(cbor2::from_reader::<T, _>(bytes).unwrap(), item);
            let value: Value = cbor2::from_slice(bytes).unwrap();
            assert_eq!(value.deserialized::<T>().unwrap(), item);
        }
    }
    check(Big(1 << 64));
    check(Date(RequireExact(7)));
    check(Dynamic(Value::Tag(1, Box::new(7.into()))));
}

#[test]
fn flattened_extensions_keep_cbor_values_and_declaration_order() {
    use std::collections::BTreeMap;
    #[derive(Debug, PartialEq, Cbor)]
    struct Fields {
        #[cbor(key = 1)]
        #[serde(alias = "old_id", skip_serializing_if = "Option::is_none")]
        id: Option<u64>,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
        tail: bool,
    }
    let mut fields = Fields {
        id: Some(7),
        tail: true,
        extra: [
            ("s".into(), cbor2::Simple::new(59).unwrap().into()),
            ("when".into(), Value::Tag(1, Box::new(7.into()))),
        ]
        .into(),
    };
    let bytes = cbor2::to_vec(&fields).unwrap();
    assert_eq!(
        cbor2::to_cdn(&bytes[..]).unwrap(),
        r#"{1: 7, "s": simple(59), "when": 1(7), "tail": true}"#
    );
    assert_eq!(cbor2::from_slice::<Fields>(&bytes).unwrap(), fields);
    assert_eq!(cbor2::from_reader::<Fields, _>(&bytes[..]).unwrap(), fields);
    assert_eq!(
        Value::serialized(&fields)
            .unwrap()
            .deserialized::<Fields>()
            .unwrap(),
        fields
    );
    for name in ["id", "old_id", "tail"] {
        fields.extra.insert(name.into(), 8.into());
        assert!(cbor2::to_vec(&fields)
            .unwrap_err()
            .to_string()
            .contains("conflicts"));
        fields.id = None;
        assert!(Value::serialized(&fields).is_err());
        fields.extra.remove(name);
    }
}

#[test]
fn multiple_flattened_fields_claim_struct_keys_and_borrow_extension_values() {
    #[derive(Debug, PartialEq, Cbor)]
    struct Inner {
        when: cbor2::tag::RequireExact<u64, 1>,
    }
    #[derive(Debug, PartialEq, Cbor)]
    struct Outer<'a> {
        #[serde(flatten)]
        inner: Inner,
        id: bool,
        #[serde(flatten, borrow)]
        extra: std::collections::BTreeMap<&'a str, &'a str>,
    }
    let item = Outer {
        inner: Inner {
            when: cbor2::tag::RequireExact(8),
        },
        id: true,
        extra: [("text", "borrowed")].into(),
    };
    let bytes = cbor2::to_vec(&item).unwrap();
    assert_eq!(
        cbor2::to_cdn(&bytes[..]).unwrap(),
        r#"{"when": 1(8), "id": true, "text": "borrowed"}"#
    );
    let item2: Outer<'_> = cbor2::from_slice(&bytes).unwrap();
    assert_eq!(item, item2);
    assert!(bytes.as_ptr_range().contains(&item2.extra["text"].as_ptr()));
}

#[test]
fn flattened_integer_collisions_fail_and_custom_struct_fields_keep_their_options() {
    let extra = CoseMap([(Label::Int(1), Value::from("duplicate"))].into());
    let claims = LabelClaims {
        issuer: "me".into(),
        expiration: 9,
        extra,
    };
    assert!(cbor2::to_vec(&claims)
        .unwrap_err()
        .to_string()
        .contains("conflicts"));

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Details {
        name: String,
    }
    #[derive(Debug, PartialEq, Cbor)]
    #[serde(deny_unknown_fields)]
    struct Message {
        id: u8,
        #[serde(flatten)]
        details: Option<Details>,
    }
    for details in [None, Some(Details { name: "abc".into() })] {
        let value = Message { id: 1, details };
        assert_eq!(
            cbor2::from_slice::<Message>(&cbor2::to_vec(&value).unwrap()).unwrap(),
            value
        );
    }
    let unknown = cbor2::cdn_to_vec(r#"{"id":1,"unrecognized":true}"#).unwrap();
    assert!(cbor2::from_slice::<Message>(&unknown).is_err());
}

#[test]
fn flattened_struct_honors_serde_container_conversions() {
    use std::collections::BTreeMap;
    type WireMap = BTreeMap<String, u8>;

    #[derive(Clone, Debug, PartialEq, Cbor)]
    #[serde(into = "WireMap", try_from = "WireMap")]
    struct Record {
        id: u8,
        #[serde(flatten)]
        extra: WireMap,
    }

    impl From<Record> for WireMap {
        fn from(mut record: Record) -> Self {
            record.extra.insert("id".into(), record.id);
            record.extra
        }
    }

    impl TryFrom<WireMap> for Record {
        type Error = &'static str;

        fn try_from(mut extra: WireMap) -> Result<Self, Self::Error> {
            let id = extra.remove("id").ok_or("missing id")?;
            Ok(Self { id, extra })
        }
    }

    let record = Record {
        id: 7,
        extra: [("x".into(), 8)].into(),
    };
    let wire = cbor2::cdn_to_vec(r#"{"id":7, "x":8}"#).unwrap();
    assert_eq!(cbor2::to_vec(&record).unwrap(), wire);
    assert_eq!(cbor2::from_slice::<Record>(&wire).unwrap(), record);
    assert_eq!(cbor2::from_reader::<Record, _>(&wire[..]).unwrap(), record);
    assert_eq!(
        Value::serialized(&record)
            .unwrap()
            .deserialized::<Record>()
            .unwrap(),
        record
    );

    #[derive(Debug, PartialEq, Cbor)]
    #[serde(transparent)]
    struct Transparent {
        #[serde(flatten)]
        fields: WireMap,
    }

    let wrapper = Transparent {
        fields: record.into(),
    };
    assert_eq!(cbor2::to_vec(&wrapper).unwrap(), wire);
    assert_eq!(cbor2::from_slice::<Transparent>(&wire).unwrap(), wrapper);
}

#[test]
fn flattened_struct_serializes_serde_tag_when_all_fields_are_skipped() {
    #[derive(Cbor)]
    #[serde(tag = "kind")]
    struct Record {
        #[serde(flatten, skip_serializing)]
        _extra: std::collections::BTreeMap<String, u8>,
    }

    let record = Record {
        _extra: [("x".into(), 8)].into(),
    };
    let wire = cbor2::cdn_to_vec(r#"{"kind":"Record"}"#).unwrap();
    assert_eq!(cbor2::to_vec(&record).unwrap(), wire);
}
