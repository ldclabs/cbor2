//! `cbor` — the command line CBOR converter and inspector.
//!
//! Without a command, shows every CBOR item in the input as pretty diagnostic
//! notation (RFC 8949 §8), exactly as it appears on the wire.
//! `decode` converts CBOR items into pretty-printed JSON or — with
//! `--diag` — pretty-printed diagnostic notation; `encode` converts JSON
//! values, or CDN text with `--diag`, into CBOR items, optionally as
//! copyable hex text with `--hex`;
//! `validate` checks one or more complete CBOR items. Data errors exit with
//! status 1, usage errors with status 2.
//!
//! Install with Homebrew or Cargo:
//!
//! ```text
//! brew install ldclabs/tap/cbor2-cli   # installs cbor
//! cargo install cbor2-cli              # installs cbor
//! ```

use std::env;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, BufReader, Cursor, Read, Write};
use std::path::Path;
use std::process;

use cbor2::{RawValue, Value};

const USAGE: &str = "\
Usage: cbor [COMMAND] [INPUT]

Shows, decodes and encodes CBOR (RFC 8949). Without a command, every
CBOR item in INPUT is shown as pretty diagnostic notation (\u{a7}8),
exactly as it appears on the wire.

Commands:
  decode  Convert CBOR items to pretty-printed JSON, or to
          pretty-printed diagnostic notation with --diag
  encode  Convert JSON values, or CDN text with --diag, to CBOR items
  validate
          Validate one or more complete CBOR items

Input:
  INPUT is a file path, a hex string (optionally 0x-prefixed), a base64
  or base64url string, or `-` for stdin; stdin is the default. An
  argument containing a path separator is always a file path. `encode`
  reads JSON text, or CDN text with --diag, from a file or stdin only.
  Output goes to stdout.

Options:
  -d, --diag     With `decode`: print diagnostic notation instead of JSON
                 With `encode`: read Concise Diagnostic Notation, not JSON
      --hex      With `encode`: print lowercase hex text instead of raw bytes
  -h, --help     Print this help
  -V, --version  Print the version

Examples:
  cbor a201020326                  # show hex CBOR
  cbor decode message.cbor         # CBOR file -> pretty JSON
  printf \"{1: h'dead'}\" | cbor encode --diag --hex
  echo '{\"a\": 1}' | cbor encode --hex
  echo '{\"a\": 1}' | cbor encode    # JSON -> CBOR bytes";

enum Command {
    Show,
    Decode,
    Encode,
    Validate,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EncodeOutput {
    Raw,
    Hex,
}

fn main() {
    let (command, diag, encode_output, input) = parse_args();

    let result = match command {
        Command::Show => show(open_cbor_input(input.as_deref())),
        Command::Decode => decode(open_cbor_input(input.as_deref()), diag),
        Command::Encode => encode(open_text_input(input.as_deref()), encode_output, diag),
        Command::Validate => validate(open_cbor_input(input.as_deref())),
    };

    if let Err(err) = result {
        eprintln!("cbor: {err}");
        process::exit(1);
    }
}

// Parses the command line. `-h`/`--help` and `-V`/`--version` print and
// exit; anything malformed exits with 2.
fn parse_args() -> (Command, bool, EncodeOutput, Option<String>) {
    let mut diag = false;
    let mut encode_output = EncodeOutput::Raw;
    let mut positional = Vec::new();

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("cbor {}", env!("CARGO_PKG_VERSION"));
                process::exit(0);
            }
            "-d" | "--diag" => diag = true,
            "--hex" => encode_output = EncodeOutput::Hex,
            _ if arg.starts_with('-') && arg != "-" => {
                usage_error(format_args!("unrecognized option `{arg}`"));
            }
            _ => positional.push(arg),
        }
    }

    let mut positional = positional.into_iter().peekable();
    let command = match positional.peek().map(String::as_str) {
        Some("decode") => {
            positional.next();
            Command::Decode
        }
        Some("encode") => {
            positional.next();
            Command::Encode
        }
        Some("validate") => {
            positional.next();
            Command::Validate
        }
        _ => Command::Show,
    };

    let input = positional.next();
    if positional.next().is_some() {
        usage_error(format_args!("at most one INPUT argument"));
    }
    if diag && !matches!(command, Command::Decode | Command::Encode) {
        usage_error(format_args!(
            "`--diag` only applies to `decode` or `encode`"
        ));
    }
    if encode_output == EncodeOutput::Hex && !matches!(command, Command::Encode) {
        usage_error(format_args!("`--hex` only applies to `encode`"));
    }

    (command, diag, encode_output, input)
}

