//! A small timestamp parser covering the layouts go-pflow's `eventlog`
//! package tries by default (`DefaultCSVConfig`/`DefaultJSONLConfig`'s
//! `TimestampFormats`). Go's `time.Parse` takes an arbitrary *reference-time*
//! layout (`"2006-01-02T15:04:05Z07:00"`); rather than port a general layout
//! engine, this recognizes exactly the fixed set of layouts go-pflow ships as
//! defaults, plus RFC3339Nano. That covers every format the showcase fixture
//! and go-pflow's own tests use; an unsupported layout is simply skipped, the
//! same way `time.Parse` skips a layout an input string doesn't match.
//!
//! Timestamps are represented as milliseconds since the Unix epoch (UTC).

/// Milliseconds since the Unix epoch (UTC). Matches go-pflow's `time.Time`
/// closely enough for event-log purposes: ordering, subtraction (duration in
/// ms) and round-tripping through RFC3339.
pub type Timestamp = i64;

/// The layouts tried, in order, when a config doesn't specify a subset —
/// mirrors `eventlog.DefaultCSVConfig().TimestampFormats` /
/// `DefaultJSONLConfig().TimestampFormats`.
pub const DEFAULT_FORMATS: &[&str] = &[
    "RFC3339",
    "RFC3339Nano",
    "2006-01-02 15:04:05",
    "2006-01-02T15:04:05",
    "2006-01-02 15:04:05.000",
    "2006-01-02T15:04:05.000",
    "2006-01-02",
    "01/02/2006 15:04:05",
    "01/02/2006",
];

/// Tries each format in turn, returning the first successful parse — the
/// same strategy as go-pflow's `parseTimestamp`.
pub fn parse_timestamp(s: &str, formats: &[&str]) -> Option<Timestamp> {
    for fmt in formats {
        if let Some(ts) = parse_one(s, fmt) {
            return Some(ts);
        }
    }
    None
}

fn parse_one(s: &str, fmt: &str) -> Option<Timestamp> {
    match fmt {
        "RFC3339" | "RFC3339Nano" => parse_rfc3339(s),
        "2006-01-02 15:04:05" => parse_ymd_hms(s, ' '),
        "2006-01-02T15:04:05" => parse_ymd_hms(s, 'T'),
        "2006-01-02 15:04:05.000" => parse_ymd_hms_frac(s, ' '),
        "2006-01-02T15:04:05.000" => parse_ymd_hms_frac(s, 'T'),
        "2006-01-02" => parse_ymd(s),
        "01/02/2006 15:04:05" => parse_mdy_hms(s),
        "01/02/2006" => parse_mdy(s),
        _ => None,
    }
}

struct Fields {
    y: i32,
    mo: u32,
    d: u32,
    h: u32,
    mi: u32,
    s: u32,
    ms: i64,
    offset_min: i32,
}

fn to_millis(f: Fields) -> Option<Timestamp> {
    let days = days_from_civil(f.y, f.mo, f.d)?;
    let secs_of_day = (f.h as i64) * 3600 + (f.mi as i64) * 60 + (f.s as i64);
    let total_secs = days * 86_400 + secs_of_day - (f.offset_min as i64) * 60;
    Some(total_secs * 1000 + f.ms)
}

