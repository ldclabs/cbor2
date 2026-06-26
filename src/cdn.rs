//! Concise Diagnostic Notation (CDN) input support.

use alloc::{
    format,
    string::{String, ToString},
    vec::Vec,
};

use serde::de::DeserializeOwned;

use crate::core::{f16_to_f64, f64_to_f16, tag, Encoder, Header};
use crate::de::{Error, DEFAULT_RECURSION_LIMIT};
use crate::value::Value;

/// Encodes one Concise Diagnostic Notation (CDN) item as CBOR bytes.
///
/// This accepts the formalized diagnostic input syntax from
/// `draft-ietf-cbor-edn-literals`: JSON-compatible values, CBOR byte strings
/// (`'..'`, `h'..'`, `b64'..'`), comments, optional separator commas,
/// embedded CBOR sequence literals (`<<..>>`), tags, simple values and the
/// core encoding indicators (`_i`, `_0` through `_3`, and indefinite arrays
/// or maps with `[_` / `{_`). The default encoding is preferred
/// serialization.
///
/// ```rust
/// let bytes = cbor2::cdn_to_vec(r#"{ /kty/ 1: 4, "kid": h'deadbeef' }"#).unwrap();
/// assert_eq!(cbor2::to_cdn(&bytes[..]).unwrap(), r#"{1: 4, "kid": h'deadbeef'}"#);
/// ```
#[cfg(feature = "alloc")]
pub fn cdn_to_vec(input: &str) -> Result<Vec<u8>, Error> {
    let mut parser = Parser::new(input);
    let mut out = Vec::new();
    parser.skip_ws()?;
    parser.item(&mut out, DEFAULT_RECURSION_LIMIT)?;
    parser.skip_ws()?;
    if !parser.eof() {
        return Err(parser.syntax());
    }
    Ok(out)
}

/// Encodes a CDN sequence as a CBOR sequence.
///
/// Top-level items may be separated by commas or by blank space/comments.
/// This is the same sequence grammar used inside CDN's `<<..>>` embedded
/// CBOR literals, but without wrapping the result as a byte string.
#[cfg(feature = "alloc")]
pub fn cdn_sequence_to_vec(input: &str) -> Result<Vec<u8>, Error> {
    let mut parser = Parser::new(input);
    parser.sequence_to_vec(None, DEFAULT_RECURSION_LIMIT)
}

