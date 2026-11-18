use cbor2::{cdn_to_vec, from_slice, to_canonical_vec, Value};

fn cdn(s: &str) -> Vec<u8> {
    cdn_to_vec(s).unwrap()
}

#[test]
fn tuple_rejects_unconsumed_array_elements() {
    let result = from_slice::<((u8,), u8)>(&cdn("[[1, 2], 3]"));
    assert!(
        result.is_err(),
        "extra inner item was read as outer value: {result:?}"
    );
}

#[test]
fn tuple_rejects_truncated_array() {
    let result = from_slice::<(u8,)>(&[0x82, 1]);
    assert!(result.is_err(), "truncated item was accepted: {result:?}");
}

#[test]
fn tuple_consumes_indefinite_break() {
    let result = from_slice::<((u8,), u8)>(&cdn("[[_ 1], 2]"));
    assert!(result.is_ok(), "valid nested array failed: {result:?}");
    assert_eq!(result.unwrap(), ((1,), 2));
}

#[test]
fn value_tuple_rejects_unconsumed_elements() {
    let value = Value::Array(vec![1.into(), 2.into()]);
    let result = value.deserialized::<(u8,)>();
    assert!(result.is_err(), "extra data was ignored: {result:?}");
}

#[test]
fn reader_and_value_paths_enforce_container_completion() {
    type Record = ((u8,), u8);
    let valid = cdn("[[_ 1], 2]");
    assert_eq!(
        cbor2::from_reader::<Record, _>(valid.as_slice()).unwrap(),
        ((1,), 2)
    );
    let valid: Value = from_slice(&valid).unwrap();
    assert_eq!(valid.deserialized::<Record>().unwrap(), ((1,), 2));
    let extra = cdn("[[1, 2], 3]");
    assert!(cbor2::from_reader::<Record, _>(extra.as_slice()).is_err());
    assert!(from_slice::<Value>(&extra)
        .unwrap()
        .deserialized::<Record>()
        .is_err());
    for bytes in [cdn("h'0102'"), cdn("[_ 1, 2]"), vec![0x82, 1]] {
        assert!(cbor2::from_reader::<(u8,), _>(bytes.as_slice()).is_err());
    }
}

#[test]
fn a_visitor_cannot_leave_a_map_value_unread() {
    struct KeyOnly;
    impl<'de> serde::Deserialize<'de> for KeyOnly {
        fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            struct Visitor;
            impl<'de> serde::de::Visitor<'de> for Visitor {
                type Value = KeyOnly;
                fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    f.write_str("a map")
                }
                fn visit_map<A: serde::de::MapAccess<'de>>(
                    self,
                    mut map: A,
                ) -> Result<KeyOnly, A::Error> {
                    let _ = map.next_key::<u8>()?;
                    Ok(KeyOnly)
                }
            }
            deserializer.deserialize_map(Visitor)
        }
    }
    for text in ["{1: 2}", "{_ 1: 2}"] {
        let bytes = cdn(text);
        assert!(from_slice::<KeyOnly>(&bytes).is_err());
        assert!(cbor2::from_reader::<KeyOnly, _>(bytes.as_slice()).is_err());
        assert!(from_slice::<Value>(&bytes)
            .unwrap()
            .deserialized::<KeyOnly>()
            .is_err());
    }
}

#[test]
fn float_text_sequence_matches_string_shorthand() {
    assert_eq!(cdn("float<<\"3c00\">>"), cdn("float'3c00'"));
}

#[test]
fn same_string_shorthand_is_implemented() {
    assert_eq!(cdn("same'abc'"), cdn("same<<\"abc\">>"));
}

#[test]
fn same_distinguishes_null_from_undefined() {
    assert!(cdn_to_vec("same<<null, undefined>>").is_err());
}

#[test]
fn same_ignores_map_order() {
    assert!(cdn_to_vec("same<<{1: 2, 3: 4}, {3: 4, 1: 2}>>").is_ok());
}

#[test]
fn same_distinguishes_nan_payloads() {
    assert!(cdn_to_vec("same<<float'7e01', float'7e02'>>").is_err());
}

#[test]
fn hex_float_scaling_does_not_underflow_early() {
    let result: f64 = from_slice(&cdn("0x10p-1075")).unwrap();
    assert_eq!(result.to_bits(), 8, "exact value is 2^-1071");
}

#[test]
fn hex_float_scaling_does_not_overflow_early() {
    let literal = format!("0x1{}p-1080", "0".repeat(270));
    let result = cdn_to_vec(&literal);
    assert!(result.is_ok(), "exact value is 1: {result:?}");
    let value: f64 = from_slice(&result.unwrap()).unwrap();
    assert_eq!(value, 1.0);
}

