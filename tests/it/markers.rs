//! Integer map keys and struct tags through the `@@CBOR@@` container
//! marker protocol. These tests write the marker by hand to exercise the
//! library without the `derive` feature; the `cose` module covers
//! `#[derive(cbor2::Cbor)]`.

use cbor2::Value;
use serde::{Deserialize, Serialize};

// What #[derive(Cbor)] effectively expands to: only the *container* is
// renamed; field names stay as written.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename = "@@CBOR@@@@kty=1;alg=3;crv=-1;x=-2@@CoseKey")]
struct CoseKey {
    kty: u8,
    alg: i8,
    crv: u8,
    x: serde_bytes::ByteBuf,
}

fn sample() -> CoseKey {
    CoseKey {
        kty: 2,
        alg: -7,
        crv: 1,
        x: serde_bytes::ByteBuf::from(vec![0x11, 0x22, 0x33, 0x44]),
    }
}

#[test]
fn marked_fields_become_integer_keys() {
    // {1: 2, 3: -7, -1: 1, -2: h'11223344'}
    let bytes = cbor2::to_vec(&sample()).unwrap();
    assert_eq!(hex::encode(&bytes), "a4010203262001214411223344");

    let back: CoseKey = cbor2::from_slice(&bytes).unwrap();
    assert_eq!(back, sample());

    // The same bytes decode through a Value, and Value::serialized
    // produces integer keys too.
    let value: Value = cbor2::from_slice(&bytes).unwrap();
    let keys: Vec<&Value> = value.as_map().unwrap().iter().map(|(k, _)| k).collect();
    assert_eq!(
        keys,
        [
            &Value::from(1),
            &Value::from(3),
            &Value::from(-1),
            &Value::from(-2)
        ]
    );
    assert_eq!(Value::serialized(&sample()).unwrap(), value);
    assert_eq!(value.deserialized::<CoseKey>().unwrap(), sample());

    // Canonical encoding sorts integer keys like any other key.
    let canonical = cbor2::to_canonical_vec(&sample()).unwrap();
    assert_eq!(hex::encode(&canonical), "a4010203262001214411223344");
}

#[test]
fn text_keys_still_decode() {
    // The wire may also carry the field names as text keys — JSON-shaped
    // input, hand-built values — and both forms may mix.
    let mixed = cbor2::cbor!({
        1 => 2,
        "alg" => -7,
        -1 => 1,
        "x" => cbor2::Value::Bytes(vec![0x11, 0x22, 0x33, 0x44]),
    })
    .unwrap();
    let bytes = cbor2::to_vec(&mixed).unwrap();
    assert_eq!(cbor2::from_slice::<CoseKey>(&bytes).unwrap(), sample());
    assert_eq!(mixed.deserialized::<CoseKey>().unwrap(), sample());
}

#[test]
fn json_uses_the_field_names() {
    // The container rename is invisible to serde_json.
    let json = serde_json::to_string(&sample()).unwrap();
    assert_eq!(json, r#"{"kty":2,"alg":-7,"crv":1,"x":[17,34,51,68]}"#);
    assert_eq!(serde_json::from_str::<CoseKey>(&json).unwrap(), sample());
}

#[test]
fn plain_numeric_names_stay_text() {
    // Without a marker there is no ambiguity: a numeric-looking field
    // name is a text key, exactly as in ciborium.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Plain {
        #[serde(rename = "1")]
        a: u8,
    }

    let bytes = cbor2::to_vec(&Plain { a: 7 }).unwrap();
    assert_eq!(hex::encode(&bytes), "a1613107"); // {"1": 7}
    assert_eq!(cbor2::from_slice::<Plain>(&bytes).unwrap(), Plain { a: 7 });
}

#[test]
fn unknown_integer_keys_are_ignored() {
    let extra = cbor2::cbor!({
        1 => 2,
        3 => -7,
        -1 => 1,
        -2 => cbor2::Value::Bytes(vec![0x11, 0x22, 0x33, 0x44]),
        99 => ["ignored", {"deep" => null}],
        -99 => "also ignored",
    })
    .unwrap();
    let bytes = cbor2::to_vec(&extra).unwrap();
    assert_eq!(cbor2::from_slice::<CoseKey>(&bytes).unwrap(), sample());
    assert_eq!(extra.deserialized::<CoseKey>().unwrap(), sample());
}

#[test]
fn marked_tags_wrap_and_are_required() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename = "@@CBOR@@123@@alg=1@@Header")]
    struct Header {
        alg: i8,
    }

    let bytes = cbor2::to_vec(&Header { alg: -7 }).unwrap();
    assert_eq!(hex::encode(&bytes), "d87ba10126"); // 123({1: -7})
    assert_eq!(
        cbor2::from_slice::<Header>(&bytes).unwrap(),
        Header { alg: -7 }
    );
    assert_eq!(cbor2::to_canonical_vec(&Header { alg: -7 }).unwrap(), bytes);

    // Missing or different tags are rejected; foreign wrappers are not.
    let msg = cbor2::from_slice::<Header>(&hex::decode("a10126").unwrap())
        .unwrap_err()
        .to_string();
    assert!(msg.contains("expected tag(123)"), "{msg}");
    let msg = cbor2::from_slice::<Header>(&hex::decode("d87ca10126").unwrap())
        .unwrap_err()
        .to_string();
    assert!(msg.contains("expected tag(123)"), "{msg}");
    assert_eq!(
        cbor2::from_slice::<Header>(&hex::decode("d9d9f7d87ba10126").unwrap()).unwrap(),
        Header { alg: -7 }
    );

    // The Value paths agree.
    let value = Value::serialized(&Header { alg: -7 }).unwrap();
    assert_eq!(
        value,
        Value::Tag(123, Box::new(cbor2::cbor!({ 1 => -7 }).unwrap()))
    );
    assert_eq!(value.deserialized::<Header>().unwrap(), Header { alg: -7 });
    let msg = cbor2::cbor!({ 1 => -7 })
        .unwrap()
        .deserialized::<Header>()
        .unwrap_err()
        .to_string();
    assert!(msg.contains("expected tag(123)"), "{msg}");
}

