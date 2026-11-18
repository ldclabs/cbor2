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
    // Drain output while feeding stdin: large streaming fixtures can fill both
    // pipes and deadlock if the parent writes everything before reading.
    std::thread::scope(|scope| {
        let mut stdin = child.stdin.take().expect("stdin is piped");
        let writer = scope.spawn(move || stdin.write_all(input).expect("input is consumed"));
        let output = child.wait_with_output().expect("binary exits");
        writer.join().unwrap();
        output
    })
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
fn decode_json_pretty_prints_json() {
    let out = ok(&["decode", "--json"], &hex("a1616101"));
    assert_eq!(out, b"{\n  \"a\": 1\n}\n");

    // Sequences become one JSON document per item.
    let out = ok(&["decode", "--json"], &hex("01a16374776f02"));
    assert_eq!(out, b"1\n{\n  \"two\": 2\n}\n");

    // Bytes render as hex strings, integer keys as JSON-encoded strings,
    // tags are dropped.
    let out = ok(&["decode", "--json"], &hex("a10742dead"));
    assert_eq!(out, b"{\n  \"7\": \"dead\"\n}\n");
    let out = ok(&["decode", "--json"], &hex("c11a514b67b0"));
    assert_eq!(out, b"1363896240\n");

    // Generic simple values survive JSON conversion as explicit strings.
    let out = ok(&["decode", "--json"], &hex("f83b"));
    assert_eq!(out, b"\"simple(59)\"\n");

    // The flexible input forms apply to decode as well.
    let out = ok(&["decode", "--json", "a1616101"], b"");
    assert_eq!(out, b"{\n  \"a\": 1\n}\n");
}