#[test]
fn hex_float_exponent_arithmetic_never_panics() {
    assert_eq!(cdn("0x1.0p-2147483648"), cdn("0.0"));
}

#[test]
fn hex_float_rounds_once_with_sticky_bits() {
    let result: f64 = from_slice(&cdn("0x1.000000000000080001p0")).unwrap();
    assert_eq!(result.to_bits(), 0x3ff0_0000_0000_0001);
}

#[test]
fn dt_fraction_matches_decimal_float() {
    assert_eq!(cdn("dt'1970-01-01T00:00:00.12Z'"), cdn("0.12"));
}

#[test]
fn t1_concatenates_utf8_fragments_after_elision() {
    assert_eq!(
        cdn_to_vec("t1<<..., h'c3', h'a4'>>").ok(),
        Some(cdn("t1<<..., \"ä\">>"))
    );
}

#[test]
fn carriage_returns_are_ignored_inside_tokens() {
    assert_eq!(cdn_to_vec("1\r2").ok(), Some(cdn("12")));
}

#[test]
fn raw_string_rejects_unescaped_control_chars() {
    assert!(cdn_to_vec("`a\tb`").is_err());
}

#[test]
fn segmented_utf8_errors_keep_the_body_offset() {
    let invalid = [0x7f, 0x61, 0xff, 0xff];
    assert!(matches!(
        from_slice::<String>(&invalid),
        Err(cbor2::de::Error::Syntax(2))
    ));
    assert!(matches!(
        cbor2::from_reader::<String, _>(invalid.as_slice()),
        Err(cbor2::de::Error::Syntax(2))
    ));
    assert!(matches!(
        cbor2::validate_slice(&invalid),
        Err(cbor2::de::Error::Syntax(2))
    ));
}

#[test]
fn ipv4_rejects_leading_zero_octets() {
    assert!(cdn_to_vec("ip'127.000.0.1'").is_err());
}

#[cfg(feature = "cdn")]
#[test]
fn cri_accepts_numeric_registered_host() {
    assert!(cdn_to_vec("cri'https://123.456/'").is_ok());
}

#[test]
fn raw_canonical_preserves_undefined() {
    let raw = cbor2::RawValue::new(vec![0xf7]).unwrap();
    assert_eq!(to_canonical_vec(&raw).unwrap(), vec![0xf7]);
}

#[test]
fn raw_canonical_does_not_conflate_distinct_keys() {
    let raw = cbor2::RawValue::new(cdn("{undefined: 1, null: 2}")).unwrap();
    assert!(to_canonical_vec(&raw).is_ok());
}

#[test]
fn canonical_error_does_not_erase_map() {
    let mut value = Value::Map(vec![(1.into(), 2.into()), (1.into(), 3.into())]);
    let before = value.clone();
    assert!(value.canonicalize().is_err());
    assert_eq!(value, before);
}

#[test]
fn canonical_writer_does_not_write_before_semantic_validation() {
    let value = Value::Array(vec![
        Value::from(7),
        Value::Map(vec![(1.into(), 2.into()), (1.into(), 3.into())]),
    ]);
    let mut output = vec![0xaa];
    assert!(cbor2::to_canonical_writer(&value, &mut output).is_err());
    assert_eq!(output, vec![0xaa]);
}

#[cfg(feature = "derive")]
#[derive(Debug, Default, PartialEq)]
struct ExtensionMap(Vec<(Value, Value)>);

#[cfg(feature = "derive")]
impl serde::Serialize for ExtensionMap {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap as _;
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

#[cfg(feature = "derive")]
impl<'de> serde::Deserialize<'de> for ExtensionMap {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = ExtensionMap;

            fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str("a CBOR extension map")
            }

            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut access: A,
            ) -> Result<Self::Value, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = access.next_entry()? {
                    entries.push(entry);
                }
                Ok(ExtensionMap(entries))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}