/// Deserializes one CDN item into a serde value.
///
/// Borrowed output fields are not supported by this helper because the CDN
/// text is first encoded into owned CBOR bytes. Use [`cdn_to_vec`] and
/// then [`from_slice`](crate::from_slice) directly when you need control over
/// the encoded bytes.
#[cfg(feature = "alloc")]
pub fn from_cdn<T>(input: &str) -> Result<T, Error>
where
    T: DeserializeOwned,
{
    let bytes = cdn_to_vec(input)?;
    crate::from_slice(&bytes[..])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Indicator<'a> {
    None,
    Indefinite,
    Immediate,
    Ai(u8),
    Other(&'a str),
}

#[derive(Clone, Debug)]
struct BigInt {
    negative: bool,
    magnitude: Vec<u8>,
}

#[derive(Clone, Debug)]
enum Atom {
    Integer(BigInt),
    Float(f64),
    FloatRaw { bytes: Vec<u8>, value: f64 },
    Bytes(Vec<u8>),
    Text(String),
    Simple(u8),
    Raw(Vec<u8>),
}

struct Arg {
    encoded: Vec<u8>,
    value: Value,
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn eof(&self) -> bool {
        self.pos == self.input.len()
    }

    fn rest(&self) -> &'a str {
        &self.input[self.pos..]
    }

    fn syntax(&self) -> Error {
        Error::Syntax(self.pos)
    }

    fn semantic(&self, msg: impl Into<String>) -> Error {
        Error::semantic(self.pos, msg)
    }

    fn starts_with(&self, s: &str) -> bool {
        self.rest().starts_with(s)
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn eat(&mut self, s: &str) -> bool {
        if self.starts_with(s) {
            self.pos += s.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, s: &str) -> Result<(), Error> {
        if self.eat(s) {
            Ok(())
        } else {
            Err(self.syntax())
        }
    }

    fn consume_ws(&mut self) -> Result<bool, Error> {
        let start = self.pos;

        loop {
            let before = self.pos;
            while matches!(self.peek(), Some('\t' | '\n' | '\r' | ' ')) {
                self.bump();
            }

            if self.eat("//") || self.eat("#") {
                while let Some(ch) = self.bump() {
                    if ch == '\n' {
                        break;
                    }
                }
                continue;
            }

            if self.eat("/*") {
                let Some(end) = self.rest().find("*/") else {
                    return Err(self.syntax());
                };
                self.pos += end + 2;
                continue;
            }

            if self.starts_with("/") {
                let mut chars = self.rest().chars();
                let _slash = chars.next();
                let Some(next) = chars.next() else {
                    return Err(self.syntax());
                };
                if next == '/' || next == '*' {
                    return Err(self.syntax());
                }
                self.bump();
                let Some(end) = self.rest().find('/') else {
                    return Err(self.syntax());
                };
                self.pos += end + 1;
                continue;
            }

            if self.pos == before {
                break;
            }
        }

        Ok(self.pos != start)
    }

    fn skip_ws(&mut self) -> Result<(), Error> {
        self.consume_ws().map(|_| ())
    }

    fn parse_spec(&mut self) -> Indicator<'a> {
        if !self.eat("_") {
            return Indicator::None;
        }

        let start = self.pos;
        while matches!(self.peek(), Some('_' | '0'..='9' | 'A'..='Z' | 'a'..='z')) {
            self.bump();
        }
        match &self.input[start..self.pos] {
            "" => Indicator::Indefinite,
            "i" => Indicator::Immediate,
            "0" => Indicator::Ai(0),
            "1" => Indicator::Ai(1),
            "2" => Indicator::Ai(2),
            "3" => Indicator::Ai(3),
            other => Indicator::Other(other),
        }
    }

    fn sequence_to_vec(&mut self, end: Option<&str>, depth: usize) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        self.skip_ws()?;
        if let Some(end) = end {
            if self.eat(end) {
                return Ok(out);
            }
        } else if self.eof() {
            return Ok(out);
        }

        loop {
            self.item(&mut out, depth)?;
            let had_ws = self.consume_ws()?;
            if self.eat(",") {
                self.skip_ws()?;
                if let Some(end) = end {
                    if self.eat(end) {
                        break;
                    }
                } else if self.eof() {
                    break;
                }
                continue;
            }
            if let Some(end) = end {
                if self.eat(end) {
                    break;
                }
            } else if self.eof() {
                break;
            }
            if !had_ws {
                return Err(self.syntax());
            }
        }

        Ok(out)
    }

    fn sequence_args(&mut self, depth: usize) -> Result<Vec<Arg>, Error> {
        self.expect("<<")?;
        let mut args = Vec::new();
        self.skip_ws()?;
        if self.eat(">>") {
            return Ok(args);
        }

        loop {
            let mut encoded = Vec::new();
            self.item(&mut encoded, depth)?;
            let value = crate::from_slice(&encoded[..])?;
            args.push(Arg { encoded, value });

            let had_ws = self.consume_ws()?;
            if self.eat(",") {
                self.skip_ws()?;
                if self.eat(">>") {
                    break;
                }
                continue;
            }
            if self.eat(">>") {
                break;
            }
            if !had_ws {
                return Err(self.syntax());
            }
        }

        Ok(args)
    }

    fn item(&mut self, out: &mut Vec<u8>, depth: usize) -> Result<(), Error> {
        if depth == 0 {
            return Err(Error::RecursionLimitExceeded);
        }

        self.skip_ws()?;
        let Some(ch) = self.peek() else {
            return Err(self.syntax());
        };

        match ch {
            '[' => self.array(out, depth - 1),
            '{' => self.map(out, depth - 1),
            '"' => {
                let atom = Atom::Text(self.quoted_string('"')?);
                let spec = self.parse_spec();
                self.emit_atom(out, atom, spec)
            }
            '\'' => {
                let atom = Atom::Bytes(self.quoted_string('\'')?.into_bytes());
                let spec = self.parse_spec();
                self.emit_atom(out, atom, spec)
            }
            '`' => {
                let atom = Atom::Text(self.raw_string()?);
                let spec = self.parse_spec();
                self.emit_atom(out, atom, spec)
            }
            '<' if self.starts_with("<<") => {
                self.expect("<<")?;
                let bytes = self.sequence_to_vec(Some(">>"), depth - 1)?;
                let atom = Atom::Bytes(bytes);
                let spec = self.parse_spec();
                self.emit_atom(out, atom, spec)
            }
            '(' if self.starts_with("(_") => self.stream_string(out, depth - 1),
            '+' | '-' | '.' | '0'..='9' => self.number_or_tag(out, depth - 1),
            'A'..='Z' | 'a'..='z' => self.word_item(out, depth - 1),
            _ => Err(self.syntax()),
        }
    }

    fn array(&mut self, out: &mut Vec<u8>, depth: usize) -> Result<(), Error> {
        self.expect("[")?;
        let spec = self.parse_spec();
        self.skip_ws()?;

        let indefinite = spec == Indicator::Indefinite;
        if indefinite {
            out.push(0x9f);
            if self.eat("]") {
                out.push(0xff);
                return Ok(());
            }

            loop {
                self.item(out, depth)?;
                let had_ws = self.consume_ws()?;
                if self.eat(",") {
                    self.skip_ws()?;
                    if self.eat("]") {
                        out.push(0xff);
                        return Ok(());
                    }
                    continue;
                }
                if self.eat("]") {
                    out.push(0xff);
                    return Ok(());
                }
                if !had_ws {
                    return Err(self.syntax());
                }
            }
        }

        let mut body = Vec::new();
        let mut count = 0usize;
        if !self.eat("]") {
            loop {
                self.item(&mut body, depth)?;
                count += 1;
                let had_ws = self.consume_ws()?;
                if self.eat(",") {
                    self.skip_ws()?;
                    if self.eat("]") {
                        break;
                    }
                    continue;
                }
                if self.eat("]") {
                    break;
                }
                if !had_ws {
                    return Err(self.syntax());
                }
            }
        }

        self.write_len(out, 4, count as u64, spec)?;
        out.extend_from_slice(&body);
        Ok(())
    }

    fn map(&mut self, out: &mut Vec<u8>, depth: usize) -> Result<(), Error> {
        self.expect("{")?;
        let spec = self.parse_spec();
        self.skip_ws()?;

        let indefinite = spec == Indicator::Indefinite;
        if indefinite {
            out.push(0xbf);
            if self.eat("}") {
                out.push(0xff);
                return Ok(());
            }

            loop {
                self.item(out, depth)?;
                self.skip_ws()?;
                self.expect(":")?;
                self.item(out, depth)?;
                let had_ws = self.consume_ws()?;
                if self.eat(",") {
                    self.skip_ws()?;
                    if self.eat("}") {
                        out.push(0xff);
                        return Ok(());
                    }
                    continue;
                }
                if self.eat("}") {
                    out.push(0xff);
                    return Ok(());
                }
                if !had_ws {
                    return Err(self.syntax());
                }
            }
        }

        let mut body = Vec::new();
        let mut count = 0usize;
        if !self.eat("}") {
            loop {
                self.item(&mut body, depth)?;
                self.skip_ws()?;
                self.expect(":")?;
                self.item(&mut body, depth)?;
                count += 1;
                let had_ws = self.consume_ws()?;
                if self.eat(",") {
                    self.skip_ws()?;
                    if self.eat("}") {
                        break;
                    }
                    continue;
                }
                if self.eat("}") {
                    break;
                }
                if !had_ws {
                    return Err(self.syntax());
                }
            }
        }

        self.write_len(out, 5, count as u64, spec)?;
        out.extend_from_slice(&body);
        Ok(())
    }

    fn stream_string(&mut self, out: &mut Vec<u8>, depth: usize) -> Result<(), Error> {
        self.expect("(_")?;
        if !self.consume_ws()? {
            return Err(self.syntax());
        }

        let mut chunks = Vec::new();
        let mut major = None;
        loop {
            let mut chunk = Vec::new();
            self.item(&mut chunk, depth)?;
            let Some(&head) = chunk.first() else {
                return Err(self.syntax());
            };
            let chunk_major = head >> 5;
            let is_definite_string = matches!(chunk_major, 2 | 3) && (head & 0x1f) != 31;
            if !is_definite_string {
                return Err(self.semantic("stream string chunks must be definite strings"));
            }
            match major {
                Some(m) if m != chunk_major => {
                    return Err(self.semantic("stream string chunks must have one type"));
                }
                None => major = Some(chunk_major),
                _ => {}
            }
            chunks.extend_from_slice(&chunk);

            let had_ws = self.consume_ws()?;
            if self.eat(",") {
                self.skip_ws()?;
                if self.eat(")") {
                    break;
                }
                continue;
            }
            if self.eat(")") {
                break;
            }
            if !had_ws {
                return Err(self.syntax());
            }
        }

        match major {
            Some(2) => out.push(0x5f),
            Some(3) => out.push(0x7f),
            _ => return Err(self.syntax()),
        }
        out.extend_from_slice(&chunks);
        out.push(0xff);
        Ok(())
    }

    fn word_item(&mut self, out: &mut Vec<u8>, depth: usize) -> Result<(), Error> {
        if self.keyword("false") {
            let spec = self.parse_spec();
            return self.emit_atom(out, Atom::Simple(20), spec);
        }
        if self.keyword("true") {
            let spec = self.parse_spec();
            return self.emit_atom(out, Atom::Simple(21), spec);
        }
        if self.keyword("null") {
            let spec = self.parse_spec();
            return self.emit_atom(out, Atom::Simple(22), spec);
        }
        if self.keyword("undefined") {
            let spec = self.parse_spec();
            return self.emit_atom(out, Atom::Simple(23), spec);
        }
        if self.keyword("Infinity") {
            let spec = self.parse_spec();
            return self.emit_atom(out, Atom::Float(f64::INFINITY), spec);
        }
        if self.keyword("NaN") {
            let spec = self.parse_spec();
            return self.emit_atom(out, Atom::Float(f64::NAN), spec);
        }
        if self.starts_with("simple") {
            let save = self.pos;
            self.pos += "simple".len();
            if self.eat("(") {
                self.skip_ws()?;
                let n = self.dec_u64()?;
                self.skip_ws()?;
                self.expect(")")?;
                let simple =
                    u8::try_from(n).map_err(|_| self.semantic("simple value must fit in u8"))?;
                if (24..=31).contains(&simple) {
                    return Err(self.semantic("simple values 24 through 31 are reserved"));
                }
                let spec = self.parse_spec();
                return self.emit_atom(out, Atom::Simple(simple), spec);
            }
            self.pos = save;
        }

        let prefix = self.app_prefix()?;
        let atom = if self.starts_with("<<") {
            let args = self.sequence_args(depth)?;
            self.apply_app_sequence(prefix, args)?
        } else if self.peek() == Some('\'') {
            let content = self.quoted_string('\'')?;
            self.apply_app_string(prefix, content)?
        } else if self.peek() == Some('`') {
            let content = self.raw_string()?;
            self.apply_app_string(prefix, content)?
        } else {
            return Err(self.syntax());
        };
        let spec = self.parse_spec();
        self.emit_atom(out, atom, spec)
    }

    fn keyword(&mut self, word: &str) -> bool {
        if !self.starts_with(word) {
            return false;
        }
        let end = self.pos + word.len();
        if self.input[end..]
            .chars()
            .next()
            .is_some_and(is_app_char_any)
        {
            return false;
        }
        self.pos = end;
        true
    }

    fn app_prefix(&mut self) -> Result<&'a str, Error> {
        let start = self.pos;
        let Some(first) = self.bump() else {
            return Err(self.syntax());
        };
        let uppercase = first.is_ascii_uppercase();
        if !(first.is_ascii_lowercase() || uppercase) {
            return Err(self.syntax());
        }

        while self.peek().is_some_and(|ch| {
            ch.is_ascii_digit() || ch == '-' || {
                if uppercase {
                    ch.is_ascii_uppercase()
                } else {
                    ch.is_ascii_lowercase()
                }
            }
        }) {
            self.bump();
        }

        Ok(&self.input[start..self.pos])
    }

    fn number_or_tag(&mut self, out: &mut Vec<u8>, depth: usize) -> Result<(), Error> {
        if self.keyword("-Infinity") {
            let spec = self.parse_spec();
            return self.emit_atom(out, Atom::Float(f64::NEG_INFINITY), spec);
        }

        let start = self.pos;
        let lex = self.number_lexeme()?;
        let unsigned_decimal_tag = is_tag_uint(lex);
        let spec = self.parse_spec();

        if unsigned_decimal_tag && self.eat("(") {
            let tag_number = lex
                .parse::<u64>()
                .map_err(|_| self.semantic("tag number must fit in u64"))?;
            let mut body = Vec::new();
            self.skip_ws()?;
            self.item(&mut body, depth)?;
            self.skip_ws()?;
            self.expect(")")?;
            self.write_len(out, 6, tag_number, spec)?;
            out.extend_from_slice(&body);
            return Ok(());
        }

        let atom = self.parse_number_atom(lex, start)?;
        self.emit_atom(out, atom, spec)
    }

    fn number_lexeme(&mut self) -> Result<&'a str, Error> {
        let start = self.pos;
        if matches!(self.peek(), Some('+' | '-')) {
            self.bump();
        }

        if self.eat("0x") || self.eat("0X") {
            let mut digits_before = 0usize;
            while self.peek().is_some_and(|ch| ch.is_ascii_hexdigit()) {
                digits_before += 1;
                self.bump();
            }
            let mut digits_after = 0usize;
            if self.eat(".") {
                while self.peek().is_some_and(|ch| ch.is_ascii_hexdigit()) {
                    digits_after += 1;
                    self.bump();
                }
            }
            if digits_before + digits_after == 0 {
                return Err(self.syntax());
            }
            if matches!(self.peek(), Some('p' | 'P')) {
                self.bump();
                if matches!(self.peek(), Some('+' | '-')) {
                    self.bump();
                }
                let exp_start = self.pos;
                while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                    self.bump();
                }
                if self.pos == exp_start {
                    return Err(self.syntax());
                }
            } else if digits_after > 0 {
                return Err(self.syntax());
            }
            return Ok(&self.input[start..self.pos]);
        }

        if self.eat("0o") || self.eat("0O") {
            let digit_start = self.pos;
            while matches!(self.peek(), Some('0'..='7')) {
                self.bump();
            }
            if self.pos == digit_start {
                return Err(self.syntax());
            }
            return Ok(&self.input[start..self.pos]);
        }

        if self.eat("0b") || self.eat("0B") {
            let digit_start = self.pos;
            while matches!(self.peek(), Some('0' | '1')) {
                self.bump();
            }
            if self.pos == digit_start {
                return Err(self.syntax());
            }
            return Ok(&self.input[start..self.pos]);
        }

        let mut digits_before = 0usize;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            digits_before += 1;
            self.bump();
        }

        let mut is_float = false;
        if self.eat(".") {
            is_float = true;
            while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.bump();
            }
        }
        if digits_before == 0 && !is_float {
            return Err(self.syntax());
        }
        if digits_before == 0 && &self.input[start..self.pos] == "." {
            return Err(self.syntax());
        }

        if matches!(self.peek(), Some('e' | 'E')) {
            is_float = true;
            self.bump();
            if matches!(self.peek(), Some('+' | '-')) {
                self.bump();
            }
            let exp_start = self.pos;
            while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.bump();
            }
            if self.pos == exp_start {
                return Err(self.syntax());
            }
        }

        if self.input[start..self.pos].ends_with('.') && !is_float {
            return Err(self.syntax());
        }
        Ok(&self.input[start..self.pos])
    }

    fn parse_number_atom(&self, lex: &str, offset: usize) -> Result<Atom, Error> {
        let (negative, digits) = strip_sign(lex);
        if let Some(digits) = digits
            .strip_prefix("0x")
            .or_else(|| digits.strip_prefix("0X"))
        {
            if lex.contains('p') || lex.contains('P') {
                return Ok(Atom::Float(parse_hex_float(lex, offset)?));
            }
            return Ok(Atom::Integer(parse_bigint_digits(
                negative, digits, 16, offset,
            )?));
        }
        if let Some(digits) = digits
            .strip_prefix("0o")
            .or_else(|| digits.strip_prefix("0O"))
        {
            return Ok(Atom::Integer(parse_bigint_digits(
                negative, digits, 8, offset,
            )?));
        }
        if let Some(digits) = digits
            .strip_prefix("0b")
            .or_else(|| digits.strip_prefix("0B"))
        {
            return Ok(Atom::Integer(parse_bigint_digits(
                negative, digits, 2, offset,
            )?));
        }

        if lex.contains('.') || lex.contains('e') || lex.contains('E') {
            let value = lex
                .parse::<f64>()
                .map_err(|_| Error::semantic(offset, format!("invalid decimal float `{lex}`")))?;
            return Ok(Atom::Float(value));
        }

        Ok(Atom::Integer(parse_bigint_digits(
            negative, digits, 10, offset,
        )?))
    }

    fn quoted_string(&mut self, quote: char) -> Result<String, Error> {
        self.expect(if quote == '"' { "\"" } else { "'" })?;
        let mut out = String::new();
        loop {
            let Some(ch) = self.bump() else {
                return Err(self.syntax());
            };
            if ch == quote {
                return Ok(out);
            }
            if ch == '\\' {
                self.escape(&mut out, quote)?;
                continue;
            }
            if ch == '\r' {
                continue;
            }
            if ch != '\n' && ch.is_control() {
                return Err(self.syntax());
            }
            out.push(ch);
        }
    }

    fn escape(&mut self, out: &mut String, quote: char) -> Result<(), Error> {
        let Some(ch) = self.bump() else {
            return Err(self.syntax());
        };
        match ch {
            '"' if quote == '"' => out.push('"'),
            '\'' if quote == '\'' => out.push('\''),
            '/' if quote == '"' => out.push('/'),
            '\\' => out.push('\\'),
            'b' => out.push('\u{08}'),
            'f' => out.push('\u{0c}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => {
                let c = self.unicode_escape(quote)?;
                out.push(c);
            }
            _ => return Err(self.syntax()),
        }
        Ok(())
    }

    fn unicode_escape(&mut self, quote: char) -> Result<char, Error> {
        let value = if self.eat("{") {
            let start = self.pos;
            while self.peek().is_some_and(|ch| ch.is_ascii_hexdigit()) {
                self.bump();
            }
            if self.pos == start {
                return Err(self.syntax());
            }
            let digits = &self.input[start..self.pos];
            self.expect("}")?;
            u32::from_str_radix(digits, 16).map_err(|_| self.syntax())?
        } else {
            let high = self.hex4()?;
            if (0xd800..=0xdbff).contains(&high) {
                self.expect("\\")?;
                if !matches!(self.bump(), Some('u')) {
                    return Err(self.syntax());
                }
                let low = self.hex4()?;
                if !(0xdc00..=0xdfff).contains(&low) {
                    return Err(self.syntax());
                }
                0x10000 + (((high - 0xd800) << 10) | (low - 0xdc00))
            } else if (0xdc00..=0xdfff).contains(&high) {
                return Err(self.syntax());
            } else {
                high
            }
        };

        if quote == '\'' && (0x20..=0x7e).contains(&value) {
            return Err(
                self.semantic("single-quoted strings cannot use \\u escapes for printable ASCII")
            );
        }
        char::from_u32(value).ok_or_else(|| self.syntax())
    }

    fn hex4(&mut self) -> Result<u32, Error> {
        let start = self.pos;
        for _ in 0..4 {
            if self.peek().is_some_and(|ch| ch.is_ascii_hexdigit()) {
                self.bump();
            } else {
                return Err(self.syntax());
            }
        }
        u32::from_str_radix(&self.input[start..self.pos], 16).map_err(|_| self.syntax())
    }

    fn raw_string(&mut self) -> Result<String, Error> {
        let start = self.pos;
        while self.eat("`") {}
        let width = self.pos - start;
        if width == 0 {
            return Err(self.syntax());
        }
        let delimiter = &self.input[start..self.pos];
        let content_start = self.pos;
        let Some(end_rel) = self.rest().find(delimiter) else {
            return Err(self.syntax());
        };
        let content_end = self.pos + end_rel;
        let mut content = &self.input[content_start..content_end];
        self.pos = content_end + width;

        if content.is_empty() {
            return Err(self.syntax());
        }
        if let Some(stripped) = content.strip_prefix("\r\n") {
            content = stripped;
        } else if let Some(stripped) = content.strip_prefix('\n') {
            content = stripped;
        } else if content.starts_with(' ') && content.ends_with(' ') && content.len() >= 2 {
            content = &content[1..content.len() - 1];
        }

        Ok(content.to_string())
    }

    fn apply_app_string(&self, prefix: &str, content: String) -> Result<Atom, Error> {
        match prefix {
            "h" => Ok(Atom::Bytes(hex_content(&content, self.pos)?)),
            "b64" => Ok(Atom::Bytes(base64_content(&content, self.pos)?)),
            "dt" => datetime_atom(&content, false, self.pos),
            "DT" => datetime_atom(&content, true, self.pos),
            "ip" => ip_atom(&content, false, self.pos),
            "IP" => ip_atom(&content, true, self.pos),
            "b1" => Ok(Atom::Bytes(content.into_bytes())),
            "t1" => Ok(Atom::Text(content)),
            "ilbs" => {
                let mut out = Vec::new();
                out.push(0x5f);
                write_definite_bytes(&mut out, content.as_bytes(), Indicator::None)?;
                out.push(0xff);
                Ok(Atom::Raw(out))
            }
            "ilts" => {
                let mut out = Vec::new();
                out.push(0x7f);
                write_definite_text(&mut out, &content, Indicator::None)?;
                out.push(0xff);
                Ok(Atom::Raw(out))
            }
            "float" => float_atom(hex_content(&content, self.pos)?, self.pos),
            _ => Err(Error::semantic(
                self.pos,
                format!("unsupported CDN application extension `{prefix}`"),
            )),
        }
    }

    fn apply_app_sequence(&self, prefix: &str, args: Vec<Arg>) -> Result<Atom, Error> {
        match prefix {
            "dt" | "DT" => {
                let content = one_text_arg(prefix, args, self.pos)?;
                datetime_atom(&content, prefix == "DT", self.pos)
            }
            "ip" | "IP" => {
                let content = one_text_arg(prefix, args, self.pos)?;
                ip_atom(&content, prefix == "IP", self.pos)
            }
            "b1" | "t1" => {
                let mut bytes = Vec::new();
                for arg in args {
                    match arg.value {
                        Value::Text(s) => bytes.extend_from_slice(s.as_bytes()),
                        Value::Bytes(b) => bytes.extend_from_slice(&b),
                        _ => {
                            return Err(Error::semantic(
                                self.pos,
                                format!("{prefix} arguments must be strings"),
                            ));
                        }
                    }
                }
                if prefix == "b1" {
                    Ok(Atom::Bytes(bytes))
                } else {
                    let text = String::from_utf8(bytes)
                        .map_err(|_| Error::semantic(self.pos, "t1 result is not UTF-8"))?;
                    Ok(Atom::Text(text))
                }
            }
            "ilbs" | "ilts" => {
                let want_major = if prefix == "ilbs" { 2 } else { 3 };
                let mut out = Vec::new();
                out.push(if want_major == 2 { 0x5f } else { 0x7f });
                for arg in args {
                    append_indefinite_string_chunk(&mut out, prefix, want_major, arg, self.pos)?;
                }
                out.push(0xff);
                Ok(Atom::Raw(out))
            }
            "float" => {
                let bytes = one_bytes_arg(prefix, args, self.pos)?;
                float_atom(bytes, self.pos)
            }
            "h" | "b64" => {
                let content = one_text_arg(prefix, args, self.pos)?;
                if prefix == "h" {
                    Ok(Atom::Bytes(hex_content(&content, self.pos)?))
                } else {
                    Ok(Atom::Bytes(base64_content(&content, self.pos)?))
                }
            }
            _ => Err(Error::semantic(
                self.pos,
                format!("unsupported CDN application extension `{prefix}`"),
            )),
        }
    }

    fn emit_atom(&self, out: &mut Vec<u8>, atom: Atom, spec: Indicator<'a>) -> Result<(), Error> {
        match atom {
            Atom::Integer(x) => self.write_integer(out, &x, spec),
            Atom::Float(x) => self.write_float(out, x, spec),
            Atom::FloatRaw { bytes, value } => {
                if spec == Indicator::None {
                    out.extend_from_slice(&bytes);
                    Ok(())
                } else {
                    self.write_float(out, value, spec)
                }
            }
            Atom::Bytes(x) => {
                if spec == Indicator::Indefinite {
                    if !x.is_empty() {
                        return Err(
                            self.semantic("only empty byte strings can use the `_` indicator")
                        );
                    }
                    out.extend_from_slice(&[0x5f, 0xff]);
                    return Ok(());
                }
                write_definite_bytes(out, &x, spec)
            }
            Atom::Text(x) => {
                if spec == Indicator::Indefinite {
                    if !x.is_empty() {
                        return Err(
                            self.semantic("only empty text strings can use the `_` indicator")
                        );
                    }
                    out.extend_from_slice(&[0x7f, 0xff]);
                    return Ok(());
                }
                write_definite_text(out, &x, spec)
            }
            Atom::Simple(x) => self.write_simple(out, x, spec),
            Atom::Raw(x) => {
                if spec != Indicator::None {
                    return Err(
                        self.semantic("encoding indicator is not supported on this literal")
                    );
                }
                out.extend_from_slice(&x);
                Ok(())
            }
        }
    }

    fn write_integer(
        &self,
        out: &mut Vec<u8>,
        int: &BigInt,
        spec: Indicator<'a>,
    ) -> Result<(), Error> {
        if int.magnitude.is_empty() {
            return self.write_len(out, 0, 0, spec);
        }

        if int.negative {
            let arg = subtract_one(&int.magnitude);
            if let Some(arg) = bytes_to_u64(&arg) {
                return self.write_len(out, 1, arg, spec);
            }
            self.write_len(out, 6, tag::BIGNEG, Indicator::None)?;
            write_definite_bytes(out, &arg, Indicator::None)
        } else if let Some(arg) = bytes_to_u64(&int.magnitude) {
            self.write_len(out, 0, arg, spec)
        } else {
            self.write_len(out, 6, tag::BIGPOS, Indicator::None)?;
            write_definite_bytes(out, &int.magnitude, Indicator::None)
        }
    }

    fn write_float(&self, out: &mut Vec<u8>, value: f64, spec: Indicator<'a>) -> Result<(), Error> {
        match spec {
            Indicator::None | Indicator::Other(..) | Indicator::Immediate => {
                let mut enc = Encoder::from(out);
                enc.push(Header::Float(value))?;
                Ok(())
            }
            Indicator::Ai(1) => {
                let bits = f64_to_f16(value)
                    .ok_or_else(|| self.semantic("float cannot be represented as binary16"))?;
                out.push(0xf9);
                out.extend_from_slice(&bits.to_be_bytes());
                Ok(())
            }
            Indicator::Ai(2) => {
                let bits = f64_to_f32_bits(value).ok_or_else(|| {
                    self.semantic("float cannot be represented exactly as binary32")
                })?;
                out.push(0xfa);
                out.extend_from_slice(&bits.to_be_bytes());
                Ok(())
            }
            Indicator::Ai(3) => {
                out.push(0xfb);
                out.extend_from_slice(&value.to_bits().to_be_bytes());
                Ok(())
            }
            Indicator::Ai(0) => Err(self.semantic("float cannot use `_0` encoding indicator")),
            Indicator::Ai(_) => Err(self.semantic("unsupported float encoding indicator")),
            Indicator::Indefinite => {
                let mut enc = Encoder::from(out);
                enc.push(Header::Float(value))?;
                Ok(())
            }
        }
    }

    fn write_simple(&self, out: &mut Vec<u8>, value: u8, spec: Indicator<'a>) -> Result<(), Error> {
        match spec {
            Indicator::None | Indicator::Other(..) | Indicator::Immediate => {
                let mut enc = Encoder::from(out);
                enc.push(Header::Simple(value))?;
                Ok(())
            }
            Indicator::Ai(0) if value >= 32 => {
                out.extend_from_slice(&[0xf8, value]);
                Ok(())
            }
            Indicator::Ai(0) => {
                Err(self
                    .semantic("two-byte encodings of simple values below 32 are not well-formed"))
            }
            Indicator::Ai(..) => Err(self.semantic("simple values only support `_i` or `_0`")),
            Indicator::Indefinite => {
                let mut enc = Encoder::from(out);
                enc.push(Header::Simple(value))?;
                Ok(())
            }
        }
    }

    fn write_len(
        &self,
        out: &mut Vec<u8>,
        major: u8,
        value: u64,
        spec: Indicator<'a>,
    ) -> Result<(), Error> {
        write_uint(out, major, value, spec)
            .map_err(|msg| Error::semantic(self.pos, msg.to_string()))
    }

    fn dec_u64(&mut self) -> Result<u64, Error> {
        let start = self.pos;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.bump();
        }
        if self.pos == start {
            return Err(self.syntax());
        }
        self.input[start..self.pos]
            .parse()
            .map_err(|_| self.semantic("integer must fit in u64"))
    }
}

