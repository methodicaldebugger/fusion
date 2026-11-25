//! Standard-library builtins added in 0.3: text, data formats, crypto, time, networking,
//! file system/process, concurrency and test assertions.
//!
//! The interpreter calls `is_builtin` / `call`; the type checker reads `signatures`.
//! All heavy lifting lives in the sibling `*_ext` modules, which depend only on `std`.

use crate::conc_ext;
use crate::crypto_ext;
use crate::data_ext::{self, Data};
use crate::interpreter::Interpreter;
use crate::net_ext;
use crate::os_ext;
use crate::text_ext::{self, Regex};
use crate::time_ext;
use crate::types::Type;
use crate::tz_ext::{self, Zone};
use crate::value::Value;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Duration;

// Parameter / return codes:
//   s String   n Num   f Float   b Bool   a Array<Unknown>   S Array<String>
//   u Unknown  m HashMap   o Option<Unknown>   r Result<Unknown, Unknown>   v Void
const SIGS: &[(&str, &str, char)] = &[
    // text
    ("regex_is_match", "ss", 'b'),
    ("regex_find", "ss", 'o'),
    ("regex_find_all", "ss", 'S'),
    ("regex_captures", "ss", 'o'),
    ("regex_replace", "sss", 's'),
    ("regex_split", "ss", 'S'),
    ("utf8_valid", "a", 'b'),
    ("utf8_encode", "s", 'a'),
    ("utf8_decode", "a", 'o'),
    ("str_char_count", "s", 'n'),
    ("str_byte_len", "s", 'n'),
    ("str_find", "ss", 'n'),
    ("str_slice", "snn", 's'),
    ("str_join", "as", 's'),
    ("str_repeat", "sn", 's'),
    ("str_parse_int", "s", 'o'),
    ("str_parse_float", "s", 'o'),
    // data formats
    ("json_parse", "s", 'u'),
    ("json_try_parse", "s", 'o'),
    ("json_stringify", "u", 's'),
    ("json_pretty", "u", 's'),
    ("toml_parse", "s", 'u'),
    ("toml_stringify", "u", 's'),
    // crypto
    ("sha256", "s", 's'),
    ("hmac_sha256", "ss", 's'),
    ("constant_time_eq", "ss", 'b'),
    ("random_bytes_hex", "n", 's'),
    ("secure_random_int", "nn", 'n'),
    ("secure_random_float", "", 'f'),
    ("uuid4", "", 's'),
    // time
    ("date_format", "nss", 's'),
    ("date_parse", "s", 'o'),
    ("date_parts", "ns", 'a'),
    ("date_from_parts", "nnnnnns", 'n'),
    ("date_add_months", "nns", 'n'),
    ("days_in_month", "nn", 'n'),
    ("is_leap_year", "n", 'b'),
    ("now_iso", "", 's'),
    ("tz_offset", "ns", 'n'),
    ("tz_abbrev", "ns", 's'),
    ("tz_is_valid", "s", 'b'),
    ("tz_names", "", 'S'),
    // paths and file system
    ("path_join", "ss", 's'),
    ("path_parent", "s", 'o'),
    ("path_file_name", "s", 'o'),
    ("path_extension", "s", 'o'),
    ("path_stem", "s", 'o'),
    ("path_is_absolute", "s", 'b'),
    ("path_normalize", "s", 's'),
    ("path_is_file", "s", 'b'),
    ("path_is_dir", "s", 'b'),
    ("file_size", "s", 'n'),
    ("modified_time", "s", 'n'),
    ("copy_file", "ss", 'n'),
    ("rename_path", "ss", 'v'),
    ("remove_dir", "s", 'v'),
    ("append_file", "ss", 'v'),
    ("read_lines", "s", 'S'),
    ("walk_dir", "s", 'S'),
    ("is_readonly", "s", 'b'),
    ("set_readonly", "sb", 'v'),
    ("file_mode", "s", 'n'),
    ("set_file_mode", "sn", 'v'),
    ("temp_dir", "", 's'),
    ("home_dir", "", 'o'),
    ("env_set", "ss", 'v'),
    ("env_vars", "", 'm'),
    // processes
    ("process_output", "sS", 'm'),
    ("process_output_input", "sSs", 'm'),
    ("process_spawn", "sS", 'n'),
    ("process_write", "ns", 'v'),
    ("process_close_stdin", "n", 'v'),
    ("process_try_wait", "n", 'o'),
    ("process_kill", "n", 'v'),
    ("process_wait", "n", 'm'),
    // networking
    ("tcp_listen", "sn", 'n'),
    ("tcp_dial", "sn", 'n'),
    ("tcp_accept", "n", 'n'),
    ("tcp_read", "nn", 's'),
    ("tcp_write", "ns", 'n'),
    ("tcp_close", "n", 'v'),
    ("tcp_set_timeout", "nn", 'v'),
    ("tcp_local_port", "n", 'n'),
    ("tcp_peer_addr", "n", 's'),
    ("udp_bind", "sn", 'n'),
    ("udp_send_to", "nsns", 'n'),
    ("udp_recv_from", "nn", 'a'),
    ("udp_close", "n", 'v'),
    ("http_request", "ssms", 'm'),
    ("http_get", "s", 'm'),
    ("http_post", "ss", 'm'),
    ("http_client_open", "sn", 'n'),
    ("http_client_request", "nssms", 'm'),
    ("http_client_close", "n", 'v'),
    ("http_listen", "sn", 'n'),
    ("https_listen", "snss", 'n'),
    ("http_accept", "n", 'm'),
    ("http_respond", "nnms", 'v'),
    ("url_parse_query", "s", 'm'),
    ("url_encode", "s", 's'),
    ("url_decode", "s", 's'),
    // concurrency
    ("task_spawn", "sa", 'n'),
    ("task_join", "n", 'u'),
    ("task_join_all", "a", 'a'),
    ("task_is_finished", "n", 'b'),
    ("pool_spawn", "sa", 'n'),
    ("scope_new", "", 'n'),
    ("scope_spawn", "nsa", 'n'),
    ("scope_join", "n", 'a'),
    ("scope_cancel", "n", 'v'),
    ("scope_is_cancelled", "n", 'b'),
    ("parallel_map", "sa", 'a'),
    ("cpu_count", "", 'n'),
    ("channel_new", "n", 'n'),
    ("channel_send", "nu", 'v'),
    ("channel_recv", "n", 'o'),
    ("channel_try_recv", "n", 'o'),
    ("channel_recv_timeout", "nn", 'o'),
    ("channel_close", "n", 'v'),
    ("channel_len", "n", 'n'),
    ("counter_new", "n", 'n'),
    ("counter_add", "nn", 'n'),
    ("counter_get", "n", 'n'),
    // option and map helpers
    ("option_is_some", "o", 'b'),
    ("option_unwrap", "o", 'u'),
    ("option_unwrap_or", "ou", 'u'),
    ("map_get_or", "muu", 'u'),
    // Option and Result constructors (`Some(x)`, `None`, `Ok(x)`, `Err(e)`) and helpers. Their
    // type arguments are `Unknown`, which the checker accepts against any `Option<T>` /
    // `Result<T, E>` annotation. `None` is parsed as the call `None()`.
    ("Some", "u", 'o'),
    ("None", "", 'o'),
    ("Ok", "u", 'r'),
    ("Err", "u", 'r'),
    ("option_is_none", "o", 'b'),
    ("result_is_ok", "r", 'b'),
    ("result_is_err", "r", 'b'),
    ("result_unwrap", "r", 'u'),
    ("result_unwrap_err", "r", 'u'),
    ("result_unwrap_or", "ru", 'u'),
    // Higher-order functions: the second argument is a function value (`Unknown` here).
    ("array_map", "au", 'a'),
    ("array_filter", "au", 'a'),
    ("array_fold", "auu", 'u'),
    ("array_for_each", "au", 'v'),
    ("array_any", "au", 'b'),
    ("array_all", "au", 'b'),
    ("array_sort_by", "au", 'a'),
    ("option_map", "ou", 'o'),
    ("option_and_then", "ou", 'o'),
    ("result_map", "ru", 'r'),
    ("result_map_err", "ru", 'r'),
    ("result_and_then", "ru", 'r'),
    // assertions (for #[test] functions)
    ("assert", "b", 'v'),
    ("assert_msg", "bs", 'v'),
    ("assert_eq", "uu", 'v'),
    ("assert_ne", "uu", 'v'),
    ("test_fail", "s", 'v'),
];

