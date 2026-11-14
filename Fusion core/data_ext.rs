//! Structured data: a neutral value tree plus JSON and TOML parsers/serializers.
//!
//! `Data` is also the transport format used for values that cross task boundaries and for
//! HTTP request/response maps, so everything that leaves one interpreter is a plain copy.

#[derive(Debug, Clone, PartialEq)]
pub enum Data {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Data>),
    Map(Vec<(String, Data)>),
}

impl Data {
    pub fn get(&self, key: &str) -> Option<&Data> {
        match self {
            Data::Map(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

struct JsonParser<'a> {
    text: &'a [u8],
    pos: usize,
}

pub fn json_parse(text: &str) -> Result<Data, String> {
    let mut p = JsonParser { text: text.as_bytes(), pos: 0 };
    p.skip_ws();
    let value = p.value(0)?;
    p.skip_ws();
    if p.pos != p.text.len() {
        return Err(p.err("trailing characters after JSON value"));
    }
    Ok(value)
}

impl<'a> JsonParser<'a> {
    fn err(&self, msg: &str) -> String {
        let consumed = &self.text[..self.pos.min(self.text.len())];
        let line = consumed.iter().filter(|b| **b == b'\n').count() + 1;
        let col = consumed.iter().rev().take_while(|b| **b != b'\n').count() + 1;
        format!("JSON error at line {}, column {}: {}", line, col, msg)
    }

    fn skip_ws(&mut self) {
        while self.pos < self.text.len() && matches!(self.text[self.pos], b' ' | b'\t' | b'\n' | b'\r') {
            self.pos += 1;
        }
    }

    fn eat(&mut self, literal: &str) -> bool {
        if self.text[self.pos..].starts_with(literal.as_bytes()) {
            self.pos += literal.len();
            true
        } else {
            false
        }
    }

    fn value(&mut self, depth: usize) -> Result<Data, String> {
        if depth > 256 {
            return Err(self.err("nesting too deep"));
        }
        if self.pos >= self.text.len() {
            return Err(self.err("unexpected end of input"));
        }
        match self.text[self.pos] {
            b'n' => if self.eat("null") { Ok(Data::Null) } else { Err(self.err("expected 'null'")) },
            b't' => if self.eat("true") { Ok(Data::Bool(true)) } else { Err(self.err("expected 'true'")) },
            b'f' => if self.eat("false") { Ok(Data::Bool(false)) } else { Err(self.err("expected 'false'")) },
            b'"' => self.string().map(Data::Str),
            b'[' => {
                self.pos += 1;
                let mut items = Vec::new();
                self.skip_ws();
                if self.pos < self.text.len() && self.text[self.pos] == b']' {
                    self.pos += 1;
                    return Ok(Data::List(items));
                }
                loop {
                    self.skip_ws();
                    items.push(self.value(depth + 1)?);
                    self.skip_ws();
                    match self.text.get(self.pos) {
                        Some(b',') => self.pos += 1,
                        Some(b']') => {
                            self.pos += 1;
                            return Ok(Data::List(items));
                        }
                        _ => return Err(self.err("expected ',' or ']'")),
                    }
                }
            }
            b'{' => {
                self.pos += 1;
                let mut entries: Vec<(String, Data)> = Vec::new();
                self.skip_ws();
                if self.pos < self.text.len() && self.text[self.pos] == b'}' {
                    self.pos += 1;
                    return Ok(Data::Map(entries));
                }
                loop {
                    self.skip_ws();
                    if self.text.get(self.pos) != Some(&b'"') {
                        return Err(self.err("expected string key"));
                    }
                    let key = self.string()?;
                    self.skip_ws();
                    if self.text.get(self.pos) != Some(&b':') {
                        return Err(self.err("expected ':'"));
                    }
                    self.pos += 1;
                    self.skip_ws();
                    let value = self.value(depth + 1)?;
                    if let Some(slot) = entries.iter_mut().find(|(k, _)| *k == key) {
                        slot.1 = value;
                    } else {
                        entries.push((key, value));
                    }
                    self.skip_ws();
                    match self.text.get(self.pos) {
                        Some(b',') => self.pos += 1,
                        Some(b'}') => {
                            self.pos += 1;
                            return Ok(Data::Map(entries));
                        }
                        _ => return Err(self.err("expected ',' or '}'")),
                    }
                }
            }
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(self.err("unexpected character")),
        }
    }

    fn number(&mut self) -> Result<Data, String> {
        let start = self.pos;
        if self.text[self.pos] == b'-' {
            self.pos += 1;
        }
        let mut is_float = false;
        while self.pos < self.text.len() {
            match self.text[self.pos] {
                b'0'..=b'9' => self.pos += 1,
                b'.' | b'e' | b'E' | b'+' | b'-' => {
                    is_float = true;
                    self.pos += 1;
                }
                _ => break,
            }
        }
        let slice = std::str::from_utf8(&self.text[start..self.pos]).unwrap_or("");
        if !is_float {
            if let Ok(n) = slice.parse::<i64>() {
                return Ok(Data::Int(n));
            }
        }
        slice.parse::<f64>().map(Data::Float).map_err(|_| self.err("invalid number"))
    }

    fn hex4(&mut self) -> Result<u32, String> {
        if self.pos + 4 > self.text.len() {
            return Err(self.err("truncated \\u escape"));
        }
        let s = std::str::from_utf8(&self.text[self.pos..self.pos + 4]).map_err(|_| self.err("invalid \\u escape"))?;
        let v = u32::from_str_radix(s, 16).map_err(|_| self.err("invalid \\u escape"))?;
        self.pos += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String, String> {
        self.pos += 1; // opening quote
        let mut out: Vec<u8> = Vec::new();
        loop {
            let b = *self.text.get(self.pos).ok_or_else(|| self.err("unterminated string"))?;
            self.pos += 1;
            match b {
                b'"' => break,
                b'\\' => {
                    let e = *self.text.get(self.pos).ok_or_else(|| self.err("unterminated escape"))?;
                    self.pos += 1;
                    let ch = match e {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let hi = self.hex4()?;
                            if (0xD800..0xDC00).contains(&hi) {
                                if !self.eat("\\u") {
                                    return Err(self.err("unpaired surrogate"));
                                }
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err(self.err("invalid low surrogate"));
                                }
                                char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00))
                                    .ok_or_else(|| self.err("invalid code point"))?
                            } else {
                                char::from_u32(hi).ok_or_else(|| self.err("invalid code point"))?
                            }
                        }
                        _ => return Err(self.err("invalid escape")),
                    };
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                b if b < 0x20 => return Err(self.err("control character in string")),
                b => out.push(b),
            }
        }
        String::from_utf8(out).map_err(|_| self.err("invalid UTF-8 in string"))
    }
}

pub fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn json_stringify(data: &Data, pretty: bool) -> String {
    let mut out = String::new();
    write_json(data, pretty, 0, &mut out);
    out
}

fn write_json(data: &Data, pretty: bool, depth: usize, out: &mut String) {
    let nl = |out: &mut String, d: usize| {
        if pretty {
            out.push('\n');
            for _ in 0..d {
                out.push_str("  ");
            }
        }
    };
    match data {
        Data::Null => out.push_str("null"),
        Data::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Data::Int(n) => out.push_str(&n.to_string()),
        Data::Float(f) => {
            if f.is_finite() {
                if f.fract() == 0.0 && f.abs() < 1e15 {
                    out.push_str(&format!("{:.1}", f));
                } else {
                    out.push_str(&f.to_string());
                }
            } else {
                out.push_str("null");
            }
        }
        Data::Str(s) => out.push_str(&json_escape(s)),
        Data::List(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                nl(out, depth + 1);
                write_json(item, pretty, depth + 1, out);
            }
            nl(out, depth);
            out.push(']');
        }
        Data::Map(entries) => {
            if entries.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push('{');
            for (i, (k, v)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                nl(out, depth + 1);
                out.push_str(&json_escape(k));
                out.push(':');
                if pretty {
                    out.push(' ');
                }
                write_json(v, pretty, depth + 1, out);
            }
            nl(out, depth);
            out.push('}');
        }
    }
}