fn is_app_char_any(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '-'
}

fn is_tag_uint(s: &str) -> bool {
    if s == "0" {
        return true;
    }
    let mut chars = s.chars();
    matches!(chars.next(), Some('1'..='9')) && chars.all(|ch| ch.is_ascii_digit())
}

fn strip_sign(s: &str) -> (bool, &str) {
    if let Some(rest) = s.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = s.strip_prefix('+') {
        (false, rest)
    } else {
        (false, s)
    }
}

fn parse_bigint_digits(
    negative: bool,
    digits: &str,
    base: u32,
    offset: usize,
) -> Result<BigInt, Error> {
    let mut magnitude = Vec::new();
    for ch in digits.chars() {
        let digit = ch.to_digit(base).ok_or(Error::Syntax(offset))?;
        mul_add(&mut magnitude, base, digit);
    }
    strip_leading_zeroes(&mut magnitude);
    Ok(BigInt {
        negative: negative && !magnitude.is_empty(),
        magnitude,
    })
}

fn mul_add(bytes: &mut Vec<u8>, base: u32, digit: u32) {
    let mut carry = digit;
    for byte in bytes.iter_mut().rev() {
        let x = u32::from(*byte) * base + carry;
        *byte = x as u8;
        carry = x >> 8;
    }
    while carry > 0 {
        bytes.insert(0, carry as u8);
        carry >>= 8;
    }
}

