//! Named time zones.
//!
//! A zone is resolved from a fixed offset, the host TZif database, an embedded IANA tzdata
//! 2025b TZif snapshot, and finally a small POSIX-rule fallback table. The embedded database
//! preserves historical transitions on systems without a timezone database (including standard
//! Windows installations). The snapshot should be refreshed as part of timezone-data maintenance.

use crate::time_ext;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

/// UTC offset in effect at an instant.
#[derive(Debug, Clone, PartialEq)]
pub struct Local {
    pub offset_seconds: i64,
    pub abbreviation: String,
    pub is_dst: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct RuleDate {
    /// 'M' = month/week/day, 'J' = Julian day 1..=365 (no leap day), 'N' = zero-based day 0..=365.
    kind: char,
    month: i64,
    week: i64,
    day: i64,
    /// Seconds after local midnight (may be negative or exceed 24h).
    time: i64,
}

#[derive(Debug, Clone, PartialEq)]
struct DstRule {
    name: String,
    offset: i64,
    start: RuleDate,
    end: RuleDate,
}

/// A parsed POSIX TZ string such as `CET-1CEST,M3.5.0,M10.5.0/3`.
#[derive(Debug, Clone, PartialEq)]
pub struct PosixRule {
    std_name: String,
    std_offset: i64,
    dst: Option<DstRule>,
}

fn parse_name(s: &[u8], i: &mut usize) -> Result<String, String> {
    if *i < s.len() && s[*i] == b'<' {
        let start = *i + 1;
        while *i < s.len() && s[*i] != b'>' {
            *i += 1;
        }
        if *i >= s.len() {
            return Err("unterminated <name> in TZ string".to_string());
        }
        let name = String::from_utf8_lossy(&s[start..*i]).into_owned();
        *i += 1;
        Ok(name)
    } else {
        let start = *i;
        while *i < s.len() && s[*i].is_ascii_alphabetic() {
            *i += 1;
        }
        if *i - start < 3 {
            return Err("zone abbreviation must have at least 3 letters".to_string());
        }
        Ok(String::from_utf8_lossy(&s[start..*i]).into_owned())
    }
}

/// Parses `[+-]hh[:mm[:ss]]` into seconds.
fn parse_hms(s: &[u8], i: &mut usize) -> Result<i64, String> {
    let mut sign = 1;
    if *i < s.len() && (s[*i] == b'+' || s[*i] == b'-') {
        if s[*i] == b'-' {
            sign = -1;
        }
        *i += 1;
    }
    let mut parts = [0i64; 3];
    for (k, part) in parts.iter_mut().enumerate() {
        let start = *i;
        while *i < s.len() && s[*i].is_ascii_digit() {
            *i += 1;
        }
        if start == *i {
            if k == 0 {
                return Err("expected a time in TZ string".to_string());
            }
            return Err("malformed time in TZ string".to_string());
        }
        *part = std::str::from_utf8(&s[start..*i]).unwrap_or("0").parse().map_err(|_| "bad number in TZ string".to_string())?;
        if *i < s.len() && s[*i] == b':' && k < 2 {
            *i += 1;
        } else {
            break;
        }
    }
    Ok(sign * (parts[0] * 3600 + parts[1] * 60 + parts[2]))
}

fn parse_number(s: &[u8], i: &mut usize) -> Result<i64, String> {
    let start = *i;
    while *i < s.len() && s[*i].is_ascii_digit() {
        *i += 1;
    }
    if start == *i {
        return Err("expected a number in TZ rule".to_string());
    }
    std::str::from_utf8(&s[start..*i]).unwrap_or("0").parse().map_err(|_| "bad number in TZ rule".to_string())
}

fn parse_rule_date(s: &[u8], i: &mut usize) -> Result<RuleDate, String> {
    let mut rule = RuleDate { kind: 'N', month: 0, week: 0, day: 0, time: 7200 };
    if *i < s.len() && s[*i] == b'M' {
        *i += 1;
        rule.kind = 'M';
        rule.month = parse_number(s, i)?;
        if *i >= s.len() || s[*i] != b'.' {
            return Err("expected '.' in Mm.w.d rule".to_string());
        }
        *i += 1;
        rule.week = parse_number(s, i)?;
        if *i >= s.len() || s[*i] != b'.' {
            return Err("expected '.' in Mm.w.d rule".to_string());
        }
        *i += 1;
        rule.day = parse_number(s, i)?;
        if !(1..=12).contains(&rule.month) || !(1..=5).contains(&rule.week) || !(0..=6).contains(&rule.day) {
            return Err("Mm.w.d rule out of range".to_string());
        }
    } else if *i < s.len() && s[*i] == b'J' {
        *i += 1;
        rule.kind = 'J';
        rule.day = parse_number(s, i)?;
    } else {
        rule.day = parse_number(s, i)?;
    }
    if *i < s.len() && s[*i] == b'/' {
        *i += 1;
        rule.time = parse_hms(s, i)?;
    }
    Ok(rule)
}

impl PosixRule {
    pub fn parse(text: &str) -> Result<PosixRule, String> {
        let s = text.trim().as_bytes();
        let mut i = 0;
        let std_name = parse_name(s, &mut i)?;
        // POSIX offsets are positive west of Greenwich.
        let std_offset = -parse_hms(s, &mut i)?;
        if i >= s.len() {
            return Ok(PosixRule { std_name, std_offset, dst: None });
        }
        let dst_name = parse_name(s, &mut i)?;
        let mut dst_offset = std_offset + 3600;
        if i < s.len() && s[i] != b',' {
            dst_offset = -parse_hms(s, &mut i)?;
        }
        let (start, end) = if i < s.len() && s[i] == b',' {
            i += 1;
            let start = parse_rule_date(s, &mut i)?;
            if i >= s.len() || s[i] != b',' {
                return Err("TZ string needs both a start and an end rule".to_string());
            }
            i += 1;
            (start, parse_rule_date(s, &mut i)?)
        } else {
            // US rules before 2007 are the conventional default.
            let mut a = RuleDate { kind: 'M', month: 3, week: 2, day: 0, time: 7200 };
            let mut b = RuleDate { kind: 'M', month: 11, week: 1, day: 0, time: 7200 };
            a.time = 7200;
            b.time = 7200;
            (a, b)
        };
        if i != s.len() {
            return Err("unexpected trailing characters in TZ string".to_string());
        }
        Ok(PosixRule { std_name, std_offset, dst: Some(DstRule { name: dst_name, offset: dst_offset, start, end }) })
    }