// ---------------------------------------------------------------------------
// TOML (the subset used by manifests and configuration files)
// ---------------------------------------------------------------------------

struct TomlParser {
    chars: Vec<char>,
    pos: usize,
}

pub fn toml_parse(text: &str) -> Result<Data, String> {
    let mut p = TomlParser { chars: text.chars().collect(), pos: 0 };
    let mut root = Data::Map(Vec::new());
    let mut current: Vec<String> = Vec::new();
    loop {
        p.skip_blank();
        if p.pos >= p.chars.len() {
            break;
        }
        if p.peek() == Some('[') {
            p.pos += 1;
            let array_table = p.peek() == Some('[');
            if array_table {
                p.pos += 1;
            }
            let path = p.key_path()?;
            p.skip_inline_ws();
            for _ in 0..(if array_table { 2 } else { 1 }) {
                if p.peek() != Some(']') {
                    return Err(p.err("expected ']' after table name"));
                }
                p.pos += 1;
            }
            p.end_of_line()?;
            if array_table {
                let (last, parent) = path.split_last().ok_or_else(|| p.err("empty table name"))?;
                let table = table_mut(&mut root, parent).map_err(|e| p.err(&e))?;
                let Data::Map(entries) = table else { return Err(p.err("invalid array of tables")) };
                match entries.iter_mut().find(|(k, _)| k == last) {
                    Some((_, Data::List(items))) => items.push(Data::Map(Vec::new())),
                    Some(_) => return Err(p.err(&format!("'{}' is not an array of tables", last))),
                    None => entries.push((last.clone(), Data::List(vec![Data::Map(Vec::new())]))),
                }
            } else {
                table_mut(&mut root, &path).map_err(|e| p.err(&e))?;
            }
            current = path;
        } else {
            let key_path = p.key_path()?;
            p.skip_inline_ws();
            if p.peek() != Some('=') {
                return Err(p.err("expected '=' after key"));
            }
            p.pos += 1;
            p.skip_inline_ws();
            let value = p.value()?;
            p.end_of_line()?;
            let table = table_mut(&mut root, &current).map_err(|e| p.err(&e))?;
            let (last, parents) = key_path.split_last().ok_or_else(|| p.err("empty key"))?;
            let target = table_mut(table, parents).map_err(|e| p.err(&e))?;
            let Data::Map(entries) = target else { return Err(p.err("cannot assign into a non-table")) };
            if entries.iter().any(|(k, _)| k == last) {
                return Err(p.err(&format!("duplicate key '{}'", last)));
            }
            entries.push((last.clone(), value));
        }
    }
    Ok(root)
}

