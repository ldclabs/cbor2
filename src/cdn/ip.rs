use alloc::vec::Vec;

use crate::de::Error;

use super::encode::{write_definite_bytes, write_uint};
use super::types::{Atom, Indicator};

pub(super) fn ip_atom(content: &str, tagged: bool, offset: usize) -> Result<Atom, Error> {
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

pub(super) fn parse_ipv4(input: &str, offset: usize) -> Result<[u8; 4], Error> {
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

pub(super) fn parse_ipv6(input: &str, offset: usize) -> Result<[u8; 16], Error> {
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
