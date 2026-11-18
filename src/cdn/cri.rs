use alloc::{string::String, vec::Vec};

use crate::de::Error;

use super::encode::{
    write_array_len, write_definite_bytes, write_definite_text, write_int, write_tag,
};
use super::ip::{parse_ipv4, parse_ipv6};
use super::types::{Atom, Indicator, CRI_TAG};

pub(super) fn cri_atom(content: &str, tagged: bool, offset: usize) -> Result<Atom, Error> {
    let iri = iref::IriRef::new(content)
        .map_err(|_| Error::semantic(offset, "cri requires a valid IRI reference"))?;
    let mut raw = cri_reference_bytes(iri, offset)?;
    if tagged {
        let mut out = Vec::new();
        write_tag(&mut out, CRI_TAG)?;
        out.append(&mut raw);
        Ok(Atom::Raw(out))
    } else {
        Ok(Atom::Raw(raw))
    }
}

fn cri_reference_bytes(iri: &iref::IriRef, offset: usize) -> Result<Vec<u8>, Error> {
    let mut sections = Vec::new();

    let original_path = iri.path().as_str();
    let mut path = original_path;
    let relative = iri.scheme().is_none() && iri.authority().is_none();
    match iri.scheme() {
        Some(scheme) => {
            sections.push(cri_scheme_bytes(scheme.as_str())?);
            sections.push(match iri.authority() {
                Some(authority) => cri_authority_bytes(authority, offset)?,
                None => cri_no_authority_bytes(iri.path().as_str()),
            });
        }
        None => {
            if let Some(authority) = iri.authority() {
                sections.push(write_simple_value(22));
                sections.push(cri_authority_bytes(authority, offset)?);
            } else {
                let (discard, tail) = cri_discard(path, offset)?;
                sections.push(discard);
                path = tail;
            }
        }
    }

    sections.push(if original_path.is_empty() {
        if relative {
            write_simple_value(22)
        } else {
            write_empty_array()?
        }
    } else {
        // Keep the empty segment in a path like "/" or "../".
        let path = original_path.strip_prefix('/').unwrap_or(path);
        cri_path_bytes(path, offset)?
    });
    sections.push(match iri.query() {
        Some(query) => cri_query_bytes(query.as_str(), offset)?,
        None if relative => write_simple_value(22),
        None => write_empty_array()?,
    });
    sections.push(match iri.fragment() {
        Some(fragment) => cri_text_bytes(&percent_decode_component(
            fragment.as_str(),
            PercentContext::Fragment,
            offset,
        )?)?,
        None => write_simple_value(22),
    });

    trim_cri_defaults(&mut sections);
    if relative && sections.len() == 1 && sections[0] == [0] {
        sections.clear(); // The empty URI reference has the interchange form [].
    }

    let mut out = Vec::new();
    write_array_len(&mut out, sections.len())?;
    for section in sections {
        out.extend_from_slice(&section);
    }
    Ok(out)
}

fn trim_cri_defaults(sections: &mut Vec<Vec<u8>>) {
    while sections
        .last()
        .is_some_and(|section| section.as_slice() == [0xf6] || section.as_slice() == [0x80])
    {
        sections.pop();
    }
}

fn cri_scheme_bytes(scheme: &str) -> Result<Vec<u8>, Error> {
    let lower = scheme.to_ascii_lowercase();
    if let Some(number) = cri_scheme_number(&lower) {
        Ok(write_i128(-1 - i128::from(number)))
    } else {
        cri_text_bytes(&lower)
    }
}

fn cri_scheme_number(scheme: &str) -> Option<u64> {
    match scheme {
        "coap" => Some(0),
        "coaps" => Some(1),
        "http" => Some(2),
        "https" => Some(3),
        "urn" => Some(4),
        "did" => Some(5),
        "coap+tcp" => Some(6),
        "coaps+tcp" => Some(7),
        "coap+ws" => Some(24),
        "coaps+ws" => Some(25),
        _ => None,
    }
}

fn cri_no_authority_bytes(path: &str) -> Vec<u8> {
    if path.is_empty() || path.starts_with('/') {
        write_simple_value(22)
    } else {
        write_simple_value(21)
    }
}

fn cri_discard(mut path: &str, offset: usize) -> Result<(Vec<u8>, &str), Error> {
    if path.is_empty() {
        return Ok((write_i128(0), path));
    }
    if path.starts_with('/') {
        return Ok((write_simple_value(21), path));
    }
    let mut discard = 1;
    while let Some(rest) = path.strip_prefix("../") {
        discard += 1;
        if discard > 127 {
            return Err(Error::semantic(offset, "CRI discard must fit in 0..=127"));
        }
        path = rest;
    }
    // A leading "./" disambiguates a colon in the first relative segment.
    if discard == 1 {
        if let Some(rest) = path.strip_prefix("./") {
            if rest
                .split('/')
                .next()
                .is_some_and(|first| first.contains(':'))
            {
                path = rest;
            }
        }
    }
    Ok((write_i128(discard), path))
}

