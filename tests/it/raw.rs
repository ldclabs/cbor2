//! Tests for `RawValue`: pre-encoded items spliced into and captured out
//! of streams without decoding.

use cbor2::{RawValue, Value};
use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Deserialize, Serialize)]
struct Envelope {
    kind: u8,
    body: RawValue,
}

#[test]
fn splices_and_captures_byte_for_byte() {
    // 0x1801 spells the integer 1 in a non-preferred form this crate's
    // encoder would never produce; RawValue must keep it untouched.
    let envelope = Envelope {
        kind: 1,
        body: RawValue::new(hex::decode("1801").unwrap()).unwrap(),
    };

    let bytes = cbor2::to_vec(&envelope).unwrap();
    assert_eq!(hex::encode(&bytes), "a2646b696e640164626f64791801");

    let back: Envelope = cbor2::from_slice(&bytes).unwrap();
    assert_eq!(back, envelope);
    assert_eq!(back.body.as_bytes(), hex::decode("1801").unwrap());

    // The reader-based path captures identically.
    let back: Envelope = cbor2::from_reader(&bytes[..]).unwrap();
    assert_eq!(back, envelope);

    // Indefinite-length spellings survive too.
    let raw = RawValue::new(hex::decode("9f0102ff").unwrap()).unwrap();
    let bytes = cbor2::to_vec(&raw).unwrap();
    assert_eq!(hex::encode(&bytes), "9f0102ff");
    assert_eq!(cbor2::from_slice::<RawValue>(&bytes).unwrap(), raw);

    // Tags belong to the item and are captured with it.
    let tagged: RawValue = cbor2::from_slice(&hex::decode("c11801").unwrap()).unwrap();
    assert_eq!(hex::encode(tagged.as_bytes()), "c11801");
}

#[test]
fn optional_raw_values_capture_exactly() {
    // `Option` peeks at the first header before deserializing `Some`,
    // which exercises the recording seed for pushed-back headers — with
    // a non-preferred header to prove exactness.
    #[derive(Debug, PartialEq, Deserialize)]
    struct Opt {
        body: Option<RawValue>,
    }

    let bytes = hex::decode("a164626f64791801").unwrap(); // {"body": 0x1801}
    let opt: Opt = cbor2::from_slice(&bytes).unwrap();
    assert_eq!(hex::encode(opt.body.unwrap().as_bytes()), "1801");

    let bytes = hex::decode("a164626f6479f6").unwrap(); // {"body": null}
    let opt: Opt = cbor2::from_slice(&bytes).unwrap();
    assert_eq!(opt.body, None);
}

#[test]
fn constructors_enforce_well_formedness() {
    assert!(RawValue::new(hex::decode("01").unwrap()).is_ok());
    // Truncated, trailing data, lone break and invalid UTF-8 all fail.
    assert!(RawValue::new(hex::decode("1a0000").unwrap()).is_err());
    assert!(RawValue::new(hex::decode("0101").unwrap()).is_err());
    assert!(RawValue::new(hex::decode("ff").unwrap()).is_err());
    assert!(RawValue::new(hex::decode("62fffe").unwrap()).is_err());
    assert!(RawValue::try_from(vec![0x01]).is_ok());

    // A malformed item on the wire fails the capture as well.
    let bytes = hex::decode("a2646b696e640164626f647962fffe").unwrap();
    assert!(cbor2::from_slice::<Envelope>(&bytes).is_err());

    // serialized/deserialized round-trip typed values.
    let raw = RawValue::serialized(&("hi", 42)).unwrap();
    assert_eq!(
        raw.deserialized::<(String, u8)>().unwrap(),
        ("hi".to_string(), 42)
    );
}

#[test]
fn value_paths_decode_and_reencode() {
    // Value has no raw form: serializing decodes, deserializing captures
    // the preferred re-encoding.
    let raw = RawValue::new(hex::decode("1801").unwrap()).unwrap();
    assert_eq!(Value::serialized(&raw).unwrap(), Value::from(1));
    let back: RawValue = Value::from(1).deserialized().unwrap();
    assert_eq!(hex::encode(back.as_bytes()), "01");

    // The canonical encoders therefore normalize raw contents.
    let envelope = Envelope { kind: 1, body: raw };
    let canonical = cbor2::to_canonical_vec(&envelope).unwrap();
    assert_eq!(hex::encode(&canonical), "a264626f647901646b696e6401");
}

#[test]
fn formats_as_diagnostic_notation() {
    let raw = RawValue::new(hex::decode("9f0102ff").unwrap()).unwrap();
    assert_eq!(raw.to_string(), "[_ 1, 2]");
    assert_eq!(format!("{raw:?}"), "RawValue([_ 1, 2])");
}

#[test]
fn json_round_trips_as_plain_bytes() {
    let raw = RawValue::new(hex::decode("1801").unwrap()).unwrap();
    let json = serde_json::to_string(&raw).unwrap();
    assert_eq!(json, "[24,1]");
    assert_eq!(serde_json::from_str::<RawValue>(&json).unwrap(), raw);

    // Invalid CBOR is rejected on the way back in.
    let msg = serde_json::from_str::<RawValue>("[255]")
        .unwrap_err()
        .to_string();
    assert!(msg.contains("syntax error"), "{msg}");
}
