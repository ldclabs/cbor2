//! `cbor` — the command line CBOR converter and inspector.
//!
//! Without a command, shows every CBOR item in the input as pretty diagnostic
//! notation (RFC 8949 §8), exactly as it appears on the wire.
//! `decode` shows CBOR items as pretty-printed diagnostic notation, or
//! converts them to pretty-printed JSON with `--json`; `--diag` and `--cdn`
//! are explicit spellings of the default diagnostic/CDN output. `encode`
//! converts JSON-compatible values or CDN text into CBOR items, optionally
//! restricted with `--json`, `--diag` or `--cdn`, and optionally as copyable
//! hex text with `--hex`;
//! `validate` checks one or more complete CBOR items. Data errors exit with
//! status 1, usage errors with status 2.
//!
//! Install with Homebrew or Cargo:
//!
//! ```text
//! brew install ldclabs/tap/cbor2-cli   # installs cbor
//! cargo install cbor2-cli              # installs cbor
//! ```

mod json_input;

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashSet;
use std::env;
use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Cursor, Read, Write};
use std::path::Path;
use std::process;

use cbor2::{RawValue, Value};

const USAGE: &str = "\
Usage: cbor [COMMAND] [INPUT]

Shows, decodes and encodes CBOR (RFC 8949). Without a command, every
CBOR item in INPUT is shown as pretty diagnostic notation (\u{a7}8),
exactly as it appears on the wire.

Commands:
  decode  Show CBOR items as pretty-printed diagnostic notation,
          or convert them to pretty-printed JSON with --json
  encode  Convert JSON-compatible values or CDN text to CBOR items
  validate
          Validate one or more complete CBOR items

Input:
  INPUT is a file path, a hex string (optionally 0x-prefixed), a base64
  or base64url string, or `-` for stdin; stdin is the default. An
  argument containing a path separator is always a file path. `encode`
  reads JSON-compatible values or CDN text from a file or stdin only.
  Use --json or --diag/--cdn to restrict the accepted input syntax.
  CBOR-reading commands read raw CBOR bytes from files and stdin.
  Output goes to stdout.

Options:
  -d, --diag     With `decode`: print diagnostic notation (the default)
                 With `encode`: read only Concise Diagnostic Notation
      --cdn      With `decode`: print Concise Diagnostic Notation (the default)
                 With `encode`: read only Concise Diagnostic Notation
      --json     With `decode`: print pretty JSON instead of diagnostic notation
                 With `encode`: read only JSON text
      --hex      With `encode`: print lowercase hex text instead of raw bytes
      --         End options (e.g. cbor -- -Ds= for base64url input)
  -h, --help     Print this help
  -V, --version  Print the version

Examples:
  cbor a201020326                  # show hex CBOR
  cbor decode message.cbor         # CBOR file -> diagnostic notation
  cbor decode --json message.cbor  # CBOR file -> pretty JSON
  printf \"{1: h'dead'}\" | cbor encode --hex
  echo '{\"a\": 1}' | cbor encode --hex
  echo '{\"a\": 1}' | cbor encode    # JSON -> CBOR bytes";

enum Command {
    Show,
    Decode,
    Encode,
    Validate,
}

struct Args {
    command: Command,
    // `--json`: JSON output for `decode`, JSON-only input for `encode`.
    json: bool,
    // `--hex`: copyable hex output for `encode`.
    hex: bool,
    input: Option<String>,
}

fn main() {
    let Args {
        command,
        json,
        hex,
        input,
    } = parse_args();
    let input = input.as_deref();

    let result = match command {
        Command::Show | Command::Decode => decode(open_cbor_input(input), json),
        Command::Encode => encode(open_text_input(input), json, hex),
        Command::Validate => validate(open_cbor_input(input)),
    };

    if let Err(err) = result {
        if is_broken_pipe(err.as_ref()) {
            return;
        }
        eprintln!("cbor: {err}");
        process::exit(1);
    }
}