fn table_mut<'a>(node: &'a mut Data, path: &[String]) -> Result<&'a mut Data, String> {
    if path.is_empty() {
        // Inside an array of tables, the "current table" is its most recent element.
        // Check through a shared borrow first, then take the mutable borrow only
        // after the decision is complete (avoids moving `node` while `l` is borrowed).
        let is_array_table = match &*node {
            Data::List(items) => matches!(items.last(), Some(Data::Map(_))),
            _ => false,
        };
        if is_array_table {
            if let Data::List(items) = node {
                return Ok(items.last_mut().expect("array-table was checked to be non-empty"));
            }
        }
        return Ok(node);
    }
    let entries = match node {
        Data::Map(e) => e,
        Data::List(l) => match l.last_mut() {
            Some(Data::Map(e)) => e,
            _ => return Err(format!("'{}' is not a table", path[0])),
        },
        _ => return Err(format!("'{}' is not a table", path[0])),
    };
    let index = match entries.iter().position(|(k, _)| *k == path[0]) {
        Some(i) => i,
        None => {
            entries.push((path[0].clone(), Data::Map(Vec::new())));
            entries.len() - 1
        }
    };
    table_mut(&mut entries[index].1, &path[1..])
}

impl TomlParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn err(&self, msg: &str) -> String {
        let line = self.chars[..self.pos.min(self.chars.len())].iter().filter(|c| **c == '\n').count() + 1;
        format!("TOML error at line {}: {}", line, msg)
    }

    fn skip_inline_ws(&mut self) {
        while matches!(self.peek(), Some(' ') | Some('\t')) {
            self.pos += 1;
        }
    }

    fn skip_comment(&mut self) {
        if self.peek() == Some('#') {
            while let Some(c) = self.peek() {
                if c == '\n' {
                    break;
                }
                self.pos += 1;
            }
        }
    }

    fn skip_blank(&mut self) {
        loop {
            self.skip_inline_ws();
            self.skip_comment();
            match self.peek() {
                Some('\n') | Some('\r') => self.pos += 1,
                _ => break,
            }
        }
    }

    fn end_of_line(&mut self) -> Result<(), String> {
        self.skip_inline_ws();
        self.skip_comment();
        match self.peek() {
            None => Ok(()),
            Some('\n') => {
                self.pos += 1;
                Ok(())
            }
            Some('\r') => {
                self.pos += 1;
                if self.peek() == Some('\n') {
                    self.pos += 1;
                }
                Ok(())
            }
            Some(_) => Err(self.err("unexpected characters after value")),
        }
    }

    fn key_path(&mut self) -> Result<Vec<String>, String> {
        let mut path = Vec::new();
        loop {
            self.skip_inline_ws();
            path.push(self.key()?);
            self.skip_inline_ws();
            if self.peek() == Some('.') {
                self.pos += 1;
            } else {
                return Ok(path);
            }
        }
    }

    fn key(&mut self) -> Result<String, String> {
        match self.peek() {
            Some('"') => self.basic_string(),
            Some('\'') => self.literal_string(),
            Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '-' => {
                let mut s = String::new();
                while let Some(c) = self.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                        s.push(c);
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                Ok(s)
            }
            _ => Err(self.err("expected a key")),
        }
    }

    fn basic_string(&mut self) -> Result<String, String> {
        let multiline = self.chars[self.pos..].starts_with(&['"', '"', '"']);
        self.pos += if multiline { 3 } else { 1 };
        if multiline && self.peek() == Some('\n') {
            self.pos += 1;
        }
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or_else(|| self.err("unterminated string"))?;
            self.pos += 1;
            match c {
                '"' if !multiline => return Ok(out),
                '"' if multiline && self.chars[self.pos..].starts_with(&['"', '"']) => {
                    self.pos += 2;
                    return Ok(out);
                }
                '\n' if !multiline => return Err(self.err("newline in string")),
                '\\' => {
                    let e = self.peek().ok_or_else(|| self.err("unterminated escape"))?;
                    self.pos += 1;
                    match e {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        'u' | 'U' => {
                            let n = if e == 'u' { 4 } else { 8 };
                            let hex: String = self.chars[self.pos..(self.pos + n).min(self.chars.len())].iter().collect();
                            let code = u32::from_str_radix(&hex, 16).map_err(|_| self.err("invalid unicode escape"))?;
                            out.push(char::from_u32(code).ok_or_else(|| self.err("invalid code point"))?);
                            self.pos += n;
                        }
                        '\n' if multiline => {
                            while matches!(self.peek(), Some(' ') | Some('\t') | Some('\n') | Some('\r')) {
                                self.pos += 1;
                            }
                        }
                        _ => return Err(self.err("invalid escape sequence")),
                    }
                }
                c => out.push(c),
            }
        }
    }

    fn literal_string(&mut self) -> Result<String, String> {
        let multiline = self.chars[self.pos..].starts_with(&['\'', '\'', '\'']);
        self.pos += if multiline { 3 } else { 1 };
        if multiline && self.peek() == Some('\n') {
            self.pos += 1;
        }
        let mut out = String::new();
        loop {
            let c = self.peek().ok_or_else(|| self.err("unterminated string"))?;
            self.pos += 1;
            if c == '\'' && !multiline {
                return Ok(out);
            }
            if c == '\'' && multiline && self.chars[self.pos..].starts_with(&['\'', '\'']) {
                self.pos += 2;
                return Ok(out);
            }
            if c == '\n' && !multiline {
                return Err(self.err("newline in string"));
            }
            out.push(c);
        }
    }

    fn skip_array_space(&mut self) {
        loop {
            self.skip_inline_ws();
            self.skip_comment();
            match self.peek() {
                Some('\n') | Some('\r') => self.pos += 1,
                _ => break,
            }
        }
    }

    fn value(&mut self) -> Result<Data, String> {
        match self.peek() {
            Some('"') => self.basic_string().map(Data::Str),
            Some('\'') => self.literal_string().map(Data::Str),
            Some('[') => {
                self.pos += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_array_space();
                    if self.peek() == Some(']') {
                        self.pos += 1;
                        return Ok(Data::List(items));
                    }
                    items.push(self.value()?);
                    self.skip_array_space();
                    match self.peek() {
                        Some(',') => self.pos += 1,
                        Some(']') => {}
                        _ => return Err(self.err("expected ',' or ']' in array")),
                    }
                }
            }
            Some('{') => {
                self.pos += 1;
                let mut table = Data::Map(Vec::new());
                loop {
                    self.skip_inline_ws();
                    if self.peek() == Some('}') {
                        self.pos += 1;
                        return Ok(table);
                    }
                    let path = self.key_path()?;
                    self.skip_inline_ws();
                    if self.peek() != Some('=') {
                        return Err(self.err("expected '=' in inline table"));
                    }
                    self.pos += 1;
                    self.skip_inline_ws();
                    let value = self.value()?;
                    let (last, parents) = path.split_last().ok_or_else(|| self.err("empty key"))?;
                    let target = table_mut(&mut table, parents).map_err(|e| self.err(&e))?;
                    let Data::Map(entries) = target else { return Err(self.err("invalid inline table")) };
                    if entries.iter().any(|(k, _)| k == last) {
                        return Err(self.err(&format!("duplicate key '{}'", last)));
                    }
                    entries.push((last.clone(), value));
                    self.skip_inline_ws();
                    match self.peek() {
                        Some(',') => self.pos += 1,
                        Some('}') => {}
                        _ => return Err(self.err("expected ',' or '}' in inline table")),
                    }
                }
            }
            Some(_) => self.scalar(),
            None => Err(self.err("expected a value")),
        }
    }

    fn scalar(&mut self) -> Result<Data, String> {
        let mut word = String::new();
        while let Some(c) = self.peek() {
            if c == ',' || c == ']' || c == '}' || c == '#' || c == '\n' || c == '\r' {
                break;
            }
            // Datetimes contain a space between date and time.
            if c == ' ' && !(word.len() == 10 && word.as_bytes()[4] == b'-' && self.chars.get(self.pos + 1).map_or(false, |n| n.is_ascii_digit())) {
                break;
            }
            word.push(c);
            self.pos += 1;
        }
        let w = word.trim();
        match w {
            "true" => return Ok(Data::Bool(true)),
            "false" => return Ok(Data::Bool(false)),
            "inf" | "+inf" => return Ok(Data::Float(f64::INFINITY)),
            "-inf" => return Ok(Data::Float(f64::NEG_INFINITY)),
            "nan" | "+nan" | "-nan" => return Ok(Data::Float(f64::NAN)),
            _ => {}
        }
        let clean: String = w.chars().filter(|c| *c != '_').collect();
        if let Some(hex) = clean.strip_prefix("0x") {
            return i64::from_str_radix(hex, 16).map(Data::Int).map_err(|_| self.err("invalid hex integer"));
        }
        if let Some(bin) = clean.strip_prefix("0b") {
            return i64::from_str_radix(bin, 2).map(Data::Int).map_err(|_| self.err("invalid binary integer"));
        }
        if let Some(oct) = clean.strip_prefix("0o") {
            return i64::from_str_radix(oct, 8).map(Data::Int).map_err(|_| self.err("invalid octal integer"));
        }
        if let Ok(n) = clean.parse::<i64>() {
            return Ok(Data::Int(n));
        }
        if clean.chars().next().map_or(false, |c| c.is_ascii_digit() || c == '+' || c == '-') {
            if let Ok(f) = clean.parse::<f64>() {
                return Ok(Data::Float(f));
            }
            // Dates and times are kept as their original text.
            if clean.len() >= 5 && (clean.contains('-') || clean.contains(':')) {
                return Ok(Data::Str(w.to_string()));
            }
        }
        Err(self.err(&format!("invalid value '{}'", w)))
    }
}

