//! Calendar arithmetic, formatting and parsing for UTC and fixed-offset times.
//!
//! Named IANA time zones (e.g. `Europe/Ljubljana`) need a tz database and are not supported;
//! use fixed offsets such as `+02:00`, `Z` or `UTC`.

const WEEKDAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November",
    "December",
];

pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn days_in_month(y: i64, m: i64) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => {
            if is_leap(y) {
                29
            } else {
                28
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parts {
    pub year: i64,
    pub month: i64,
    pub day: i64,
    pub hour: i64,
    pub minute: i64,
    pub second: i64,
    /// 0 = Sunday .. 6 = Saturday.
    pub weekday: i64,
    /// Day of year, 1-based.
    pub yday: i64,
}

/// Breaks a Unix timestamp (seconds) into calendar fields at a fixed offset (minutes east of UTC).
pub fn parts(unix: i64, offset_minutes: i64) -> Parts {
    let local = unix + offset_minutes * 60;
    let days = local.div_euclid(86400);
    let rem = local.rem_euclid(86400);
    let (year, month, day) = civil_from_days(days);
    Parts {
        year,
        month,
        day,
        hour: rem / 3600,
        minute: (rem % 3600) / 60,
        second: rem % 60,
        weekday: (days + 4).rem_euclid(7),
        yday: days - days_from_civil(year, 1, 1) + 1,
    }
}

/// Builds a Unix timestamp from calendar fields interpreted at the given offset.
pub fn from_parts(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64, offset_minutes: i64) -> Result<i64, String> {
    if !(1..=12).contains(&month) {
        return Err(format!("month {} out of range", month));
    }
    if day < 1 || day > days_in_month(year, month) {
        return Err(format!("day {} out of range for {}-{:02}", day, year, month));
    }
    if !(0..24).contains(&hour) || !(0..60).contains(&minute) || !(0..61).contains(&second) {
        return Err("time of day out of range".to_string());
    }
    Ok(days_from_civil(year, month, day) * 86400 + hour * 3600 + minute * 60 + second - offset_minutes * 60)
}

/// Adds calendar months, clamping the day to the target month's length.
pub fn add_months(unix: i64, months: i64, offset_minutes: i64) -> i64 {
    let p = parts(unix, offset_minutes);
    let total = p.year * 12 + (p.month - 1) + months;
    let year = total.div_euclid(12);
    let month = total.rem_euclid(12) + 1;
    let day = p.day.min(days_in_month(year, month));
    days_from_civil(year, month, day) * 86400 + p.hour * 3600 + p.minute * 60 + p.second - offset_minutes * 60
}

/// Parses `Z`, `UTC`, `GMT`, `+HH:MM`, `-HH:MM`, `+HHMM` or `+HH` into minutes east of UTC.
pub fn parse_offset(text: &str) -> Result<i64, String> {
    let t = text.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("z") || t.eq_ignore_ascii_case("utc") || t.eq_ignore_ascii_case("gmt") {
        return Ok(0);
    }
    let (sign, rest) = match t.chars().next() {
        Some('+') => (1, &t[1..]),
        Some('-') => (-1, &t[1..]),
        _ => return Err(format!("unsupported time zone '{}': use UTC or a fixed offset like +02:00", text)),
    };
    let digits: String = rest.chars().filter(|c| *c != ':').collect();
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("invalid UTC offset '{}'", text));
    }
    let (h, m) = match digits.len() {
        1 | 2 => (digits.parse::<i64>().unwrap_or(0), 0),
        4 => (digits[..2].parse::<i64>().unwrap_or(0), digits[2..].parse::<i64>().unwrap_or(0)),
        _ => return Err(format!("invalid UTC offset '{}'", text)),
    };
    if h > 23 || m > 59 {
        return Err(format!("UTC offset '{}' out of range", text));
    }
    Ok(sign * (h * 60 + m))
}

pub fn format_offset(offset_minutes: i64, colon: bool) -> String {
    let sign = if offset_minutes < 0 { '-' } else { '+' };
    let a = offset_minutes.abs();
    if colon {
        format!("{}{:02}:{:02}", sign, a / 60, a % 60)
    } else {
        format!("{}{:02}{:02}", sign, a / 60, a % 60)
    }
}

/// strftime-style formatting. Supported: %Y %y %m %d %e %H %I %M %S %p %j %a %A %b %B %F %T %s %z %Z %%.
pub fn format(unix: i64, fmt: &str, offset_minutes: i64) -> String {
    format_with(unix, fmt, offset_minutes, None)
}

