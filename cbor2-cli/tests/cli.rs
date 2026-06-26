//! End-to-end tests driving the compiled `cbor` binary through its
//! actual command-line and stdin/stdout interface.

use std::io::Write as _;
use std::process::{Command, Output, Stdio};

const CBOR: &str = env!("CARGO_BIN_EXE_cbor");

fn run(args: &[&str], input: &[u8]) -> Output {
    let mut child = Command::new(CBOR)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary spawns");
    child
        .stdin
        .take()
        .expect("stdin is piped")
        .write_all(input)
        .expect("input is consumed");
    child.wait_with_output().expect("binary exits")
}

// Asserts a clean run and returns stdout.
fn ok(args: &[&str], input: &[u8]) -> Vec<u8> {
    let out = run(args, input);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty());
    out.stdout
}

#[test]
fn show_preserves_wire_details() {
    // Indefinite-length items keep their `_` markers, pretty-printed.
    let out = ok(&[], &hex("bf61610161629f0203ffff"));
    assert_eq!(
        out,
        b"{_\n  \"a\": 1,\n  \"b\": [_\n    2,\n    3\n  ]\n}\n"
    );

    let out = ok(&[], &hex("f7c074323031332d30332d32315432303a30343a30305a"));
    assert_eq!(out, b"undefined\n0(\"2013-03-21T20:04:00Z\")\n");

    // Small bignums print as plain integers (RFC 8949 Appendix A).
    let out = ok(&[], &hex("c249010000000000000000"));
    assert_eq!(out, b"18446744073709551616\n");
}

#[test]
fn show_accepts_hex_and_base64_arguments() {
    // Plain, 0x-prefixed and whitespace-littered hex.
    assert_eq!(ok(&["a201020326"], b""), b"{\n  1: 2,\n  3: -7\n}\n");
    assert_eq!(ok(&["0xA201020326"], b""), b"{\n  1: 2,\n  3: -7\n}\n");
    assert_eq!(ok(&["a2 0102 0326"], b""), b"{\n  1: 2,\n  3: -7\n}\n");

    // Base64 — padded or not — and base64url. {"a": 1} is "oWFhAQ==";
    // bytes(3) fbefbe is "Q_vvvg" url-safe.
    assert_eq!(ok(&["oWFhAQ=="], b""), b"{\n  \"a\": 1\n}\n");
    assert_eq!(ok(&["oWFhAQ"], b""), b"{\n  \"a\": 1\n}\n");

    assert_eq!(ok(&["Q_vvvg"], b""), b"h'fbefbe'\n");

    // Hex wins when a string parses as both: "0102" is the sequence
    // 1, 2 — not base64.
    assert_eq!(ok(&["0102"], b""), b"1\n2\n");
}

#[test]
fn decode_pretty_prints_json() {
    let out = ok(&["decode"], &hex("a1616101"));
    assert_eq!(out, b"{\n  \"a\": 1\n}\n");

    // Sequences become one JSON document per item.
    let out = ok(&["decode"], &hex("01a16374776f02"));
    assert_eq!(out, b"1\n{\n  \"two\": 2\n}\n");

    // Bytes render as hex strings, integer keys as JSON-encoded strings,
    // tags are dropped.
    let out = ok(&["decode"], &hex("a10742dead"));
    assert_eq!(out, b"{\n  \"7\": \"dead\"\n}\n");
    let out = ok(&["decode"], &hex("c11a514b67b0"));
    assert_eq!(out, b"1363896240\n");

    // Generic simple values survive JSON conversion as explicit strings.
    let out = ok(&["decode"], &hex("f83b"));
    assert_eq!(out, b"\"simple(59)\"\n");

    // The flexible input forms apply to decode as well.
    let out = ok(&["decode", "a1616101"], b"");
    assert_eq!(out, b"{\n  \"a\": 1\n}\n");
}

#[test]
fn decode_diag_pretty_prints_diagnostic_notation() {
    // {1: [2, 3]} spreads over indented lines, keys stay integers.
    let out = ok(&["decode", "--diag", "a101820203"], b"");
    assert_eq!(out, b"{\n  1: [\n    2,\n    3\n  ]\n}\n");

    // Scalars stay on one line; `-d` is the short form.
    let out = ok(&["decode", "-d", "01"], b"");
    assert_eq!(out, b"1\n");

    // The wire-level path preserves indefinite-length markers.
    let out = ok(&["decode", "-d", "bf0101ff"], b"");
    assert_eq!(out, b"{_\n  1: 1\n}\n");
}