#[cfg(feature = "derive")]
#[test]
fn flattened_struct_rejects_tagged_extension_keys_without_stripping_tags() {
    #[derive(Debug, PartialEq, cbor2::Cbor)]
    struct Message {
        #[cbor(key = 1)]
        id: u8,
        #[serde(flatten)]
        extra: ExtensionMap,
    }

    let message = Message {
        id: 1,
        extra: ExtensionMap(vec![(
            Value::Tag(100, Box::new(Value::from("x"))),
            Value::from(7),
        )]),
    };
    let bytes = cbor2::to_vec(&message).unwrap();
    let error = cbor2::from_slice::<Message>(&bytes).unwrap_err();
    assert!(
        error.to_string().contains("tagged extension key 100"),
        "unexpected error: {error}"
    );
    let error = cbor2::from_reader::<Message, _>(bytes.as_slice()).unwrap_err();
    assert!(error.to_string().contains("tagged extension key 100"));
    let value = Value::serialized(&message).unwrap();
    let error = value.deserialized::<Message>().unwrap_err();
    assert!(error.to_string().contains("tagged extension key 100"));

    // A tag around a declared integer key remains transparent.
    let tagged_field = cdn("{100(1): 1}");
    assert_eq!(
        cbor2::from_slice::<Message>(&tagged_field).unwrap(),
        Message {
            id: 1,
            extra: ExtensionMap::default(),
        }
    );
    assert_eq!(
        cbor2::from_slice::<Value>(&tagged_field)
            .unwrap()
            .deserialized::<Message>()
            .unwrap(),
        Message {
            id: 1,
            extra: ExtensionMap::default(),
        }
    );
}

#[test]
fn value_unit_newtype_enum_matches_wire() {
    #[derive(Debug, PartialEq, serde::Deserialize)]
    enum E {
        N(()),
    }
    let raw_result = from_slice::<E>(&cdn("\"N\""));
    let value_result = Value::from("N").deserialized::<E>();
    assert_eq!(
        raw_result.is_ok(),
        value_result.is_ok(),
        "wire: {raw_result:?}; Value: {value_result:?}"
    );
}

#[cfg(feature = "derive")]
#[test]
fn flattened_struct_preserves_simple_value() {
    #[derive(Debug, cbor2::Cbor)]
    struct Message {
        #[cbor(key = 1)]
        s: cbor2::Simple,
        #[serde(flatten)]
        extra: std::collections::BTreeMap<String, Value>,
    }
    let message = Message {
        s: cbor2::Simple::UNDEFINED,
        extra: Default::default(),
    };
    let bytes = cbor2::to_vec(&message).unwrap();
    let back: Message = from_slice(&bytes).unwrap();
    assert_eq!(back.s, message.s);
}

#[test]
fn canonical_rejects_equivalent_signed_zero_keys() {
    let value = Value::Map(vec![
        (Value::Float(0.0), 1.into()),
        (Value::Float(-0.0), 2.into()),
    ]);
    assert!(to_canonical_vec(&value).is_err());
    assert!(cbor2::to_canonical_vec_with(&value, cbor2::KeyOrder::LengthFirst).is_err());
}

#[test]
fn canonical_rejects_signed_zero_nested_in_keys() {
    let value = Value::Map(vec![
        (Value::Array(vec![Value::Float(0.0)]), 1.into()),
        (Value::Array(vec![Value::Float(-0.0)]), 2.into()),
    ]);
    assert!(to_canonical_vec(&value).is_err());
    assert!(cbor2::to_canonical_vec_with(&value, cbor2::KeyOrder::LengthFirst).is_err());
}

#[test]
fn nan_payload_uses_preferred_half_width() {
    let nan = f64::from_bits(0x7ff8_0400_0000_0000);
    assert_eq!(cbor2::to_vec(&nan).unwrap(), vec![0xf9, 0x7e, 0x01]);
}

#[test]
fn signaling_nan_roundtrips_through_typed_and_value_paths() {
    for bits in [0x7f800001u32, 0x7f802000, 0xff802000, 0x7fc02000] {
        let value = f32::from_bits(bits);
        let bytes = cbor2::to_vec(&value).unwrap();
        assert_eq!(from_slice::<f32>(&bytes).unwrap().to_bits(), bits);
        assert_eq!(
            cbor2::from_reader::<f32, _>(bytes.as_slice())
                .unwrap()
                .to_bits(),
            bits
        );
        assert_eq!(
            Value::from(value).deserialized::<f32>().unwrap().to_bits(),
            bits
        );
        let text = cbor2::to_cdn(bytes.as_slice()).unwrap();
        assert_eq!(cdn(&text), bytes);
    }
}

#[test]
fn same_uses_key_equivalence_without_changing_float_values() {
    assert!(cdn_to_vec("same<<{0.0: 1}, {-0.0: 1}>>").is_ok());
    assert!(cdn_to_vec("same<<0.0, -0.0>>").is_err());
    assert!(cdn_to_vec("same<<{1: 0.0}, {1: -0.0}>>").is_err());
}

#[test]
fn diagnostic_preserves_nondefault_nan_value() {
    let bytes = vec![0xf9, 0xfe, 0x00];
    let text = cbor2::to_cdn(bytes.as_slice()).unwrap();
    assert_eq!(cdn(&text), bytes, "negative NaN rendered as {text}");
}