/// Howard Hinnant's `days_from_civil` — converts a civil (Y-M-D) date to
/// days since the Unix epoch, valid over the full proleptic Gregorian range.
fn days_from_civil(y: i32, m: u32, d: u32) -> Option<i64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y as i64 - 1 } else { y as i64 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (m as i64 + 9) % 12; // [0, 11]
    let doy = (153 * mp + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    Some(era * 146_097 + doe - 719_468)
}

fn parse_digits(s: &str, start: usize, len: usize) -> Option<(u32, usize)> {
    let end = start + len;
    let slice = s.get(start..end)?;
    if !slice.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((slice.parse().ok()?, end))
}

fn parse_ymd(s: &str) -> Option<Timestamp> {
    let bytes = s.as_bytes();
    if bytes.len() != 10 {
        return None;
    }
    let (y, p) = parse_digits(s, 0, 4)?;
    if bytes[p] != b'-' {
        return None;
    }
    let (mo, p) = parse_digits(s, p + 1, 2)?;
    if bytes[p] != b'-' {
        return None;
    }
    let (d, p) = parse_digits(s, p + 1, 2)?;
    if p != s.len() {
        return None;
    }
    to_millis(Fields {
        y: y as i32,
        mo,
        d,
        h: 0,
        mi: 0,
        s: 0,
        ms: 0,
        offset_min: 0,
    })
}

fn parse_ymd_hms(s: &str, sep: char) -> Option<Timestamp> {
    let bytes = s.as_bytes();
    if bytes.len() != 19 {
        return None;
    }
    let (y, p) = parse_digits(s, 0, 4)?;
    if bytes[p] != b'-' {
        return None;
    }
    let (mo, p) = parse_digits(s, p + 1, 2)?;
    if bytes[p] != b'-' {
        return None;
    }
    let (d, p) = parse_digits(s, p + 1, 2)?;
    if bytes[p] as char != sep {
        return None;
    }
    let (h, p) = parse_digits(s, p + 1, 2)?;
    if bytes[p] != b':' {
        return None;
    }
    let (mi, p) = parse_digits(s, p + 1, 2)?;
    if bytes[p] != b':' {
        return None;
    }
    let (sec, p) = parse_digits(s, p + 1, 2)?;
    if p != s.len() {
        return None;
    }
    to_millis(Fields {
        y: y as i32,
        mo,
        d,
        h,
        mi,
        s: sec,
        ms: 0,
        offset_min: 0,
    })
}

fn parse_ymd_hms_frac(s: &str, sep: char) -> Option<Timestamp> {
    let bytes = s.as_bytes();
    if bytes.len() != 23 || bytes[19] != b'.' {
        return None;
    }
    let base = parse_ymd_hms(&s[..19], sep)?;
    let (ms, p) = parse_digits(s, 20, 3)?;
    if p != s.len() {
        return None;
    }
    Some(base + ms as i64)
}

fn parse_mdy(s: &str) -> Option<Timestamp> {
    let bytes = s.as_bytes();
    if bytes.len() != 10 {
        return None;
    }
    let (mo, p) = parse_digits(s, 0, 2)?;
    if bytes[p] != b'/' {
        return None;
    }
    let (d, p) = parse_digits(s, p + 1, 2)?;
    if bytes[p] != b'/' {
        return None;
    }
    let (y, p) = parse_digits(s, p + 1, 4)?;
    if p != s.len() {
        return None;
    }
    to_millis(Fields {
        y: y as i32,
        mo,
        d,
        h: 0,
        mi: 0,
        s: 0,
        ms: 0,
        offset_min: 0,
    })
}

fn parse_mdy_hms(s: &str) -> Option<Timestamp> {
    let space = s.find(' ')?;
    let date = parse_mdy(&s[..space])?;
    let time_part = &s[space + 1..];
    let bytes = time_part.as_bytes();
    if bytes.len() != 8 {
        return None;
    }
    let (h, p) = parse_digits(time_part, 0, 2)?;
    if bytes[p] != b':' {
        return None;
    }
    let (mi, p) = parse_digits(time_part, p + 1, 2)?;
    if bytes[p] != b':' {
        return None;
    }
    let (sec, p) = parse_digits(time_part, p + 1, 2)?;
    if p != time_part.len() {
        return None;
    }
    Some(date + (h as i64 * 3600 + mi as i64 * 60 + sec as i64) * 1000)
}

/// RFC3339 / RFC3339Nano: `2006-01-02T15:04:05[.fraction](Z|+hh:mm|-hh:mm)`.
fn parse_rfc3339(s: &str) -> Option<Timestamp> {
    let bytes = s.as_bytes();
    if bytes.len() < 20 {
        return None;
    }
    let (y, p) = parse_digits(s, 0, 4)?;
    if bytes[p] != b'-' {
        return None;
    }
    let (mo, p) = parse_digits(s, p + 1, 2)?;
    if bytes[p] != b'-' {
        return None;
    }
    let (d, p) = parse_digits(s, p + 1, 2)?;
    if bytes[p] != b'T' && bytes[p] != b't' {
        return None;
    }
    let (h, p) = parse_digits(s, p + 1, 2)?;
    if bytes[p] != b':' {
        return None;
    }
    let (mi, p) = parse_digits(s, p + 1, 2)?;
    if bytes[p] != b':' {
        return None;
    }
    let (sec, mut p) = parse_digits(s, p + 1, 2)?;

    let mut ms: i64 = 0;
    if p < bytes.len() && bytes[p] == b'.' {
        let frac_start = p + 1;
        let mut frac_end = frac_start;
        while frac_end < bytes.len() && bytes[frac_end].is_ascii_digit() {
            frac_end += 1;
        }
        if frac_end == frac_start {
            return None;
        }
        let frac = &s[frac_start..frac_end];
        // Take up to 3 digits as milliseconds, ignoring finer precision.
        let ms_str: String = frac.chars().chain(std::iter::repeat('0')).take(3).collect();
        ms = ms_str.parse().ok()?;
        p = frac_end;
    }

    let offset_min = if p < bytes.len() && (bytes[p] == b'Z' || bytes[p] == b'z') {
        p += 1;
        0
    } else if p < bytes.len() && (bytes[p] == b'+' || bytes[p] == b'-') {
        let sign = if bytes[p] == b'-' { -1 } else { 1 };
        let (oh, np) = parse_digits(s, p + 1, 2)?;
        if bytes.get(np) != Some(&b':') {
            return None;
        }
        let (om, np) = parse_digits(s, np + 1, 2)?;
        p = np;
        sign * (oh as i32 * 60 + om as i32)
    } else {
        return None;
    };

    if p != s.len() {
        return None;
    }

    to_millis(Fields {
        y: y as i32,
        mo,
        d,
        h,
        mi,
        s: sec,
        ms,
        offset_min,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_utc() {
        let ts = parse_timestamp("2026-09-06T08:01:00Z", DEFAULT_FORMATS).unwrap();
        // 2026-09-06T08:01:00Z
        assert_eq!(ts, days_from_civil(2026, 9, 6).unwrap() * 86_400_000 + (8 * 3600 + 60) * 1000);
    }

    #[test]
    fn rfc3339_with_offset() {
        let plus = parse_timestamp("2026-09-06T10:01:00+02:00", DEFAULT_FORMATS).unwrap();
        let utc = parse_timestamp("2026-09-06T08:01:00Z", DEFAULT_FORMATS).unwrap();
        assert_eq!(plus, utc);
    }

    #[test]
    fn rfc3339_nano_fraction_truncated_to_millis() {
        let ts = parse_timestamp("2026-09-06T08:01:00.123456789Z", DEFAULT_FORMATS).unwrap();
        let base = parse_timestamp("2026-09-06T08:01:00Z", DEFAULT_FORMATS).unwrap();
        assert_eq!(ts, base + 123);
    }

    #[test]
    fn space_separated() {
        let a = parse_timestamp("2026-09-06 08:01:00", DEFAULT_FORMATS).unwrap();
        let b = parse_timestamp("2026-09-06T08:01:00", DEFAULT_FORMATS).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn date_only() {
        let ts = parse_timestamp("2026-09-06", DEFAULT_FORMATS).unwrap();
        let midnight = parse_timestamp("2026-09-06T00:00:00Z", DEFAULT_FORMATS).unwrap();
        assert_eq!(ts, midnight);
    }

    #[test]
    fn us_date_format() {
        let ts = parse_timestamp("09/06/2026 08:01:00", DEFAULT_FORMATS).unwrap();
        let iso = parse_timestamp("2026-09-06T08:01:00Z", DEFAULT_FORMATS).unwrap();
        assert_eq!(ts, iso);
    }

    #[test]
    fn ordering_is_monotonic_within_a_trace() {
        let a = parse_timestamp("2026-09-06T08:01:00Z", DEFAULT_FORMATS).unwrap();
        let b = parse_timestamp("2026-09-06T08:01:20Z", DEFAULT_FORMATS).unwrap();
        assert!(a < b);
    }

    #[test]
    fn unparseable_returns_none() {
        assert_eq!(parse_timestamp("not a date", DEFAULT_FORMATS), None);
    }
}
