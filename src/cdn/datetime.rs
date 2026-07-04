use alloc::vec::Vec;

use crate::de::Error;

use super::encode::write_uint;
use super::parser::Parser;
use super::types::{Atom, BigInt, Indicator};

pub(super) fn datetime_atom(content: &str, tagged: bool, offset: usize) -> Result<Atom, Error> {
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
    expect_byte_ci(b, 10, b'T', offset)?;
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
        Some(b'Z' | b'z') => {
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
    if second == 60 && !is_possible_leap_second_epoch(seconds) {
        return Err(Error::Syntax(offset));
    }
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

fn expect_byte_ci(bytes: &[u8], idx: usize, expected: u8, offset: usize) -> Result<(), Error> {
    if bytes
        .get(idx)
        .is_some_and(|byte| byte.eq_ignore_ascii_case(&expected))
    {
        Ok(())
    } else {
        Err(Error::Syntax(offset + idx))
    }
}

// A positive leap second is only ever inserted as 23:59:60 UTC on the last
// day of a month (IERS schedules them at month ends, preferring June and
// December). Checking that shape — instead of a hardcoded table of past
// leap seconds — keeps future leap seconds parseable while still rejecting
// times like 12:30:60 that cannot be leap seconds at all. `seconds` already
// includes the :60, so it must land exactly on a following midnight that
// starts a new month.
fn is_possible_leap_second_epoch(seconds: i128) -> bool {
    if seconds % 86_400 != 0 {
        return false;
    }

    let Ok(days) = i64::try_from(seconds / 86_400) else {
        return false;
    };
    let (.., day) = civil_from_days(days);
    day == 1
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

// The inverse of `days_from_civil`.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}
