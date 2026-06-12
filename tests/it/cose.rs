//! End-to-end tests for the `#[cbor2::int_keys]` attribute macro
//! (`derive` feature).

use cbor2::Value;
use serde::{Deserialize, Serialize};

// A COSE_Key-shaped structure (RFC 9052 §7): all map keys are integers.
#[cbor2::int_keys]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct CoseKey {
    #[cbor(key = 1)]
    kty: u8,
    #[cbor(key = 3)]
    #[serde(alias = "alg")]
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

    // Through Value, with the textual alias accepted alongside.
    let value = Value::serialized(&sample()).unwrap();
    assert_eq!(value.deserialized::<CoseKey>().unwrap(), sample());

    let aliased = cbor2::cbor!({
        1 => 2,
        "alg" => -7,
        -1 => 1,
        -2 => cbor2::Value::Bytes(vec![0x11, 0x22, 0x33, 0x44]),
        "note" => null,
    })
    .unwrap();
    assert_eq!(aliased.deserialized::<CoseKey>().unwrap(), sample());
}

#[test]
fn enums_and_generics_work_too() {
    #[cbor2::int_keys]
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
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
}

#[test]
fn full_key_range() {
    #[cbor2::int_keys]
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
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
    // {0: 0, 18446744073709551615: 1, -18446744073709551616: 2}
    assert_eq!(
        hex::encode(&bytes),
        "a300001bffffffffffffffff013bffffffffffffffff02"
    );
    assert_eq!(cbor2::from_slice::<Edges>(&bytes).unwrap(), edges);
}