fn cri_authority_bytes(authority: &iref::iri::Authority, offset: usize) -> Result<Vec<u8>, Error> {
    let mut items = Vec::new();
    if let Some(user_info) = authority.user_info() {
        items.push(write_simple_value(20));
        items.push(cri_text_bytes(&percent_decode_component(
            user_info.as_str(),
            PercentContext::UserInfo,
            offset,
        )?)?);
    }

    let host = authority.host().as_str();
    if let Some(bytes) = host_ip_bytes(host, offset)? {
        items.push(cri_bytes_bytes(&bytes)?);
    } else {
        for label in host.split('.') {
            let label = percent_decode_component(label, PercentContext::Host, offset)?;
            if label.contains('.') {
                return Err(Error::semantic(
                    offset,
                    "CRI host labels cannot contain dots",
                ));
            }
            items.push(cri_text_bytes(&label.to_lowercase())?);
        }
    }

    if let Some(port) = authority.port() {
        let n = port
            .as_str()
            .parse::<u16>()
            .map_err(|_| Error::semantic(offset, "CRI port must fit in u16"))?;
        items.push(write_i128(i128::from(n)));
    }

    let mut out = Vec::new();
    write_array_len(&mut out, items.len())?;
    for item in items {
        out.extend_from_slice(&item);
    }
    Ok(out)
}

fn host_ip_bytes(host: &str, offset: usize) -> Result<Option<Vec<u8>>, Error> {
    if let Some(inner) = host.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        if inner.starts_with(['v', 'V']) {
            return Err(Error::semantic(
                offset,
                "CRI does not support IPvFuture hosts",
            ));
        }
        if inner.contains('%') {
            return Err(Error::semantic(
                offset,
                "CRI zone identifiers are not supported",
            ));
        }
        return parse_ipv6(inner, offset).map(|bytes| Some(bytes.to_vec()));
    }

    if host.bytes().all(|b| b.is_ascii_digit() || b == b'.') && host.contains('.') {
        return Ok(parse_ipv4(host, offset).ok().map(|bytes| bytes.to_vec()));
    }

    Ok(None)
}

fn cri_path_bytes(path: &str, offset: usize) -> Result<Vec<u8>, Error> {
    let segments = path.split('/');
    let mut out = Vec::new();
    write_array_len(&mut out, segments.clone().count())?;
    for segment in segments {
        let segment = percent_decode_component(segment, PercentContext::Path, offset)?;
        if segment == "." || segment == ".." {
            return Err(Error::semantic(
                offset,
                "CRI paths cannot contain dot segments",
            ));
        }
        write_definite_text(&mut out, &segment, Indicator::None)?;
    }
    Ok(out)
}

fn cri_query_bytes(query: &str, offset: usize) -> Result<Vec<u8>, Error> {
    let params: Vec<&str> = query.split('&').collect();
    let mut out = Vec::new();
    write_array_len(&mut out, params.len())?;
    for param in params {
        let param = percent_decode_component(param, PercentContext::Query, offset)?;
        write_definite_text(&mut out, &param, Indicator::None)?;
    }
    Ok(out)
}

fn cri_text_bytes(text: &str) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    write_definite_text(&mut out, text, Indicator::None)?;
    Ok(out)
}

fn cri_bytes_bytes(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    write_definite_bytes(&mut out, bytes, Indicator::None)?;
    Ok(out)
}

fn write_empty_array() -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    write_array_len(&mut out, 0)?;
    Ok(out)
}

// CRI only uses the one-byte simple values false, true and null.
fn write_simple_value(value: u8) -> Vec<u8> {
    debug_assert!(value <= 23);
    alloc::vec![0xe0 | value]
}

fn write_i128(value: i128) -> Vec<u8> {
    let mut out = Vec::new();
    write_int(&mut out, value);
    out
}

#[derive(Clone, Copy)]
enum PercentContext {
    UserInfo,
    Host,
    Path,
    Query,
    Fragment,
}

fn percent_decode_component(
    input: &str,
    context: PercentContext,
    offset: usize,
) -> Result<String, Error> {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(Error::Syntax(offset + i));
            }
            let hi = hex_value(bytes[i + 1]).ok_or(Error::Syntax(offset + i + 1))?;
            let lo = hex_value(bytes[i + 2]).ok_or(Error::Syntax(offset + i + 2))?;
            let byte = (hi << 4) | lo;
            validate_percent_decoded(byte, context, offset + i)?;
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out)
        .map_err(|_| Error::semantic(offset, "CRI percent-decoded text is not UTF-8"))
}

fn validate_percent_decoded(byte: u8, context: PercentContext, offset: usize) -> Result<(), Error> {
    if !byte.is_ascii() || is_unreserved(byte) {
        return Ok(());
    }

    let would_be_reencoded = match context {
        PercentContext::UserInfo => !is_userinfo_uri_char(byte),
        PercentContext::Host => {
            if byte == b'.' {
                return Err(Error::semantic(
                    offset,
                    "CRI host label dots cannot be percent-encoded",
                ));
            }
            !is_host_uri_char(byte)
        }
        PercentContext::Path => !is_path_uri_char(byte),
        PercentContext::Query => byte == b'&' || !is_query_fragment_uri_char(byte),
        PercentContext::Fragment => !is_query_fragment_uri_char(byte),
    };

    if would_be_reencoded {
        Ok(())
    } else {
        Err(Error::semantic(
            offset,
            "CRI simple form cannot preserve this percent-encoded reserved character",
        ))
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

fn is_sub_delim(byte: u8) -> bool {
    matches!(
        byte,
        b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'='
    )
}

fn is_userinfo_uri_char(byte: u8) -> bool {
    is_unreserved(byte) || is_sub_delim(byte) || byte == b':'
}

fn is_host_uri_char(byte: u8) -> bool {
    is_unreserved(byte) || is_sub_delim(byte)
}

fn is_path_uri_char(byte: u8) -> bool {
    is_unreserved(byte) || is_sub_delim(byte) || matches!(byte, b':' | b'@')
}

fn is_query_fragment_uri_char(byte: u8) -> bool {
    is_path_uri_char(byte) || matches!(byte, b'/' | b'?')
}