#[cfg(feature = "derive")]
#[test]
fn flattened_struct_accepts_generic_simple_value() {
    #[derive(Debug, cbor2::Cbor)]
    struct Message {
        #[cbor(key = 1)]
        s: cbor2::Simple,
        #[serde(flatten)]
        extra: std::collections::BTreeMap<String, Value>,
    }
    let value = Message {
        s: cbor2::Simple::new(59).unwrap(),
        extra: Default::default(),
    };
    let bytes = cbor2::to_vec(&value).unwrap();
    let result = from_slice::<Message>(&bytes);
    assert!(result.is_ok(), "valid simple value rejected: {result:?}");
}

#[cfg(feature = "derive")]
#[test]
fn flattened_struct_preserves_raw_field_encoding() {
    #[derive(Debug, cbor2::Cbor)]
    struct Message {
        #[cbor(key = 1)]
        body: cbor2::RawValue,
        #[serde(flatten)]
        extra: std::collections::BTreeMap<String, Value>,
    }
    let body = cbor2::RawValue::new(vec![0x18, 1]).unwrap();
    let value = Message {
        body,
        extra: Default::default(),
    };
    assert_eq!(cbor2::to_vec(&value).unwrap(), vec![0xa1, 1, 0x18, 1]);
}

#[test]
fn value_seq_supports_byte_string_like_wire() {
    let value = Value::Bytes(vec![1, 2]);
    let wire: Vec<u8> = from_slice(&cbor2::to_vec(&value).unwrap()).unwrap();
    assert_eq!(value.deserialized::<Vec<u8>>().ok(), Some(wire));
}

#[test]
fn hex_float_rounding_boundaries_and_random_exact_values() {
    for (input, bits) in [
        ("0x1p-1075", 0),
        ("-0x1p-1075", 1 << 63),
        ("0x1.0001p-1075", 1),
        ("0x0.fffffffffffff8p-1022", 1 << 52),
        ("0x1.00000000000007p0", 0x3ff0000000000000),
        ("0x1.00000000000008p0", 0x3ff0000000000000),
        ("0x1.00000000000018p0", 0x3ff0000000000002),
        ("0x1.fffffffffffff7p1023", 0x7fefffffffffffff),
        ("0x0p999999999999999999999", 0),
    ] {
        assert_eq!(
            from_slice::<f64>(&cdn(input)).unwrap().to_bits(),
            bits,
            "{input}"
        );
    }
    assert!(cdn_to_vec("0x1.fffffffffffff8p1023").is_err());
    let mut state = 0x47110815deadbeefu64;
    for _ in 0..10_000 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let value = f64::from_bits(state);
        if !value.is_finite() {
            continue;
        }
        let exponent = (state >> 52) & 0x7ff;
        let mantissa = (state & ((1 << 52) - 1)) | if exponent == 0 { 0 } else { 1 << 52 };
        let power = if exponent == 0 {
            -1074
        } else {
            exponent as i32 - 1075
        };
        let input = format!(
            "{}0x{mantissa:x}p{power}",
            if state >> 63 == 0 { "" } else { "-" }
        );
        assert_eq!(
            from_slice::<f64>(&cdn(&input)).unwrap().to_bits(),
            state,
            "{input}"
        );
    }
}

#[test]
fn alternate_large_integer_radices_agree() {
    for n in [129, 257, 1025, 4097] {
        // 2^n - 1 in two radices, straddling byte and limb boundaries.
        let binary = format!("0b{}", "1".repeat(n));
        let hex = format!("0x{:x}{}", (1u8 << (n % 4)) - 1, "f".repeat(n / 4));
        assert_eq!(cdn(&binary), cdn(&hex));
        assert_eq!(cdn(&format!("-{binary}")), cdn(&format!("-{hex}")));
    }
    for digits in [
        "999999999999999999999999999999999999999",
        "1000000000000000000000000000000000000000",
    ] {
        let bytes = cdn(digits);
        assert_eq!(cbor2::to_cdn(bytes.as_slice()).unwrap(), digits);
    }
}

