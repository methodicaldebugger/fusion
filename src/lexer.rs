use std::collections::VecDeque;

use crate::span::{Span, Spanned};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockMode {
    Unknown,
    Indentation,
    Braces,
}

#[derive(Debug, Clone)]
pub struct LexError {
    pub message: String,
    pub position: usize,
    pub line: usize,
    pub column: usize,
}

impl std::fmt::Display for LexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "error: {} at {}:{}", self.message, self.line, self.column)
    }
}

impl std::error::Error for LexError {}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Num(i64), Float(f64), Boolean(bool), String(String), Identifier(String),
    FloatType, NumType, BoolType, StringType,
    Main, Const, Fn, Return, If, Else, While, For, In, Defer, Break, Continue,
    And, Or, Not,
    Plus, Minus, Star, Slash, Equal, EqualEqual, NotEqual, Less, LessEqual,
    Greater, GreaterEqual, AndAnd, OrOr, Bang,
    LeftParen, RightParen, LeftBracket, RightBracket, Comma, Colon, Dot, DotDot,
    Arrow, Indent, Dedent, NewLine, Eof, FatArrow, DoubleColon,
    Struct, Trait, Impl, Match, Import, From, Async, Await, LeftBrace, RightBrace, Enum,
}

pub struct Lexer {
    input: Vec<char>,
    position: usize,
    line: usize,
    column: usize,
    line_start: bool,
    indentation_stack: Vec<usize>,
    pending_tokens: VecDeque<Spanned<Token>>,
    block_mode: BlockMode,
    brace_depth: usize,
    init_error: Option<LexError>,
}

impl Lexer {
    fn detect_block_mode(source: &str) -> Result<BlockMode, LexError> {
    for (line_no, line) in source.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("//")
        {
            continue;
        }

        if trimmed == "main:" {
            return Ok(BlockMode::Indentation);
        }

        if trimmed == "main{" || trimmed == "main {" {
            return Ok(BlockMode::Braces);
        }

        if trimmed.starts_with("main") {
            return Err(LexError {
                message: "invalid main declaration; use `main:` or `main {`".into(),
                position: 0,
                line: line_no + 1,
                column: 1,
            });
        }
    }

