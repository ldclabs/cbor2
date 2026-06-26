//! Tests for generic CBOR simple values (RFC 8949 §3.3).

use std::collections::BTreeMap;

use cbor2::core::{Encoder, Header};
use cbor2::{cbor, Simple, Value};

#[test]
fn simple_wrapper_preserves_wire_value() {
    let simple = Simple::new(59).unwrap();
    let bytes = cbor2::to_vec(&simple).unwrap();

    assert_eq!(hex::encode(&bytes), "f83b");
    assert_eq!(cbor2::from_slice::<Simple>(&bytes).unwrap(), simple);
    assert_eq!(
        cbor2::from_slice::<Value>(&bytes).unwrap(),
        Value::Simple(simple)
    );
    assert_eq!(cbor2::diagnostic(&bytes[..]).unwrap(), "simple(59)");
}

#[test]
fn one_byte_simple_values_roundtrip() {
    let simple = Simple::new(16).unwrap();
    let bytes = cbor2::to_vec(&simple).unwrap();

    assert_eq!(hex::encode(&bytes), "f0");
    assert_eq!(cbor2::from_slice::<Simple>(&bytes).unwrap(), simple);
    assert_eq!(
        cbor2::from_slice::<Value>(&bytes).unwrap(),
        Value::Simple(simple)
    );
}

#[test]
fn explicit_simple_can_capture_builtin_values() {
    assert_eq!(cbor2::from_slice::<Simple>(&[0xf4]).unwrap(), Simple::FALSE);
    assert_eq!(cbor2::from_slice::<Simple>(&[0xf5]).unwrap(), Simple::TRUE);
    assert_eq!(cbor2::from_slice::<Simple>(&[0xf6]).unwrap(), Simple::NULL);
    assert_eq!(
        cbor2::from_slice::<Simple>(&[0xf7]).unwrap(),
        Simple::UNDEFINED
    );

    // The default dynamic model keeps existing serde-compatible behavior for
    // the built-ins.
    assert_eq!(
        cbor2::from_slice::<Value>(&[0xf4]).unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        cbor2::from_slice::<Value>(&[0xf5]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(cbor2::from_slice::<Value>(&[0xf6]).unwrap(), Value::Null);
    assert_eq!(cbor2::from_slice::<Value>(&[0xf7]).unwrap(), Value::Null);
}

#[test]
fn simple_values_can_be_map_keys() {
    let redacted_claim_keys = Simple::new(59).unwrap();
    let value = Value::Map(vec![(
        Value::Simple(redacted_claim_keys),
        Value::Array(vec![Value::Bytes(vec![0xde, 0xad, 0xbe, 0xef])]),
    )]);

    let bytes = cbor2::to_vec(&value).unwrap();
    assert_eq!(hex::encode(&bytes), "a1f83b8144deadbeef");
    assert_eq!(cbor2::from_slice::<Value>(&bytes).unwrap(), value);

    let via_macro = cbor!({
        (Simple::new(59).unwrap()) => [Value::Bytes(vec![0xde, 0xad, 0xbe, 0xef])],
    })
    .unwrap();
    assert_eq!(via_macro, value);
}

#[test]
fn typed_maps_can_use_simple_keys() {
    let mut map = BTreeMap::new();
    map.insert(Simple::new(59).unwrap(), 1u8);

    let bytes = cbor2::to_vec(&map).unwrap();
    assert_eq!(hex::encode(&bytes), "a1f83b01");

    let back: BTreeMap<Simple, u8> = cbor2::from_slice(&bytes).unwrap();
    assert_eq!(back, map);
}

#[test]
fn value_bridge_preserves_simple_values() {
    let simple = Simple::new(59).unwrap();
    let value = Value::serialized(&simple).unwrap();

    assert_eq!(value, Value::Simple(simple));
    assert_eq!(value.deserialized::<Simple>().unwrap(), simple);
    assert_eq!(value.to_string(), "simple(59)");
    assert_eq!(format!("{value:?}"), "simple(59)");
}

#[test]
fn value_bridge_decodes_builtin_simple_values() {
    assert_eq!(
        Value::Bool(false).deserialized::<Simple>().unwrap(),
        Simple::FALSE
    );
    assert_eq!(
        Value::Bool(true).deserialized::<Simple>().unwrap(),
        Simple::TRUE
    );
    assert_eq!(Value::Null.deserialized::<Simple>().unwrap(), Simple::NULL);
    assert_eq!(
        Value::Simple(Simple::UNDEFINED)
            .deserialized::<Simple>()
            .unwrap(),
        Simple::UNDEFINED
    );
}

#[test]
fn canonical_encoding_sorts_simple_keys_by_encoded_bytes() {
    let mut value = Value::Map(vec![
        (Value::Simple(Simple::new(59).unwrap()), Value::Null),
        (Value::Simple(Simple::new(16).unwrap()), Value::Null),
        (Value::from(0), Value::Null),
    ]);

    value.canonicalize().unwrap();
    assert_eq!(
        value
            .as_map()
            .unwrap()
            .iter()
            .map(|(k, _)| k)
            .collect::<Vec<_>>(),
        vec![
            &Value::from(0),
            &Value::Simple(Simple::new(16).unwrap()),
            &Value::Simple(Simple::new(59).unwrap()),
        ]
    );
    assert_eq!(
        hex::encode(cbor2::to_canonical_vec(&value).unwrap()),
        "a300f6f0f6f83bf6"
    );
}

#[test]
fn reserved_simple_values_are_rejected() {
    assert!(Simple::new(24).is_none());
    assert_eq!(Simple::try_from(31).unwrap_err().value(), 31);

    let mut bytes = Vec::new();
    assert!(Encoder::from(&mut bytes).push(Header::Simple(24)).is_err());
    assert!(bytes.is_empty());

    assert!(cbor2::validate(&[0xf8, 0x18][..]).is_err());
    assert!(cbor2::from_slice::<Simple>(&[0xf8, 0x18]).is_err());
}
