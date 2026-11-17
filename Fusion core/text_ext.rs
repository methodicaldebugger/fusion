//! Text processing: a small backtracking regular-expression engine and UTF-8 helpers.
//!
//! Supported regex syntax: literals, `.`, classes `[a-z0-9_]` / `[^...]`, escapes
//! `\d \D \w \W \s \S \b \n \t \r`, anchors `^ $`, groups `( )` and `(?: )`,
//! alternation `|`, quantifiers `* + ? {n} {n,} {n,m}` (append `?` for lazy), and a
//! leading `(?i)` flag for case-insensitive matching. Matching is over Unicode scalar
//! values (`char`), not bytes.

type Caps = Vec<Option<(usize, usize)>>;

#[derive(Debug, Clone)]
enum ClassItem {
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
}

#[derive(Debug, Clone)]
enum Node {
    Char(char),
    Any,
    Class(Vec<ClassItem>, bool),
    Start,
    End,
    WordBoundary,
    Group(Box<Node>, Option<usize>),
    Alt(Vec<Node>),
    Concat(Vec<Node>),
    Repeat(Box<Node>, usize, usize, bool),
}

#[derive(Debug, Clone)]
pub struct Regex {
    root: Node,
    groups: usize,
    icase: bool,
}

struct ReParser {
    chars: Vec<char>,
    pos: usize,
    groups: usize,
}

impl ReParser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn parse_alt(&mut self) -> Result<Node, String> {
        let mut alts = vec![self.parse_concat()?];
        while self.peek() == Some('|') {
            self.pos += 1;
            alts.push(self.parse_concat()?);
        }
        if alts.len() == 1 {
            Ok(alts.remove(0))
        } else {
            Ok(Node::Alt(alts))
        }
    }

    fn parse_concat(&mut self) -> Result<Node, String> {
        let mut items = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.parse_atom()?;
            items.push(self.parse_quantifier(atom)?);
        }
        Ok(Node::Concat(items))
    }

    fn parse_quantifier(&mut self, atom: Node) -> Result<Node, String> {
        let (min, max) = match self.peek() {
            Some('*') => {
                self.pos += 1;
                (0, usize::MAX)
            }
            Some('+') => {
                self.pos += 1;
                (1, usize::MAX)
            }
            Some('?') => {
                self.pos += 1;
                (0, 1)
            }
            Some('{') => {
                let save = self.pos;
                match self.parse_braces() {
                    Some(range) => range,
                    None => {
                        self.pos = save;
                        return Ok(atom);
                    }
                }
            }
            _ => return Ok(atom),
        };
        let greedy = if self.peek() == Some('?') {
            self.pos += 1;
            false
        } else {
            true
        };
        if matches!(atom, Node::Start | Node::End | Node::WordBoundary) {
            return Err("quantifier applied to an anchor".to_string());
        }
        Ok(Node::Repeat(Box::new(atom), min, max, greedy))
    }

    fn parse_braces(&mut self) -> Option<(usize, usize)> {
        self.pos += 1; // '{'
        let mut first = String::new();
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                first.push(c);
                self.pos += 1;
            } else {
                break;
            }
        }
        if first.is_empty() {
            return None;
        }
        let min: usize = first.parse().ok()?;
        match self.peek() {
            Some('}') => {
                self.pos += 1;
                Some((min, min))
            }
            Some(',') => {
                self.pos += 1;
                let mut second = String::new();
                while let Some(c) = self.peek() {
                    if c.is_ascii_digit() {
                        second.push(c);
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                if self.peek() != Some('}') {
                    return None;
                }
                self.pos += 1;
                if second.is_empty() {
                    Some((min, usize::MAX))
                } else {
                    let max: usize = second.parse().ok()?;
                    if max < min {
                        None
                    } else {
                        Some((min, max))
                    }
                }
            }
            _ => None,
        }
    }

    fn parse_atom(&mut self) -> Result<Node, String> {
        let c = self.peek().ok_or("unexpected end of pattern")?;
        self.pos += 1;
        match c {
            '.' => Ok(Node::Any),
            '^' => Ok(Node::Start),
            '$' => Ok(Node::End),
            '(' => {
                let mut index = None;
                if self.peek() == Some('?') {
                    if self.chars.get(self.pos + 1) == Some(&':') {
                        self.pos += 2;
                    } else {
                        return Err("unsupported group syntax".to_string());
                    }
                } else {
                    self.groups += 1;
                    index = Some(self.groups);
                }
                let inner = self.parse_alt()?;
                if self.peek() != Some(')') {
                    return Err("unclosed group".to_string());
                }
                self.pos += 1;
                Ok(Node::Group(Box::new(inner), index))
            }
            '[' => self.parse_class(),
            '\\' => self.parse_escape(false).map(|e| match e {
                Escape::Char(ch) => Node::Char(ch),
                Escape::Item(item) => Node::Class(vec![item], false),
                Escape::Boundary => Node::WordBoundary,
            }),
            '*' | '+' | '?' => Err(format!("nothing to repeat before '{}'", c)),
            other => Ok(Node::Char(other)),
        }
    }

    fn parse_escape(&mut self, in_class: bool) -> Result<Escape, String> {
        let c = self.peek().ok_or("trailing backslash")?;
        self.pos += 1;
        Ok(match c {
            'd' => Escape::Item(ClassItem::Digit(true)),
            'D' => Escape::Item(ClassItem::Digit(false)),
            'w' => Escape::Item(ClassItem::Word(true)),
            'W' => Escape::Item(ClassItem::Word(false)),
            's' => Escape::Item(ClassItem::Space(true)),
            'S' => Escape::Item(ClassItem::Space(false)),
            'b' if !in_class => Escape::Boundary,
            'n' => Escape::Char('\n'),
            't' => Escape::Char('\t'),
            'r' => Escape::Char('\r'),
            other => Escape::Char(other),
        })
    }

    fn parse_class(&mut self) -> Result<Node, String> {
        let mut negated = false;
        if self.peek() == Some('^') {
            negated = true;
            self.pos += 1;
        }
        let mut items = Vec::new();
        let mut first = true;
        loop {
            let c = self.peek().ok_or("unclosed character class")?;
            self.pos += 1;
            if c == ']' && !first {
                break;
            }
            first = false;
            let lo = if c == '\\' {
                match self.parse_escape(true)? {
                    Escape::Item(item) => {
                        items.push(item);
                        continue;
                    }
                    Escape::Char(ch) => ch,
                    Escape::Boundary => continue,
                }
            } else {
                c
            };
            if self.peek() == Some('-') && self.chars.get(self.pos + 1).map_or(false, |n| *n != ']')
            {
                self.pos += 1;
                let mut hi = self.peek().ok_or("unclosed character class")?;
                self.pos += 1;
                if hi == '\\' {
                    match self.parse_escape(true)? {
                        Escape::Char(ch) => hi = ch,
                        _ => return Err("invalid range end in character class".to_string()),
                    }
                }
                if hi < lo {
                    return Err("invalid character class range".to_string());
                }
                items.push(ClassItem::Range(lo, hi));
            } else {
                items.push(ClassItem::Range(lo, lo));
            }
        }
        Ok(Node::Class(items, negated))
    }
}