    /// Local midnight (as seconds since the epoch, interpreted as if UTC) of the rule's date in `year`.
    fn rule_day_start(rule: &RuleDate, year: i64) -> i64 {
        let days = match rule.kind {
            'M' => {
                let first = time_ext::days_from_civil(year, rule.month, 1);
                let first_weekday = (first + 4).rem_euclid(7);
                let mut day = 1 + (rule.day - first_weekday).rem_euclid(7) + (rule.week - 1) * 7;
                let month_len = time_ext::days_in_month(year, rule.month);
                while day > month_len {
                    day -= 7;
                }
                time_ext::days_from_civil(year, rule.month, day)
            }
            'J' => {
                let mut d = rule.day.clamp(1, 365) - 1;
                if time_ext::is_leap(year) && rule.day >= 60 {
                    d += 1;
                }
                time_ext::days_from_civil(year, 1, 1) + d
            }
            _ => time_ext::days_from_civil(year, 1, 1) + rule.day,
        };
        days * 86400
    }

    pub fn at(&self, unix: i64) -> Local {
        let std_local = Local { offset_seconds: self.std_offset, abbreviation: self.std_name.clone(), is_dst: false };
        let Some(dst) = &self.dst else { return std_local };
        let (year, _, _) = time_ext::civil_from_days((unix + self.std_offset).div_euclid(86400));
        // Transition instants in UTC: the start is expressed in standard time, the end in DST.
        let start = Self::rule_day_start(&dst.start, year) + dst.start.time - self.std_offset;
        let end = Self::rule_day_start(&dst.end, year) + dst.end.time - dst.offset;
        let in_dst = if start < end { unix >= start && unix < end } else { !(unix >= end && unix < start) };
        if in_dst {
            Local { offset_seconds: dst.offset, abbreviation: dst.name.clone(), is_dst: true }
        } else {
            std_local
        }
    }
}

// ---------------------------------------------------------------------------
// TZif (the system tz database format)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
struct TypeInfo {
    offset: i64,
    is_dst: bool,
    abbreviation: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tzif {
    transitions: Vec<i64>,
    indices: Vec<usize>,
    types: Vec<TypeInfo>,
    footer: Option<PosixRule>,
}

fn be_i32(b: &[u8]) -> i64 {
    i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as i64
}

fn be_i64(b: &[u8]) -> i64 {
    i64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

impl Tzif {
    pub fn parse(data: &[u8]) -> Result<Tzif, String> {
        let bad = || "malformed TZif data".to_string();
        if data.len() < 44 || &data[0..4] != b"TZif" {
            return Err("not a TZif file".to_string());
        }
        let version = data[4];
        let counts = |d: &[u8]| -> Result<[usize; 6], String> {
            if d.len() < 44 {
                return Err(bad());
            }
            let mut c = [0usize; 6];
            for (k, slot) in c.iter_mut().enumerate() {
                *slot = be_i32(&d[20 + 4 * k..24 + 4 * k]).max(0) as usize;
            }
            Ok(c)
        };
        // counts: isutcnt, isstdcnt, leapcnt, timecnt, typecnt, charcnt
        let c1 = counts(data)?;
        let v1_len = c1[3] * 4 + c1[3] + c1[4] * 6 + c1[5] + c1[2] * 8 + c1[1] + c1[0];
        let (body, time_size, c) = if version >= b'2' {
            let second = 44 + v1_len;
            if data.len() < second + 44 || &data[second..second + 4] != b"TZif" {
                return Err(bad());
            }
            (&data[second + 44..], 8usize, counts(&data[second..])?)
        } else {
            (&data[44..], 4usize, c1)
        };
        let (timecnt, typecnt, charcnt) = (c[3], c[4], c[5]);
        let need = timecnt * time_size + timecnt + typecnt * 6 + charcnt + c[2] * (time_size + 4) + c[1] + c[0];
        if body.len() < need || typecnt == 0 {
            return Err(bad());
        }
        let mut pos = 0;
        let mut transitions = Vec::with_capacity(timecnt);
        for _ in 0..timecnt {
            transitions.push(if time_size == 8 { be_i64(&body[pos..pos + 8]) } else { be_i32(&body[pos..pos + 4]) });
            pos += time_size;
        }
        let indices: Vec<usize> = body[pos..pos + timecnt].iter().map(|b| *b as usize).collect();
        pos += timecnt;
        let raw_types: Vec<(i64, bool, usize)> = (0..typecnt)
            .map(|k| {
                let o = pos + k * 6;
                (be_i32(&body[o..o + 4]), body[o + 4] != 0, body[o + 5] as usize)
            })
            .collect();
        pos += typecnt * 6;
        let chars = &body[pos..pos + charcnt];
        pos += charcnt + c[2] * (time_size + 4) + c[1] + c[0];
        let types: Vec<TypeInfo> = raw_types
            .into_iter()
            .map(|(offset, is_dst, ai)| {
                let end = chars[ai.min(chars.len())..].iter().position(|b| *b == 0).map_or(chars.len(), |p| ai + p);
                TypeInfo { offset, is_dst, abbreviation: String::from_utf8_lossy(&chars[ai.min(chars.len())..end]).into_owned() }
            })
            .collect();
        if indices.iter().any(|i| *i >= types.len()) {
            return Err(bad());
        }
        let mut footer = None;
        if version >= b'2' && pos < body.len() && body[pos] == b'\n' {
            let rest = &body[pos + 1..];
            if let Some(end) = rest.iter().position(|b| *b == b'\n') {
                let text = String::from_utf8_lossy(&rest[..end]).into_owned();
                if !text.is_empty() {
                    footer = PosixRule::parse(&text).ok();
                }
            }
        }
        Ok(Tzif { transitions, indices, types, footer })
    }

    pub fn at(&self, unix: i64) -> Local {
        let info_local = |t: &TypeInfo| Local { offset_seconds: t.offset, abbreviation: t.abbreviation.clone(), is_dst: t.is_dst };
        if self.transitions.is_empty() || unix < self.transitions[0] {
            let first = self.types.iter().find(|t| !t.is_dst).unwrap_or(&self.types[0]);
            if self.transitions.is_empty() {
                if let Some(rule) = &self.footer {
                    return rule.at(unix);
                }
            }
            return info_local(first);
        }
        let last = *self.transitions.last().unwrap_or(&0);
        if unix >= last {
            if let Some(rule) = &self.footer {
                return rule.at(unix);
            }
        }
        let idx = match self.transitions.binary_search(&unix) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        info_local(&self.types[self.indices[idx]])
    }
}

// ---------------------------------------------------------------------------
// Zones
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum Zone {
    Fixed { offset_seconds: i64, label: String },
    Posix(PosixRule),
    Tzif(Arc<Tzif>),
}

/// Built-in rules (current rules only), used when no tz database is installed.
const BUILTIN: &[(&str, &str)] = &[
    ("UTC", "UTC0"), ("Etc/UTC", "UTC0"), ("GMT", "GMT0"), ("Etc/GMT", "GMT0"),
    // Central European Time
    ("Europe/Ljubljana", "CET-1CEST,M3.5.0,M10.5.0/3"), ("Europe/Zagreb", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Belgrade", "CET-1CEST,M3.5.0,M10.5.0/3"), ("Europe/Sarajevo", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Skopje", "CET-1CEST,M3.5.0,M10.5.0/3"), ("Europe/Podgorica", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Berlin", "CET-1CEST,M3.5.0,M10.5.0/3"), ("Europe/Vienna", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Paris", "CET-1CEST,M3.5.0,M10.5.0/3"), ("Europe/Rome", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Madrid", "CET-1CEST,M3.5.0,M10.5.0/3"), ("Europe/Amsterdam", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Brussels", "CET-1CEST,M3.5.0,M10.5.0/3"), ("Europe/Zurich", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Prague", "CET-1CEST,M3.5.0,M10.5.0/3"), ("Europe/Budapest", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Warsaw", "CET-1CEST,M3.5.0,M10.5.0/3"), ("Europe/Stockholm", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Oslo", "CET-1CEST,M3.5.0,M10.5.0/3"), ("Europe/Copenhagen", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Bratislava", "CET-1CEST,M3.5.0,M10.5.0/3"), ("Europe/Luxembourg", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Malta", "CET-1CEST,M3.5.0,M10.5.0/3"), ("Europe/Tirane", "CET-1CEST,M3.5.0,M10.5.0/3"),
    // Western / Eastern European Time
    ("Europe/London", "GMT0BST,M3.5.0/1,M10.5.0"), ("Europe/Dublin", "GMT0IST,M3.5.0/1,M10.5.0"),
    ("Europe/Lisbon", "WET0WEST,M3.5.0/1,M10.5.0"),
    ("Europe/Athens", "EET-2EEST,M3.5.0/3,M10.5.0/4"), ("Europe/Helsinki", "EET-2EEST,M3.5.0/3,M10.5.0/4"),
    ("Europe/Kyiv", "EET-2EEST,M3.5.0/3,M10.5.0/4"), ("Europe/Kiev", "EET-2EEST,M3.5.0/3,M10.5.0/4"),
    ("Europe/Bucharest", "EET-2EEST,M3.5.0/3,M10.5.0/4"), ("Europe/Sofia", "EET-2EEST,M3.5.0/3,M10.5.0/4"),
    ("Europe/Riga", "EET-2EEST,M3.5.0/3,M10.5.0/4"), ("Europe/Tallinn", "EET-2EEST,M3.5.0/3,M10.5.0/4"),
    ("Europe/Vilnius", "EET-2EEST,M3.5.0/3,M10.5.0/4"),
    ("Europe/Moscow", "MSK-3"), ("Europe/Istanbul", "<+03>-3"), ("Europe/Minsk", "<+03>-3"),
    // Americas
    ("America/New_York", "EST5EDT,M3.2.0,M11.1.0"), ("America/Toronto", "EST5EDT,M3.2.0,M11.1.0"),
    ("America/Detroit", "EST5EDT,M3.2.0,M11.1.0"), ("America/Chicago", "CST6CDT,M3.2.0,M11.1.0"),
    ("America/Winnipeg", "CST6CDT,M3.2.0,M11.1.0"), ("America/Denver", "MST7MDT,M3.2.0,M11.1.0"),
    ("America/Edmonton", "MST7MDT,M3.2.0,M11.1.0"), ("America/Phoenix", "MST7"),
    ("America/Los_Angeles", "PST8PDT,M3.2.0,M11.1.0"), ("America/Vancouver", "PST8PDT,M3.2.0,M11.1.0"),
    ("America/Anchorage", "AKST9AKDT,M3.2.0,M11.1.0"), ("Pacific/Honolulu", "HST10"),
    ("America/Halifax", "AST4ADT,M3.2.0,M11.1.0"), ("America/St_Johns", "NST3:30NDT,M3.2.0,M11.1.0"),
    ("America/Mexico_City", "CST6"), ("America/Bogota", "<-05>5"), ("America/Lima", "<-05>5"),
    ("America/Sao_Paulo", "<-03>3"), ("America/Argentina/Buenos_Aires", "<-03>3"), ("America/Buenos_Aires", "<-03>3"),
    ("America/Caracas", "<-04>4"), ("America/Havana", "CST5CDT,M3.2.0/0,M11.1.0/1"),
    // Asia
    ("Asia/Tokyo", "JST-9"), ("Asia/Seoul", "KST-9"), ("Asia/Shanghai", "CST-8"), ("Asia/Hong_Kong", "HKT-8"),
    ("Asia/Taipei", "CST-8"), ("Asia/Singapore", "<+08>-8"), ("Asia/Manila", "PST-8"), ("Asia/Jakarta", "WIB-7"),
    ("Asia/Bangkok", "<+07>-7"), ("Asia/Ho_Chi_Minh", "<+07>-7"), ("Asia/Kolkata", "IST-5:30"),
    ("Asia/Calcutta", "IST-5:30"), ("Asia/Karachi", "PKT-5"), ("Asia/Dhaka", "<+06>-6"),
    ("Asia/Kathmandu", "<+0545>-5:45"), ("Asia/Dubai", "<+04>-4"), ("Asia/Tehran", "<+0330>-3:30"),
    ("Asia/Baghdad", "<+03>-3"), ("Asia/Riyadh", "<+03>-3"), ("Asia/Jerusalem", "IST-2IDT,M3.4.4/26,M10.5.0"),
    ("Asia/Tashkent", "<+05>-5"), ("Asia/Almaty", "<+05>-5"),
    // Africa
    ("Africa/Cairo", "EET-2EEST,M4.5.5/0,M10.5.4/24"), ("Africa/Johannesburg", "SAST-2"), ("Africa/Lagos", "WAT-1"),
    ("Africa/Nairobi", "EAT-3"), ("Africa/Casablanca", "<+01>-1"), ("Africa/Algiers", "CET-1"), ("Africa/Accra", "GMT0"),
    // Oceania
    ("Australia/Sydney", "AEST-10AEDT,M10.1.0,M4.1.0/3"), ("Australia/Melbourne", "AEST-10AEDT,M10.1.0,M4.1.0/3"),
    ("Australia/Hobart", "AEST-10AEDT,M10.1.0,M4.1.0/3"), ("Australia/Brisbane", "AEST-10"),
    ("Australia/Adelaide", "ACST-9:30ACDT,M10.1.0,M4.1.0/3"), ("Australia/Darwin", "ACST-9:30"),
    ("Australia/Perth", "AWST-8"), ("Pacific/Auckland", "NZST-12NZDT,M9.5.0,M4.1.0/3"),
    ("Pacific/Fiji", "<+12>-12"),
];

pub fn builtin_names() -> Vec<String> {
    BUILTIN.iter().map(|(n, _)| n.to_string()).collect()
}

fn valid_zone_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains("..")
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '+'))
}

fn zoneinfo_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for var in ["ZONEINFO", "TZDIR"] {
        if let Ok(v) = std::env::var(var) {
            if !v.is_empty() {
                dirs.push(PathBuf::from(v));
            }
        }
    }
    for fixed in ["/usr/share/zoneinfo", "/usr/lib/zoneinfo", "/usr/share/lib/zoneinfo", "/etc/zoneinfo"] {
        dirs.push(PathBuf::from(fixed));
    }
    dirs
}

