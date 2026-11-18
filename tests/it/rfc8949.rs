//! The test vectors from RFC 8949 Appendix A.
//!
//! Every vector is decoded into a `Value` through the slice and reader paths
//! and compared against the expected item, and rendered as CDN. The appendix notation
//! and current CDN both read back as the same item. Vectors that are
//! in the preferred serialization are also encoded back byte for byte.

use cbor2::{cbor, Value};

struct Vector {
    hex: &'static str,
    // The diagnostic notation column, verbatim.
    diag: &'static str,
    value: Value,
    // Whether encoding `value` reproduces `hex` exactly.
    preferred: bool,
}

fn v(hex: &'static str, diag: &'static str, value: Value, preferred: bool) -> Vector {
    Vector {
        hex,
        diag,
        value,
        preferred,
    }
}

fn vectors() -> Vec<Vector> {
    let big = |s: &str| Value::Bytes(hex::decode(s).unwrap());

    vec![
        v("00", "0", Value::from(0), true),
        v("01", "1", Value::from(1), true),
        v("0a", "10", Value::from(10), true),
        v("17", "23", Value::from(23), true),
        v("1818", "24", Value::from(24), true),
        v("1819", "25", Value::from(25), true),
        v("1864", "100", Value::from(100), true),
        v("1903e8", "1000", Value::from(1000), true),
        v("1a000f4240", "1000000", Value::from(1000000), true),
        v("1b000000e8d4a51000", "1000000000000", Value::from(1000000000000u64), true),
        v(
            "1bffffffffffffffff", "18446744073709551615",
            Value::from(18446744073709551615u64),
            true,
        ),
        v(
            "c249010000000000000000", "18446744073709551616",
            Value::from(18446744073709551616u128),
            true,
        ),
        v(
            "3bffffffffffffffff", "-18446744073709551616",
            Value::from(-18446744073709551616i128),
            true,
        ),
        v(
            "c349010000000000000000", "-18446744073709551617",
            Value::Tag(3, big("010000000000000000").into()),
            true,
        ),
        v("20", "-1", Value::from(-1), true),
        v("29", "-10", Value::from(-10), true),
        v("3863", "-100", Value::from(-100), true),
        v("3903e7", "-1000", Value::from(-1000), true),
        v("f90000", "0.0", Value::from(0.0), true),
        v("f98000", "-0.0", Value::from(-0.0), true),
        v("f93c00", "1.0", Value::from(1.0), true),
        v("fb3ff199999999999a", "1.1", Value::from(1.1), true),
        v("f93e00", "1.5", Value::from(1.5), true),
        v("f97bff", "65504.0", Value::from(65504.0), true),
        v("fa47c35000", "100000.0", Value::from(100000.0), true),
        v("fa7f7fffff", "3.4028234663852886e+38", Value::from(3.4028234663852886e+38), true),
        v("fb7e37e43c8800759c", "1.0e+300", Value::from(1.0e+300), true),
        v("f90001", "5.960464477539063e-8", Value::from(5.960464477539063e-8), true),
        v("f90400", "0.00006103515625", Value::from(0.00006103515625), true),
        v("f9c400", "-4.0", Value::from(-4.0), true),
        v("fbc010666666666666", "-4.1", Value::from(-4.1), true),
        v("f97c00", "Infinity", Value::from(f64::INFINITY), true),
        v("f9fc00", "-Infinity", Value::from(f64::NEG_INFINITY), true),
        v("fa7f800000", "Infinity", Value::from(f64::INFINITY), false),
        v("faff800000", "-Infinity", Value::from(f64::NEG_INFINITY), false),
        v("fb7ff0000000000000", "Infinity", Value::from(f64::INFINITY), false),
        v("fbfff0000000000000", "-Infinity", Value::from(f64::NEG_INFINITY), false),
        v("f4", "false", Value::from(false), true),
        v("f5", "true", Value::from(true), true),
        v("f6", "null", Value::Null, true),
        // 0xf7 (undefined) is tested separately: it decodes as null.
        v(
            "c074323031332d30332d32315432303a30343a30305a", "0(\"2013-03-21T20:04:00Z\")",
            Value::Tag(0, Value::from("2013-03-21T20:04:00Z").into()),
            true,
        ),
        v(
            "c11a514b67b0", "1(1363896240)",
            Value::Tag(1, Value::from(1363896240).into()),
            true,
        ),
        v(
            "c1fb41d452d9ec200000", "1(1363896240.5)",
            Value::Tag(1, Value::from(1363896240.5).into()),
            true,
        ),
        v("d74401020304", "23(h'01020304')", Value::Tag(23, big("01020304").into()), true),
        v(
            "d818456449455446", "24(h'6449455446')",
            Value::Tag(24, big("6449455446").into()),
            true,
        ),
        v(
            "d82076687474703a2f2f7777772e6578616d706c652e636f6d", "32(\"http://www.example.com\")",
            Value::Tag(32, Value::from("http://www.example.com").into()),
            true,
        ),
        v("40", "h''", Value::Bytes(vec![]), true),
        v("4401020304", "h'01020304'", big("01020304"), true),
        v("60", "\"\"", Value::from(""), true),
        v("6161", "\"a\"", Value::from("a"), true),
        v("6449455446", "\"IETF\"", Value::from("IETF"), true),
        v("62225c", "\"\\\"\\\\\"", Value::from("\"\\"), true),
        v("62c3bc", "\"\u{00fc}\"", Value::from("\u{00fc}"), true),
        v("63e6b0b4", "\"\u{6c34}\"", Value::from("\u{6c34}"), true),
        v("64f0908591", "\"\u{10151}\"", Value::from("\u{10151}"), true),
        v("80", "[]", Value::Array(vec![]), true),
        v("83010203", "[1, 2, 3]", cbor!([1, 2, 3]).unwrap(), true),
        v(
            "8301820203820405", "[1, [2, 3], [4, 5]]",
            cbor!([1, [2, 3], [4, 5]]).unwrap(),
            true,
        ),
        v(
            "98190102030405060708090a0b0c0d0e0f101112131415161718181819", "[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25]",
            cbor!([
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
                24, 25
            ])
            .unwrap(),
            true,
        ),
        v("a0", "{}", Value::Map(vec![]), true),
        v(
            "a201020304", "{1: 2, 3: 4}",
            Value::Map(vec![
                (Value::from(1), Value::from(2)),
                (Value::from(3), Value::from(4)),
            ]),
            true,
        ),
        v(
            "a26161016162820203", "{\"a\": 1, \"b\": [2, 3]}",
            cbor!({"a" => 1, "b" => [2, 3]}).unwrap(),
            true,
        ),
        v(
            "826161a161626163", "[\"a\", {\"b\": \"c\"}]",
            cbor!(["a", {"b" => "c"}]).unwrap(),
            true,
        ),
        v(
            "a56161614161626142616361436164614461656145", "{\"a\": \"A\", \"b\": \"B\", \"c\": \"C\", \"d\": \"D\", \"e\": \"E\"}",
            cbor!({"a" => "A", "b" => "B", "c" => "C", "d" => "D", "e" => "E"}).unwrap(),
            true,
        ),
        v("5f42010243030405ff", "(_ h'0102', h'030405')", big("0102030405"), false),
        v(
            "7f657374726561646d696e67ff", "(_ \"strea\", \"ming\")",
            Value::from("streaming"),
            false,
        ),
        v("9fff", "[_ ]", Value::Array(vec![]), false),
        v(
            "9f018202039f0405ffff", "[_ 1, [2, 3], [_ 4, 5]]",
            cbor!([1, [2, 3], [4, 5]]).unwrap(),
            false,
        ),
        v(
            "9f01820203820405ff", "[_ 1, [2, 3], [4, 5]]",
            cbor!([1, [2, 3], [4, 5]]).unwrap(),
            false,
        ),
        v(
            "83018202039f0405ff", "[1, [2, 3], [_ 4, 5]]",
            cbor!([1, [2, 3], [4, 5]]).unwrap(),
            false,
        ),
        v(
            "83019f0203ff820405", "[1, [_ 2, 3], [4, 5]]",
            cbor!([1, [2, 3], [4, 5]]).unwrap(),
            false,
        ),
        v(
            "9f0102030405060708090a0b0c0d0e0f101112131415161718181819ff", "[_ 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25]",
            cbor!([
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
                24, 25
            ])
            .unwrap(),
            false,
        ),
        v(
            "bf61610161629f0203ffff", "{_ \"a\": 1, \"b\": [_ 2, 3]}",
            cbor!({"a" => 1, "b" => [2, 3]}).unwrap(),
            false,
        ),
        v(
            "826161bf61626163ff", "[\"a\", {_ \"b\": \"c\"}]",
            cbor!(["a", {"b" => "c"}]).unwrap(),
            false,
        ),
        v(
            "bf6346756ef563416d7421ff", "{_ \"Fun\": true, \"Amt\": -2}",
            cbor!({"Fun" => true, "Amt" => -2}).unwrap(),
            false,
        ),
    ]
}