#[test]
fn nested_key_equality_and_transactional_canonicalization() {
    use cbor2::KeyOrder;
    let positive = Value::Tag(
        100,
        Box::new(Value::Map(vec![(1.into(), Value::Float(0.0))])),
    );
    let negative = Value::Tag(
        100,
        Box::new(Value::Map(vec![(1.into(), Value::Float(-0.0))])),
    );
    for order in [KeyOrder::Bytewise, KeyOrder::LengthFirst] {
        let mut value = Value::Array(vec![Value::Map(vec![
            (positive.clone(), 1.into()),
            (negative.clone(), 2.into()),
        ])]);
        let bytes = cbor2::to_vec(&value).unwrap();
        assert!(value.canonicalize_with(order).is_err());
        assert_eq!(cbor2::to_vec(&value).unwrap(), bytes);
        assert!(cbor2::to_canonical_vec_with(&value, order).is_err());
    }
    let mut value = cbor2::cbor!({3: {2: 1, 1: -0.0}, 1: [2, 3], 2: 0}).unwrap();
    let expected = to_canonical_vec(&value).unwrap();
    value.canonicalize().unwrap();
    assert_eq!(cbor2::to_vec(&value).unwrap(), expected);
}

#[test]
fn cr_offsets_raw_controls_and_indicator_separators() {
    assert_eq!(cdn("tr\rue"), cdn("true"));
    assert_eq!(cdn("`\r a `"), cdn("\"a\""));
    assert_eq!(cdn("\"\\u0\r061\""), cdn("\"a\""));
    assert_eq!(cdn("\"\\r\""), vec![0x61, b'\r']);
    assert!(matches!(
        cdn_to_vec("\r[1 \r?]"),
        Err(cbor2::de::Error::Syntax(5))
    ));
    for bad in ["`\0`", "`\x7f`", "[_1[]]", "{_1\"k\":1}"] {
        assert!(cdn_to_vec(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn source_limits_precede_cdn_normalization_and_parsing() {
    assert!(cbor2::cdn_to_vec_with_limit("\r\r\r1", 3).is_err());
    assert_eq!(cbor2::cdn_to_vec_with_limit("\r1", 2).unwrap(), [1]);
    assert!(cbor2::cdn_sequence_to_vec_with_limit("1 2", 2).is_err());
    assert_eq!(
        cbor2::cdn_sequence_to_vec_with_limit("1 2", 3).unwrap(),
        [1, 2]
    );
}

#[cfg(feature = "cdn-hash")]
#[test]
fn independently_enabled_hash_extension() {
    assert_eq!(
        hex::encode(cdn("hash'foo'")),
        "58202c26b46b68ffc68ff99b453c1d30413413422d706483bfa0f98a5e886266e7ae"
    );
}

#[cfg(feature = "cdn-cri")]
#[test]
fn independently_enabled_cri_extension() {
    assert_eq!(
        cdn("cri'https://example.com/a'"),
        cdn("[-4, [\"example\", \"com\"], [\"a\"]]")
    );
}

#[test]
fn cdn_extension_errors_point_to_source_arguments() {
    for (input, expected) in [
        ("dt'2024-01-01T00:00:00X'", 2),
        ("ip'192.0.2.0/999'", 2),
        ("h'gg'", 1),
        ("b64'!'", 3),
        ("dt<<\"2024-01-01T00:00:00X\">>", 2),
        ("dt<<t1<<'2024-01-01', 'T00:00:00X'>>>>", 2),
        ("dt<<\"2024-01-01T00:00:00\\u{58}\">>", 2),
        ("\r[dt\r'2024-01-01T00:00:00X']", 5),
        ("[\"中文\", dt'2024-01-01T00:00:00X']", 13),
        ("float<<h'zz'>>", 8),
        ("dt`2024-01-01T00:00:00X`", 2),
        #[cfg(feature = "cdn-hash")]
        ("hash<<5>>", 4),
        #[cfg(feature = "cdn-cri")]
        ("cri'https://example.com/%3A'", 3),
    ] {
        let error = cdn_to_vec(input).unwrap_err();
        let offset = match error {
            cbor2::de::Error::Syntax(offset) | cbor2::de::Error::Semantic(Some(offset), _) => {
                offset
            }
            error => panic!("missing source offset for {input:?}: {error}"),
        };
        assert_eq!(offset, expected, "{input:?}");
        assert!(offset < input.len());
    }
}

#[test]
fn hex_literals_pair_digits_across_comments_and_reject_dangling_nibbles() {
    assert_eq!(cdn("h'a /comment/ b # next\nc d'"), cdn("h'abcd'"));
    assert_eq!(cdn("h'ab...cd'"), cdn("888([h'ab', 888(null), h'cd'])"));
    assert_eq!(
        cdn("h'...ab......cd...'"),
        cdn("888([888(null), h'ab', 888(null), h'cd', 888(null)])")
    );
    for input in ["h'a'", "h'a...bc'", "h'ab...c'", "h'ab...c...de'"] {
        assert!(cdn_to_vec(input).is_err(), "{input}");
    }
}