enum Escape {
    Char(char),
    Item(ClassItem),
    Boundary,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn item_matches(item: &ClassItem, c: char) -> bool {
    match item {
        ClassItem::Range(lo, hi) => *lo <= c && c <= *hi,
        ClassItem::Digit(yes) => c.is_ascii_digit() == *yes,
        ClassItem::Word(yes) => is_word(c) == *yes,
        ClassItem::Space(yes) => c.is_whitespace() == *yes,
    }
}

struct Matcher<'a> {
    text: &'a [char],
    icase: bool,
}

impl<'a> Matcher<'a> {
    fn same(&self, a: char, b: char) -> bool {
        if a == b {
            return true;
        }
        self.icase && a.to_lowercase().eq(b.to_lowercase())
    }

    fn class_matches(&self, items: &[ClassItem], negated: bool, c: char) -> bool {
        let mut hit = items.iter().any(|item| item_matches(item, c));
        if !hit && self.icase {
            hit = c
                .to_lowercase()
                .chain(c.to_uppercase())
                .any(|v| items.iter().any(|item| item_matches(item, v)));
        }
        hit != negated
    }

    fn m(
        &self,
        node: &Node,
        pos: usize,
        caps: &mut Caps,
        k: &mut dyn FnMut(usize, &mut Caps) -> bool,
    ) -> bool {
        match node {
            Node::Char(c) => {
                pos < self.text.len() && self.same(self.text[pos], *c) && k(pos + 1, caps)
            }
            Node::Any => pos < self.text.len() && self.text[pos] != '\n' && k(pos + 1, caps),
            Node::Class(items, negated) => {
                pos < self.text.len()
                    && self.class_matches(items, *negated, self.text[pos])
                    && k(pos + 1, caps)
            }
            Node::Start => pos == 0 && k(pos, caps),
            Node::End => pos == self.text.len() && k(pos, caps),
            Node::WordBoundary => {
                let before = pos > 0 && is_word(self.text[pos - 1]);
                let after = pos < self.text.len() && is_word(self.text[pos]);
                before != after && k(pos, caps)
            }
            Node::Concat(items) => self.concat(items, 0, pos, caps, k),
            Node::Alt(alts) => {
                for alt in alts {
                    if self.m(alt, pos, caps, &mut *k) {
                        return true;
                    }
                }
                false
            }
            Node::Group(inner, index) => match index {
                None => self.m(inner, pos, caps, k),
                Some(i) => {
                    let i = *i;
                    let old = caps[i];
                    let ok = self.m(inner, pos, caps, &mut |end: usize, c: &mut Caps| {
                        let previous = c[i];
                        c[i] = Some((pos, end));
                        if k(end, c) {
                            true
                        } else {
                            c[i] = previous;
                            false
                        }
                    });
                    if !ok {
                        caps[i] = old;
                    }
                    ok
                }
            },
            Node::Repeat(inner, min, max, greedy) => {
                self.rep(inner, *min, *max, *greedy, 0, pos, caps, k)
            }
        }
    }