fn usage_error(msg: core::fmt::Arguments<'_>) -> ! {
    eprintln!("cbor: {msg}");
    eprintln!("Try `cbor --help`.");
    process::exit(2);
}

// Opens the input of the CBOR-reading commands: stdin (absent or `-`),
// an existing file, a hex string or a base64/base64url string.
fn open_cbor_input(arg: Option<&str>) -> Box<dyn Read> {
    let arg = match arg {
        None | Some("-") => return Box::new(BufReader::new(io::stdin().lock())),
        Some(arg) => arg,
    };

    if Path::new(arg).exists() {
        match File::open(arg) {
            Ok(file) => return Box::new(BufReader::new(file)),
            Err(err) => usage_error(format_args!("{arg}: {err}")),
        }
    }

    // Anything with a path separator is always a path — `/` is also a
    // standard-base64 character, and a mistyped file name must not be
    // decoded as inline data. Base64 containing `/` can come from stdin.
    if arg.contains('/') || arg.contains('\\') {
        usage_error(format_args!("{arg}: no such file"));
    }

    if let Some(bytes) = from_hex(arg) {
        return Box::new(Cursor::new(bytes));
    }
    if let Some(bytes) = from_base64(arg) {
        return Box::new(Cursor::new(bytes));
    }

    usage_error(format_args!(
        "`{arg}` is not a file, a hex string or a base64 string"
    ));
}

// Opens the input of `encode`: stdin (absent or `-`) or a text file.
fn open_text_input(arg: Option<&str>) -> Box<dyn Read> {
    match arg {
        None | Some("-") => Box::new(BufReader::new(io::stdin().lock())),
        Some(path) => match File::open(path) {
            Ok(file) => Box::new(BufReader::new(file)),
            Err(err) => usage_error(format_args!("{path}: {err}")),
        },
    }
}

type Error = Box<dyn std::error::Error>;

// Pretty-prints each CBOR item in diagnostic notation, preserving wire
// details: indefinite-length `_` markers, `undefined`, unassigned simple
// values and bignums as plain integers.
fn show(input: Box<dyn Read>) -> Result<(), Error> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    for item in cbor2::de::Deserializer::from_reader(input).into_iter::<RawValue>() {
        let diag = cbor2::to_cdn_pretty(item?.as_ref())?;
        writeln!(stdout, "{diag}")?;
    }

    Ok(stdout.flush()?)
}

// Decodes each CBOR item and pretty-prints it as JSON or — with
// `diag` — as indented diagnostic notation. The `diag` path works on
// wire bytes and preserves indefinite-length markers; the JSON path
// re-spells through `Value`.
fn decode(input: Box<dyn Read>, diag: bool) -> Result<(), Error> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    if diag {
        for item in cbor2::de::Deserializer::from_reader(input).into_iter::<RawValue>() {
            let text = cbor2::to_cdn_pretty(item?.as_ref())?;
            writeln!(stdout, "{text}")?;
        }
    } else {
        for item in cbor2::de::Deserializer::from_reader(input).into_iter::<Value>() {
            serde_json::to_writer_pretty(&mut stdout, &to_json(item?))?;
            stdout.write_all(b"\n")?;
        }
    }

    Ok(stdout.flush()?)
}

// Validates one or more complete CBOR items. This is deliberately a sequence
// check because the rest of the CLI accepts CBOR sequences item by item.
fn validate(input: Box<dyn Read>) -> Result<(), Error> {
    let mut count = 0usize;
    for item in cbor2::de::Deserializer::from_reader(input).into_iter::<RawValue>() {
        item?;
        count += 1;
    }

    if count == 0 {
        return Err("expected at least one CBOR item".into());
    }

    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    writeln!(stdout, "valid")?;
    Ok(stdout.flush()?)
}

