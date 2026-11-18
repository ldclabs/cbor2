use alloc::{
    borrow::Cow,
    format,
    string::{String, ToString},
    vec::Vec,
};

use crate::core::{f64_to_f32, tag, Encoder, Header};
use crate::de::{Error, DEFAULT_RECURSION_LIMIT};

use super::applications::{
    append_indefinite_string_chunk, base64_content, concat_app_strings, concat_bytes, hex_atom,
    one_bytes_arg, one_text_arg, same_args, unresolved_app_sequence,
};
#[cfg(feature = "cdn-cri")]
use super::cri::cri_atom;
use super::datetime::datetime_atom;
use super::encode::{ellipsis_item, write_definite_bytes, write_definite_text, write_uint};
use super::float::{f64_to_f16_preserving, float_atom};
#[cfg(feature = "cdn-hash")]
use super::hash::{hash_args, hash_atom};
use super::ip::ip_atom;
use super::number::{is_app_char_any, is_tag_uint, parse_hex_float, parse_integer, strip_sign};
use super::types::{Arg, Atom, Indicator};

pub(super) fn item_to_vec(input: &str) -> Result<Vec<u8>, Error> {
    normalized(input, |input| {
        let mut parser = Parser::new(input);
        let mut out = Vec::new();
        parser.skip_ws()?;
        parser.item(&mut out, DEFAULT_RECURSION_LIMIT)?;
        parser.skip_ws()?;
        if !parser.eof() {
            return Err(parser.syntax());
        }
        Ok(out)
    })
}

pub(super) fn sequence_to_vec(input: &str) -> Result<Vec<u8>, Error> {
    normalized(input, |input| {
        Parser::new(input).sequence_to_vec(None, DEFAULT_RECURSION_LIMIT)
    })
}

// CR is ignored before tokenization, while diagnostic offsets still name
// bytes of the original source. An escaped \r is not removed here.
fn normalized(
    input: &str,
    parse: impl FnOnce(&str) -> Result<Vec<u8>, Error>,
) -> Result<Vec<u8>, Error> {
    let text = if input.contains('\r') {
        Cow::Owned(input.replace('\r', ""))
    } else {
        Cow::Borrowed(input)
    };
    parse(&text).map_err(|error| {
        if matches!(text, Cow::Borrowed(_)) {
            return error;
        }
        let offset = |pos: usize| {
            input
                .bytes()
                .enumerate()
                .filter(|(_, byte)| *byte != b'\r')
                .nth(pos)
                .map_or(input.len(), |(i, _)| i)
        };
        match error {
            Error::Syntax(pos) => Error::Syntax(offset(pos)),
            Error::Semantic(Some(pos), message) => Error::Semantic(Some(offset(pos)), message),
            other => other,
        }
    })
}