fn strip_leading_zeroes(bytes: &mut Vec<u8>) {
    let keep = bytes
        .iter()
        .position(|&byte| byte != 0)
        .unwrap_or(bytes.len());
    if keep > 0 {
        bytes.drain(..keep);
    }
}

fn bytes_to_u64(bytes: &[u8]) -> Option<u64> {
    if bytes.len() > 8 {
        return None;
    }
    let mut value = 0u64;
    for &byte in bytes {
        value = (value << 8) | u64::from(byte);
    }
    Some(value)
}

fn subtract_one(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    for byte in out.iter_mut().rev() {
        let (next, borrow) = byte.overflowing_sub(1);
        *byte = next;
        if !borrow {
            break;
        }
    }
    strip_leading_zeroes(&mut out);
    out
}

fn parse_hex_float(lex: &str, offset: usize) -> Result<f64, Error> {
    let (negative, rest) = strip_sign(lex);
    let rest = rest
        .strip_prefix("0x")
        .or_else(|| rest.strip_prefix("0X"))
        .ok_or(Error::Syntax(offset))?;
    let (mantissa, exp) = rest.split_once(['p', 'P']).ok_or(Error::Syntax(offset))?;
    let exponent: i32 = exp
        .parse()
        .map_err(|_| Error::semantic(offset, format!("invalid hex float exponent `{exp}`")))?;

    let mut value = 0.0f64;
    let mut frac_digits = 0i32;
    let mut after_point = false;
    for ch in mantissa.chars() {
        if ch == '.' {
            if after_point {
                return Err(Error::Syntax(offset));
            }
            after_point = true;
            continue;
        }
        let digit = ch.to_digit(16).ok_or(Error::Syntax(offset))?;
        value = value * 16.0 + f64::from(digit);
        if after_point {
            frac_digits += 1;
        }
    }

    value *= exp2(exponent - 4 * frac_digits);
    if negative {
        value = -value;
    }
    Ok(value)
}