#[test]
fn decode_json_rejects_colliding_keys_before_writing_the_item() {
    for text in [
        r#"{1: "a", "1": "b"}"#,
        r#"{42("id"): 1, "id": 2}"#,
        r#"{42(43(18446744073709551617)): 1, "18446744073709551617": 2}"#,
        r#"{null: 1, "null": 2}"#,
        r#"{"same": 1, "same": 2}"#,
        r#"[{"nested": {1: 2, "1": 3}}]"#,
        r#"{{1: 2, "1": 3}: 4}"#,
    ] {
        let bytes = cbor2::cdn_to_vec(text).unwrap();
        let out = run(&["decode", "--json"], &bytes);
        assert_eq!(out.status.code(), Some(1), "{text}");
        assert!(out.stdout.is_empty(), "{text}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("duplicate JSON object key"));
    }
    assert_eq!(
        ok(&["decode", "--json", "a20102617803"], b""),
        b"{\n  \"1\": 2,\n  \"x\": 3\n}\n"
    );
    let out = run(&["decode", "--json", "01a201616161316162"], b"");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(out.stdout, b"1\n");
}

#[test]
fn base64_checks_padding_and_option_terminator() {
    for args in [&["--", "-Ds="][..], &["decode", "--", "-Ds"][..]] {
        assert_eq!(ok(args, b""), b"simple(59)\n");
    }
    assert_eq!(ok(&["validate", "--", "-Ds="], b""), b"valid\n");
    for input in ["AQ", "AQ==", " A Q = = ", "AQI", "AQI="] {
        assert!(run(&[input], b"").status.success(), "{input}");
    }
    for input in [
        "AQ=", "AQ===", "AQ=====", "AQI==", "AQID=", "A=Q=", "AR==", "AR", "AQJ=", "AQJ",
    ] {
        assert_eq!(run(&[input], b"").status.code(), Some(2), "{input}");
    }
    // After `--`, even a help option is a literal file/input argument.
    assert_eq!(run(&["--", "--help"], b"").status.code(), Some(2));
}

#[test]
fn hex_output_handles_large_items_and_sequences() {
    let json = format!("\"{}\"\n1", "large payload ".repeat(4096));
    for mode in ["--json", "--cdn"] {
        let bytes = ok(&["encode", mode], json.as_bytes());
        let text = ok(&["encode", mode, "--hex"], json.as_bytes());
        assert_eq!(text.last(), Some(&b'\n'));
        assert_eq!(text.iter().filter(|&&b| b == b'\n').count(), 1);
        assert_eq!(
            hex(std::str::from_utf8(&text[..text.len() - 1]).unwrap()),
            bytes
        );
    }
    // An empty sequence is an empty hex line in both input modes.
    assert_eq!(ok(&["encode", "--json", "--hex"], b""), b"\n");
    assert_eq!(ok(&["encode", "--cdn", "--hex"], b""), b"\n");
}

#[test]
fn json_conversion_keeps_map_order() {
    // CBOR map order survives JSON output, including converted keys.
    assert_eq!(
        ok(&["decode", "--json", "a3010203262001"], b""),
        b"{\n  \"1\": 2,\n  \"3\": -7,\n  \"-1\": 1\n}\n"
    );
    assert_eq!(
        ok(&["decode", "--json", "a2616201616102"], b""),
        b"{\n  \"b\": 1,\n  \"a\": 2\n}\n"
    );

    // JSON input keeps member order and duplicate names, exactly like the
    // CDN parser reading the same text.
    for (json, expected) in [
        (&br#"{"b": 1, "a": 2}"#[..], &b"a2616201616102\n"[..]),
        (br#"{"a": 1, "a": 2}"#, b"a2616101616102\n"),
        (br#"[{"z": null, "y": [true]}]"#, b"81a2617af6617981f5\n"),
    ] {
        assert_eq!(ok(&["encode", "--json", "--hex"], json), expected);
        assert_eq!(ok(&["encode", "--cdn", "--hex"], json), expected);
    }
}

#[test]
fn streaming_commands_flush_each_item_before_input_eof() {
    use std::io::Read as _;
    for (args, input, expected) in [
        (&["decode"][..], &[1u8][..], &b"1\n"[..]),
        (&["decode", "--json"][..], &[1], b"1\n"),
        (&["encode", "--json"][..], b"1 ", &[1]),
        (&["encode", "--json", "--hex"][..], b"1 ", b"01"),
    ] {
        let mut child = Command::new(CBOR)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = child.stdout.take().unwrap();
        let (send, recv) = std::sync::mpsc::channel();
        let len = expected.len();
        let reader = std::thread::spawn(move || {
            let mut output = vec![0; len];
            stdout.read_exact(&mut output).unwrap();
            send.send(output).unwrap();
        });
        stdin.write_all(input).unwrap();
        let output = recv.recv_timeout(std::time::Duration::from_secs(5));
        // Always close stdin and reap the child, including on a flush regression.
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        reader.join().unwrap();
        assert_eq!(output.unwrap(), expected, "{args:?}");
        assert!(out.status.success(), "{args:?}");
        assert!(out.stderr.is_empty(), "{args:?}");
    }
}

#[test]
fn closing_the_output_pipe_is_not_a_data_error() {
    for arg in ["--help", "--version"] {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let output = Command::new(CBOR).arg(arg).stdout(writer).output().unwrap();
        assert!(output.status.success(), "{arg}: {:?}", output.stderr);
        assert!(output.stderr.is_empty());
    }
    let json = format!("\"{}\"", "x".repeat(100_000));
    let bytes = cbor2::to_vec(&"x".repeat(100_000)).unwrap();
    for (args, input) in [
        (&["decode"][..], bytes.as_slice()),
        (&["decode", "--json"][..], bytes.as_slice()),
        (&["encode", "--json"][..], json.as_bytes()),
        (&["encode", "--json", "--hex"][..], json.as_bytes()),
        (&["encode"][..], json.as_bytes()),
        (&["encode", "--hex"][..], json.as_bytes()),
    ] {
        let mut child = Command::new(CBOR)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        let result = child.stdin.take().unwrap().write_all(input);
        if let Err(error) = result {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        }
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{args:?}: {:?}", out.stderr);
        assert!(out.stderr.is_empty());
    }
}

#[test]
fn decode_pretty_prints_diagnostic_notation_by_default() {
    // {1: [2, 3]} spreads over indented lines, keys stay integers.
    let out = ok(&["decode", "a101820203"], b"");
    assert_eq!(out, b"{\n  1: [\n    2,\n    3\n  ]\n}\n");

    // Scalars stay on one line; `--diag` remains an explicit spelling.
    let out = ok(&["decode", "--diag", "01"], b"");
    assert_eq!(out, b"1\n");

    // `--cdn` is the same explicit spelling, using the current CDN name.
    let out = ok(&["decode", "--cdn", "01"], b"");
    assert_eq!(out, b"1\n");

    // The wire-level path preserves indefinite-length markers; `-d` is the
    // short form.
    let out = ok(&["decode", "-d", "bf0101ff"], b"");
    assert_eq!(out, b"{_\n  1: 1\n}\n");
}

#[test]
fn encode_streams_json_values() {
    assert_eq!(ok(&["encode"], br#"{"a": 1}"#), hex("a1616101"));
    assert_eq!(ok(&["encode", "--json"], br#"{"a": 1}"#), hex("a1616101"));

    // Multiple JSON values become a CBOR sequence.
    assert_eq!(
        ok(&["encode"], b"1 {\"two\":2}\n[3]"),
        hex("01a16374776f028103")
    );
}

#[test]
fn encode_can_emit_copyable_hex() {
    assert_eq!(ok(&["encode", "--hex"], br#"{"a": 1}"#), b"a1616101\n");
    assert_eq!(
        ok(&["encode", "--json", "--hex"], br#"{"a": 1}"#),
        b"a1616101\n"
    );

    // Multiple JSON values still represent one CBOR sequence, just as
    // copyable lowercase hex text.
    assert_eq!(
        ok(&["encode", "--hex"], b"1 {\"two\":2}\n[3]"),
        b"01a16374776f028103\n"
    );
}

#[test]
fn encode_default_and_cdn_flags_read_cdn_text() {
    assert_eq!(
        ok(&["encode", "--hex"], br#"{ /k/ 1: h'dead' }"#),
        b"a10142dead\n"
    );

    assert_eq!(
        ok(&["encode", "--diag", "--hex"], br#"{ /k/ 1: h'dead' }"#),
        b"a10142dead\n"
    );
    assert_eq!(
        ok(&["encode", "--cdn", "--hex"], br#"{ /k/ 1: h'dead' }"#),
        b"a10142dead\n"
    );

    // CDN input can be a sequence, matching the rest of the CLI's CBOR
    // sequence handling.
    assert_eq!(ok(&["encode", "--diag", "--hex"], br#"1 "x""#), b"016178\n");

    let raw = ok(&["encode", "--diag"], br#"[_0 false, true]"#);
    assert_eq!(raw, hex("9802f4f5"));
}

#[test]
fn cdn_revision_29_rejects_removed_syntax_and_renders_string_chunks() {
    for args in [
        &["encode"][..],
        &["encode", "--cdn"][..],
        &["encode", "--diag"][..],
    ] {
        for text in [
            "1 ...",
            "unknown'x'",
            "+1",
            "01",
            "simple(23)_i",
            "1 // old comment\n",
        ] {
            let out = run(args, text.as_bytes());
            assert_eq!(out.status.code(), Some(1), "{args:?}: {text}");
            assert!(out.stdout.is_empty());
        }
    }
    let text = "ilbs<<h'01', h'0203'>>";
    let wire = hex("5f4101420203ff");
    assert_eq!(ok(&["decode"], &wire), format!("{text}\n").as_bytes());
    assert_eq!(
        ok(&["encode", "--cdn"], format!("# chunks\n{text}").as_bytes()),
        wire
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

    let back = ok(&["decode", "--json"], &cbor);
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

#[cfg(unix)]
#[test]
fn handles_non_utf8_file_names() {
    use std::os::unix::ffi::OsStrExt;
    let name = format!("cbor_cli_path_{}_", std::process::id()).into_bytes();
    let name = [name.as_slice(), b"\xff.cbor"].concat();
    let path = std::env::temp_dir().join(std::ffi::OsStr::from_bytes(&name));
    let output = Command::new(CBOR).arg(&path).output().unwrap();
    assert_eq!(output.status.code(), Some(2), "{:?}", output.stderr);
    // Linux also accepts these filenames on disk. macOS filesystems can
    // reject the filename at creation time.
    #[cfg(target_os = "linux")]
    {
        std::fs::write(&path, [1]).unwrap();
        let output = Command::new(CBOR).arg(&path).output().unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(output.status.success(), "{:?}", output.stderr);
        assert_eq!(output.stdout, b"1\n");
    }
}

#[test]
fn help_and_version_print_and_exit_cleanly() {
    for args in [&["--help"][..], &["-h"][..], &["decode", "--help"][..]] {
        let out = run(args, b"");
        assert!(out.status.success());
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(text.contains("Usage: cbor [COMMAND] [INPUT]"), "{text}");
        assert!(text.contains("--json"), "{text}");
        assert!(text.contains("--cdn"), "{text}");
        assert!(text.contains("--hex"), "{text}");
        assert!(text.contains("Concise Diagnostic Notation"), "{text}");
        assert!(text.contains("validate"), "{text}");
    }

    let out = run(&["--version"], b"");
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");
}

#[test]
fn usage_errors_exit_with_status_2() {
    let dir = env!("CARGO_MANIFEST_DIR");
    // An unknown option, too many arguments, flags on the wrong command, an
    // unreadable input and something that is no known input form.
    for args in [
        &["--bogus"][..],
        &["decode", "a1616101", "01"][..],
        &["decode", "--diag", "--json", "01"][..],
        &["decode", "--cdn", "--json", "01"][..],
        &["--diag", "01"][..],
        &["--json", "01"][..],
        &["--cdn", "01"][..],
        &["--hex", "01"][..],
        &["decode", "--hex", "01"][..],
        &["encode", "--json", "--diag"][..],
        &["encode", "--json", "--cdn"][..],
        &["/nonexistent/cbor_cli_test"][..],
        &["--", "-/"][..],
        // A directory is an unreadable input on every platform.
        &[dir][..],
        &["decode", dir][..],
        &["encode", dir][..],
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
    // Truncated CBOR, a lone break, broken text input and empty validation
    // input.
    for (args, input) in [
        (&["1a0000"][..], &b""[..]),
        (&["decode"][..], &hex("ff")[..]),
        (&["encode"][..], &b"{broken"[..]),
        (&["encode", "--json"][..], &b"{1:2}"[..]),
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
    assert!(s.len().is_multiple_of(2));
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn json_numeric_spelling_and_bignum_sign_survive_conversion() {
    for source in [
        "18446744073709551617",
        "-9223372036854775809",
        "0.12345678901234567",
        "-0",
        r#"{"a":18446744073709551617,"a":-0}"#,
    ] {
        assert_eq!(
            ok(&["encode", "--json"], source.as_bytes()),
            ok(&["encode", "--cdn"], source.as_bytes()),
            "{source}"
        );
    }
    let values = [
        "18446744073709551616",
        "-18446744073709551617",
        &"9".repeat(3000),
    ];
    for value in values {
        let encoded = ok(&["encode", "--json"], value.as_bytes());
        let json = ok(&["decode", "--json"], &encoded);
        assert_eq!(serde_json::from_slice::<String>(&json).unwrap(), value);
    }
    let map = ok(
        &["encode", "--cdn"],
        b"{18446744073709551616: 1, -18446744073709551617: 2}",
    );
    let json: serde_json::Value = serde_json::from_slice(&ok(&["decode", "--json"], &map)).unwrap();
    assert_eq!(json["18446744073709551616"], 1);
    assert_eq!(json["-18446744073709551617"], 2);
    let bytes = ok(
        &["encode", "--cdn"],
        br#"{18446744073709551616: 1, "18446744073709551616": 2}"#,
    );
    let failed = run(&["decode", "--json"], &bytes);
    assert_eq!(failed.status.code(), Some(1));
    assert!(failed.stdout.is_empty());
    assert_eq!(json.as_object().unwrap().len(), 2);
}

#[test]
fn json_sequences_cross_buffers_and_keep_strict_boundaries() {
    // A number may stop at the buffer edge and continue with more digits.
    let capacity = 32 * 1024;
    for prefix in [8191, capacity - 1] {
        let text = format!("{}18446744073709551617 [1,2]\n-0", " ".repeat(prefix));
        assert_eq!(
            ok(&["encode", "--json"], text.as_bytes()),
            ok(&["encode", "--cdn"], text.as_bytes())
        );
    }
    // After a long string the next items can start at the closing delimiter.
    let text = format!(r#"["{}"]{{"a":1,"a":2}}-0"#, "x".repeat(3 * capacity));
    let bytes = ok(&["encode", "--json"], text.as_bytes());
    let mut stream = cbor2::de::Deserializer::from_slice(&bytes).into_iter::<cbor2::Value>();
    assert_eq!(stream.next().unwrap().unwrap().as_array().unwrap().len(), 1);
    assert_eq!(stream.next().unwrap().unwrap().as_map().unwrap().len(), 2);
    assert_eq!(stream.next().unwrap().unwrap(), 0.into());
    assert!(stream.next().is_none());
    let invalid = format!("{}truefalse", " ".repeat(capacity - 4));
    let out = run(&["encode", "--json"], invalid.as_bytes());
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}