#[test]
fn encode_streams_json_values() {
    assert_eq!(ok(&["encode"], br#"{"a": 1}"#), hex("a1616101"));

    // Multiple JSON values become a CBOR sequence.
    assert_eq!(
        ok(&["encode"], b"1 {\"two\":2}\n[3]"),
        hex("01a16374776f028103")
    );
}

#[test]
fn encode_can_emit_copyable_hex() {
    assert_eq!(ok(&["encode", "--hex"], br#"{"a": 1}"#), b"a1616101\n");

    // Multiple JSON values still represent one CBOR sequence, just as
    // copyable lowercase hex text.
    assert_eq!(
        ok(&["encode", "--hex"], b"1 {\"two\":2}\n[3]"),
        b"01a16374776f028103\n"
    );
}

#[test]
fn validate_reports_complete_cbor_sequences() {
    assert_eq!(ok(&["validate", "a1616101"], b""), b"valid\n");
    assert_eq!(ok(&["validate"], &hex("01a16374776f02")), b"valid\n");
}

#[test]
fn commands_chain_into_a_round_trip() {
    let json = br#"{"name": "example", "ok": true, "tags": [1, 2.5]}"#;
    let cbor = ok(&["encode"], json);
    let shown = ok(&[], &cbor);
    assert_eq!(
        shown,
        b"{\n  \"name\": \"example\",\n  \"ok\": true,\n  \"tags\": [\n    1,\n    2.5\n  ]\n}\n"
    );

    let back = ok(&["decode"], &cbor);
    let reparsed: serde_json::Value = serde_json::from_slice(&back).unwrap();
    assert_eq!(
        reparsed,
        serde_json::from_slice::<serde_json::Value>(json).unwrap()
    );
}

#[test]
fn reads_from_a_file_argument() {
    let path = std::env::temp_dir().join(format!("cbor_cli_test_{}.cbor", std::process::id()));
    std::fs::write(&path, hex("8401020304")).unwrap();

    let out = ok(&[path.to_str().unwrap()], b"");
    assert_eq!(out, b"[\n  1,\n  2,\n  3,\n  4\n]\n");
    let out = ok(&["decode", path.to_str().unwrap()], b"");
    assert_eq!(out, b"[\n  1,\n  2,\n  3,\n  4\n]\n");

    // `-` explicitly selects stdin.
    let out = ok(&["-"], &hex("00"));
    assert_eq!(out, b"0\n");

    std::fs::remove_file(&path).unwrap();
}

#[test]
fn help_and_version_print_and_exit_cleanly() {
    for args in [&["--help"][..], &["-h"][..], &["decode", "--help"][..]] {
        let out = run(args, b"");
        assert!(out.status.success());
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(text.contains("Usage: cbor [COMMAND] [INPUT]"), "{text}");
        assert!(text.contains("--hex"), "{text}");
        assert!(text.contains("validate"), "{text}");
    }

    let out = run(&["--version"], b"");
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");
}

#[test]
fn usage_errors_exit_with_status_2() {
    // An unknown option, too many arguments, --diag outside decode, an
    // unreadable input and something that is no known input form.
    for args in [
        &["--bogus"][..],
        &["decode", "a1616101", "01"][..],
        &["--diag", "01"][..],
        &["encode", "--diag"][..],
        &["--hex", "01"][..],
        &["decode", "--hex", "01"][..],
        &["/nonexistent/cbor_cli_test"][..],
        // `/` makes it a path, even though it is valid standard base64.
        &["Q/vvvg=="][..],
        &["not hex, not base64!"][..],
    ] {
        let out = run(args, b"");
        assert_eq!(out.status.code(), Some(2), "args: {args:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("Try `cbor --help`"),
            "args: {args:?}"
        );
    }
}

#[test]
fn data_errors_exit_with_status_1() {
    // Truncated CBOR, a lone break, broken JSON and empty validation input.
    for (args, input) in [
        (&["1a0000"][..], &b""[..]),
        (&["decode"][..], &hex("ff")[..]),
        (&["encode"][..], &b"{broken"[..]),
        (&["validate"][..], &b""[..]),
    ] {
        let out = run(args, input);
        assert_eq!(out.status.code(), Some(1), "args: {args:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).starts_with("cbor:"),
            "args: {args:?}"
        );
    }
}

fn hex(s: &str) -> Vec<u8> {
    assert!(s.len() % 2 == 0);
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}