fn exp2(n: i32) -> f64 {
    if n < -1074 {
        return 0.0;
    }
    if n > 1023 {
        return f64::INFINITY;
    }
    if n >= -1022 {
        f64::from_bits(((n + 1023) as u64) << 52)
    } else {
        f64::from_bits(1u64 << (n + 1074))
    }
}

fn f64_to_f32_bits(value: f64) -> Option<u32> {
    if value.is_nan() {
        let sign = ((value.to_bits() >> 32) & 0x8000_0000) as u32;
        return Some(sign | 0x7fc0_0000);
    }
    let f = value as f32;
    if (f as f64).to_bits() == value.to_bits() {
        Some(f.to_bits())
    } else {
        None
    }
}

fn write_uint(
    out: &mut Vec<u8>,
    major: u8,
    value: u64,
    spec: Indicator<'_>,
) -> Result<(), &'static str> {
    let prefix = major << 5;
    match spec {
        Indicator::None | Indicator::Other(..) | Indicator::Indefinite => {
            write_preferred_uint(out, major, value);
        }
        Indicator::Immediate => {
            if value > 23 {
                return Err("value does not fit `_i` encoding indicator");
            }
            out.push(prefix | value as u8);
        }
        Indicator::Ai(0) => {
            let value =
                u8::try_from(value).map_err(|_| "value does not fit `_0` encoding indicator")?;
            out.extend_from_slice(&[prefix | 24, value]);
        }
        Indicator::Ai(1) => {
            let value =
                u16::try_from(value).map_err(|_| "value does not fit `_1` encoding indicator")?;
            out.push(prefix | 25);
            out.extend_from_slice(&value.to_be_bytes());
        }
        Indicator::Ai(2) => {
            let value =
                u32::try_from(value).map_err(|_| "value does not fit `_2` encoding indicator")?;
            out.push(prefix | 26);
            out.extend_from_slice(&value.to_be_bytes());
        }
        Indicator::Ai(3) => {
            out.push(prefix | 27);
            out.extend_from_slice(&value.to_be_bytes());
        }
        Indicator::Ai(_) => return Err("unsupported encoding indicator"),
    }
    Ok(())
}

fn write_preferred_uint(out: &mut Vec<u8>, major: u8, value: u64) {
    let prefix = major << 5;
    match value {
        x if x <= 23 => out.push(prefix | x as u8),
        x if x <= u8::MAX as u64 => out.extend_from_slice(&[prefix | 24, x as u8]),
        x if x <= u16::MAX as u64 => {
            out.push(prefix | 25);
            out.extend_from_slice(&(x as u16).to_be_bytes());
        }
        x if x <= u32::MAX as u64 => {
            out.push(prefix | 26);
            out.extend_from_slice(&(x as u32).to_be_bytes());
        }
        x => {
            out.push(prefix | 27);
            out.extend_from_slice(&x.to_be_bytes());
        }
    }
}

fn write_definite_bytes(out: &mut Vec<u8>, bytes: &[u8], spec: Indicator<'_>) -> Result<(), Error> {
    write_uint(out, 2, bytes.len() as u64, spec).map_err(|msg| Error::semantic(None, msg))?;
    out.extend_from_slice(bytes);
    Ok(())
}