    fn concat(
        &self,
        items: &[Node],
        i: usize,
        pos: usize,
        caps: &mut Caps,
        k: &mut dyn FnMut(usize, &mut Caps) -> bool,
    ) -> bool {
        if i == items.len() {
            return k(pos, caps);
        }
        self.m(&items[i], pos, caps, &mut |p: usize, c: &mut Caps| {
            self.concat(items, i + 1, p, c, &mut *k)
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn rep(
        &self,
        inner: &Node,
        min: usize,
        max: usize,
        greedy: bool,
        count: usize,
        pos: usize,
        caps: &mut Caps,
        k: &mut dyn FnMut(usize, &mut Caps) -> bool,
    ) -> bool {
        if greedy {
            if count < max {
                let matched = self.m(inner, pos, caps, &mut |p: usize, c: &mut Caps| {
                    if p == pos && count >= min {
                        return false;
                    }
                    self.rep(inner, min, max, greedy, count + 1, p, c, &mut *k)
                });
                if matched {
                    return true;
                }
            }
            count >= min && k(pos, caps)
        } else {
            if count >= min && k(pos, caps) {
                return true;
            }
            count < max
                && self.m(inner, pos, caps, &mut |p: usize, c: &mut Caps| {
                    if p == pos && count >= min {
                        return false;
                    }
                    self.rep(inner, min, max, greedy, count + 1, p, c, &mut *k)
                })
        }
    }
}

impl Regex {
    pub fn new(pattern: &str) -> Result<Regex, String> {
        let (body, icase) = match pattern.strip_prefix("(?i)") {
            Some(rest) => (rest, true),
            None => (pattern, false),
        };
        let mut parser = ReParser {
            chars: body.chars().collect(),
            pos: 0,
            groups: 0,
        };
        let root = parser.parse_alt()?;
        if parser.pos != parser.chars.len() {
            return Err("unmatched ')' in pattern".to_string());
        }
        Ok(Regex {
            root,
            groups: parser.groups,
            icase,
        })
    }

    /// Finds the leftmost match starting at or after `from`. Returns capture spans
    /// (index 0 is the whole match) in char positions.
    pub fn find_at(&self, text: &[char], from: usize) -> Option<Caps> {
        let matcher = Matcher {
            text,
            icase: self.icase,
        };
        for start in from..=text.len() {
            let mut caps: Caps = vec![None; self.groups + 1];
            let found = matcher.m(
                &self.root,
                start,
                &mut caps,
                &mut |end: usize, c: &mut Caps| {
                    c[0] = Some((start, end));
                    true
                },
            );
            if found {
                return Some(caps);
            }
        }
        None
    }

    pub fn is_match(&self, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        self.find_at(&chars, 0).is_some()
    }

    pub fn find(&self, text: &str) -> Option<String> {
        let chars: Vec<char> = text.chars().collect();
        self.find_at(&chars, 0)
            .and_then(|caps| span_text(&chars, caps[0]))
    }

    pub fn find_all(&self, text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        let mut from = 0;
        while from <= chars.len() {
            let Some(caps) = self.find_at(&chars, from) else {
                break;
            };
            let (s, e) = caps[0].unwrap_or((from, from));
            out.push(chars[s..e].iter().collect());
            from = if e == s { e + 1 } else { e };
        }
        out
    }

    /// Capture groups of the first match; index 0 is the whole match.
    pub fn captures(&self, text: &str) -> Option<Vec<Option<String>>> {
        let chars: Vec<char> = text.chars().collect();
        let caps = self.find_at(&chars, 0)?;
        Some(
            caps.into_iter()
                .map(|span| span_text(&chars, span))
                .collect(),
        )
    }

    /// Replaces all matches. `$0`..`$9` in `replacement` refer to capture groups; `$$` is a literal `$`.
    pub fn replace_all(&self, text: &str, replacement: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::new();
        let mut from = 0;
        let mut last = 0;
        while from <= chars.len() {
            let Some(caps) = self.find_at(&chars, from) else {
                break;
            };
            let (s, e) = caps[0].unwrap_or((from, from));
            out.extend(chars[last..s].iter());
            let rep: Vec<char> = replacement.chars().collect();
            let mut i = 0;
            while i < rep.len() {
                if rep[i] == '$' && i + 1 < rep.len() {
                    if rep[i + 1] == '$' {
                        out.push('$');
                        i += 2;
                        continue;
                    }
                    if let Some(d) = rep[i + 1].to_digit(10) {
                        if let Some(Some(span)) = caps.get(d as usize) {
                            out.extend(chars[span.0..span.1].iter());
                        }
                        i += 2;
                        continue;
                    }
                }
                out.push(rep[i]);
                i += 1;
            }
            last = e;
            if e == s {
                if s < chars.len() {
                    out.push(chars[s]);
                }
                last = s + 1;
                from = s + 1;
            } else {
                from = e;
            }
        }
        if last < chars.len() {
            out.extend(chars[last..].iter());
        }
        out
    }

    pub fn split(&self, text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        let mut out = Vec::new();
        let mut from = 0;
        let mut last = 0;
        while from <= chars.len() {
            let Some(caps) = self.find_at(&chars, from) else {
                break;
            };
            let (s, e) = caps[0].unwrap_or((from, from));
            if e == s {
                from = s + 1;
                continue;
            }
            out.push(chars[last..s].iter().collect());
            last = e;
            from = e;
        }
        out.push(chars[last.min(chars.len())..].iter().collect());
        out
    }
}

fn span_text(chars: &[char], span: Option<(usize, usize)>) -> Option<String> {
    span.map(|(s, e)| chars[s..e].iter().collect())
}

// ---------------------------------------------------------------------------
// UTF-8 helpers
// ---------------------------------------------------------------------------

/// Returns true if `bytes` (each 0..=255) form valid UTF-8.
pub fn utf8_valid(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok()
}

pub fn utf8_encode(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

pub fn utf8_decode(bytes: &[u8]) -> Result<String, String> {
    String::from_utf8(bytes.to_vec())
        .map_err(|e| format!("invalid UTF-8 at byte {}", e.utf8_error().valid_up_to()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn re(p: &str) -> Regex {
        Regex::new(p).unwrap()
    }

    #[test]
    fn literals_and_classes() {
        assert!(re("abc").is_match("xxabcxx"));
        assert!(!re("abd").is_match("xxabcxx"));
        assert!(re("^[a-c]+$").is_match("abccba"));
        assert!(!re("^[a-c]+$").is_match("abcd"));
        assert!(re(r"^\d{3}-\d{4}$").is_match("555-1234"));
        assert!(re("(?i)hello").is_match("HeLLo"));
    }

    #[test]
    fn find_and_captures() {
        assert_eq!(re(r"\d+").find("abc 123 def"), Some("123".to_string()));
        assert_eq!(re(r"\d+").find_all("1 22 333"), vec!["1", "22", "333"]);
        let caps = re(r"(\w+)@(\w+)\.com")
            .captures("mail bob@example.com now")
            .unwrap();
        assert_eq!(caps[1].as_deref(), Some("bob"));
        assert_eq!(caps[2].as_deref(), Some("example"));
    }

    #[test]
    fn alternation_repeat_lazy() {
        assert!(re("^(cat|dog)s?$").is_match("dogs"));
        assert_eq!(re("a.*?b").find("aXbYb"), Some("aXb".to_string()));
        assert_eq!(re("a.*b").find("aXbYb"), Some("aXbYb".to_string()));
        assert!(re(r"\bfoo\b").is_match("a foo b"));
        assert!(!re(r"\bfoo\b").is_match("afoob"));
    }

    #[test]
    fn replace_and_split() {
        assert_eq!(
            re(r"(\w+) (\w+)").replace_all("hello world", "$2 $1"),
            "world hello"
        );
        assert_eq!(re(r"\s+").split("a  b   c"), vec!["a", "b", "c"]);
        assert_eq!(re("x*").replace_all("abc", "-"), "-a-b-c-");
    }

    #[test]
    fn invalid_patterns_are_errors() {
        assert!(Regex::new("(abc").is_err());
        assert!(Regex::new("*a").is_err());
        assert!(Regex::new("[abc").is_err());
    }

    #[test]
    fn utf8() {
        assert!(utf8_valid("héllo".as_bytes()));
        assert!(!utf8_valid(&[0xff, 0xfe]));
        assert_eq!(utf8_decode(&[104, 105]).unwrap(), "hi");
    }
}