#[test]
fn decode() {
    for vector in vectors() {
        let bytes = hex::decode(vector.hex).unwrap();
        let decoded: Value = cbor2::from_slice(&bytes).unwrap();
        assert_eq!(decoded, vector.value, "decoding {}", vector.hex);
        let decoded: Value = cbor2::from_reader(&bytes[..]).unwrap();
        assert_eq!(decoded, vector.value, "reading {}", vector.hex);
    }
}

#[test]
fn diagnostic_notation() {
    // The NaN, undefined and simple-value rows have no Value vector above.
    let rows = [
        ("f97e00", "NaN"),
        ("fa7fc00000", "NaN"),
        ("fb7ff8000000000000", "NaN"),
        ("f7", "undefined"),
        ("f0", "simple(16)"),
        ("f8ff", "simple(255)"),
    ];
    let vectors = vectors();
    for (hex, text) in vectors.iter().map(|v| (v.hex, v.diag)).chain(rows) {
        let bytes = hex::decode(hex).unwrap();
        let rendered = match hex {
            "5f42010243030405ff" => "ilbs<<h'0102', h'030405'>>",
            "7f657374726561646d696e67ff" => r#"ilts<<"strea", "ming">>"#,
            _ => text,
        };
        assert_eq!(cbor2::diagnostic(&bytes[..]).unwrap(), rendered, "0x{hex}");

        // Parsing the text yields the same item, in preferred form unless
        // the notation keeps an indefinite length.
        let parsed = cbor2::cdn_to_vec(text).unwrap();
        assert_eq!(cbor2::cdn_to_vec(rendered).unwrap(), parsed, "{rendered}");
        let item = |bytes: &[u8]| cbor2::to_vec(&cbor2::from_slice::<Value>(bytes).unwrap());
        assert_eq!(item(&parsed).unwrap(), item(&bytes).unwrap(), "{text}");
        if text.contains('_') {
            assert_eq!(parsed, bytes, "{text}");
        }
    }
}