fn write_definite_text(out: &mut Vec<u8>, text: &str, spec: Indicator<'_>) -> Result<(), Error> {
    write_uint(out, 3, text.len() as u64, spec).map_err(|msg| Error::semantic(None, msg))?;
    out.extend_from_slice(text.as_bytes());
    Ok(())
}

fn hex_content(content: &str, offset: usize) -> Result<Vec<u8>, Error> {
    let mut parser = HexContent {
        input: content,
        pos: 0,
        base_offset: offset.saturating_sub(content.len()),
    };
    parser.parse()
}

struct HexContent<'a> {
    input: &'a str,
    pos: usize,
    base_offset: usize,
}

impl HexContent<'_> {
    fn syntax(&self) -> Error {
        Error::Syntax(self.base_offset + self.pos)
    }

    fn rest(&self) -> &str {
        &self.input[self.pos..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += ch.len_utf8();
        Some(ch)
    }

    fn eat(&mut self, s: &str) -> bool {
        if self.rest().starts_with(s) {
            self.pos += s.len();
            true
        } else {
            false
        }
    }

    fn skip(&mut self) -> Result<(), Error> {
        loop {
            let before = self.pos;
            while matches!(self.peek(), Some('\n' | ' ')) {
                self.bump();
            }
            if self.eat("//") || self.eat("#") {
                while let Some(ch) = self.bump() {
                    if ch == '\n' {
                        break;
                    }
                }
                continue;
            }
            if self.eat("/*") {
                let Some(end) = self.rest().find("*/") else {
                    return Err(self.syntax());
                };
                self.pos += end + 2;
                continue;
            }
            if self.rest().starts_with('/') {
                self.bump();
                let Some(end) = self.rest().find('/') else {
                    return Err(self.syntax());
                };
                self.pos += end + 1;
                continue;
            }
            if before == self.pos {
                return Ok(());
            }
        }
    }

    fn parse(&mut self) -> Result<Vec<u8>, Error> {
        let mut nibbles = Vec::new();
        loop {
            self.skip()?;
            if self.pos == self.input.len() {
                break;
            }
            if self.rest().starts_with("...") {
                return Err(Error::semantic(
                    self.base_offset + self.pos,
                    "CDN ellipses are not supported",
                ));
            }
            let ch = self.bump().ok_or_else(|| self.syntax())?;
            let Some(digit) = ch.to_digit(16) else {
                return Err(self.syntax());
            };
            nibbles.push(digit as u8);
        }
        if nibbles.len() % 2 != 0 {
            return Err(self.syntax());
        }
        let mut out = Vec::with_capacity(nibbles.len() / 2);
        for pair in nibbles.chunks_exact(2) {
            out.push((pair[0] << 4) | pair[1]);
        }
        Ok(out)
    }
}

fn base64_content(content: &str, offset: usize) -> Result<Vec<u8>, Error> {
    let mut filtered = Vec::new();
    let mut chars = content.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        match ch {
            '\n' | ' ' => {}
            '#' => {
                for (_, next) in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
            }
            '=' => filtered.push(64),
            _ => {
                let Some(value) = b64_value(ch) else {
                    return Err(Error::Syntax(offset.saturating_sub(content.len()) + idx));
                };
                filtered.push(value);
            }
        }
    }

    if let Some(first_pad) = filtered.iter().position(|&value| value == 64) {
        if filtered[first_pad..].iter().any(|&value| value != 64) {
            return Err(Error::semantic(offset, "invalid base64 padding"));
        }
        let pad_count = filtered.len() - first_pad;
        if pad_count > 2 || filtered.len() % 4 != 0 {
            return Err(Error::semantic(offset, "invalid base64 padding"));
        }
        match (first_pad % 4, pad_count) {
            (2, 2) | (3, 1) => filtered.truncate(first_pad),
            _ => return Err(Error::semantic(offset, "invalid base64 padding")),
        }
    } else if filtered.len() % 4 == 1 {
        return Err(Error::semantic(offset, "invalid base64 length"));
    }

    let mut out = Vec::new();
    for chunk in filtered.chunks(4) {
        let a = chunk[0];
        let b = *chunk
            .get(1)
            .ok_or_else(|| Error::semantic(offset, "invalid base64"))?;
        if a == 64 || b == 64 {
            return Err(Error::semantic(offset, "invalid base64 padding"));
        }
        out.push((a << 2) | (b >> 4));
        if let Some(&c) = chunk.get(2) {
            if c == 64 {
                continue;
            }
            out.push((b << 4) | (c >> 2));
            if let Some(&d) = chunk.get(3) {
                if d == 64 {
                    continue;
                }
                out.push((c << 6) | d);
            }
        }
    }
    Ok(out)
}

fn append_indefinite_string_chunk(
    out: &mut Vec<u8>,
    prefix: &str,
    want_major: u8,
    arg: Arg,
    offset: usize,
) -> Result<(), Error> {
    let Some(&head) = arg.encoded.first() else {
        return Err(Error::Syntax(offset));
    };
    let source_major = head >> 5;
    if !matches!(source_major, 2 | 3) || (head & 0x1f) == 31 {
        return Err(Error::semantic(
            offset,
            format!("{prefix} arguments must be definite strings"),
        ));
    }

    if want_major == 3 {
        match &arg.value {
            Value::Text(..) => {}
            Value::Bytes(bytes) => {
                core::str::from_utf8(bytes)
                    .map_err(|_| Error::semantic(offset, "ilts argument is not UTF-8"))?;
            }
            _ => unreachable!("string major type decoded as non-string value"),
        }
    }

    let mut encoded = arg.encoded;
    encoded[0] = (want_major << 5) | (head & 0x1f);
    out.extend_from_slice(&encoded);
    Ok(())
}

fn b64_value(ch: char) -> Option<u8> {
    match ch {
        'A'..='Z' => Some(ch as u8 - b'A'),
        'a'..='z' => Some(ch as u8 - b'a' + 26),
        '0'..='9' => Some(ch as u8 - b'0' + 52),
        '+' | '-' => Some(62),
        '/' | '_' => Some(63),
        _ => None,
    }
}

fn one_text_arg(prefix: &str, args: Vec<Arg>, offset: usize) -> Result<String, Error> {
    if args.len() != 1 {
        return Err(Error::semantic(
            offset,
            format!("{prefix} expects exactly one argument"),
        ));
    }
    match args.into_iter().next().unwrap().value {
        Value::Text(s) => Ok(s),
        Value::Bytes(b) => String::from_utf8(b)
            .map_err(|_| Error::semantic(offset, format!("{prefix} argument is not UTF-8"))),
        _ => Err(Error::semantic(
            offset,
            format!("{prefix} argument must be a string"),
        )),
    }
}

fn one_bytes_arg(prefix: &str, args: Vec<Arg>, offset: usize) -> Result<Vec<u8>, Error> {
    if args.len() != 1 {
        return Err(Error::semantic(
            offset,
            format!("{prefix} expects exactly one argument"),
        ));
    }
    match args.into_iter().next().unwrap().value {
        Value::Text(s) => Ok(s.into_bytes()),
        Value::Bytes(b) => Ok(b),
        _ => Err(Error::semantic(
            offset,
            format!("{prefix} argument must be a string"),
        )),
    }
}

fn datetime_atom(content: &str, tagged: bool, offset: usize) -> Result<Atom, Error> {
    let (seconds, fractional) = parse_datetime(content, offset)?;
    let atom = if let Some(frac) = fractional {
        Atom::Float(seconds as f64 + frac)
    } else {
        Atom::Integer(BigInt::from_i128(seconds))
    };
    if !tagged {
        return Ok(atom);
    }

    let mut inner = Vec::new();
    Parser::new("").emit_atom(&mut inner, atom, Indicator::None)?;
    let mut out = Vec::new();
    write_uint(&mut out, 6, 1, Indicator::None).map_err(|msg| Error::semantic(offset, msg))?;
    out.extend_from_slice(&inner);
    Ok(Atom::Raw(out))
}