fn cache() -> &'static Mutex<std::collections::HashMap<String, Option<Arc<Tzif>>>> {
    static CACHE: OnceLock<Mutex<std::collections::HashMap<String, Option<Arc<Tzif>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

fn embedded_database() -> &'static HashMap<String, Arc<Tzif>> {
    static DATABASE: OnceLock<HashMap<String, Arc<Tzif>>> = OnceLock::new();
    DATABASE.get_or_init(|| {
        // FZT1: magic, u16 entry count, then repeated (u16 name length, u32 data
        // length, UTF-8 relative name, TZif bytes). The source snapshot is IANA tzdata 2025b.
        let bytes: &[u8] = include_bytes!("tzdata.bin");
        let mut result = HashMap::new();
        if bytes.len() < 6 || &bytes[..4] != b"FZT1" {
            return result;
        }
        let count = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
        let mut pos = 6usize;
        for _ in 0..count {
            if pos + 6 > bytes.len() { break; }
            let name_len = u16::from_be_bytes([bytes[pos], bytes[pos + 1]]) as usize;
            let data_len = u32::from_be_bytes([bytes[pos + 2], bytes[pos + 3], bytes[pos + 4], bytes[pos + 5]]) as usize;
            pos += 6;
            let Some(end_name) = pos.checked_add(name_len) else { break };
            let Some(end_data) = end_name.checked_add(data_len) else { break };
            if end_data > bytes.len() { break; }
            let name = match std::str::from_utf8(&bytes[pos..end_name]) { Ok(v) => v, Err(_) => break };
            if valid_zone_name(name) {
                if let Ok(zone) = Tzif::parse(&bytes[end_name..end_data]) {
                    result.insert(name.to_string(), Arc::new(zone));
                }
            }
            pos = end_data;
        }
        result
    })
}