/// Like `format`, but `%Z` prints `abbreviation` (for example `CEST`) when one is given, and
/// `%:z` prints the offset with a colon (`+02:00`).
pub fn format_with(unix: i64, fmt: &str, offset_minutes: i64, abbreviation: Option<&str>) -> String {
    let p = parts(unix, offset_minutes);
    let mut out = String::new();
    let mut chars = fmt.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('Y') => out.push_str(&format!("{:04}", p.year)),
            Some('y') => out.push_str(&format!("{:02}", p.year.rem_euclid(100))),
            Some('m') => out.push_str(&format!("{:02}", p.month)),
            Some('d') => out.push_str(&format!("{:02}", p.day)),
            Some('e') => out.push_str(&format!("{:2}", p.day)),
            Some('H') => out.push_str(&format!("{:02}", p.hour)),
            Some('I') => out.push_str(&format!("{:02}", if p.hour % 12 == 0 { 12 } else { p.hour % 12 })),
            Some('M') => out.push_str(&format!("{:02}", p.minute)),
            Some('S') => out.push_str(&format!("{:02}", p.second)),
            Some('p') => out.push_str(if p.hour < 12 { "AM" } else { "PM" }),
            Some('j') => out.push_str(&format!("{:03}", p.yday)),
            Some('a') => out.push_str(&WEEKDAYS[p.weekday as usize][..3]),
            Some('A') => out.push_str(WEEKDAYS[p.weekday as usize]),
            Some('b') => out.push_str(&MONTHS[(p.month - 1) as usize][..3]),
            Some('B') => out.push_str(MONTHS[(p.month - 1) as usize]),
            Some('F') => out.push_str(&format!("{:04}-{:02}-{:02}", p.year, p.month, p.day)),
            Some('T') => out.push_str(&format!("{:02}:{:02}:{:02}", p.hour, p.minute, p.second)),
            Some('s') => out.push_str(&unix.to_string()),
            Some('z') => out.push_str(&format_offset(offset_minutes, false)),
            Some('Z') => match abbreviation {
                Some(name) if !name.is_empty() => out.push_str(name),
                _ => out.push_str(&if offset_minutes == 0 { "UTC".to_string() } else { format_offset(offset_minutes, true) }),
            },
            Some(':') => match chars.next() {
                Some('z') => out.push_str(&format_offset(offset_minutes, true)),
                Some(other) => {
                    out.push_str("%:");
                    out.push(other);
                }
                None => out.push_str("%:"),
            },
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

/// Parses ISO 8601 / RFC 3339 timestamps: `YYYY-MM-DD`, optionally followed by `T` or a space,
/// `HH:MM[:SS[.fraction]]` and `Z` / `±HH:MM`. Returns Unix seconds in UTC.
pub fn parse(text: &str) -> Result<i64, String> {
    let t = text.trim();
    let bad = || format!("cannot parse '{}' as a date/time (expected e.g. 2026-10-31T12:30:00Z)", text);
    let bytes: Vec<char> = t.chars().collect();
    if bytes.len() < 10 {
        return Err(bad());
    }
    let num = |s: usize, e: usize| -> Result<i64, String> {
        let part: String = bytes[s..e].iter().collect();
        if part.chars().all(|c| c.is_ascii_digit()) {
            part.parse::<i64>().map_err(|_| bad())
        } else {
            Err(bad())
        }
    };
    if bytes[4] != '-' || bytes[7] != '-' {
        return Err(bad());
    }
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (mut hour, mut minute, mut second) = (0, 0, 0);
    let mut offset = 0;
    if bytes.len() > 10 {
        if bytes[10] != 'T' && bytes[10] != 't' && bytes[10] != ' ' {
            return Err(bad());
        }
        if bytes.len() < 16 || bytes[13] != ':' {
            return Err(bad());
        }
        hour = num(11, 13)?;
        minute = num(14, 16)?;
        let mut i = 16;
        if i < bytes.len() && bytes[i] == ':' {
            if bytes.len() < 19 {
                return Err(bad());
            }
            second = num(17, 19)?;
            i = 19;
            if i < bytes.len() && bytes[i] == '.' {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
        }
        if i < bytes.len() {
            let tail: String = bytes[i..].iter().collect();
            offset = parse_offset(tail.trim()).map_err(|_| bad())?;
        }
    }
    from_parts(year, month, day, hour, minute, second, offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_round_trip() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(days_from_civil(2000, 2, 29), 11016);
        for z in [-800000, -1, 0, 1, 19000, 2932896] {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
    }

    #[test]
    fn parts_weekday_and_offset() {
        let p = parts(0, 0);
        assert_eq!((p.year, p.month, p.day, p.weekday, p.yday), (1970, 1, 1, 4, 1));
        let p = parts(0, 120);
        assert_eq!((p.hour, p.minute), (2, 0));
    }

    #[test]
    fn formatting() {
        // 2026-10-31T12:30:45Z
        let t = parse("2026-10-31T12:30:45Z").unwrap();
        assert_eq!(format(t, "%Y-%m-%d %H:%M:%S %Z", 0), "2026-10-31 12:30:45 UTC");
        assert_eq!(format(t, "%A, %d %B %Y", 0), "Saturday, 31 October 2026");
        assert_eq!(format(t, "%F %T %z", 120), "2026-10-31 14:30:45 +0200");
        assert_eq!(format(t, "%I:%M %p", 0), "12:30 PM");
    }

    #[test]
    fn parsing_offsets() {
        let utc = parse("2026-10-31T12:00:00Z").unwrap();
        assert_eq!(parse("2026-10-31T14:00:00+02:00").unwrap(), utc);
        assert_eq!(parse("2026-10-31 10:00:00-02:00").unwrap(), utc);
        assert_eq!(parse("2026-10-31").unwrap(), utc - 12 * 3600);
        assert!(parse("2026-13-01").is_err());
        assert!(parse("nonsense").is_err());
        assert!(parse("2026-02-30").is_err());
    }

    #[test]
    fn month_arithmetic() {
        let jan31 = parse("2026-01-31T00:00:00Z").unwrap();
        assert_eq!(format(add_months(jan31, 1, 0), "%F", 0), "2026-02-28");
        assert_eq!(format(add_months(jan31, -2, 0), "%F", 0), "2025-11-30");
        assert_eq!(format(add_months(jan31, 13, 0), "%F", 0), "2027-02-28");
    }

    #[test]
    fn offsets() {
        assert_eq!(parse_offset("Z").unwrap(), 0);
        assert_eq!(parse_offset("+05:30").unwrap(), 330);
        assert_eq!(parse_offset("-0800").unwrap(), -480);
        assert!(parse_offset("Europe/Ljubljana").is_err());
    }
}