impl BigInt {
    fn from_i128(value: i128) -> Self {
        let negative = value < 0;
        let magnitude = if negative {
            value.unsigned_abs()
        } else {
            value as u128
        };
        let mut bytes = magnitude.to_be_bytes().to_vec();
        strip_leading_zeroes(&mut bytes);
        Self {
            negative: negative && !bytes.is_empty(),
            magnitude: bytes,
        }
    }
}

fn parse_datetime(content: &str, offset: usize) -> Result<(i128, Option<f64>), Error> {
    let b = content.as_bytes();
    if b.len() < 20 {
        return Err(Error::Syntax(offset));
    }
    let year = dec_fixed(b, 0, 4, offset)? as i32;
    expect_byte(b, 4, b'-', offset)?;
    let month = dec_fixed(b, 5, 2, offset)?;
    expect_byte(b, 7, b'-', offset)?;
    let day = dec_fixed(b, 8, 2, offset)?;
    expect_byte(b, 10, b'T', offset)?;
    let hour = dec_fixed(b, 11, 2, offset)?;
    expect_byte(b, 13, b':', offset)?;
    let minute = dec_fixed(b, 14, 2, offset)?;
    expect_byte(b, 16, b':', offset)?;
    let second = dec_fixed(b, 17, 2, offset)?;
    let mut pos = 19usize;

    let fractional = if b.get(pos) == Some(&b'.') {
        pos += 1;
        let start = pos;
        while b.get(pos).is_some_and(u8::is_ascii_digit) {
            pos += 1;
        }
        if pos == start {
            return Err(Error::Syntax(offset + pos));
        }
        let mut frac = 0.0;
        let mut scale = 1.0;
        for &digit in &b[start..pos] {
            scale *= 10.0;
            frac += f64::from(digit - b'0') / scale;
        }
        Some(frac)
    } else {
        None
    };

    let offset_seconds = match b.get(pos).copied() {
        Some(b'Z') => {
            pos += 1;
            0i32
        }
        Some(sign @ (b'+' | b'-')) => {
            let off_hour = dec_fixed(b, pos + 1, 2, offset)? as i32;
            expect_byte(b, pos + 3, b':', offset)?;
            let off_min = dec_fixed(b, pos + 4, 2, offset)? as i32;
            pos += 6;
            if off_hour > 23 || off_min > 59 {
                return Err(Error::Syntax(offset + pos));
            }
            let value = off_hour * 3600 + off_min * 60;
            if sign == b'+' {
                value
            } else {
                -value
            }
        }
        _ => return Err(Error::Syntax(offset + pos)),
    };
    if pos != b.len() {
        return Err(Error::Syntax(offset + pos));
    }

    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return Err(Error::Syntax(offset));
    }

    let days = days_from_civil(year, month, day);
    let seconds = i128::from(days) * 86_400
        + i128::from(hour) * 3600
        + i128::from(minute) * 60
        + i128::from(second)
        - i128::from(offset_seconds);
    Ok((seconds, fractional))
}

fn dec_fixed(bytes: &[u8], start: usize, len: usize, offset: usize) -> Result<u32, Error> {
    let end = start + len;
    if end > bytes.len() {
        return Err(Error::Syntax(offset + start));
    }
    let mut value = 0u32;
    for (idx, &b) in bytes[start..end].iter().enumerate() {
        if !b.is_ascii_digit() {
            return Err(Error::Syntax(offset + start + idx));
        }
        value = value * 10 + u32::from(b - b'0');
    }
    Ok(value)
}

fn expect_byte(bytes: &[u8], idx: usize, expected: u8, offset: usize) -> Result<(), Error> {
    if bytes.get(idx) == Some(&expected) {
        Ok(())
    } else {
        Err(Error::Syntax(offset + idx))
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i32;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    i64::from(era * 146097 + doe - 719468)
}

fn ip_atom(content: &str, tagged: bool, offset: usize) -> Result<Atom, Error> {
    let (addr, prefix) = match content.split_once('/') {
        Some((addr, prefix)) => {
            if prefix.is_empty() || !prefix.bytes().all(|b| b.is_ascii_digit()) {
                return Err(Error::Syntax(offset));
            }
            let prefix = prefix
                .parse::<u8>()
                .map_err(|_| Error::Syntax(offset + addr.len() + 1))?;
            (addr, Some(prefix))
        }
        None => (content, None),
    };

    let parsed = if addr.contains(':') {
        IpAddr::V6(parse_ipv6(addr, offset)?)
    } else {
        IpAddr::V4(parse_ipv4(addr, offset)?)
    };

    let (tag_number, max_prefix, bytes) = match parsed {
        IpAddr::V4(bytes) => (52, 32, bytes.to_vec()),
        IpAddr::V6(bytes) => (54, 128, bytes.to_vec()),
    };

    let raw = if let Some(prefix) = prefix {
        if prefix > max_prefix {
            return Err(Error::semantic(offset, "IP prefix length is out of range"));
        }
        let mut prefix_bytes = mask_prefix(bytes, prefix);
        while prefix_bytes.last() == Some(&0) {
            prefix_bytes.pop();
        }
        let mut array = Vec::new();
        write_uint(&mut array, 4, 2, Indicator::None)
            .map_err(|msg| Error::semantic(offset, msg))?;
        write_uint(&mut array, 0, u64::from(prefix), Indicator::None)
            .map_err(|msg| Error::semantic(offset, msg))?;
        write_definite_bytes(&mut array, &prefix_bytes, Indicator::None)?;
        array
    } else {
        let mut out = Vec::new();
        write_definite_bytes(&mut out, &bytes, Indicator::None)?;
        if !tagged {
            return Ok(Atom::Bytes(bytes));
        }
        out
    };

    if tagged {
        let mut out = Vec::new();
        write_uint(&mut out, 6, tag_number, Indicator::None)
            .map_err(|msg| Error::semantic(offset, msg))?;
        out.extend_from_slice(&raw);
        Ok(Atom::Raw(out))
    } else if prefix.is_some() {
        Ok(Atom::Raw(raw))
    } else {
        unreachable!()
    }
}

enum IpAddr {
    V4([u8; 4]),
    V6([u8; 16]),
}

fn parse_ipv4(input: &str, offset: usize) -> Result<[u8; 4], Error> {
    let mut out = [0u8; 4];
    let mut count = 0usize;
    for part in input.split('.') {
        if count == 4 || part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::Syntax(offset));
        }
        let value = part.parse::<u16>().map_err(|_| Error::Syntax(offset))?;
        if value > 255 {
            return Err(Error::Syntax(offset));
        }
        out[count] = value as u8;
        count += 1;
    }
    if count == 4 {
        Ok(out)
    } else {
        Err(Error::Syntax(offset))
    }
}

fn parse_ipv6(input: &str, offset: usize) -> Result<[u8; 16], Error> {
    if input.is_empty() {
        return Err(Error::Syntax(offset));
    }
    let (left, right, compressed) = match input.split_once("::") {
        Some((left, right)) => {
            if right.contains("::") {
                return Err(Error::Syntax(offset));
            }
            (left, right, true)
        }
        None => (input, "", false),
    };

    let mut groups = Vec::new();
    parse_ipv6_side(left, !compressed && right.is_empty(), offset, &mut groups)?;
    let left_len = groups.len();
    if compressed {
        let mut right_groups = Vec::new();
        parse_ipv6_side(
            right,
            true,
            offset + input.len() - right.len(),
            &mut right_groups,
        )?;
        if left_len + right_groups.len() > 7 {
            return Err(Error::Syntax(offset));
        }
        let zeroes = 8 - left_len - right_groups.len();
        groups.extend(core::iter::repeat_n(0, zeroes));
        groups.extend(right_groups);
    }

    if groups.len() != 8 {
        return Err(Error::Syntax(offset));
    }

    let mut out = [0u8; 16];
    for (idx, group) in groups.into_iter().enumerate() {
        out[idx * 2..idx * 2 + 2].copy_from_slice(&group.to_be_bytes());
    }
    Ok(out)
}