    Err(LexError {
        message: "missing main block; expected `main:` or `main {`".into(),
        position: 0,
        line: 1,
        column: 1,
    })
}

    pub fn new(source: &str, block_mode: BlockMode) -> Self {
        let (mode, init_error) = match block_mode {
            BlockMode::Unknown => match Self::detect_block_mode(source) {
                Ok(mode) => (mode, None),
                Err(error) => (BlockMode::Unknown, Some(error)),
            },
            mode => (mode, None),
        };
        Self {
            input: source.chars().collect(), position: 0, line: 1, column: 1,
            line_start: true, indentation_stack: vec![0], pending_tokens: VecDeque::new(),
            block_mode: mode, brace_depth: 0, init_error,
        }
    }

    fn peek(&self) -> Option<char> { self.input.get(self.position).copied() }
    fn peek_next(&self) -> Option<char> { self.input.get(self.position + 1).copied() }

    fn advance(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.position += 1;
        if ch == '\n' { self.line += 1; self.column = 1; } else { self.column += 1; }
        Some(ch)
    }

    fn error<T>(&self, message: impl Into<String>) -> Result<T, LexError> {
        Err(LexError { message: message.into(), position: self.position, line: self.line, column: self.column })
    }

    fn read_identifier(&mut self) -> String {
        let mut value = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' { value.push(ch); self.advance(); } else { break; }
        }
        value
    }

    fn read_string(&mut self) -> Result<String, LexError> {
        self.advance();
        let mut value = String::new();
        while let Some(ch) = self.peek() {
            match ch {
                '"' => { self.advance(); return Ok(value); }
                '\n' => return self.error("unterminated string literal"),
                '\\' => {
                    self.advance();
                    let escaped = match self.peek() {
                        Some('n') => '\n', Some('r') => '\r', Some('t') => '\t',
                        Some('"') => '"', Some('\\') => '\\',
                        Some(other) => return self.error(format!("unknown escape sequence \\{}", other)),
                        None => return self.error("unterminated string literal"),
                    };
                    self.advance(); value.push(escaped);
                }
                _ => { value.push(ch); self.advance(); }
            }
        }
        self.error("unterminated string literal")
    }

    fn read_number(&mut self) -> Result<Token, LexError> {
        let start = self.position;
        let mut value = String::new();
        let mut has_dot = false;
        while let Some(ch) = self.peek() {
            if ch.is_ascii_digit() { value.push(ch); self.advance(); }
            else if ch == '.' && self.peek_next() != Some('.') {
                if has_dot { return self.error("multiple decimal points in number literal"); }
                has_dot = true; value.push(ch); self.advance();
            } else { break; }
        }
        if has_dot {
            value.parse::<f64>().map(Token::Float).map_err(|_| LexError {
                message: "invalid float literal".into(), position: start, line: self.line, column: self.column,
            })
        } else {
            value.parse::<i64>().map(Token::Num).map_err(|_| LexError {
                message: "invalid integer literal".into(), position: start, line: self.line, column: self.column,
            })
        }
    }

    fn read_indentation(&self) -> (usize, usize) {
        let mut index = self.position;
        let mut width = 0;
        while let Some(ch) = self.input.get(index).copied() {
            match ch {
                ' ' => { width += 1; index += 1; }
                '\t' => { width += 8 - width % 8; index += 1; }
                _ => break,
            }
        }
        (index, width)
    }

    fn handle_indentation(&mut self) -> Result<(), LexError> {
        if self.block_mode != BlockMode::Indentation { return Ok(()); }
        let start = self.position;
        let (end, width) = self.read_indentation();
        while self.position < end { self.advance(); }
        if matches!(self.peek(), Some('\n') | None) { return Ok(()); }

        let current = *self.indentation_stack.last().unwrap();
        if width > current {
            self.indentation_stack.push(width);
            self.pending_tokens.push_back(Spanned::new(Token::Indent, start, end));
        } else if width < current {
            while self.indentation_stack.len() > 1 && width < *self.indentation_stack.last().unwrap() {
                self.indentation_stack.pop();
                self.pending_tokens.push_back(Spanned::new(Token::Dedent, start, end));
            }
            if width != *self.indentation_stack.last().unwrap() {
                return Err(LexError {
                    message: "unindent does not match any outer indentation level".into(),
                    position: start, line: self.line, column: self.column,
                });
            }
        }
        Ok(())
    }

    fn keyword(ident: &str) -> Token {
        match ident {
            "num" => Token::NumType, "float" => Token::FloatType, "bool" => Token::BoolType,
            "string" => Token::StringType, "const" => Token::Const, "enum" => Token::Enum,
            "fn" => Token::Fn, "main" => Token::Main, "defer" => Token::Defer, "if" => Token::If,
            "and" => Token::And, "or" => Token::Or, "not" => Token::Not, "for" => Token::For,
            "in" => Token::In, "else" => Token::Else, "while" => Token::While,
            "break" => Token::Break, "continue" => Token::Continue, "struct" => Token::Struct,
            "trait" => Token::Trait, "impl" => Token::Impl, "match" => Token::Match,
            "import" => Token::Import, "from" => Token::From, "async" => Token::Async,
            "await" => Token::Await, "return" => Token::Return, "true" => Token::Boolean(true),
            "false" => Token::Boolean(false), _ => Token::Identifier(ident.into()),
        }



}

    fn next_spanned(&mut self) -> Result<Spanned<Token>, LexError> {
        loop {
            if let Some(token) = self.pending_tokens.pop_front() { return Ok(token); }

            if self.line_start {
                let line_start = self.position;
                let (indent_end, _) = self.read_indentation();
                let mut look = indent_end;
                while let Some(' ' | '\t') = self.input.get(look).copied() { look += 1; }
                if self.input.get(look) == Some(&'\n') {
                    while matches!(self.peek(), Some(' ' | '\t')) { self.advance(); }
                    self.advance();
                    self.line_start = true;
                    continue;
                }
                if self.position == line_start { /* no indentation */ }
                self.handle_indentation()?;
                self.line_start = false;
                if let Some(token) = self.pending_tokens.pop_front() { return Ok(token); }
            }

            while matches!(self.peek(), Some(' ' | '\t')) { self.advance(); }
            let start = self.position;
            let ch = match self.peek() {
                Some(ch) => ch,
                None => {
                    if self.block_mode == BlockMode::Indentation && self.indentation_stack.len() > 1 {
                        self.indentation_stack.pop();
                        return Ok(Spanned::new(Token::Dedent, start, start));
                    }
                    if self.brace_depth != 0 { return self.error("unclosed `{` at end of file"); }
                    return Ok(Spanned::new(Token::Eof, start, start));
                }
            };

            match ch {
                '\n' => { self.advance(); self.line_start = true; return Ok(Spanned::new(Token::NewLine, start, self.position)); }
                '#' => { while let Some(c) = self.peek() { self.advance(); if c == '\n' { self.line_start = true; break; } } continue; }
                '/' if self.peek_next() == Some('/') => {
                    self.advance(); self.advance();
                    if self.peek() == Some('/') { return self.error("triple-slash comments are not supported"); }
                    while let Some(c) = self.peek() { self.advance(); if c == '\n' { self.line_start = true; break; } }
                    continue;
                }
                '/' if self.peek_next() == Some('*') => {
                    let comment_start = self.position;
                    self.advance(); self.advance();
                    if self.peek() == Some('*') { return self.error("documentation comments are not supported"); }
                    let mut closed = false;
                    while let Some(c) = self.peek() {
                        if c == '*' && self.peek_next() == Some('/') { self.advance(); self.advance(); closed = true; break; }
                        self.advance();
                    }
                    if !closed { return Err(LexError { message: "unterminated block comment".into(), position: comment_start, line: self.line, column: self.column }); }
                    continue;
                }


                '{' => {
    self.advance();
    self.brace_depth += 1;

    return Ok(Spanned::new(
        Token::LeftBrace,
        start,
        self.position,
    ));
}

'}' => {
    if self.brace_depth == 0 {
        return self.error("unexpected `}`");
    }

    self.advance();
    self.brace_depth -= 1;

    return Ok(Spanned::new(
        Token::RightBrace,
        start,
        self.position,
    ));
}
                ';' => return self.error("semicolons are not allowed in Fusion"),
                '"' => return Ok(Spanned::new(Token::String(self.read_string()?), start, self.position)),
                '[' => { self.advance(); return Ok(Spanned::new(Token::LeftBracket, start, self.position)); }
                ']' => { self.advance(); return Ok(Spanned::new(Token::RightBracket, start, self.position)); }
                '(' => { self.advance(); return Ok(Spanned::new(Token::LeftParen, start, self.position)); }
                ')' => { self.advance(); return Ok(Spanned::new(Token::RightParen, start, self.position)); }
                ',' => { self.advance(); return Ok(Spanned::new(Token::Comma, start, self.position)); }
                ':' => { self.advance(); let token = if self.peek() == Some(':') { self.advance(); Token::DoubleColon } else { Token::Colon }; return Ok(Spanned::new(token, start, self.position)); }
                '.' => { self.advance(); let token = if self.peek() == Some('.') { self.advance(); Token::DotDot } else { Token::Dot }; return Ok(Spanned::new(token, start, self.position)); }
                '+' => { self.advance(); return Ok(Spanned::new(Token::Plus, start, self.position)); }
                '-' => { self.advance(); let token = if self.peek() == Some('>') { self.advance(); Token::Arrow } else { Token::Minus }; return Ok(Spanned::new(token, start, self.position)); }
                '*' => { self.advance(); return Ok(Spanned::new(Token::Star, start, self.position)); }
                '/' => { self.advance(); return Ok(Spanned::new(Token::Slash, start, self.position)); }
                '=' => { self.advance(); let token = if self.peek() == Some('=') { self.advance(); Token::EqualEqual } else if self.peek() == Some('>') { self.advance(); Token::FatArrow } else { Token::Equal }; return Ok(Spanned::new(token, start, self.position)); }
                '<' => { self.advance(); let token = if self.peek() == Some('=') { self.advance(); Token::LessEqual } else { Token::Less }; return Ok(Spanned::new(token, start, self.position)); }
                '>' => { self.advance(); let token = if self.peek() == Some('=') { self.advance(); Token::GreaterEqual } else { Token::Greater }; return Ok(Spanned::new(token, start, self.position)); }
                '!' => { self.advance(); let token = if self.peek() == Some('=') { self.advance(); Token::NotEqual } else { Token::Bang }; return Ok(Spanned::new(token, start, self.position)); }
                '&' => { self.advance(); if self.peek() == Some('&') { self.advance(); return Ok(Spanned::new(Token::AndAnd, start, self.position)); } return self.error("expected `&&`"); }
                '|' => { self.advance(); if self.peek() == Some('|') { self.advance(); return Ok(Spanned::new(Token::OrOr, start, self.position)); } return self.error("expected `||`"); }
                c if c.is_ascii_digit() => return Ok(Spanned::new(self.read_number()?, start, self.position)),
                c if c.is_alphabetic() || c == '_' => {
                    let ident = self.read_identifier();
                    return Ok(Spanned::new(Self::keyword(&ident), start, self.position));
                }
                _ => return self.error(format!("unexpected character `{}`", ch)),
            }
        }
    }

    pub fn tokenize(&mut self) -> Result<Vec<Spanned<Token>>, LexError> {
        if let Some(error) = self.init_error.take() { return Err(error); }
        let mut tokens = Vec::new();
        loop {
            let token = self.next_spanned()?;
            let eof = token.node == Token::Eof;
            tokens.push(token);
            if eof { break; }
        }
        Ok(tokens)
    }
}