fn load_system(name: &str) -> Option<Arc<Tzif>> {
    if let Some(hit) = cache().lock().ok()?.get(name) {
        return hit.clone();
    }
    let mut found = None;
    // Prefer the administrator's current timezone database when available.
    for dir in zoneinfo_dirs() {
        if let Ok(data) = std::fs::read(dir.join(name)) {
            if let Ok(tz) = Tzif::parse(&data) {
                found = Some(Arc::new(tz));
                break;
            }
        }
    }
    // A bundled IANA snapshot guarantees historical data on hosts without a tzdb.
    if found.is_none() {
        found = embedded_database().get(name).cloned();
    }
    cache().lock().ok()?.insert(name.to_string(), found.clone());
    found
}

impl Zone {
    /// Resolves `UTC`, `Z`, a fixed offset (`+02:00`), an IANA name (`Europe/Ljubljana`) or a POSIX TZ string.
    pub fn parse(name: &str) -> Result<Zone, String> {
        let t = name.trim();
        if let Ok(minutes) = time_ext::parse_offset(t) {
            let label = if minutes == 0 { "UTC".to_string() } else { time_ext::format_offset(minutes, true) };
            return Ok(Zone::Fixed { offset_seconds: minutes * 60, label });
        }
        if valid_zone_name(t) {
            if let Some(tz) = load_system(t) {
                return Ok(Zone::Tzif(tz));
            }
            if let Some((_, rule)) = BUILTIN.iter().find(|(n, _)| n.eq_ignore_ascii_case(t)) {
                return PosixRule::parse(rule).map(Zone::Posix);
            }
        }
        if t.len() >= 4 && t.as_bytes()[0].is_ascii_alphabetic() || t.starts_with('<') {
            if let Ok(rule) = PosixRule::parse(t) {
                return Ok(Zone::Posix(rule));
            }
        }
        Err(format!("unknown time zone '{}': use UTC, a fixed offset like +02:00, an IANA name like Europe/Ljubljana or a POSIX TZ string", name))
    }

