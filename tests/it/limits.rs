//! Tests for robustness against adversarial input.

use cbor2::de::Error;
use cbor2::Value;

#[test]
fn recursion_limit() {
    // 64Ki nested arrays would exhaust the stack without a recursion limit.
    let bomb = vec![0x81u8; 65536];
    match cbor2::from_slice::<Value>(&bomb) {
        Err(Error::RecursionLimitExceeded) => {}
        other => panic!("expected recursion limit error, got {other:?}"),
    }

    // The limit is configurable.
    let shallow = [0x81, 0x81, 0x81, 0x01]; // [[[1]]]
    let mut de = cbor2::de::Deserializer::with_recursion_limit(&shallow[..], 2);
    assert!(matches!(
        serde::de::Deserialize::deserialize(&mut de),
        Err::<Value, _>(Error::RecursionLimitExceeded)
    ));

    let mut de = cbor2::de::Deserializer::with_recursion_limit(&shallow[..], 3);
    let value: Value = serde::de::Deserialize::deserialize(&mut de).unwrap();
    assert_eq!(
        value,
        Value::Array(vec![Value::Array(vec![Value::Array(vec![Value::from(1)])])])
    );
}

#[test]
fn forged_length_does_not_allocate() {
    // A byte string claiming u64::MAX bytes followed by EOF must fail with
    // an error quickly instead of attempting a giant allocation.
    let forged = hex::decode("5bffffffffffffffff").unwrap();
    assert!(matches!(
        cbor2::from_slice::<Value>(&forged),
        Err(Error::Io(..))
    ));

    // Same for text and arrays.
    let forged = hex::decode("7bffffffffffffffff").unwrap();
    assert!(cbor2::from_slice::<Value>(&forged).is_err());

    let forged = hex::decode("9bffffffffffffffff").unwrap();
    assert!(cbor2::from_slice::<Value>(&forged).is_err());
}

#[test]
fn truncated_input() {
    let bytes = cbor2::to_vec(&vec![1u32, 2, 3]).unwrap();
    for n in 0..bytes.len() {
        assert!(
            cbor2::from_slice::<Vec<u32>>(&bytes[..n]).is_err(),
            "truncation at {n} must fail"
        );
    }
}

#[test]
fn invalid_utf8_is_rejected() {
    // 0x62 (text of length 2) followed by invalid UTF-8.
    assert!(matches!(
        cbor2::from_slice::<String>(&[0x62, 0xff, 0xfe]),
        Err(Error::Syntax(..))
    ));

    // Each segment of an indefinite text item must be valid on its own:
    // splitting a multi-byte character across segments is not well-formed,
    // even though the joined bytes "e2 82 ac" spell "€".
    let split = hex::decode("7f62e28261acff").unwrap();
    assert!(matches!(
        cbor2::from_slice::<String>(&split),
        Err(Error::Syntax(2))
    ));
    assert!(matches!(
        cbor2::from_reader::<String, _>(&split[..]),
        Err(Error::Syntax(2))
    ));
}

#[test]
fn malformed_breaks_and_arguments() {
    // A lone break.
    assert!(cbor2::from_slice::<Value>(&[0xff]).is_err());

    // Reserved additional information values 28-30.
    for prefix in [0x1c, 0x1d, 0x1e, 0x3c, 0x5c, 0x7c, 0x9c, 0xbc, 0xdc, 0xfc] {
        assert!(
            matches!(
                cbor2::from_slice::<Value>(&[prefix, 0]),
                Err(Error::Syntax(0))
            ),
            "{prefix:02x} must be rejected"
        );
    }

    // An indefinite-length integer or tag is not well-formed.
    for prefix in [0x1f, 0x3f, 0xdf] {
        assert!(
            matches!(cbor2::from_slice::<Value>(&[prefix]), Err(Error::Syntax(0))),
            "{prefix:02x} must be rejected"
        );
    }

    // Nested indefinite string segments are not well-formed.
    let nested = hex::decode("5f5f4101ffff").unwrap();
    assert!(cbor2::from_slice::<Value>(&nested).is_err());
}

#[test]
fn error_offsets() {
    // The syntax error offset points at the offending item.
    let bytes = hex::decode("83011cff").unwrap(); // [1, <reserved>, ...]
    match cbor2::from_slice::<Value>(&bytes) {
        Err(Error::Syntax(2)) => {}
        other => panic!("expected syntax error at offset 2, got {other:?}"),
    }
}

#[test]
fn transparent_tags_count_towards_the_enclosing_budget() {
    use cbor2::de::Deserializer;
    fn check<T: serde::de::DeserializeOwned>(input: &[u8], limit: usize, ok: bool) {
        let mut slice = Deserializer::from_slice_with_recursion_limit(input, limit);
        let mut reader = Deserializer::with_recursion_limit(input, limit);
        for result in [T::deserialize(&mut slice), T::deserialize(&mut reader)] {
            if ok {
                assert!(result.is_ok());
            } else {
                assert!(matches!(result, Err(Error::RecursionLimitExceeded)));
            }
        }
    }
    for (limit, ok) in [(1, false), (2, true)] {
        check::<u8>(&[0xc8, 0xc9, 1], limit, ok);
        check::<Value>(&[0xc8, 0xc9, 1], limit, ok);
        check::<Vec<u8>>(&[0xc8, 0x82, 1, 2], limit, ok);
        check::<Vec<bool>>(&[0x82, 0xc8, 0xf5, 0xc9, 0xf4], limit, ok);
        check::<u64>(&[0xc8, 0xc2, 0x41, 1], limit, ok);
        check::<serde::de::IgnoredAny>(&[0xc8, 0xc9, 1], limit, ok);
        #[cfg(feature = "derive")]
        {
            #[derive(cbor2::Cbor)]
            struct Message {
                #[serde(flatten)]
                extra: std::collections::BTreeMap<String, Value>,
            }
            check::<Message>(&[0xa1, 0x61, b'x', 0xc1, 1], limit, ok);
        }
    }
}