#[test]
fn encode_preferred() {
    for vector in vectors().into_iter().filter(|x| x.preferred) {
        let bytes = cbor2::to_vec(&vector.value).unwrap();
        assert_eq!(
            hex::encode(&bytes),
            vector.hex,
            "encoding {:?}",
            vector.value
        );
    }
}

#[test]
fn nan() {
    for hex in ["f97e00", "fa7fc00000", "fb7ff8000000000000"] {
        let bytes = hex::decode(hex).unwrap();
        let decoded: Value = cbor2::from_slice(&bytes).unwrap();
        assert!(matches!(decoded, Value::Float(x) if x.is_nan()), "{hex}");
    }

    // NaN encodes to the preferred (smallest) form.
    assert_eq!(
        cbor2::to_vec(&f64::NAN).unwrap(),
        hex::decode("f97e00").unwrap()
    );
}

#[test]
fn undefined_decodes_as_null() {
    let decoded: Value = cbor2::from_slice(&[0xf7]).unwrap();
    assert_eq!(decoded, Value::Null);
}

#[test]
fn simple_values() {
    // Unassigned simple values are preserved by the dynamic layer.
    assert_eq!(
        cbor2::from_slice::<Value>(&hex::decode("f0").unwrap()).unwrap(),
        Value::Simple(cbor2::Simple::new(16).unwrap())
    );
    assert_eq!(
        cbor2::from_slice::<Value>(&hex::decode("f8ff").unwrap()).unwrap(),
        Value::Simple(cbor2::Simple::new(255).unwrap())
    );

    // Two-byte encodings of values below 32 are not well-formed.
    for x in 0u8..32 {
        let err = cbor2::from_slice::<Value>(&[0xf8, x]).unwrap_err();
        assert!(matches!(err, cbor2::de::Error::Syntax(0)), "f8{x:02x}");
    }
}