fn parse_ipv6_side(
    side: &str,
    allow_ipv4_tail: bool,
    offset: usize,
    groups: &mut Vec<u16>,
) -> Result<(), Error> {
    if side.is_empty() {
        return Ok(());
    }
    for (idx, part) in side.split(':').enumerate() {
        if part.is_empty() {
            return Err(Error::Syntax(offset));
        }
        if part.contains('.') {
            if !allow_ipv4_tail || idx + 1 != side.split(':').count() {
                return Err(Error::Syntax(offset));
            }
            let v4 = parse_ipv4(part, offset)?;
            groups.push(u16::from_be_bytes([v4[0], v4[1]]));
            groups.push(u16::from_be_bytes([v4[2], v4[3]]));
            continue;
        }
        if part.len() > 4 || !part.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::Syntax(offset));
        }
        groups.push(u16::from_str_radix(part, 16).map_err(|_| Error::Syntax(offset))?);
    }
    Ok(())
}

fn mask_prefix(mut bytes: Vec<u8>, prefix: u8) -> Vec<u8> {
    let full = usize::from(prefix / 8);
    let rem = prefix % 8;
    if rem == 0 {
        bytes.truncate(full);
    } else {
        bytes.truncate(full + 1);
        let mask = 0xff << (8 - rem);
        if let Some(last) = bytes.last_mut() {
            *last &= mask;
        }
    }
    bytes
}

fn float_atom(bytes: Vec<u8>, offset: usize) -> Result<Atom, Error> {
    let value = match bytes.as_slice() {
        [a, b] => f16_to_f64(u16::from_be_bytes([*a, *b])),
        [a, b, c, d] => f32::from_bits(u32::from_be_bytes([*a, *b, *c, *d])) as f64,
        [a, b, c, d, e, f, g, h] => {
            f64::from_bits(u64::from_be_bytes([*a, *b, *c, *d, *e, *f, *g, *h]))
        }
        _ => {
            return Err(Error::semantic(
                offset,
                "float extension requires 2, 4, or 8 bytes",
            ));
        }
    };

    let mut raw = Vec::new();
    raw.push(match bytes.len() {
        2 => 0xf9,
        4 => 0xfa,
        _ => 0xfb,
    });
    raw.extend_from_slice(&bytes);
    Ok(Atom::FloatRaw { bytes: raw, value })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(input: &str) -> String {
        let bytes = cdn_to_vec(input).unwrap();
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn parses_json_superset_core_items() {
        assert_eq!(hex("0"), "00");
        assert_eq!(hex("-0"), "00");
        assert_eq!(hex("+0x1267"), "191267");
        assert_eq!(hex("0o11147"), "191267");
        assert_eq!(hex("0b1001001100111"), "191267");
        assert_eq!(hex("-18446744073709551617"), "c349010000000000000000");
        assert_eq!(hex("1.5"), "f93e00");
        assert_eq!(hex("0x1.8p0"), "f93e00");
        assert_eq!(hex("0x18p-4"), "f93e00");
        assert_eq!(hex("Infinity"), "f97c00");
        assert_eq!(hex("-Infinity"), "f9fc00");
        assert_eq!(hex("NaN"), "f97e00");
        assert_eq!(hex("false"), "f4");
        assert_eq!(hex("undefined"), "f7");
        assert_eq!(hex("simple(59)"), "f83b");
    }

    #[test]
    fn parses_strings_comments_and_containers() {
        assert_eq!(
            hex(r#""D\u{6f}mino's \uD83C\uDC73""#),
            "6d446f6d696e6f277320f09f81b3"
        );
        assert_eq!(hex(r#"'hello world'"#), "4b68656c6c6f20776f726c64");
        assert_eq!(hex("`raw\\\\text`"), "697261775c5c74657874");
        assert_eq!(hex("``\n`leading``"), "68606c656164696e67");
        assert_eq!(
            hex(r#"{
                    /kty/ 1: 4 # symmetric
                    "k": h'66 84 52 3a'
                }"#),
            "a20104616b446684523a"
        );
        assert_eq!(hex("[1 2, 3,]"), "83010203");
        assert_eq!(hex("{_ 1: 2,}"), "bf0102ff");
    }

    #[test]
    fn parses_base_encodings_and_embedded_sequences() {
        assert_eq!(hex("h'68 65 6c /comment/ 6c 6f'"), "4568656c6c6f");
        assert_eq!(hex("b64'SGVsbG8'"), "4548656c6c6f");
        assert_eq!(hex("b64'SGV sbG8='"), "4548656c6c6f");
        assert_eq!(hex("b64'AA=='"), "4100");
        assert_eq!(hex("b64'AAA='"), "420000");
        assert_eq!(hex("<<1, 2>>"), "420102");
        assert_eq!(hex("[<<\"hello\", null>>]"), "81476568656c6c6ff6");
    }

    #[test]
    fn honors_encoding_indicators() {
        assert_eq!(hex("1_i"), "01");
        assert_eq!(hex("1_0"), "1801");
        assert_eq!(hex("1_1"), "190001");
        assert_eq!(hex("0x4711_3"), "1b0000000000004711");
        assert_eq!(hex("'A'_1"), "59000141");
        assert_eq!(hex(r#""A"_1"#), "79000141");
        assert_eq!(hex("[_0 false, true]"), "9802f4f5");
        assert_eq!(hex("{_1 \"bar\": 1}"), "b900016362617201");
        assert_eq!(hex("1_1(4711)"), "d90001191267");
        assert_eq!(hex("1.5_2"), "fa3fc00000");
        assert_eq!(hex("Infinity_3"), "fb7ff0000000000000");
        assert_eq!(hex("''_"), "5fff");
        assert_eq!(hex("\"\"_"), "7fff");
    }

    #[test]
    fn parses_application_extensions() {
        assert_eq!(hex("dt'1969-07-21T02:56:16Z'"), "3a00d80caf");
        assert_eq!(hex("DT'1969-07-21T02:56:16Z'"), "c13a00d80caf");
        assert_eq!(hex("ip'192.0.2.42'"), "44c000022a");
        assert_eq!(hex("IP'192.0.2.42'"), "d83444c000022a");
        assert_eq!(hex("IP'192.0.2.0/24'"), "d83482181843c00002");
        assert_eq!(
            hex("IP'2001:db8::42'"),
            "d8365020010db8000000000000000000000042"
        );
        assert_eq!(hex("IP'2001:db8::/64'"), "d8368218404420010db8");
        assert_eq!(
            hex(r#"t1<<"Hello", h'20', "world">>"#),
            "6b48656c6c6f20776f726c64"
        );
        assert_eq!(
            hex(r#"b1<<"Hello", h'20', "world">>"#),
            "4b48656c6c6f20776f726c64"
        );
        assert_eq!(
            hex(r#"ilbs<<'Hello '_0, 'world'>>"#),
            "5f580648656c6c6f2045776f726c64ff"
        );
        assert_eq!(
            hex(r#"ilbs<<"Hello world">>"#),
            "5f4b48656c6c6f20776f726c64ff"
        );
        assert_eq!(
            hex(r#"ilts<<'Hello '_0, h'776f726c64'>>"#),
            "7f780648656c6c6f2065776f726c64ff"
        );
        assert_eq!(hex("float'fe00'"), "f9fe00");
        assert_eq!(hex("float'fe00'_2"), "faffc00000");
    }

    #[test]
    fn parses_sequences_and_deserializes() {
        assert_eq!(
            cdn_sequence_to_vec("1 {\"two\": 2}").unwrap(),
            Vec::from([0x01, 0xa1, 0x63, b't', b'w', b'o', 0x02])
        );
        let value: Value = from_cdn("{1: [2, 3]}").unwrap();
        assert_eq!(value.to_string(), "{1: [2, 3]}");
    }

    #[test]
    fn rejects_invalid_cdn() {
        for input in [
            "",
            "[1[]]",
            "h'0'",
            "'\\u{41}'",
            "simple(24)",
            "1.1_1",
            "float'00'",
            "unknown'foo'",
            "b64'AA=A'",
            "b64'AA='",
            "b64'AAA=='",
            "b64'===='",
            "b64'SG=V'",
            "ilts<<h'ff'>>",
            "...",
        ] {
            assert!(cdn_to_vec(input).is_err(), "{input}");
        }
    }
}
