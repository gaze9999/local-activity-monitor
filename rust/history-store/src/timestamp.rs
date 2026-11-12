use chrono::{DateTime, Datelike, Duration, NaiveDate, SecondsFormat, Utc};

/// Parse a legacy ISO timestamp with an explicit offset.
pub fn parse_utc_stamp(value: &str) -> Option<DateTime<Utc>> {
    let (date, clock) = split_date(value)?;
    let split = clock.find(['Z', '+', '-'])?;
    let (time, offset) = clock.split_at(split);
    let (hours, minutes, seconds, micros) = clock_parts(time)?;
    let local = date.and_hms_micro_opt(hours, minutes, seconds, micros)?;
    let offset = match offset {
        "Z" => 0,
        value if value.starts_with(['+', '-']) => {
            let (hours, minutes, seconds, micros) = clock_parts(&value[1..])?;
            let magnitude =
                i64::from(hours * 3600 + minutes * 60 + seconds) * 1_000_000 + i64::from(micros);
            if magnitude >= 86_400_000_000 {
                return None;
            }
            if value.starts_with('-') {
                -magnitude
            } else {
                magnitude
            }
        }
        _ => return None,
    };
    let utc = local.checked_sub_signed(Duration::microseconds(offset))?;
    (1..=9999).contains(&utc.year()).then(|| utc.and_utc())
}

pub fn normalize_utc_stamp(value: &str) -> Option<String> {
    normalize(value, true)
}

pub fn normalize(value: &str, zulu: bool) -> Option<String> {
    Some(parse_utc_stamp(value)?.to_rfc3339_opts(SecondsFormat::Millis, zulu))
}

fn split_date(value: &str) -> Option<(NaiveDate, &str)> {
    for (length, format, weekday) in [
        (10, "%Y-%m-%d", ""),
        (10, "%G-W%V-%u", ""),
        (8, "%Y%m%d", ""),
        (8, "%GW%V%u", ""),
        (8, "%G-W%V-%u", "-1"),
        (7, "%GW%V%u", "1"),
    ] {
        let Some(date) = value.get(..length) else {
            continue;
        };
        let result = if weekday.is_empty() {
            NaiveDate::parse_from_str(date, format)
        } else {
            NaiveDate::parse_from_str(&format!("{date}{weekday}"), format)
        };
        let Ok(parsed) = result else {
            continue;
        };
        if !(1..=9999).contains(&parsed.year()) {
            return None;
        }
        let time = value.get(length..)?;
        let separator = time.chars().next()?;
        return Some((parsed, &time[separator.len_utf8()..]));
    }
    None
}

fn clock_parts(value: &str) -> Option<(u32, u32, u32, u32)> {
    let (clock, fraction) = match value.split_once(['.', ',']) {
        Some((whole, fraction)) => (whole.as_bytes(), Some(fraction)),
        None => (value.as_bytes(), None),
    };
    let (hours, minutes, seconds) = match clock {
        [h, i] if fraction.is_none() => (pair(*h, *i)?, 0, 0),
        [h, i, m, n] | [h, i, b':', m, n] if fraction.is_none() => {
            (pair(*h, *i)?, pair(*m, *n)?, 0)
        }
        [h, i, m, n, s, t] | [h, i, b':', m, n, b':', s, t] => {
            (pair(*h, *i)?, pair(*m, *n)?, pair(*s, *t)?)
        }
        _ => return None,
    };
    let micros = match fraction {
        Some(digits) if !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()) => {
            digits
                .bytes()
                .take(6)
                .chain(std::iter::repeat(b'0'))
                .take(6)
                .fold(0, |value, byte| value * 10 + u32::from(byte - b'0'))
        }
        Some(_) => return None,
        None => 0,
    };
    Some((hours, minutes, seconds, micros))
}

fn pair(first: u8, second: u8) -> Option<u32> {
    if !first.is_ascii_digit() || !second.is_ascii_digit() {
        return None;
    }
    Some(u32::from(first - b'0') * 10 + u32::from(second - b'0'))
}