// Parses the command line. `-h`/`--help` and `-V`/`--version` print and
// exit; anything malformed exits with 2.
fn parse_args() -> Args {
    let mut diag = false;
    let mut cdn = false;
    let mut json = false;
    let mut hex = false;
    let mut positional = Vec::new();
    let mut input_only = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--" => {
                input_only = positional.is_empty();
                positional.extend(args);
                break;
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                process::exit(0);
            }
            "-V" | "--version" => {
                println!("cbor {}", env!("CARGO_PKG_VERSION"));
                process::exit(0);
            }
            "-d" | "--diag" => diag = true,
            "--cdn" => cdn = true,
            "--json" => json = true,
            "--hex" => hex = true,
            _ if arg.starts_with('-') && arg != "-" => {
                usage_error(format_args!("unrecognized option `{arg}`"));
            }
            _ => positional.push(arg),
        }
    }

    let mut positional = positional.into_iter().peekable();
    let command = match positional
        .peek()
        .map(String::as_str)
        .filter(|_| !input_only)
    {
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
    let codec = matches!(command, Command::Decode | Command::Encode);
    for (set, flag) in [(json, "--json"), (diag, "--diag"), (cdn, "--cdn")] {
        if set && !codec {
            usage_error(format_args!(
                "`{flag}` only applies to `decode` or `encode`"
            ));
        }
    }
    if json && (diag || cdn) {
        usage_error(format_args!(
            "`--json` cannot be combined with `--diag` or `--cdn`"
        ));
    }
    if hex && !matches!(command, Command::Encode) {
        usage_error(format_args!("`--hex` only applies to `encode`"));
    }

    Args {
        command,
        json,
        hex,
        input,
    }
}

fn usage_error(msg: core::fmt::Arguments<'_>) -> ! {
    eprintln!("cbor: {msg}");
    eprintln!("Try `cbor --help`.");
    process::exit(2);
}

// Opens the input of the CBOR-reading commands: stdin (absent or `-`),
// an existing file, a hex string or a base64/base64url string. Commands add
// their own buffering.
fn open_cbor_input(arg: Option<&str>) -> Box<dyn Read> {
    let arg = match arg {
        None | Some("-") => return Box::new(io::stdin().lock()),
        Some(arg) => arg,
    };

    if Path::new(arg).exists() {
        return open_file(arg);
    }

    // Anything with a path separator is always a path — `/` is also a
    // standard-base64 character, and a mistyped file name must not be
    // decoded as inline data. Use base64url or externally decode to raw CBOR
    // before piping to stdin when standard base64 contains `/`.
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
        None | Some("-") => Box::new(io::stdin().lock()),
        Some(path) => open_file(path),
    }
}

// A directory opens on some platforms but cannot be read; like any other
// unreadable input it is a usage error.
fn open_file(path: &str) -> Box<dyn Read> {
    match File::open(path) {
        Ok(file) if file.metadata().is_ok_and(|meta| meta.is_dir()) => {
            usage_error(format_args!("{path}: is a directory"))
        }
        Ok(file) => Box::new(file),
        Err(err) => usage_error(format_args!("{path}: {err}")),
    }
}

type Error = Box<dyn std::error::Error>;

// A consumer such as `head` may close stdout early. Buffered output is also
// flushed before reading more input, so the error can arrive wrapped as an
// input error of the CBOR or JSON deserializer.
fn is_broken_pipe(error: &(dyn std::error::Error + 'static)) -> bool {
    if let Some(error) = error.downcast_ref::<io::Error>() {
        return error.kind() == io::ErrorKind::BrokenPipe;
    }
    if let Some(error) = error.downcast_ref::<serde_json::Error>() {
        return error.io_error_kind() == Some(io::ErrorKind::BrokenPipe);
    }
    matches!(error.downcast_ref::<cbor2::de::Error>(),
        Some(cbor2::de::Error::Io(error)) if error.kind() == io::ErrorKind::BrokenPipe)
}

type Output = RefCell<BufWriter<io::StdoutLock<'static>>>;

fn output() -> Output {
    RefCell::new(BufWriter::new(io::stdout().lock()))
}

// Flushes the buffered output before each read of the underlying input.
// Complete items become visible before a command may wait for more input,
// without one write per item.
struct FlushBeforeRead<'a> {
    input: Box<dyn Read>,
    output: &'a Output,
}

