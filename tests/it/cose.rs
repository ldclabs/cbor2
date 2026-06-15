//! End-to-end tests for `#[derive(cbor2::Cbor)]` (`derive` feature).

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