fn table() -> &'static HashMap<&'static str, (&'static str, char)> {
    static TABLE: OnceLock<HashMap<&'static str, (&'static str, char)>> = OnceLock::new();
    TABLE.get_or_init(|| SIGS.iter().map(|(n, p, r)| (*n, (*p, *r))).collect())
}

pub fn is_builtin(name: &str) -> bool {
    table().contains_key(name)
}

fn kind_type(code: char) -> Type {
    match code {
        's' => Type::String,
        'n' => Type::Num,
        'f' => Type::Float,
        'b' => Type::Bool,
        'a' => Type::Array(Box::new(Type::Unknown)),
        'S' => Type::Array(Box::new(Type::String)),
        'm' => Type::HashMap(Box::new(Type::Unknown), Box::new(Type::Unknown)),
        'o' => Type::Option(Box::new(Type::Unknown)),
        'r' => Type::Result(Box::new(Type::Unknown), Box::new(Type::Unknown)),
        'v' => Type::Void,
        _ => Type::Unknown,
    }
}

/// Signatures for the type checker.
pub fn signatures() -> Vec<(&'static str, Vec<Type>, Type)> {
    SIGS.iter()
        .map(|(name, params, ret)| {
            (
                *name,
                params.chars().map(kind_type).collect(),
                kind_type(*ret),
            )
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Value <-> Data
// ---------------------------------------------------------------------------

pub fn value_to_data(value: &Value) -> Result<Data, String> {
    Ok(match value {
        Value::Number(n) => Data::Int(*n),
        Value::Wide(w) => return Err(format!("integer {} does not fit in 64 bits", w)),
        Value::Float(f) => Data::Float(*f),
        Value::String(s) => Data::Str(s.clone()),
        Value::Boolean(b) => Data::Bool(*b),
        Value::Array(items) | Value::Tuple(items) | Value::Iterator(items) => Data::List(
            items
                .iter()
                .map(value_to_data)
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Value::HashMap(entries) => {
            let mut out = Vec::new();
            for (k, v) in entries {
                out.push((k.to_string(), value_to_data(v)?));
            }
            Data::Map(out)
        }
        Value::Struct { fields, .. } => {
            let mut keys: Vec<&String> = fields.keys().collect();
            keys.sort();
            let mut out = Vec::new();
            for k in keys {
                out.push((k.clone(), value_to_data(&fields[k])?));
            }
            Data::Map(out)
        }
        Value::Enum {
            variant, values, ..
        } => Data::Map(vec![
            ("variant".to_string(), Data::Str(variant.clone())),
            (
                "values".to_string(),
                Data::List(
                    values
                        .iter()
                        .map(value_to_data)
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            ),
        ]),
        Value::Option(Some(inner)) => value_to_data(inner)?,
        Value::Option(None) | Value::None => Data::Null,
        Value::Task(inner) => value_to_data(inner)?,
        Value::File(_) | Value::TcpStream(_) => {
            return Err("files and streams cannot be copied".to_string())
        }
        Value::Function(_) | Value::Builtin(_) => return Err("function values cannot be copied".to_string()),
    })
}

pub fn data_to_value(data: Data) -> Value {
    match data {
        Data::Null => Value::Option(None),
        Data::Bool(b) => Value::Boolean(b),
        Data::Int(n) => Value::Number(n),
        Data::Float(f) => Value::Float(f),
        Data::Str(s) => Value::String(s),
        Data::List(items) => Value::Array(items.into_iter().map(data_to_value).collect()),
        Data::Map(entries) => Value::HashMap(
            entries
                .into_iter()
                .map(|(k, v)| (Value::String(k), data_to_value(v)))
                .collect(),
        ),
    }
}

// ---------------------------------------------------------------------------
// Argument helpers (runtime errors follow the interpreter's panic convention)
// ---------------------------------------------------------------------------

fn callback_array(a: &[Value], i: usize, f: &str) -> Vec<Value> {
    match a.get(i) {
        Some(Value::Array(items)) => items.clone(),
        _ => panic!("{}() expects an array as argument {}", f, i + 1),
    }
}

fn callback_bool(value: Value, f: &str) -> bool {
    match value {
        Value::Boolean(flag) => flag,
        other => panic!("{}() callback must return a bool, found {}", f, other),
    }
}

fn result_value(variant: &str, payload: Value) -> Value {
    Value::Enum {
        enum_name: "Result".to_string(),
        variant: variant.to_string(),
        values: vec![payload],
    }
}

fn s(a: &[Value], i: usize, f: &str) -> String {
    match a.get(i) {
        Some(Value::String(x)) => x.clone(),
        _ => panic!("{}() expects argument {} to be a string", f, i + 1),
    }
}

fn n(a: &[Value], i: usize, f: &str) -> i64 {
    match a.get(i) {
        Some(Value::Number(x)) => *x,
        _ => panic!("{}() expects argument {} to be a number", f, i + 1),
    }
}

fn b(a: &[Value], i: usize, f: &str) -> bool {
    match a.get(i) {
        Some(Value::Boolean(x)) => *x,
        _ => panic!("{}() expects argument {} to be a bool", f, i + 1),
    }
}

fn arr(a: &[Value], i: usize, f: &str) -> Vec<Value> {
    match a.get(i) {
        Some(Value::Array(x)) | Some(Value::Iterator(x)) => x.clone(),
        _ => panic!("{}() expects argument {} to be an array", f, i + 1),
    }
}

fn strings(a: &[Value], i: usize, f: &str) -> Vec<String> {
    arr(a, i, f)
        .into_iter()
        .map(|v| match v {
            Value::String(x) => x,
            other => panic!("{}() expects an array of strings, found {}", f, other),
        })
        .collect()
}

fn header_pairs(a: &[Value], i: usize, f: &str) -> Vec<(String, String)> {
    match a.get(i) {
        Some(Value::HashMap(entries)) => entries
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        _ => panic!("{}() expects argument {} to be a map of headers", f, i + 1),
    }
}

fn ok<T>(f: &str, r: Result<T, String>) -> T {
    match r {
        Ok(v) => v,
        Err(e) => panic!("{}: {}", f, e),
    }
}

fn some(v: Value) -> Value {
    Value::Option(Some(Box::new(v)))
}

fn none() -> Value {
    Value::Option(None)
}

fn opt_string(o: Option<String>) -> Value {
    match o {
        Some(x) => some(Value::String(x)),
        None => none(),
    }
}

fn str_array(items: Vec<String>) -> Value {
    Value::Array(items.into_iter().map(Value::String).collect())
}

fn byte_array(a: &[Value], i: usize, f: &str) -> Vec<u8> {
    arr(a, i, f)
        .into_iter()
        .map(|v| match v {
            Value::Number(x) if (0..=255).contains(&x) => x as u8,
            _ => panic!("{}() expects an array of numbers from 0 to 255", f),
        })
        .collect()
}

fn process_value(r: os_ext::ProcessResult) -> Value {
    Value::HashMap(vec![
        (Value::String("status".into()), Value::Number(r.status)),
        (Value::String("stdout".into()), Value::String(r.stdout)),
        (Value::String("stderr".into()), Value::String(r.stderr)),
    ])
}

fn regex(f: &str, pattern: &str) -> Regex {
    ok(
        f,
        Regex::new(pattern).map_err(|e| format!("invalid pattern '{}': {}", pattern, e)),
    )
}

fn http_value(f: &str, r: Result<Data, String>) -> Value {
    data_to_value(ok(f, r))
}

fn char_slice(text: &str, start: i64, end: i64) -> String {
    let len = text.chars().count() as i64;
    let lo = start.clamp(0, len);
    let hi = end.clamp(lo, len);
    text.chars()
        .skip(lo as usize)
        .take((hi - lo) as usize)
        .collect()
}

// ---------------------------------------------------------------------------
// Dispatcher
// ---------------------------------------------------------------------------

pub fn call(interp: &mut Interpreter, name: &str, a: Vec<Value>) -> Value {
    let (params, _) = table().get(name).copied().unwrap_or(("", 'v'));
    if a.len() != params.len() {
        panic!(
            "{}() expects {} argument(s), got {}",
            name,
            params.len(),
            a.len()
        );
    }
    let f = name;
    match name {
        // ---- text ----
        "regex_is_match" => Value::Boolean(regex(f, &s(&a, 0, f)).is_match(&s(&a, 1, f))),
        "regex_find" => opt_string(regex(f, &s(&a, 0, f)).find(&s(&a, 1, f))),
        "regex_find_all" => str_array(regex(f, &s(&a, 0, f)).find_all(&s(&a, 1, f))),
        "regex_captures" => match regex(f, &s(&a, 0, f)).captures(&s(&a, 1, f)) {
            Some(groups) => some(Value::Array(groups.into_iter().map(opt_string).collect())),
            None => none(),
        },
        "regex_replace" => {
            Value::String(regex(f, &s(&a, 0, f)).replace_all(&s(&a, 1, f), &s(&a, 2, f)))
        }
        "regex_split" => str_array(regex(f, &s(&a, 0, f)).split(&s(&a, 1, f))),
        "utf8_valid" => Value::Boolean(text_ext::utf8_valid(&byte_array(&a, 0, f))),
        "utf8_encode" => Value::Array(
            text_ext::utf8_encode(&s(&a, 0, f))
                .into_iter()
                .map(|x| Value::Number(x as i64))
                .collect(),
        ),
        "utf8_decode" => match text_ext::utf8_decode(&byte_array(&a, 0, f)) {
            Ok(text) => some(Value::String(text)),
            Err(_) => none(),
        },
        "str_char_count" => Value::Number(s(&a, 0, f).chars().count() as i64),
        "str_byte_len" => Value::Number(s(&a, 0, f).len() as i64),
        "str_find" => {
            let (text, needle) = (s(&a, 0, f), s(&a, 1, f));
            match text.find(&needle) {
                Some(byte) => Value::Number(text[..byte].chars().count() as i64),
                None => Value::Number(-1),
            }
        }
        "str_slice" => Value::String(char_slice(&s(&a, 0, f), n(&a, 1, f), n(&a, 2, f))),
        "str_join" => {
            let parts: Vec<String> = arr(&a, 0, f).iter().map(|v| v.to_string()).collect();
            Value::String(parts.join(&s(&a, 1, f)))
        }
        "str_repeat" => Value::String(s(&a, 0, f).repeat(n(&a, 1, f).clamp(0, 1 << 24) as usize)),
        "str_parse_int" => match s(&a, 0, f).trim().parse::<i64>() {
            Ok(v) => some(Value::Number(v)),
            Err(_) => none(),
        },
        "str_parse_float" => match s(&a, 0, f).trim().parse::<f64>() {
            Ok(v) => some(Value::Float(v)),
            Err(_) => none(),
        },

        // ---- data formats ----
        "json_parse" => data_to_value(ok(f, data_ext::json_parse(&s(&a, 0, f)))),
        "json_try_parse" => match data_ext::json_parse(&s(&a, 0, f)) {
            Ok(d) => some(data_to_value(d)),
            Err(_) => none(),
        },
        "json_stringify" => Value::String(data_ext::json_stringify(
            &ok(f, value_to_data(&a[0])),
            false,
        )),
        "json_pretty" => {
            Value::String(data_ext::json_stringify(&ok(f, value_to_data(&a[0])), true))
        }
        "toml_parse" => data_to_value(ok(f, data_ext::toml_parse(&s(&a, 0, f)))),
        "toml_stringify" => Value::String(ok(
            f,
            data_ext::toml_stringify(&ok(f, value_to_data(&a[0]))),
        )),

        // ---- crypto ----
        "sha256" => Value::String(crypto_ext::to_hex(&crypto_ext::sha256(
            s(&a, 0, f).as_bytes(),
        ))),
        "hmac_sha256" => Value::String(crypto_ext::to_hex(&crypto_ext::hmac_sha256(
            s(&a, 0, f).as_bytes(),
            s(&a, 1, f).as_bytes(),
        ))),
        "constant_time_eq" => Value::Boolean(crypto_ext::constant_time_eq(
            s(&a, 0, f).as_bytes(),
            s(&a, 1, f).as_bytes(),
        )),
        "random_bytes_hex" => Value::String(crypto_ext::to_hex(&ok(
            f,
            crypto_ext::secure_random_bytes(n(&a, 0, f).clamp(0, 1 << 20) as usize),
        ))),
        "secure_random_int" => Value::Number(ok(
            f,
            crypto_ext::secure_random_range(n(&a, 0, f), n(&a, 1, f)),
        )),
        "secure_random_float" => Value::Float(
            (ok(f, crypto_ext::secure_random_u64()) >> 11) as f64 / (1u64 << 53) as f64,
        ),
        "uuid4" => {
            let mut x = ok(f, crypto_ext::secure_random_bytes(16));
            x[6] = (x[6] & 0x0f) | 0x40;
            x[8] = (x[8] & 0x3f) | 0x80;
            let h = crypto_ext::to_hex(&x);
            Value::String(format!(
                "{}-{}-{}-{}-{}",
                &h[0..8],
                &h[8..12],
                &h[12..16],
                &h[16..20],
                &h[20..32]
            ))
        }

        // ---- time ----
        "date_format" => {
            let t = n(&a, 0, f);
            let local = ok(f, Zone::parse(&s(&a, 2, f))).at(t);
            Value::String(time_ext::format_with(
                t,
                &s(&a, 1, f),
                local.offset_seconds.div_euclid(60),
                Some(&local.abbreviation),
            ))
        }
        "date_parse" => match time_ext::parse(&s(&a, 0, f)) {
            Ok(t) => some(Value::Number(t)),
            Err(_) => none(),
        },
        "date_parts" => {
            let t = n(&a, 0, f);
            let off = ok(f, Zone::parse(&s(&a, 1, f)))
                .at(t)
                .offset_seconds
                .div_euclid(60);
            let p = time_ext::parts(t, off);
            Value::Array(
                [
                    p.year, p.month, p.day, p.hour, p.minute, p.second, p.weekday, p.yday,
                ]
                .iter()
                .map(|v| Value::Number(*v))
                .collect(),
            )
        }
        "date_from_parts" => {
            let zone = ok(f, Zone::parse(&s(&a, 6, f)));
            let naive = ok(
                f,
                time_ext::from_parts(
                    n(&a, 0, f),
                    n(&a, 1, f),
                    n(&a, 2, f),
                    n(&a, 3, f),
                    n(&a, 4, f),
                    n(&a, 5, f),
                    0,
                ),
            );
            Value::Number(zone.local_to_unix(naive))
        }
        "date_add_months" => {
            let zone = ok(f, Zone::parse(&s(&a, 2, f)));
            let t = n(&a, 0, f);
            let off = zone.at(t).offset_seconds.div_euclid(60);
            // Month arithmetic happens on the wall-clock time; the offset is then re-resolved.
            let naive = time_ext::add_months(t, n(&a, 1, f), off) + off * 60;
            Value::Number(zone.local_to_unix(naive))
        }
        "tz_offset" => Value::Number(
            ok(f, Zone::parse(&s(&a, 1, f)))
                .at(n(&a, 0, f))
                .offset_seconds
                .div_euclid(60),
        ),
        "tz_abbrev" => Value::String(
            ok(f, Zone::parse(&s(&a, 1, f)))
                .at(n(&a, 0, f))
                .abbreviation,
        ),
        "tz_is_valid" => Value::Boolean(Zone::parse(&s(&a, 0, f)).is_ok()),
        "tz_names" => str_array(tz_ext::builtin_names()),
        "days_in_month" => {
            let m = n(&a, 1, f);
            if !(1..=12).contains(&m) {
                panic!("days_in_month(): month must be 1..=12");
            }
            Value::Number(time_ext::days_in_month(n(&a, 0, f), m))
        }
        "is_leap_year" => Value::Boolean(time_ext::is_leap(n(&a, 0, f))),
        "now_iso" => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            Value::String(time_ext::format(now, "%Y-%m-%dT%H:%M:%SZ", 0))
        }

        // ---- paths and file system ----
        "path_join" => Value::String(os_ext::path_join(&s(&a, 0, f), &s(&a, 1, f))),
        "path_parent" => opt_string(os_ext::path_parent(&s(&a, 0, f))),
        "path_file_name" => opt_string(os_ext::path_file_name(&s(&a, 0, f))),
        "path_extension" => opt_string(os_ext::path_extension(&s(&a, 0, f))),
        "path_stem" => opt_string(os_ext::path_stem(&s(&a, 0, f))),
        "path_is_absolute" => Value::Boolean(os_ext::path_is_absolute(&s(&a, 0, f))),
        "path_normalize" => Value::String(os_ext::path_normalize(&s(&a, 0, f))),
        "path_is_file" => Value::Boolean(os_ext::is_file(&s(&a, 0, f))),
        "path_is_dir" => Value::Boolean(os_ext::is_dir(&s(&a, 0, f))),
        "file_size" => Value::Number(ok(f, os_ext::file_size(&s(&a, 0, f)))),
        "modified_time" => Value::Number(ok(f, os_ext::modified_time(&s(&a, 0, f)))),
        "copy_file" => Value::Number(ok(f, os_ext::copy_file(&s(&a, 0, f), &s(&a, 1, f)))),
        "rename_path" => {
            ok(f, os_ext::rename_path(&s(&a, 0, f), &s(&a, 1, f)));
            Value::None
        }
        "remove_dir" => {
            ok(f, os_ext::remove_dir(&s(&a, 0, f)));
            Value::None
        }
        "append_file" => {
            ok(f, os_ext::append_file(&s(&a, 0, f), &s(&a, 1, f)));
            Value::None
        }
        "read_lines" => str_array(ok(f, os_ext::read_lines(&s(&a, 0, f)))),
        "walk_dir" => str_array(ok(f, os_ext::walk_dir(&s(&a, 0, f)))),
        "is_readonly" => Value::Boolean(ok(f, os_ext::is_readonly(&s(&a, 0, f)))),
        "set_readonly" => {
            ok(f, os_ext::set_readonly(&s(&a, 0, f), b(&a, 1, f)));
            Value::None
        }
        "file_mode" => Value::Number(ok(f, os_ext::file_mode(&s(&a, 0, f)))),
        "set_file_mode" => {
            ok(f, os_ext::set_file_mode(&s(&a, 0, f), n(&a, 1, f)));
            Value::None
        }
        "temp_dir" => Value::String(os_ext::temp_dir()),
        "home_dir" => opt_string(os_ext::home_dir()),
        "env_set" => {
            let (k, v) = (s(&a, 0, f), s(&a, 1, f));
            if k.is_empty() || k.contains('=') || k.contains('\0') || v.contains('\0') {
                panic!("env_set(): invalid variable name or value");
            }
            std::env::set_var(k, v);
            Value::None
        }
        "env_vars" => {
            let mut vars: Vec<(String, String)> = std::env::vars().collect();
            vars.sort();
            Value::HashMap(
                vars.into_iter()
                    .map(|(k, v)| (Value::String(k), Value::String(v)))
                    .collect(),
            )
        }

        // ---- processes ----
        "process_output" => process_value(ok(
            f,
            os_ext::process_output(&s(&a, 0, f), &strings(&a, 1, f), None),
        )),
        "process_output_input" => process_value(ok(
            f,
            os_ext::process_output(&s(&a, 0, f), &strings(&a, 1, f), Some(&s(&a, 2, f))),
        )),
        "process_spawn" => Value::Number(ok(
            f,
            os_ext::process_spawn(&s(&a, 0, f), &strings(&a, 1, f)),
        )),
        "process_write" => {
            ok(f, os_ext::process_write(n(&a, 0, f), &s(&a, 1, f)));
            Value::None
        }
        "process_close_stdin" => {
            ok(f, os_ext::process_close_stdin(n(&a, 0, f)));
            Value::None
        }
        "process_try_wait" => match ok(f, os_ext::process_try_wait(n(&a, 0, f))) {
            Some(code) => some(Value::Number(code)),
            None => none(),
        },
        "process_kill" => {
            ok(f, os_ext::process_kill(n(&a, 0, f)));
            Value::None
        }
        "process_wait" => process_value(ok(f, os_ext::process_wait(n(&a, 0, f)))),

        // ---- networking ----
        "tcp_listen" => Value::Number(ok(f, net_ext::tcp_listen(&s(&a, 0, f), n(&a, 1, f)))),
        "tcp_dial" => Value::Number(ok(f, net_ext::tcp_connect(&s(&a, 0, f), n(&a, 1, f)))),
        "tcp_accept" => Value::Number(ok(f, net_ext::tcp_accept(n(&a, 0, f)))),
        "tcp_read" => Value::String(ok(f, net_ext::tcp_read(n(&a, 0, f), n(&a, 1, f)))),
        "tcp_write" => Value::Number(ok(f, net_ext::tcp_write(n(&a, 0, f), &s(&a, 1, f)))),
        "tcp_close" | "udp_close" => {
            ok(f, net_ext::close(n(&a, 0, f)));
            Value::None
        }
        "tcp_set_timeout" => {
            ok(f, net_ext::set_timeout(n(&a, 0, f), n(&a, 1, f)));
            Value::None
        }
        "tcp_local_port" => Value::Number(ok(f, net_ext::local_port(n(&a, 0, f)))),
        "tcp_peer_addr" => Value::String(ok(f, net_ext::peer_address(n(&a, 0, f)))),
        "udp_bind" => Value::Number(ok(f, net_ext::udp_bind(&s(&a, 0, f), n(&a, 1, f)))),
        "udp_send_to" => Value::Number(ok(
            f,
            net_ext::udp_send_to(n(&a, 0, f), &s(&a, 1, f), n(&a, 2, f), &s(&a, 3, f)),
        )),
        "udp_recv_from" => {
            let (text, from) = ok(f, net_ext::udp_recv_from(n(&a, 0, f), n(&a, 1, f)));
            Value::Array(vec![Value::String(text), Value::String(from)])
        }
        "http_request" => http_value(
            f,
            net_ext::http_request(
                &s(&a, 0, f),
                &s(&a, 1, f),
                &header_pairs(&a, 2, f),
                &s(&a, 3, f),
                0,
            ),
        ),
        "http_get" => http_value(f, net_ext::http_request("GET", &s(&a, 0, f), &[], "", 0)),
        "http_post" => http_value(
            f,
            net_ext::http_request("POST", &s(&a, 0, f), &[], &s(&a, 1, f), 0),
        ),
        "http_client_open" => {
            Value::Number(ok(f, net_ext::http_client_open(&s(&a, 0, f), n(&a, 1, f))))
        }
        "http_client_request" => http_value(
            f,
            net_ext::http_client_request(
                n(&a, 0, f),
                &s(&a, 1, f),
                &s(&a, 2, f),
                &header_pairs(&a, 3, f),
                &s(&a, 4, f),
            ),
        ),
        "http_client_close" => {
            ok(f, net_ext::http_client_close(n(&a, 0, f)));
            Value::None
        }
        "http_listen" => {
            Value::Number(ok(f, net_ext::http_listen(&s(&a, 0, f), n(&a, 1, f), None)))
        }
        "https_listen" => Value::Number(ok(
            f,
            net_ext::https_listen(&s(&a, 0, f), n(&a, 1, f), &s(&a, 2, f), &s(&a, 3, f)),
        )),
        "http_accept" => http_value(f, net_ext::http_accept(n(&a, 0, f))),
        "http_respond" => {
            ok(
                f,
                net_ext::http_respond(
                    n(&a, 0, f),
                    n(&a, 1, f),
                    &header_pairs(&a, 2, f),
                    &s(&a, 3, f),
                ),
            );
            Value::None
        }
        "url_parse_query" => data_to_value(net_ext::parse_query(&s(&a, 0, f))),
        "url_encode" => Value::String(net_ext::percent_encode(&s(&a, 0, f))),
        "url_decode" => Value::String(net_ext::percent_decode(&s(&a, 0, f))),

        // ---- concurrency ----
        "task_spawn" => {
            let program = interp
                .program_arc()
                .unwrap_or_else(|| panic!("task_spawn() needs a running program"));
            let args: Vec<Data> = arr(&a, 1, f)
                .iter()
                .map(|v| ok(f, value_to_data(v)))
                .collect();
            Value::Number(ok(f, conc_ext::spawn(program, s(&a, 0, f), args)))
        }
        "task_join" => {
            let (data, lines) = ok(f, conc_ext::join(n(&a, 0, f)));
            interp.push_output(lines);
            data_to_value(data)
        }
        "task_join_all" => {
            let mut results = Vec::new();
            for id in arr(&a, 0, f) {
                let Value::Number(id) = id else {
                    panic!("task_join_all() expects an array of task ids")
                };
                let (data, lines) = ok(f, conc_ext::join(id));
                interp.push_output(lines);
                results.push(data_to_value(data));
            }
            Value::Array(results)
        }
        "task_is_finished" => Value::Boolean(ok(f, conc_ext::is_finished(n(&a, 0, f)))),
        "pool_spawn" => {
            let program = interp
                .program_arc()
                .unwrap_or_else(|| panic!("pool_spawn() needs a running program"));
            let args: Vec<Data> = arr(&a, 1, f)
                .iter()
                .map(|v| ok(f, value_to_data(v)))
                .collect();
            Value::Number(conc_ext::pool_spawn(program, s(&a, 0, f), args))
        }
        "scope_new" => Value::Number(conc_ext::scope_new()),
        "scope_spawn" => {
            let program = interp
                .program_arc()
                .unwrap_or_else(|| panic!("scope_spawn() needs a running program"));
            let args: Vec<Data> = arr(&a, 2, f)
                .iter()
                .map(|v| ok(f, value_to_data(v)))
                .collect();
            Value::Number(ok(
                f,
                conc_ext::scope_spawn(program, n(&a, 0, f), s(&a, 1, f), args),
            ))
        }
        "scope_join" => {
            let (values, lines) = ok(f, conc_ext::scope_join(n(&a, 0, f)));
            interp.push_output(lines);
            Value::Array(values.into_iter().map(data_to_value).collect())
        }
        "scope_cancel" => {
            ok(f, conc_ext::scope_cancel(n(&a, 0, f)));
            Value::None
        }
        "scope_is_cancelled" => Value::Boolean(ok(f, conc_ext::scope_is_cancelled(n(&a, 0, f)))),
        "parallel_map" => {
            let program = interp
                .program_arc()
                .unwrap_or_else(|| panic!("parallel_map() needs a running program"));
            let items: Vec<Data> = arr(&a, 1, f)
                .iter()
                .map(|v| ok(f, value_to_data(v)))
                .collect();
            let (values, lines) = ok(f, conc_ext::parallel_map(&program, &s(&a, 0, f), items));
            interp.push_output(lines);
            Value::Array(values.into_iter().map(data_to_value).collect())
        }
        "cpu_count" => Value::Number(conc_ext::cpu_count() as i64),
        "channel_new" => Value::Number(conc_ext::channel_new(n(&a, 0, f))),
        "channel_send" => {
            ok(
                f,
                conc_ext::channel_send(n(&a, 0, f), ok(f, value_to_data(&a[1]))),
            );
            Value::None
        }
        "channel_recv" => match ok(f, conc_ext::channel_recv(n(&a, 0, f), None)) {
            Some(d) => some(data_to_value(d)),
            None => none(),
        },
        "channel_try_recv" => match ok(f, conc_ext::channel_try_recv(n(&a, 0, f))) {
            Some(d) => some(data_to_value(d)),
            None => none(),
        },
        "channel_recv_timeout" => {
            let ms = n(&a, 1, f).max(0) as u64;
            match ok(
                f,
                conc_ext::channel_recv(n(&a, 0, f), Some(Duration::from_millis(ms))),
            ) {
                Some(d) => some(data_to_value(d)),
                None => none(),
            }
        }
        "channel_close" => {
            ok(f, conc_ext::channel_close(n(&a, 0, f)));
            Value::None
        }
        "channel_len" => Value::Number(ok(f, conc_ext::channel_len(n(&a, 0, f)))),
        "counter_new" => Value::Number(conc_ext::counter_new(n(&a, 0, f))),
        "counter_add" => Value::Number(ok(f, conc_ext::counter_add(n(&a, 0, f), n(&a, 1, f)))),
        "counter_get" => Value::Number(ok(f, conc_ext::counter_get(n(&a, 0, f)))),

        // ---- option and map helpers ----
        "option_is_some" => Value::Boolean(matches!(&a[0], Value::Option(Some(_)))),
        "option_unwrap" => match &a[0] {
            Value::Option(Some(inner)) => (**inner).clone(),
            _ => panic!("option_unwrap() called on None"),
        },
        "option_unwrap_or" => match &a[0] {
            Value::Option(Some(inner)) => (**inner).clone(),
            _ => a[1].clone(),
        },
        "option_is_none" => Value::Boolean(matches!(&a[0], Value::Option(None))),

        // ---- Higher-order functions (the callback is a function value) ----
        "array_map" => {
            let items = callback_array(&a, 0, "array_map");
            let function = a[1].clone();
            Value::Array(
                items
                    .into_iter()
                    .map(|item| interp.call_function_value(&function, vec![item]))
                    .collect(),
            )
        }
        "array_filter" => {
            let items = callback_array(&a, 0, "array_filter");
            let function = a[1].clone();
            let mut kept = Vec::new();
            for item in items {
                let verdict = interp.call_function_value(&function, vec![item.clone()]);
                if callback_bool(verdict, "array_filter") {
                    kept.push(item);
                }
            }
            Value::Array(kept)
        }
        "array_fold" => {
            let items = callback_array(&a, 0, "array_fold");
            let function = a[2].clone();
            let mut accumulator = a[1].clone();
            for item in items {
                accumulator = interp.call_function_value(&function, vec![accumulator, item]);
            }
            accumulator
        }
        "array_for_each" => {
            let items = callback_array(&a, 0, "array_for_each");
            let function = a[1].clone();
            for item in items {
                interp.call_function_value(&function, vec![item]);
            }
            Value::None
        }
        "array_any" => {
            let items = callback_array(&a, 0, "array_any");
            let function = a[1].clone();
            let mut found = false;
            for item in items {
                let verdict = interp.call_function_value(&function, vec![item]);
                if callback_bool(verdict, "array_any") {
                    found = true;
                    break;
                }
            }
            Value::Boolean(found)
        }
        "array_all" => {
            let items = callback_array(&a, 0, "array_all");
            let function = a[1].clone();
            let mut all = true;
            for item in items {
                let verdict = interp.call_function_value(&function, vec![item]);
                if !callback_bool(verdict, "array_all") {
                    all = false;
                    break;
                }
            }
            Value::Boolean(all)
        }
        "array_sort_by" => {
            let mut items = callback_array(&a, 0, "array_sort_by");
            let function = a[1].clone();
            items.sort_by(|left, right| {
                match interp.call_function_value(&function, vec![left.clone(), right.clone()]) {
                    Value::Number(order) => order.cmp(&0),
                    other => panic!(
                        "array_sort_by() comparator must return a number, found {}",
                        other
                    ),
                }
            });
            Value::Array(items)
        }
        "option_map" => match &a[0] {
            Value::Option(Some(inner)) => Value::Option(Some(Box::new(
                interp.call_function_value(&a[1], vec![(**inner).clone()]),
            ))),
            Value::Option(None) => Value::Option(None),
            _ => panic!("option_map() expects an Option"),
        },
        "option_and_then" => match &a[0] {
            Value::Option(Some(inner)) => {
                interp.call_function_value(&a[1], vec![(**inner).clone()])
            }
            Value::Option(None) => Value::Option(None),
            _ => panic!("option_and_then() expects an Option"),
        },
        "result_map" => match &a[0] {
            Value::Enum { enum_name, variant, values }
                if enum_name == "Result" && variant == "Ok" && values.len() == 1 =>
            {
                result_value("Ok", interp.call_function_value(&a[1], vec![values[0].clone()]))
            }
            Value::Enum { enum_name, .. } if enum_name == "Result" => a[0].clone(),
            _ => panic!("result_map() expects a Result"),
        },
        "result_map_err" => match &a[0] {
            Value::Enum { enum_name, variant, values }
                if enum_name == "Result" && variant == "Err" && values.len() == 1 =>
            {
                result_value("Err", interp.call_function_value(&a[1], vec![values[0].clone()]))
            }
            Value::Enum { enum_name, .. } if enum_name == "Result" => a[0].clone(),
            _ => panic!("result_map_err() expects a Result"),
        },
        "result_and_then" => match &a[0] {
            Value::Enum { enum_name, variant, values }
                if enum_name == "Result" && variant == "Ok" && values.len() == 1 =>
            {
                interp.call_function_value(&a[1], vec![values[0].clone()])
            }
            Value::Enum { enum_name, .. } if enum_name == "Result" => a[0].clone(),
            _ => panic!("result_and_then() expects a Result"),
        },

        // ---- Option / Result constructors and helpers ----
        // A Result is the runtime enum `Result` with variants `Ok` / `Err`, each carrying one
        // value, so the interpreter's ordinary enum equality, printing and matching apply.
        "Some" => Value::Option(Some(Box::new(a[0].clone()))),
        "None" => Value::Option(None),
        "Ok" => Value::Enum {
            enum_name: "Result".to_string(),
            variant: "Ok".to_string(),
            values: vec![a[0].clone()],
        },
        "Err" => Value::Enum {
            enum_name: "Result".to_string(),
            variant: "Err".to_string(),
            values: vec![a[0].clone()],
        },
        "result_is_ok" => Value::Boolean(matches!(
            &a[0],
            Value::Enum { enum_name, variant, .. } if enum_name == "Result" && variant == "Ok"
        )),
        "result_is_err" => Value::Boolean(matches!(
            &a[0],
            Value::Enum { enum_name, variant, .. } if enum_name == "Result" && variant == "Err"
        )),
        "result_unwrap" => match &a[0] {
            Value::Enum { enum_name, variant, values }
                if enum_name == "Result" && variant == "Ok" && values.len() == 1 =>
            {
                values[0].clone()
            }
            Value::Enum { enum_name, variant, values }
                if enum_name == "Result" && variant == "Err" && values.len() == 1 =>
            {
                panic!("result_unwrap() called on Err({})", values[0])
            }
            _ => panic!("result_unwrap() expects a Result"),
        },
        "result_unwrap_err" => match &a[0] {
            Value::Enum { enum_name, variant, values }
                if enum_name == "Result" && variant == "Err" && values.len() == 1 =>
            {
                values[0].clone()
            }
            Value::Enum { enum_name, variant, values }
                if enum_name == "Result" && variant == "Ok" && values.len() == 1 =>
            {
                panic!("result_unwrap_err() called on Ok({})", values[0])
            }
            _ => panic!("result_unwrap_err() expects a Result"),
        },
        "result_unwrap_or" => match &a[0] {
            Value::Enum { enum_name, variant, values }
                if enum_name == "Result" && variant == "Ok" && values.len() == 1 =>
            {
                values[0].clone()
            }
            _ => a[1].clone(),
        },
        "map_get_or" => match &a[0] {
            Value::HashMap(entries) => entries
                .iter()
                .find(|(k, _)| *k == a[1])
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| a[2].clone()),
            _ => panic!("map_get_or() expects a map"),
        },

        // ---- assertions ----
        "assert" => {
            if !b(&a, 0, f) {
                panic!("assertion failed");
            }
            Value::None
        }
        "assert_msg" => {
            if !b(&a, 0, f) {
                panic!("assertion failed: {}", s(&a, 1, f));
            }
            Value::None
        }
        "assert_eq" => {
            if a[0] != a[1] {
                panic!(
                    "assertion failed: left == right\n  left: {}\n right: {}",
                    a[0], a[1]
                );
            }
            Value::None
        }
        "assert_ne" => {
            if a[0] == a[1] {
                panic!("assertion failed: left != right\n  both: {}", a[0]);
            }
            Value::None
        }
        "test_fail" => panic!("{}", s(&a, 0, f)),
        _ => panic!(
            "internal error: builtin '{}' has a signature but no implementation",
            name
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_signature_has_unique_name_and_valid_codes() {
        let mut seen = std::collections::HashSet::new();
        for (name, params, ret) in SIGS {
            assert!(seen.insert(*name), "duplicate builtin {}", name);
            assert!(
                params.chars().all(|c| "snfbaSumor".contains(c)),
                "bad param code in {}",
                name
            );
            assert!("snfbaSumorv".contains(*ret), "bad return code in {}", name);
        }
        assert_eq!(signatures().len(), SIGS.len());
    }

    #[test]
    fn value_data_round_trip() {
        let v = Value::Array(vec![
            Value::Number(1),
            Value::String("x".into()),
            Value::Boolean(true),
            Value::Float(2.5),
        ]);
        let d = value_to_data(&v).unwrap();
        assert_eq!(data_to_value(d), v);
        assert!(value_to_data(&Value::File(Default::default())).is_err());
    }

    #[test]
    fn char_slicing_clamps() {
        assert_eq!(char_slice("héllo", 1, 3), "él");
        assert_eq!(char_slice("abc", -5, 99), "abc");
        assert_eq!(char_slice("abc", 2, 1), "");
    }
}