impl Read for FlushBeforeRead<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.output.borrow_mut().flush()?;
        self.input.read(buf)
    }
}

fn streaming(input: Box<dyn Read>, output: &Output) -> BufReader<FlushBeforeRead<'_>> {
    BufReader::new(FlushBeforeRead { input, output })
}

// Decodes each CBOR item and pretty-prints it as diagnostic notation or, with
// `--json`, as JSON. The diagnostic path works on wire bytes and preserves
// indefinite-length markers; the JSON path re-spells through `Value`.
fn decode(input: Box<dyn Read>, json: bool) -> Result<(), Error> {
    let output = output();
    let input = streaming(input, &output);

    if json {
        let mut text = Vec::new();
        for item in cbor2::de::Deserializer::from_reader(input).into_iter::<Value>() {
            // Convert the whole item before writing any of it: a colliding
            // key must not leave a partial document behind.
            text.clear();
            serde_json::to_writer_pretty(&mut text, &Json(&item?))?;
            text.push(b'\n');
            output.borrow_mut().write_all(&text)?;
        }
    } else {
        for item in cbor2::de::Deserializer::from_reader(input).into_iter::<RawValue>() {
            let text = cbor2::to_cdn_pretty(item?.as_ref())?;
            writeln!(output.borrow_mut(), "{text}")?;
        }
    }

    Ok(output.into_inner().flush()?)
}

// Validates one or more complete CBOR items. This is deliberately a sequence
// check because the rest of the CLI accepts CBOR sequences item by item.
fn validate(input: Box<dyn Read>) -> Result<(), Error> {
    let input = BufReader::new(input);
    let mut count = 0usize;
    for item in cbor2::de::Deserializer::from_reader(input).into_iter::<serde::de::IgnoredAny>() {
        item?;
        count += 1;
    }

    if count == 0 {
        return Err("expected at least one CBOR item".into());
    }

    let mut stdout = io::stdout().lock();
    writeln!(stdout, "valid")?;
    Ok(stdout.flush()?)
}

// Reads a stream with the CDN parser by default, or the strict JSON parser
// with `--json`, and writes it to stdout as CBOR items. Raw output streams
// bytes; hex output streams one copyable lowercase hex line for the complete
// CBOR sequence. Both parsers keep map entries in input order, including
// duplicate keys, so the same JSON text encodes to the same bytes.
fn encode(mut input: Box<dyn Read>, json: bool, hex: bool) -> Result<(), Error> {
    let output = output();
    let write = |bytes: &[u8]| {
        let mut output = output.borrow_mut();
        if hex {
            write!(output, "{}", Hex(bytes))
        } else {
            output.write_all(bytes)
        }
    };

    if json {
        json_input::encode(
            FlushBeforeRead {
                input,
                output: &output,
            },
            write,
        )?;
    } else {
        let mut text = String::new();
        input.read_to_string(&mut text)?;
        write(&cbor2::cdn_sequence_to_vec(&text)?)?;
    }

    if hex {
        output.borrow_mut().write_all(b"\n")?;
    }
    Ok(output.into_inner().flush()?)
}

// The closest JSON form of a CBOR value.
//
// CBOR constructs that have no JSON equivalent are converted as follows:
// byte strings become lowercase hex strings, non-string map keys are
// JSON-encoded into strings, non-finite floats become null, tags are
// dropped (the inner value is kept), generic simple values become
// `simple(N)` strings, integers beyond the 64-bit ranges become strings
// and the "undefined" simple value becomes null. Map entries keep their
// CBOR order. Colliding object keys are rejected, including collisions in
// nested values or compound keys.
struct Json<'a>(&'a Value);

fn bignum(tag: u64, inner: &Value) -> Option<num_bigint::BigInt> {
    use num_bigint::{BigInt, Sign};
    let bytes = inner.as_bytes()?;
    match tag {
        2 => Some(BigInt::from_bytes_be(Sign::Plus, bytes)),
        3 => Some(BigInt::from_bytes_be(Sign::Minus, bytes) - 1),
        _ => None,
    }
}