fn toml_key(k: &str) -> String {
    if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        k.to_string()
    } else {
        json_escape(k)
    }
}

fn toml_inline(data: &Data) -> Result<String, String> {
    Ok(match data {
        Data::Null => return Err("TOML cannot represent null".to_string()),
        Data::Bool(b) => b.to_string(),
        Data::Int(n) => n.to_string(),
        Data::Float(f) => {
            if f.is_nan() {
                "nan".to_string()
            } else if f.is_infinite() {
                if *f > 0.0 { "inf".to_string() } else { "-inf".to_string() }
            } else if f.fract() == 0.0 {
                format!("{:.1}", f)
            } else {
                f.to_string()
            }
        }
        Data::Str(s) => json_escape(s),
        Data::List(items) => {
            let parts: Result<Vec<String>, String> = items.iter().map(toml_inline).collect();
            format!("[{}]", parts?.join(", "))
        }
        Data::Map(entries) => {
            let parts: Result<Vec<String>, String> = entries
                .iter()
                .map(|(k, v)| toml_inline(v).map(|s| format!("{} = {}", toml_key(k), s)))
                .collect();
            format!("{{ {} }}", parts?.join(", "))
        }
    })
}

pub fn toml_stringify(data: &Data) -> Result<String, String> {
    let Data::Map(_) = data else { return Err("a TOML document must be a map".to_string()) };
    let mut out = String::new();
    write_toml_table(data, &[], &mut out)?;
    Ok(out)
}

