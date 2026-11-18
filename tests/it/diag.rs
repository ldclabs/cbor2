//! Tests for diagnostic notation (RFC 8949 §8).

use cbor2::{cbor, Simple, Value};

use crate::util::{FailFmt, FailReader};

fn diag(hex: &str) -> String {
    let bytes = hex::decode(hex).unwrap();
    cbor2::diagnostic(&bytes[..]).unwrap()
}

fn pretty_with_key_comments(hex: &str, keys: &[(&str, i128)]) -> String {
    let bytes = hex::decode(hex).unwrap();
    let text = cbor2::diagnostic_pretty_with_key_comments(&bytes[..], keys).unwrap();
    assert_eq!(cbor2::cdn_to_vec(&text).unwrap(), bytes);
    text
}

#[test]
fn beyond_the_appendix() {
    // Chunkless strings remain distinguishable from one empty chunk.
    assert_eq!(diag("bfff"), "{_ }");
    for (hex, expected) in [
        ("5fff", "ilbs<<>>"),
        ("7fff", "ilts<<>>"),
        ("5f40ff", "ilbs<<h''>>"),
        ("7f60ff", r#"ilts<<"">>"#),
    ] {
        assert_eq!(diag(hex), expected);
        assert_eq!(
            cbor2::cdn_to_vec(expected).unwrap(),
            hex::decode(hex).unwrap()
        );
    }

    // More simple values. (Two-byte encodings below 32 are not
    // well-formed, so simple(24)..=simple(31) cannot appear at all.)
    assert_eq!(diag("f820"), "simple(32)");

    // Bignum corner cases: empty payloads, leading zeros, payloads wider
    // than 128 bits, carry propagation in the negative form.
    assert_eq!(diag("c240"), "0");
    assert_eq!(diag("c340"), "-1");
    assert_eq!(diag("c24300002a"), "42");
    assert_eq!(diag("c341ff"), "-256");
    assert_eq!(
        diag(
            "c25101000000000000000000000000000000 00"
                .replace(' ', "")
                .as_str()
        ),
        "340282366920938463463374607431768211456" // 2^128
    );
    assert_eq!(
        diag(
            "c35101000000000000000000000000000000 00"
                .replace(' ', "")
                .as_str()
        ),
        "-340282366920938463463374607431768211457" // -(2^128) - 1
    );
    // A segmented bignum payload still reads as a number.
    assert_eq!(diag("c35f4101ff"), "-2");
    // A bignum tag with a non-bytes payload falls back to the tag form.
    assert_eq!(diag("c201"), "2(1)");
    assert_eq!(diag("c36161"), "3(\"a\")");

    // Oversized bignums stay lossless but fall back to tag/bytes notation
    // instead of running the CPU-heavy arbitrary precision decimal renderer.
    let large_payload = vec![0xabu8; 1100];
    let large_bignum = Value::Tag(2, Box::new(Value::Bytes(large_payload.clone())));
    let bytes = cbor2::to_vec(&large_bignum).unwrap();
    let out = cbor2::diagnostic(&bytes[..]).unwrap();
    assert_eq!(out, format!("2(h'{}')", "ab".repeat(large_payload.len())));
    assert_eq!(large_bignum.to_string(), out);

    // Nested tags.
    assert_eq!(diag("c1c24102"), "1(2)");
    assert_eq!(diag("d9d9f780"), "55799([])");

    // Control-character escapes.
    assert_eq!(
        diag("6a085c090a0c0d2f01207f"),
        "\"\\b\\\\\\t\\n\\f\\r/\\u0001 \\u007f\""
    );

    // Invisible formatting characters (zero-width, bidirectional controls,
    // line/paragraph separators) are escaped so diagnostic output cannot be
    // visually misleading; ordinary non-ASCII text still passes through.
    assert_eq!(
        Value::from("a\u{202e}b\u{200b}c\u{2028}水").to_string(),
        "\"a\\u202eb\\u200bc\\u2028水\""
    );

    // Large bodies cross the internal 4096-byte chunking.
    let big = "a".repeat(5000);
    let bytes = cbor2::to_vec(&big).unwrap();
    assert_eq!(cbor2::diagnostic(&bytes[..]).unwrap(), format!("\"{big}\""));

    let blob = serde_bytes::ByteBuf::from(vec![0xabu8; 5000]);
    let bytes = cbor2::to_vec(&blob).unwrap();
    assert_eq!(
        cbor2::diagnostic(&bytes[..]).unwrap(),
        format!("h'{}'", "ab".repeat(5000))
    );

    // Multi-byte characters straddling the chunk boundary.
    let mut text = "a".repeat(4095);
    text.push_str("水水");
    let bytes = cbor2::to_vec(&text).unwrap();
    assert_eq!(
        cbor2::diagnostic(&bytes[..]).unwrap(),
        format!("\"{}水水\"", "a".repeat(4095))
    );
}

#[test]
fn malformed_input_is_rejected() {
    fn bad(hex: &str) {
        let bytes = hex::decode(hex).unwrap();
        assert!(cbor2::diagnostic(&bytes[..]).is_err(), "0x{hex}");
    }

    bad(""); // empty
    bad("19"); // truncated argument
    bad("62"); // truncated text body
    bad("c2"); // truncated bignum
    bad("c241"); // truncated bignum payload
    bad("ff"); // lone break
    bad("81ff"); // break as a definite array element
    bad("bf6161ff"); // dangling key in an indefinite map
    bad("62fffe"); // invalid UTF-8
    bad("62c3"); // incomplete character at the end of the body
    bad("5f6161ff"); // text segment in a byte string
    bad("7f4161ff"); // byte segment in a text string
    bad("0001"); // trailing data
    bad("f801"); // reserved simple value encoding
    bad("41"); // truncated byte-string body
    bad("c2ff"); // break as a bignum payload

    // Errors strike inside every container position.
    bad("5f"); // indefinite bytes: missing segment header
    bad("5f41"); // indefinite bytes: truncated segment body
    bad("7f"); // indefinite text: missing segment header
    bad("7f61"); // indefinite text: truncated segment body
    bad("7f62fffeff"); // indefinite text: invalid segment
    bad("9f"); // indefinite array: missing element
    bad("9f62fffeff"); // indefinite array: invalid element
    bad("a162fffe01"); // definite map: invalid key
    bad("a10162fffe"); // definite map: invalid value
    bad("bf"); // indefinite map: missing key
    bad("bf62fffeff"); // indefinite map: invalid key
    bad("bf01"); // indefinite map: missing value
    bad("bf0162fffe"); // indefinite map: invalid value

    // Nesting is depth-limited for arrays, maps and tags alike.
    let mut bomb = vec![0xc1u8; 65536];
    *bomb.last_mut().unwrap() = 0x01;
    assert!(matches!(
        cbor2::diagnostic(&bomb[..]),
        Err(cbor2::de::Error::RecursionLimitExceeded)
    ));

    // I/O failures surface as such.
    assert!(matches!(
        cbor2::diagnostic(FailReader(&[])),
        Err(cbor2::de::Error::Io(..))
    ));
}

#[test]
fn pretty_diagnostic_can_comment_integer_keys() {
    assert_eq!(
        pretty_with_key_comments("d83da201626d650442dead", &[("iss", 1), ("cti", 4)]),
        "61({\n  1: \"me\", # \"iss\"\n  4: h'dead' # \"cti\"\n})"
    );

    assert_eq!(
        pretty_with_key_comments("bf20636e6567ff", &[("neg", -1)]),
        "{_\n  -1: \"neg\" # \"neg\"\n}"
    );

    assert_eq!(
        pretty_with_key_comments("bf01bf0203ff046178ff", &[("outer", 1), ("inner", 2)]),
        "{_\n  1: {_\n    2: 3 # \"inner\"\n  }, # \"outer\"\n  4: \"x\"\n}"
    );

    assert_eq!(
        pretty_with_key_comments("a12001", &[("line\n\"", -1)]),
        "{\n  -1: 1 # \"line\\n\\\"\"\n}"
    );
}

#[test]
fn value_display_is_diagnostic_notation() {
    assert_eq!(Value::from(1).to_string(), "1");
    assert_eq!(Value::from(-1).to_string(), "-1");
    assert_eq!(Value::from(u64::MAX).to_string(), "18446744073709551615");
    assert_eq!(Value::Bytes(vec![1, 2, 3]).to_string(), "h'010203'");
    assert_eq!(Value::Float(1.5).to_string(), "1.5");
    assert_eq!(Value::Float(f64::NAN).to_string(), "NaN");
    assert_eq!(Value::Float(f64::NEG_INFINITY).to_string(), "-Infinity");
    assert_eq!(Value::Float(-1.0e300).to_string(), "-1.0e+300");
    assert_eq!(Value::from("a\"水").to_string(), "\"a\\\"水\"");
    assert_eq!(Value::Bool(false).to_string(), "false");
    assert_eq!(Value::Bool(true).to_string(), "true");
    assert_eq!(Value::Null.to_string(), "null");
    assert_eq!(
        Value::Simple(Simple::new(59).unwrap()).to_string(),
        "simple(59)"
    );
    assert_eq!(
        Value::Tag(32, Box::new(Value::from("x"))).to_string(),
        "32(\"x\")"
    );
    assert_eq!(
        cbor!({ "k" => [1, -2.5, null], 2 => {} })
            .unwrap()
            .to_string(),
        r#"{"k": [1, -2.5, null], 2: {}}"#
    );

    // Bignum tags display as numbers, like in the appendix...
    assert_eq!(
        Value::from(u64::MAX as u128 + 1).to_string(),
        "18446744073709551616"
    );
    assert_eq!(
        Value::from(-(u64::MAX as i128) - 2).to_string(),
        "-18446744073709551617"
    );
    // ...unless the payload is not a byte string.
    assert_eq!(Value::Tag(2, Box::new(Value::Null)).to_string(), "2(null)");

    // Display through a failing formatter propagates the error.
    use std::fmt::Write as _;
    assert!(write!(FailFmt, "{}", Value::Null).is_err());

    // The wire form and the Value form agree wherever both can represent
    // the item.
    for value in [
        cbor!([1, "two", h_bytes(), 3.5]).unwrap(),
        cbor!({ "deep" => { 1 => [null, true] } }).unwrap(),
        Value::Tag(1, Box::new(Value::from(1363896240))),
        Value::Simple(Simple::new(59).unwrap()),
    ] {
        let bytes = cbor2::to_vec(&value).unwrap();
        assert_eq!(cbor2::diagnostic(&bytes[..]).unwrap(), value.to_string());
    }

    fn h_bytes() -> Value {
        Value::Bytes(vec![0xde, 0xad])
    }
}

#[test]
fn value_display_depth_is_bounded() {
    use std::fmt::Write as _;

    // Every depth the wire renderer accepts also renders through `Value`.
    let mut bytes = vec![0x81u8; 255];
    bytes.push(0x01);
    let value: Value = cbor2::from_slice(&bytes).unwrap();
    assert_eq!(cbor2::diagnostic(&bytes[..]).unwrap(), value.to_string());
    assert!(!format!("{value:?}").is_empty());

    // A programmatically built value nested deeper than the recursion
    // limit reports fmt::Error instead of exhausting the native stack.
    let mut deep = Value::Null;
    for _ in 0..4096 {
        deep = Value::Tag(7, Box::new(deep));
    }
    let mut out = String::new();
    assert!(write!(out, "{deep}").is_err());
    out.clear();
    assert!(write!(out, "{deep:?}").is_err());
}

#[test]
fn float_formatting_boundaries() {
    // The plain/exponent switchover and digit-shifting paths.
    let cases: &[(f64, &str)] = &[
        (0.0, "0.0"),
        (-0.0, "-0.0"),
        (1e20, "100000000000000000000.0"),
        (1e21, "1.0e+21"),
        (1e-6, "0.000001"),
        (1e-7, "1.0e-7"),
        (1363896240.5, "1363896240.5"),
        (-65504.0, "-65504.0"),
        (5e-324, "5.0e-324"), // the smallest subnormal
        (f64::MAX, "1.7976931348623157e+308"),
    ];

    for (x, expected) in cases {
        assert_eq!(Value::Float(*x).to_string(), *expected, "{x:?}");
    }
}

#[test]
fn value_debug_is_indented_diagnostic_notation() {
    // Scalars (including bignum tags) stay on one line.
    assert_eq!(format!("{:?}", Value::from(1)), "1");
    assert_eq!(format!("{:?}", Value::from("水")), "\"水\"");
    assert_eq!(format!("{:?}", Value::Float(1.5)), "1.5");
    assert_eq!(format!("{:?}", Value::Bool(true)), "true");
    assert_eq!(format!("{:?}", Value::Null), "null");
    assert_eq!(
        format!("{:?}", Value::Simple(Simple::new(59).unwrap())),
        "simple(59)"
    );
    assert_eq!(format!("{:?}", Value::Bytes(vec![1, 2])), "h'0102'");
    assert_eq!(
        format!("{:?}", Value::from(u64::MAX as u128 + 1)),
        "18446744073709551616"
    );

    // Empty containers stay compact.
    assert_eq!(format!("{:?}", Value::Array(vec![])), "[]");
    assert_eq!(format!("{:?}", Value::Map(vec![])), "{}");

    // Non-empty containers spread, two spaces per level.
    let value = cbor!([1, [2, 3]]).unwrap();
    assert_eq!(format!("{value:?}"), "[\n  1,\n  [\n    2,\n    3\n  ]\n]");

    let value = cbor!({
        "a" => 1,
        "b" => { "c" => [true, {}] },
    })
    .unwrap();
    let expected = r#"{
  "a": 1,
  "b": {
    "c": [
      true,
      {}
    ]
  }
}"#;
    assert_eq!(format!("{value:?}"), expected);

    // Tags wrap their (possibly spread) content...
    let tagged = Value::Tag(99, Box::new(cbor!([1]).unwrap()));
    assert_eq!(format!("{tagged:?}"), "99([\n  1\n])");
    // ...and non-bytes bignum tags fall back to the tag form, for both
    // bignum tag numbers.
    let odd = Value::Tag(2, Box::new(Value::Null));
    assert_eq!(format!("{odd:?}"), "2(null)");
    let odd = Value::Tag(3, Box::new(Value::Null));
    assert_eq!(format!("{odd:?}"), "3(null)");
    assert_eq!(
        format!("{:?}", Value::from(-(u64::MAX as i128) - 2)),
        "-18446744073709551617"
    );

    // Container keys are laid out too.
    let value = Value::Map(vec![(cbor!([1]).unwrap(), Value::Null)]);
    assert_eq!(format!("{value:?}"), "{\n  [\n    1\n  ]: null\n}");

    // A failing formatter propagates the error.
    use std::fmt::Write as _;
    assert!(write!(FailFmt, "{:?}", Value::Null).is_err());
}