// Reads a stream of JSON values, or CDN values with `--diag`, and writes
// them to stdout as CBOR items. Raw output streams bytes; hex output streams
// one copyable lowercase hex string for the complete CBOR sequence.
fn encode(mut input: Box<dyn Read>, output: EncodeOutput, diag: bool) -> Result<(), Error> {
    let stdout = io::stdout();
    let mut stdout = stdout.lock();
    let mut wrote_hex = false;

    if diag {
        let mut text = String::new();
        input.read_to_string(&mut text)?;
        let bytes = cbor2::cdn_sequence_to_vec(&text)?;
        match output {
            EncodeOutput::Raw => stdout.write_all(&bytes)?,
            EncodeOutput::Hex => {
                write!(stdout, "{}", hex(&bytes))?;
                stdout.write_all(b"\n")?;
            }
        }
        return Ok(stdout.flush()?);
    }

    for value in serde_json::Deserializer::from_reader(input).into_iter::<serde_json::Value>() {
        match output {
            EncodeOutput::Raw => cbor2::to_writer(&value?, &mut stdout)?,
            EncodeOutput::Hex => {
                let item = cbor2::to_vec(&value?)?;
                write!(stdout, "{}", hex(&item))?;
                wrote_hex = true;
            }
        }
    }

    if wrote_hex {
        stdout.write_all(b"\n")?;
    }

    Ok(stdout.flush()?)
}

// Converts a CBOR value to the closest JSON value.
//
// CBOR constructs that have no JSON equivalent are converted as follows:
// byte strings become lowercase hex strings, non-string map keys are
// JSON-encoded into strings, non-finite floats become null, tags are
// dropped (the inner value is kept), generic simple values become
// `simple(N)` strings, integers beyond the 64-bit ranges become strings
// and the "undefined" simple value becomes null.
fn to_json(value: Value) -> serde_json::Value {
    use serde_json::Value as Json;

    match value {
        Value::Null => Json::Null,
        Value::Bool(x) => Json::Bool(x),
        Value::Integer(x) => match (u64::try_from(x), i64::try_from(x)) {
            (Ok(x), _) => Json::from(x),
            (_, Ok(x)) => Json::from(x),
            // Outside both ranges (e.g. near -2^64): fall back to a string.
            _ => Json::String(i128::from(x).to_string()),
        },
        Value::Float(x) => serde_json::Number::from_f64(x).map_or(Json::Null, Json::Number),
        Value::Bytes(x) => Json::String(hex(&x)),
        Value::Text(x) => Json::String(x),
        Value::Simple(x) => Json::String(format!("simple({})", x.value())),
        Value::Tag(_, x) => to_json(*x),
        Value::Array(x) => Json::Array(x.into_iter().map(to_json).collect()),
        Value::Map(x) => Json::Object(
            x.into_iter()
                .map(|(k, v)| {
                    let key = match k {
                        Value::Text(s) => s,
                        // Serializing a serde_json::Value to a string cannot
                        // fail; never fall back to an (ambiguous) empty key.
                        other => serde_json::to_string(&to_json(other))
                            .expect("serializing a JSON value cannot fail"),
                    };
                    (key, to_json(v))
                })
                .collect(),
        ),
        _ => Json::Null,
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
    out
}

// Decodes a hex string — optionally 0x-prefixed, ASCII whitespace
// ignored — or returns `None` if the text is not hex.
fn from_hex(text: &str) -> Option<Vec<u8>> {
    let digits: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let digits = digits
        .strip_prefix(b"0x")
        .or_else(|| digits.strip_prefix(b"0X"))
        .unwrap_or(&digits);

    if digits.is_empty() || digits.len() % 2 != 0 {
        return None;
    }

    digits
        .chunks(2)
        .map(|pair| {
            let hi = char::from(pair[0]).to_digit(16)?;
            let lo = char::from(pair[1]).to_digit(16)?;
            Some((hi << 4 | lo) as u8)
        })
        .collect()
}

// Decodes a base64 or base64url string — padded or not, ASCII whitespace
// ignored — or returns `None` if the text is not base64.
fn from_base64(text: &str) -> Option<Vec<u8>> {
    fn sextet(b: u8) -> Option<u32> {
        Some(match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    }

    let mut data: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    while data.last() == Some(&b'=') {
        data.pop();
    }
    if data.is_empty() || data.len() % 4 == 1 {
        return None;
    }

    let mut out = Vec::with_capacity(data.len() * 3 / 4);
    for chunk in data.chunks(4) {
        let mut acc = 0u32;
        for &b in chunk {
            acc = acc << 6 | sextet(b)?;
        }
        match chunk.len() {
            4 => out.extend_from_slice(&[(acc >> 16) as u8, (acc >> 8) as u8, acc as u8]),
            3 => out.extend_from_slice(&[(acc >> 10) as u8, (acc >> 2) as u8]),
            _ => out.push((acc >> 4) as u8), // chunks of 2; length 1 is rejected above
        }
    }

    Some(out)
}
