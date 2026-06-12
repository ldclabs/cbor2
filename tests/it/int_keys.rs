//! Integer map keys through the `@@KEY@@` marker protocol. These tests
//! write the marker by hand to exercise the library without the `derive`
//! feature; the `cose` module covers the `#[cbor2::int_keys]` macro.

use cbor2::Value;
use serde::{Deserialize, Serialize};

// What #[cbor2::int_keys] expands to.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct CoseKey {
    #[serde(rename = "@@KEY@@1")]
    kty: u8,
    #[serde(rename = "@@KEY@@3", alias = "alg")]
    alg: i8,
    #[serde(rename = "@@KEY@@-1")]
    crv: u8,
    #[serde(rename = "@@KEY@@-2")]
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
fn plain_numeric_names_stay_text() {
    // Without the marker there is no ambiguity: a numeric-looking field
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
fn aliases_match_text_and_integer_keys() {
    // A plain string alias, independent of integer keys.
    #[derive(Debug, PartialEq, Deserialize)]
    struct Named {
        #[serde(rename = "name", alias = "title")]
        n: String,
    }

    let renamed = cbor2::to_vec(&cbor2::cbor!({ "name" => "x" }).unwrap()).unwrap();
    assert_eq!(cbor2::from_slice::<Named>(&renamed).unwrap().n, "x");
    let aliased = cbor2::to_vec(&cbor2::cbor!({ "title" => "x" }).unwrap()).unwrap();
    assert_eq!(cbor2::from_slice::<Named>(&aliased).unwrap().n, "x");

    // An integer-keyed field with a textual alias accepts both forms.
    let mixed = cbor2::cbor!({
        1 => 2,
        "alg" => -7,
        -1 => 1,
        -2 => cbor2::Value::Bytes(vec![0x11, 0x22, 0x33, 0x44]),
    })
    .unwrap();
    let bytes = cbor2::to_vec(&mixed).unwrap();
    assert_eq!(cbor2::from_slice::<CoseKey>(&bytes).unwrap(), sample());
    assert_eq!(mixed.deserialized::<CoseKey>().unwrap(), sample());
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
}

#[test]
fn only_canonical_marked_decimals_become_integer_keys() {
    // Handwritten marker forms that are not canonical decimals stay text;
    // the attribute macro never generates these.
    #[derive(Serialize)]
    struct Oddballs {
        #[serde(rename = "@@KEY@@0")]
        zero: u8, // integer key 0
        #[serde(rename = "@@KEY@@18446744073709551615")]
        umax: u8, // integer key u64::MAX
        #[serde(rename = "@@KEY@@-18446744073709551616")]
        imin: u8, // integer key -2^64
        #[serde(rename = "@@KEY@@01")]
        zero_padded: u8, // text: leading zero
        #[serde(rename = "@@KEY@@-0")]
        negative_zero: u8, // text: not canonical
        #[serde(rename = "@@KEY@@+1")]
        plus: u8, // text: explicit sign
        #[serde(rename = "@@KEY@@1x")]
        suffixed: u8, // text: not a number
        #[serde(rename = "@@KEY@@-")]
        dash: u8, // text: no digits
        #[serde(rename = "@@KEY@@")]
        empty: u8, // text: no digits at all
        #[serde(rename = "@@KEY@@18446744073709551616")]
        too_big: u8, // text: beyond the CBOR integer range
        #[serde(rename = "@@KEY@@-18446744073709551617")]
        too_small: u8, // text: beyond the CBOR integer range
        #[serde(rename = "999")]
        unmarked: u8, // text: no marker
    }

    let oddballs = || Oddballs {
        zero: 0,
        umax: 1,
        imin: 2,
        zero_padded: 3,
        negative_zero: 4,
        plus: 5,
        suffixed: 6,
        dash: 7,
        empty: 8,
        too_big: 9,
        too_small: 10,
        unmarked: 11,
    };

    let value = Value::serialized(&oddballs()).unwrap();
    let keys: Vec<&Value> = value.as_map().unwrap().iter().map(|(k, _)| k).collect();
    assert_eq!(
        keys,
        [
            &Value::from(0u64),
            &Value::from(u64::MAX),
            &Value::Integer((-(u64::MAX as i128) - 1).try_into().unwrap()),
            &Value::from("@@KEY@@01"),
            &Value::from("@@KEY@@-0"),
            &Value::from("@@KEY@@+1"),
            &Value::from("@@KEY@@1x"),
            &Value::from("@@KEY@@-"),
            &Value::from("@@KEY@@"),
            &Value::from("@@KEY@@18446744073709551616"),
            &Value::from("@@KEY@@-18446744073709551617"),
            &Value::from("999"),
        ]
    );

    // The streaming serializer agrees with the Value serializer.
    let direct = cbor2::to_vec(&oddballs()).unwrap();
    assert_eq!(direct, cbor2::to_vec(&value).unwrap());
}

#[test]
fn struct_variants_use_integer_keys_too() {
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    enum Message {
        Signed {
            #[serde(rename = "@@KEY@@1")]
            payload: u8,
        },
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
    struct K {
        #[serde(rename = "@@KEY@@1")]
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
fn integer_key_write_failures_propagate() {
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
    struct Pos {
        #[serde(rename = "@@KEY@@1")]
        a: u8,
    }

    #[derive(Serialize)]
    struct Neg {
        #[serde(rename = "@@KEY@@-1")]
        a: u8,
    }

    // The map header fits, the integer key does not.
    assert!(matches!(
        cbor2::to_writer(&Pos { a: 1 }, Limited(1)),
        Err(cbor2::ser::Error::Io(..))
    ));
    assert!(matches!(
        cbor2::to_writer(&Neg { a: 1 }, Limited(1)),
        Err(cbor2::ser::Error::Io(..))
    ));
}