#[test]
fn only_canonical_markers_take_effect() {
    // A frame or tag segment that does not parse leaves an ordinary
    // container name: no tag, no key table.
    #[derive(Serialize)]
    #[serde(rename = "@@CBOR@@x7@@alg=1@@Bad")]
    struct BadTag {
        alg: i8,
    }

    let value = Value::serialized(&BadTag { alg: -7 }).unwrap();
    assert_eq!(value, cbor2::cbor!({ "alg" => -7 }).unwrap());

    // Within a valid frame, only canonical decimal entries map fields;
    // the rest keep their text keys.
    #[derive(Serialize)]
    #[serde(rename = "@@CBOR@@@@a=1;b=01;c=-0;d=+2;e=2x;f=18446744073709551616;g@@Odd")]
    struct Oddballs {
        a: u8,
        b: u8,
        c: u8,
        d: u8,
        e: u8,
        f: u8,
        g: u8,
        h: u8, // not in the table at all
    }

    let oddballs = Oddballs {
        a: 0,
        b: 1,
        c: 2,
        d: 3,
        e: 4,
        f: 5,
        g: 6,
        h: 7,
    };
    let value = Value::serialized(&oddballs).unwrap();
    let keys: Vec<&Value> = value.as_map().unwrap().iter().map(|(k, _)| k).collect();
    assert_eq!(
        keys,
        [
            &Value::from(1),
            &Value::from("b"),
            &Value::from("c"),
            &Value::from("d"),
            &Value::from("e"),
            &Value::from("f"),
            &Value::from("g"),
            &Value::from("h"),
        ]
    );

    // The streaming serializer agrees with the Value serializer.
    let direct = cbor2::to_vec(&oddballs).unwrap();
    assert_eq!(direct, cbor2::to_vec(&value).unwrap());

    // Edge tags and keys work.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(
        rename = "@@CBOR@@18446744073709551615@@hi=18446744073709551615;lo=-18446744073709551616;zero=0@@Edges"
    )]
    struct Edges {
        hi: u8,
        lo: u8,
        zero: u8,
    }

    let edges = Edges {
        hi: 1,
        lo: 2,
        zero: 0,
    };
    let bytes = cbor2::to_vec(&edges).unwrap();
    assert_eq!(
        hex::encode(&bytes),
        "dbffffffffffffffffa31bffffffffffffffff013bffffffffffffffff020000"
    );
    assert_eq!(cbor2::from_slice::<Edges>(&bytes).unwrap(), edges);
}

#[test]
fn struct_variants_use_integer_keys_too() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename = "@@CBOR@@@@payload=1@@Message")]
    enum Message {
        Signed { payload: u8 },
    }

    let bytes = cbor2::to_vec(&Message::Signed { payload: 7 }).unwrap();
    // {"Signed": {1: 7}}
    assert_eq!(hex::encode(&bytes), "a1665369676e6564a10107");
    assert_eq!(
        cbor2::from_slice::<Message>(&bytes).unwrap(),
        Message::Signed { payload: 7 }
    );
    assert_eq!(
        Value::serialized(&Message::Signed { payload: 7 })
            .unwrap()
            .deserialized::<Message>()
            .unwrap(),
        Message::Signed { payload: 7 }
    );
}

#[test]
fn non_identifier_keys_are_rejected() {
    #[derive(Debug, Deserialize)]
    #[allow(dead_code)]
    struct F {
        a: u8,
    }

    // A float key cannot name a field, on either path.
    let msg = cbor2::from_slice::<F>(&hex::decode("a1f93c0001").unwrap())
        .unwrap_err()
        .to_string();
    assert!(msg.contains("str, bytes or an integer"), "{msg}");

    let value = Value::Map(vec![(Value::Float(1.0), Value::from(1))]);
    let msg = value.deserialized::<F>().unwrap_err().to_string();
    assert!(msg.contains("str or integer"), "{msg}");
}

#[test]
fn tagged_integer_keys_still_match() {
    // A tag wrapped around an integer key is transparent, like elsewhere.
    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(rename = "@@CBOR@@@@a=1@@K")]
    struct K {
        a: u8,
    }

    let bytes = hex::decode("a1c10107").unwrap(); // {1(1): 7}
    assert_eq!(cbor2::from_slice::<K>(&bytes).unwrap(), K { a: 7 });

    let value = Value::Map(vec![(
        Value::Tag(9, Box::new(Value::from(1))),
        Value::from(7),
    )]);
    assert_eq!(value.deserialized::<K>().unwrap(), K { a: 7 });
}

#[test]
fn marker_write_failures_propagate() {
    struct Limited(usize);

    impl std::io::Write for Limited {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            if self.0 == 0 {
                return Err(std::io::Error::other("limit"));
            }
            let n = self.0.min(data.len());
            self.0 -= n;
            Ok(n)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[derive(Serialize)]
    #[serde(rename = "@@CBOR@@9@@a=1;b=-1@@T")]
    struct T {
        a: u8,
        b: u8,
    }

    // The tag does not fit; then the map header fits but a key does not.
    assert!(matches!(
        cbor2::to_writer(&T { a: 1, b: 2 }, Limited(0)),
        Err(cbor2::ser::Error::Io(..))
    ));
    assert!(matches!(
        cbor2::to_writer(&T { a: 1, b: 2 }, Limited(2)),
        Err(cbor2::ser::Error::Io(..))
    ));
}