fn json_key(key: &Value) -> Result<Cow<'_, str>, serde_json::Error> {
    match key {
        Value::Text(text) => return Ok(Cow::Borrowed(text)),
        Value::Integer(n) => return Ok(Cow::Owned(i128::from(*n).to_string())),
        Value::Tag(tag, inner) => {
            if let Some(n) = bignum(*tag, inner) {
                return Ok(Cow::Owned(n.to_string()));
            }
        }
        _ => {}
    }
    serde_json::to_string(&Json(key)).map(Cow::Owned)
}

impl serde::Serialize for Json<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::{Error as _, SerializeMap as _};

        match self.0 {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(x) => serializer.serialize_bool(*x),
            Value::Integer(x) => match (u64::try_from(*x), i64::try_from(*x)) {
                (Ok(x), _) => serializer.serialize_u64(x),
                (_, Ok(x)) => serializer.serialize_i64(x),
                // Outside both ranges (e.g. near -2^64): fall back to a string.
                _ => serializer.collect_str(&i128::from(*x)),
            },
            // serde_json writes non-finite floats as null.
            Value::Float(x) => serializer.serialize_f64(*x),
            Value::Bytes(x) => serializer.collect_str(&Hex(x)),
            Value::Text(x) => serializer.serialize_str(x),
            Value::Simple(x) => serializer.collect_str(&format_args!("simple({})", x.value())),
            Value::Tag(tag, inner) => match bignum(*tag, inner) {
                Some(n) => serializer.collect_str(&n),
                None => Json(inner).serialize(serializer),
            },
            Value::Array(x) => serializer.collect_seq(x.iter().map(Json)),
            Value::Map(x) => {
                let mut keys = HashSet::with_capacity(x.len());
                let mut map = serializer.serialize_map(Some(x.len()))?;
                for (key, value) in x {
                    let key = json_key(key).map_err(S::Error::custom)?;
                    if !keys.insert(key.clone()) {
                        return Err(S::Error::custom(format_args!(
                            "duplicate JSON object key {key:?} after CBOR key conversion"
                        )));
                    }
                    map.serialize_entry(&key, &Json(value))?;
                }
                map.end()
            }
            _ => serializer.serialize_unit(),
        }
    }
}

const HEX: &[u8; 16] = b"0123456789abcdef";

// Lowercase hex text, formatted through a bounded buffer so temporary memory
// does not grow with the item size.
struct Hex<'a>(&'a [u8]);

impl fmt::Display for Hex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buffer = [0u8; 4096];
        for chunk in self.0.chunks(buffer.len() / 2) {
            for (byte, pair) in chunk.iter().zip(buffer.as_chunks_mut::<2>().0) {
                *pair = [HEX[(byte >> 4) as usize], HEX[(byte & 15) as usize]];
            }
            let text = std::str::from_utf8(&buffer[..chunk.len() * 2]).map_err(|_| fmt::Error)?;
            f.write_str(text)?;
        }
        Ok(())
    }
}

// Decodes a hex string — optionally 0x-prefixed, ASCII whitespace
// ignored — or returns `None` if the text is not hex.
fn from_hex(text: &str) -> Option<Vec<u8>> {
    let digits: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let digits = digits
        .strip_prefix(b"0x")
        .or_else(|| digits.strip_prefix(b"0X"))
        .unwrap_or(&digits);

    if digits.is_empty() || !digits.len().is_multiple_of(2) {
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

    let data: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let unpadded = data.iter().position(|&b| b == b'=').unwrap_or(data.len());
    let padding = data.len() - unpadded;
    if padding > 0
        && (padding > 2
            || !data.len().is_multiple_of(4)
            || data[unpadded..].iter().any(|&b| b != b'='))
    {
        return None;
    }
    let data = &data[..unpadded];
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
            3 if acc & 3 == 0 => out.extend_from_slice(&[(acc >> 10) as u8, (acc >> 2) as u8]),
            2 if acc & 15 == 0 => out.push((acc >> 4) as u8),
            _ => return None,
        }
    }

    Some(out)
}