    pub fn at(&self, unix: i64) -> Local {
        match self {
            Zone::Fixed { offset_seconds, label } => Local { offset_seconds: *offset_seconds, abbreviation: label.clone(), is_dst: false },
            Zone::Posix(rule) => rule.at(unix),
            Zone::Tzif(tz) => tz.at(unix),
        }
    }

    /// Converts a local wall-clock time (as seconds since the epoch if it were UTC) to an instant.
    /// For a repeated hour (clocks going back) the first occurrence is chosen; for a skipped hour
    /// (clocks going forward) the time is interpreted with the offset in effect before the gap.
    pub fn local_to_unix(&self, naive: i64) -> i64 {
        let before = self.at(naive - 86400).offset_seconds;
        let after = self.at(naive + 86400).offset_seconds;
        let mut best: Option<i64> = None;
        for offset in [before, after] {
            let candidate = naive - offset;
            if self.at(candidate).offset_seconds == offset {
                best = Some(best.map_or(candidate, |b| b.min(candidate)));
            }
        }
        best.unwrap_or(naive - before)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(y: i64, m: i64, d: i64, h: i64, mi: i64) -> i64 {
        time_ext::from_parts(y, m, d, h, mi, 0, 0).unwrap()
    }

    #[test]
    fn posix_string_parsing() {
        let r = PosixRule::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap();
        assert_eq!(r.std_offset, 3600);
        assert_eq!(r.dst.as_ref().unwrap().offset, 7200);
        assert!(PosixRule::parse("CET").is_err());
        assert_eq!(PosixRule::parse("IST-5:30").unwrap().std_offset, 19800);
        assert_eq!(PosixRule::parse("<-03>3").unwrap().std_offset, -10800);
    }

    #[test]
    fn central_europe_switches_on_the_last_sunday() {
        let z = Zone::Posix(PosixRule::parse("CET-1CEST,M3.5.0,M10.5.0/3").unwrap());
        // 2026: DST starts Sunday 29 March at 01:00 UTC and ends Sunday 25 October at 01:00 UTC.
        assert_eq!(z.at(utc(2026, 3, 29, 0, 59)).offset_seconds, 3600);
        assert_eq!(z.at(utc(2026, 3, 29, 1, 0)).offset_seconds, 7200);
        assert_eq!(z.at(utc(2026, 7, 1, 12, 0)).abbreviation, "CEST");
        assert_eq!(z.at(utc(2026, 10, 25, 0, 59)).offset_seconds, 7200);
        assert_eq!(z.at(utc(2026, 10, 25, 1, 0)).offset_seconds, 3600);
        assert_eq!(z.at(utc(2026, 11, 1, 12, 0)).abbreviation, "CET");
    }

    #[test]
    fn southern_hemisphere_and_us_rules() {
        let sydney = Zone::parse("Australia/Sydney").unwrap();
        assert_eq!(sydney.at(utc(2026, 1, 15, 0, 0)).offset_seconds, 11 * 3600);
        assert_eq!(sydney.at(utc(2026, 7, 15, 0, 0)).offset_seconds, 10 * 3600);
        let ny = Zone::parse("America/New_York").unwrap();
        // 2026: DST starts Sunday 8 March, ends Sunday 1 November.
        assert_eq!(ny.at(utc(2026, 3, 8, 6, 59)).offset_seconds, -5 * 3600);
        assert_eq!(ny.at(utc(2026, 3, 8, 7, 0)).offset_seconds, -4 * 3600);
        assert_eq!(ny.at(utc(2026, 11, 1, 5, 59)).offset_seconds, -4 * 3600);
        assert_eq!(ny.at(utc(2026, 11, 1, 6, 0)).offset_seconds, -5 * 3600);
    }

    #[test]
    fn bundled_database_preserves_pre_standard_time_history() {
        let zone = embedded_database().get("Europe/Ljubljana").expect("bundled Ljubljana TZif entry");
        // Ljubljana used local mean time (+00:58:04) before standard time began in 1884.
        let before = utc(1883, 12, 31, 23, 1);
        let transition = utc(1883, 12, 31, 23, 1) + 56;
        assert_eq!(zone.at(before).offset_seconds, 3484);
        assert_eq!(zone.at(transition).offset_seconds, 3600);
    }

    #[test]
    fn named_zone_lookup_and_fixed_offsets() {
        assert!(matches!(Zone::parse("UTC").unwrap(), Zone::Fixed { .. }));
        assert_eq!(Zone::parse("+05:30").unwrap().at(0).offset_seconds, 19800);
        assert!(Zone::parse("europe/ljubljana").is_ok() || Zone::parse("Europe/Ljubljana").is_ok());
        assert!(Zone::parse("Mars/Olympus_Mons").is_err());
        assert!(Zone::parse("../../etc/passwd").is_err());
    }

    #[test]
    fn local_times_around_transitions() {
        let z = Zone::parse("Europe/Ljubljana").unwrap();
        let naive = |y, m, d, h, mi| time_ext::from_parts(y, m, d, h, mi, 0, 0).unwrap();
        // Ordinary summer time: 12:00 local is 10:00 UTC.
        assert_eq!(z.local_to_unix(naive(2026, 7, 1, 12, 0)), utc(2026, 7, 1, 10, 0));
        // Repeated hour on 25 Oct 2026: 02:30 happens twice; the first (CEST) is chosen.
        assert_eq!(z.local_to_unix(naive(2026, 10, 25, 2, 30)), utc(2026, 10, 25, 0, 30));
        // Skipped hour on 29 Mar 2026: 02:30 does not exist; the pre-gap offset (+01:00) applies.
        assert_eq!(z.local_to_unix(naive(2026, 3, 29, 2, 30)), utc(2026, 3, 29, 1, 30));
    }

    #[test]
    fn tzif_round_trip_of_a_synthetic_file() {
        // Version 2 file: one type (UTC+1 "XST"), no transitions, footer "XST-1".
        let header = |version: u8| {
            let mut h = b"TZif".to_vec();
            h.push(version);
            h.extend_from_slice(&[0u8; 15]);
            for count in [0u32, 0, 0, 0, 1, 4] {
                h.extend_from_slice(&count.to_be_bytes());
            }
            h
        };
        let mut data = header(b'2');
        data.extend_from_slice(&3600i32.to_be_bytes());
        data.extend_from_slice(&[0, 0]);
        data.extend_from_slice(b"XST\0");
        data.extend_from_slice(&header(b'2'));
        data.extend_from_slice(&3600i32.to_be_bytes());
        data.extend_from_slice(&[0, 0]);
        data.extend_from_slice(b"XST\0");
        data.extend_from_slice(b"\nXST-1\n");
        let tz = Tzif::parse(&data).unwrap();
        let local = tz.at(1_000_000);
        assert_eq!((local.offset_seconds, local.abbreviation.as_str()), (3600, "XST"));
        assert!(Tzif::parse(b"nope").is_err());
    }
}