pub(super) struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    pub(super) fn new(input: &'a str) -> Self {
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
            while matches!(self.peek(), Some('\t' | '\n' | ' ')) {
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

    // Finishes one list entry: an optional comma, then `close` (end of input
    // for `None`). Returns true once the list is closed; open entries must be
    // separated by a comma or by blank space and comments.
    fn list_end(&mut self, close: Option<&str>) -> Result<bool, Error> {
        let spaced = self.consume_ws()?;
        let comma = self.eat(",");
        if comma {
            self.skip_ws()?;
        }
        let closed = match close {
            Some(close) => self.eat(close),
            None => self.eof(),
        };
        if closed || comma || spaced {
            Ok(closed)
        } else {
            Err(self.syntax())
        }
    }

    fn sequence_to_vec(&mut self, end: Option<&str>, depth: usize) -> Result<Vec<u8>, Error> {
        let mut out = Vec::new();
        self.skip_ws()?;
        let empty = match end {
            Some(end) => self.eat(end),
            None => self.eof(),
        };
        if !empty {
            loop {
                self.item(&mut out, depth)?;
                if self.list_end(end)? {
                    break;
                }
            }
        }
        Ok(out)
    }

    fn sequence_args(&mut self, depth: usize) -> Result<Vec<Arg>, Error> {
        self.expect("<<")?;
        let mut args = Vec::new();
        self.skip_ws()?;
        if !self.eat(">>") {
            loop {
                let mut encoded = Vec::new();
                self.item(&mut encoded, depth)?;
                args.push(Arg::Encoded(encoded));
                if self.list_end(Some(">>"))? {
                    break;
                }
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
            '.' if self.starts_with("...") => {
                self.ellipsis();
                self.emit_atom(out, Atom::Raw(ellipsis_item()), Indicator::None)
            }
            '+' | '-' | '.' | '0'..='9' => self.number_or_tag(out, depth - 1),
            'A'..='Z' | 'a'..='z' => self.word_item(out, depth - 1),
            _ => Err(self.syntax()),
        }
    }

    fn ellipsis(&mut self) {
        while self.eat(".") {}
    }

    fn array(&mut self, out: &mut Vec<u8>, depth: usize) -> Result<(), Error> {
        self.expect("[")?;
        self.container(out, 4, "]", |parser, body| parser.item(body, depth))
    }

    fn map(&mut self, out: &mut Vec<u8>, depth: usize) -> Result<(), Error> {
        self.expect("{")?;
        self.container(out, 5, "}", |parser, body| {
            parser.item(body, depth)?;
            parser.skip_ws()?;
            parser.expect(":")?;
            parser.item(body, depth)
        })
    }

    // Parses the entries of an array or map after its opening bracket. An
    // indefinite container streams into `out`; a definite one buffers its
    // entries until their count, and so the header, is known.
    fn container(
        &mut self,
        out: &mut Vec<u8>,
        major: u8,
        close: &str,
        mut entry: impl FnMut(&mut Self, &mut Vec<u8>) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let spec = self.parse_spec();
        let spaced = self.consume_ws()?;
        if spec != Indicator::None && !spaced && !matches!(self.peek(), Some(']' | '}')) {
            return Err(self.syntax());
        }

        let indefinite = spec == Indicator::Indefinite;
        let mut body = Vec::new();
        let target = if indefinite {
            out.push((major << 5) | 31);
            &mut *out
        } else {
            &mut body
        };
        let mut count = 0usize;
        if !self.eat(close) {
            loop {
                entry(self, target)?;
                count += 1;
                if self.list_end(Some(close))? {
                    break;
                }
            }
        }

        if indefinite {
            out.push(0xff);
        } else {
            self.write_len(out, major, count as u64, spec)?;
            out.extend_from_slice(&body);
        }
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
            // The grammar admits an ellipsis where a chunk may stand, but no
            // semantics are defined for eliding chunks of an
            // indefinite-length string (cf. the ilbs/ilts extensions).
            if self.starts_with("...") {
                return Err(self.semantic("ellipsis is not supported as a stream string chunk"));
            }
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

            if self.list_end(Some(")"))? {
                break;
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
                let n = self.dec_u64_no_leading()?;
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
        let argument_offset = self.pos;
        let atom = if self.starts_with("<<") {
            let args = self.sequence_args(depth)?;
            self.apply_app_sequence(prefix, args)
        } else if self.peek() == Some('\'') {
            let content = self.quoted_string('\'')?;
            self.apply_app_string(prefix, content)
        } else if self.peek() == Some('`') {
            let content = self.raw_string()?;
            self.apply_app_string(prefix, content)
        } else {
            return Err(self.syntax());
        }
        .map_err(|error| {
            // Extensions work on decoded strings or computed CBOR values,
            // whose offsets are not source offsets. Locate evaluation errors
            // at the argument's opening delimiter; parse errors above keep
            // their precise source position, including nested extensions.
            match error {
                Error::Syntax(_) => Error::Syntax(argument_offset),
                Error::Semantic(_, message) => Error::semantic(argument_offset, message),
                other => other,
            }
        })?;
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

        let has_dot = self.eat(".");
        if has_dot {
            while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
                self.bump();
            }
        }
        if digits_before == 0 && !has_dot {
            return Err(self.syntax());
        }
        if digits_before == 0 && &self.input[start..self.pos] == "." {
            return Err(self.syntax());
        }

        if matches!(self.peek(), Some('e' | 'E')) {
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
            return parse_integer(negative, digits, 16, offset);
        }
        if let Some(digits) = digits
            .strip_prefix("0o")
            .or_else(|| digits.strip_prefix("0O"))
        {
            return parse_integer(negative, digits, 8, offset);
        }
        if let Some(digits) = digits
            .strip_prefix("0b")
            .or_else(|| digits.strip_prefix("0B"))
        {
            return parse_integer(negative, digits, 2, offset);
        }

        if lex.contains('.') || lex.contains('e') || lex.contains('E') {
            let value = lex
                .parse::<f64>()
                .map_err(|_| Error::semantic(offset, format!("invalid decimal float `{lex}`")))?;
            // Overflowing to infinity would silently change the data;
            // `Infinity` spells the infinite values explicitly.
            if value.is_infinite() {
                return Err(Error::semantic(
                    offset,
                    format!("decimal float `{lex}` overflows the f64 range"),
                ));
            }
            return Ok(Atom::Float(value));
        }

        parse_integer(negative, digits, 10, offset)
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
            // The `unescaped` grammar excludes the C0 controls other than
            // LF (source CR is removed before parsing) but allows DEL and
            // the C1 range.
            if ch != '\n' && (ch as u32) < 0x20 {
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

        // Source CR is removed before parsing, so LF is the only newline.
        if content.is_empty()
            || content
                .chars()
                .any(|ch| ch.is_ascii_control() && ch != '\n')
        {
            return Err(self.syntax());
        }
        if let Some(stripped) = content.strip_prefix('\n') {
            content = stripped;
        } else if content.starts_with(' ') && content.ends_with(' ') && content.len() >= 2 {
            content = &content[1..content.len() - 1];
        }

        Ok(content.to_string())
    }

    fn apply_app_string(&self, prefix: &str, content: String) -> Result<Atom, Error> {
        self.apply_app_sequence(prefix, alloc::vec![Arg::Text(content)])
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
            "b1" | "t1" => concat_app_strings(prefix, args, self.pos),
            "bytes" => concat_bytes(args, self.pos),
            "same" => same_args(args, self.pos),
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
                    hex_atom(&content, self.pos)
                } else {
                    Ok(Atom::Bytes(base64_content(&content, self.pos)?))
                }
            }
            #[cfg(feature = "cdn-hash")]
            "hash" => {
                let (data, alg) = hash_args(args, self.pos)?;
                Ok(hash_atom(data, alg))
            }
            #[cfg(feature = "cdn-cri")]
            "cri" | "CRI" => {
                let content = one_text_arg(prefix, args, self.pos)?;
                cri_atom(&content, prefix == "CRI", self.pos)
            }
            _ => unresolved_app_sequence(prefix, args),
        }
    }

    fn emit_atom(&self, out: &mut Vec<u8>, atom: Atom, spec: Indicator<'a>) -> Result<(), Error> {
        match atom {
            Atom::Integer { negative, argument } => {
                self.write_len(out, u8::from(negative), argument, spec)
            }
            Atom::Bignum { negative, argument } => {
                self.write_len(
                    out,
                    6,
                    if negative { tag::BIGNEG } else { tag::BIGPOS },
                    Indicator::None,
                )?;
                write_definite_bytes(out, &argument, Indicator::None)
            }
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
                out.extend_from_slice(&x);
                Ok(())
            }
        }
    }

    fn write_float(&self, out: &mut Vec<u8>, value: f64, spec: Indicator<'a>) -> Result<(), Error> {
        match spec {
            Indicator::None | Indicator::Other(..) | Indicator::Indefinite => {
                Encoder::from(out).push(Header::Float(value))?;
                Ok(())
            }
            Indicator::Immediate => Err(self.semantic("float cannot use `_i` encoding indicator")),
            Indicator::Ai(1) => {
                let bits = f64_to_f16_preserving(value)
                    .ok_or_else(|| self.semantic("float cannot be represented as binary16"))?;
                out.push(0xf9);
                out.extend_from_slice(&bits.to_be_bytes());
                Ok(())
            }
            Indicator::Ai(2) => {
                let bits = f64_to_f32(value).ok_or_else(|| {
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
        }
    }

    fn write_simple(&self, out: &mut Vec<u8>, value: u8, spec: Indicator<'a>) -> Result<(), Error> {
        match spec {
            Indicator::None | Indicator::Other(..) | Indicator::Indefinite => {
                Encoder::from(out).push(Header::Simple(value))?;
                Ok(())
            }
            Indicator::Immediate if value <= 23 => {
                Encoder::from(out).push(Header::Simple(value))?;
                Ok(())
            }
            Indicator::Immediate => Err(self.semantic("simple value does not fit `_i`")),
            Indicator::Ai(0) if value >= 32 => {
                out.extend_from_slice(&[0xf8, value]);
                Ok(())
            }
            Indicator::Ai(0) => {
                Err(self
                    .semantic("two-byte encodings of simple values below 32 are not well-formed"))
            }
            Indicator::Ai(..) => Err(self.semantic("simple values only support `_i` or `_0`")),
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

    fn dec_u64_no_leading(&mut self) -> Result<u64, Error> {
        let start = self.pos;
        let value = self.dec_u64()?;
        let digits = &self.input[start..self.pos];
        if digits.len() > 1 && digits.starts_with('0') {
            return Err(self.syntax());
        }
        Ok(value)
    }
}