fn is_table_list(items: &[Data]) -> bool {
    !items.is_empty() && items.iter().all(|i| matches!(i, Data::Map(_)))
}

fn write_toml_table(table: &Data, path: &[String], out: &mut String) -> Result<(), String> {
    let Data::Map(entries) = table else { return Ok(()) };
    for (k, v) in entries {
        let nested = matches!(v, Data::Map(_)) || matches!(v, Data::List(items) if is_table_list(items));
        if !nested {
            out.push_str(&format!("{} = {}\n", toml_key(k), toml_inline(v)?));
        }
    }
    for (k, v) in entries {
        let mut child = path.to_vec();
        child.push(toml_key(k));
        match v {
            Data::Map(_) => {
                out.push_str(&format!("\n[{}]\n", child.join(".")));
                write_toml_table(v, &child, out)?;
            }
            Data::List(items) if is_table_list(items) => {
                for item in items {
                    out.push_str(&format!("\n[[{}]]\n", child.join(".")));
                    write_toml_table(item, &child, out)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let text = r#"{"name":"fusion","n":[1,2.5,true,null],"nested":{"k":"v\n\u00e9\ud83d\ude00"}}"#;
        let data = json_parse(text).unwrap();
        assert_eq!(data.get("name"), Some(&Data::Str("fusion".into())));
        let again = json_parse(&json_stringify(&data, false)).unwrap();
        assert_eq!(data, again);
        let pretty = json_stringify(&data, true);
        assert!(pretty.contains("\n  \"name\": \"fusion\""));
        assert_eq!(json_parse(&pretty).unwrap(), data);
    }

    #[test]
    fn json_errors() {
        assert!(json_parse("{\"a\":}").is_err());
        assert!(json_parse("[1,2").is_err());
        assert!(json_parse("{} extra").is_err());
        assert!(json_parse("\"\\ud800\"").is_err());
    }

    #[test]
    fn toml_manifest_like() {
        let text = "# comment\n[package]\nname = \"demo\"\nversion = \"0.1.0\"\nbig = 1_000\n\n[dependencies]\nserde = { version = \"1\", features = [\"derive\"] }\nlist = [\n  1,\n  2, # two\n]\n\n[[bin]]\nname = \"a\"\n[[bin]]\nname = \"b\"\n";
        let d = toml_parse(text).unwrap();
        assert_eq!(d.get("package").unwrap().get("name"), Some(&Data::Str("demo".into())));
        assert_eq!(d.get("package").unwrap().get("big"), Some(&Data::Int(1000)));
        let serde = d.get("dependencies").unwrap().get("serde").unwrap();
        assert_eq!(serde.get("version"), Some(&Data::Str("1".into())));
        assert_eq!(d.get("dependencies").unwrap().get("list"), Some(&Data::List(vec![Data::Int(1), Data::Int(2)])));
        match d.get("bin") {
            Some(Data::List(items)) => assert_eq!(items.len(), 2),
            other => panic!("expected array of tables, got {:?}", other),
        }
    }

    #[test]
    fn toml_dotted_keys_and_errors() {
        let d = toml_parse("a.b.c = 1\n[x.y]\nz = true\n").unwrap();
        assert_eq!(d.get("a").unwrap().get("b").unwrap().get("c"), Some(&Data::Int(1)));
        assert_eq!(d.get("x").unwrap().get("y").unwrap().get("z"), Some(&Data::Bool(true)));
        assert!(toml_parse("a = 1\na = 2\n").is_err());
        assert!(toml_parse("a = \n").is_err());
        assert!(toml_parse("a = 'unterminated\n").is_err());
    }

    #[test]
    fn toml_round_trip() {
        let text = "title = \"t\"\nn = 3\n\n[owner]\nname = \"x\"\n\n[[items]]\nid = 1\n\n[[items]]\nid = 2\n";
        let d = toml_parse(text).unwrap();
        let s = toml_stringify(&d).unwrap();
        assert_eq!(toml_parse(&s).unwrap(), d);
    }
}
