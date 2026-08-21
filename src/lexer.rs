//contents of lexer.rs
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq)]
enum BlockMode {
    Unknown,
    Indentation,
    Braces,
}

#[derive(Debug)]
pub struct LexError {
    message: String,
    position: usize,
    line: usize,
    column: usize,
}
    impl std::fmt::Display for LexError {
    fn fmt( &self,f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f,"{} at line {}, column {}",
            self.message,self.line,self.column
        )
    }
}
impl std::error::Error for LexError {}
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Num(i64),
    Float(f64),
    Boolean(bool),
    String(String),
    Identifier(String),
    FloatType,
    Bool,
    StringType,
    Const,
    Fn,
    Return,
    If,
    Else,
    While,
    For,
    In,
    Defer,
    Break,
    Continue,
    And,
    Or,
    Not,
    Plus,
    Minus,
    Star,
    Slash,
    Equal,
    EqualEqual,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    AndAnd,
    OrOr,
    Bang,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Comma,
    Colon,
    Dot,
    DotDot,
    Arrow,
    Indent,
    Dedent,
    NewLine,
    Eof,
    Struct,
    Trait,
    Impl,
    Match,
    Import,
    From,
    Async,
    Await,
    LeftBrace,
    RightBrace,
    Enum,
}

pub struct Lexer {
    input: Vec<char>,
    position: usize,
    line: usize,
    column: usize,
    line_start: bool,
    at_file_start: bool,
    indentation_stack: Vec<usize>,
    pending_tokens: VecDeque<Token>,
    block_mode: BlockMode,
}

impl Lexer {
    fn set_indentation_mode(&mut self) -> Result<(), LexError> {
    match self.block_mode {
        BlockMode::Unknown => {
            self.block_mode = BlockMode::Indentation;
            Ok(())
        }
        BlockMode::Indentation => Ok(()),
        BlockMode::Braces => Err(LexError {
            message: "Cannot use indentation blocks after '{' brace syntax was selected for this file".into(),
            position: self.position,
            line: self.line,
            column: self.column,
        }),
    }
}

fn set_brace_mode(&mut self) -> Result<(), LexError> {
    match self.block_mode {
        BlockMode::Unknown => {
            self.block_mode = BlockMode::Braces;
            Ok(())
        }
        BlockMode::Braces => Ok(()),
        BlockMode::Indentation => Err(LexError {
            message: "Cannot use '{' brace blocks after indentation syntax was selected for this file".into(),
            position: self.position,
            line: self.line,
            column: self.column,
        }),
    }
}

    pub fn new(source: &str) -> Self {
   Self {
            input: source.chars().collect(),
            position: 0,
            line: 1,
            column: 1,
            line_start: true,
            at_file_start: true,
            indentation_stack: vec![0],
            pending_tokens: VecDeque::new(),
            block_mode: BlockMode::Unknown,
        }
}
    fn handle_indentation(&mut self) -> Result<(), LexError> {
    let mut spaces = 0;

    while let Some(ch) = self.peek() {
        if ch == ' ' {
            spaces += 1;
            self.advance();
        } else if ch == '\t' {
            spaces += 4;
            self.advance();
        } else {
            break;
        }
    }

    let current = *self.indentation_stack.last().unwrap();

    // Only select indentation mode when actual indentation occurs.
    if spaces > current {
        self.set_indentation_mode()?;
        self.indentation_stack.push(spaces);
        self.pending_tokens.push_back(Token::Indent);
    } else if spaces < current {
        // We are already in indentation mode if we're dedenting.
        self.set_indentation_mode()?;
        while self.indentation_stack.len() > 1
            && spaces < *self.indentation_stack.last().unwrap()
        {
            self.indentation_stack.pop();
            self.pending_tokens.push_back(Token::Dedent);
        }

        if spaces != *self.indentation_stack.last().unwrap() {
            return Err(LexError {
                message: "Invalid indentation level".into(),
                position: self.position,
                line: self.line,
                column: self.column,
            });
        }
    }

    Ok(())
}
    fn peek(&self) -> Option<char> {
        self.input.get(self.position).copied()
    }
    fn advance(&mut self) -> Option<char> {
    if self.position >= self.input.len() {
        return None;
    }
    let ch = self.input[self.position];
    self.position += 1;
    if ch == '\n' {
        self.line += 1;
        self.column = 1;
    } else {
        self.column += 1;
    }
    Some(ch)
}
    fn skip_spaces(&mut self) {
    while let Some(ch) = self.peek() {
        if ch == ' ' || ch == '\t' {
            self.advance();
        } else {
            break;
        }
    }
}

     fn skip_multiline_comment(&mut self) -> Result<(), LexError> {
    // '/' has already been consumed.

    if self.peek() != Some('*') {
        return Err(LexError {
            message: "Expected '*' after '/'".into(),
            position: self.position,
            line: self.line,
            column: self.column,
        });
    }

    self.advance(); // *

    while let Some(ch) = self.peek() {
        if ch == '*'
            && self.input.get(self.position + 1) == Some(&'/')
        {
            self.advance(); // *
            self.advance(); // /
            return Ok(());
        }
        self.advance();
    }

    Err(LexError {
        message: "Unterminated multi-line comment".into(),
        position: self.position,
        line: self.line,
        column: self.column,
    })
}
    fn read_identifier(&mut self) -> String {
        let mut value = String::new();
        while let Some(ch) = self.peek() {
            if ch.is_alphanumeric() || ch == '_' {
                value.push(ch);
                self.advance();
            } else {
                break;
            }
        }
        value
    }

    fn read_string(&mut self) -> Result<String, LexError> {
    let mut value = String::new();
    self.advance(); // consume opening quote
    while let Some(ch) = self.peek() {
        if ch == '"' {
            self.advance();
            return Ok(value);
        }
        if ch == '\n' {
            return Err(LexError {
            message: "Unterminated string literal".into(),
    position: self.position,
    line: self.line,
    column: self.column,
    });
        }
        value.push(ch);
        self.advance();
    }
    Err(LexError {
        message: "Unterminated string literal".into(),
        position: self.position,
        line: self.line,
        column: self.column,
    })
}
    fn next_token(&mut self) -> Result<Token, LexError> {
        if let Some(token) = self.pending_tokens.pop_front() {
            return Ok(token);
        }
        if self.line_start {
    if self.block_mode == BlockMode::Indentation {
        self.handle_indentation()?;
    }

    self.line_start = false;

    if let Some(token) = self.pending_tokens.pop_front() {
        return Ok(token);
    }
}
        while matches!(self.peek(), Some(' ' | '\t')) {
    self.advance();
}
        let ch = match self.peek() {
    Some(c) => c,

    None => {
        if self.block_mode == BlockMode::Indentation
            && self.indentation_stack.len() > 1
        {
            self.indentation_stack.pop();
            return Ok(Token::Dedent);
        }

        return Ok(Token::Eof);
    }
};
        match ch {

            '{' => {
    self.set_brace_mode()?;
    self.advance();
    Ok(Token::LeftBrace)
}

'}' => {
    self.set_brace_mode()?;
    self.advance();
    Ok(Token::RightBrace)
}

            '\n' => {
    self.advance();
    self.line_start = true;
    if matches!(self.peek(), Some('\n')) {
        self.next_token()
    }
    else {
        Ok(Token::NewLine)
    }
}
            '&' => {
    self.advance();
    if self.peek()==Some('&') {
        self.advance();
        Ok(Token::AndAnd)
    } else {
        Err(LexError {
            message:"Expected &&".into(),
            position:self.position,
            line:self.line,
            column:self.column,
        })
    }
}
            '#' => {
    while let Some(ch) = self.peek() {
        self.advance();
        if ch == '\n' {
            self.line_start = true;
            break;
        }
    }
    self.next_token()
    }
            '"' => {
                Ok(Token::String(self.read_string()?))
            }
            '[' => {
                self.advance();
                 Ok(Token::LeftBracket)
            }
            ']' => {
                self.advance();
                Ok(Token::RightBracket)
            }
            '|' => {
    self.advance();

    if self.peek() == Some('|') {
        self.advance();
        Ok(Token::OrOr)
    } else {
        Err(LexError {
            message: "Unexpected '|'. Use '||' for logical OR".into(),
            position: self.position,
            line: self.line,
            column: self.column,
        })
    }
}
            '(' => {
                self.advance();
                Ok(Token::LeftParen)
            }
            ')' => {
                self.advance();
                Ok(Token::RightParen)
            }
            ':' => {
                self.advance();
                Ok(Token::Colon)
            }
            ',' => {
                self.advance();
                Ok(Token::Comma)
            }
            '.' => {
                self.advance();
                if self.peek() == Some('.') {
                    self.advance();
                    Ok(Token::DotDot)
                } else {
                    Ok(Token::Dot)
                }
            }
            '+' => {
                self.advance();
                Ok(Token::Plus)
            }
            '-' => {
                self.advance();
                if self.peek() == Some('>') {
                    self.advance();
                    Ok(Token::Arrow)
                } else {
                    Ok(Token::Minus)
                }
            }
            '*' => {
                self.advance();
                Ok(Token::Star)
            }
             '/' => {
                self.advance();

                match self.peek() {
                    Some('*') => {
    if self.input.get(self.position + 1) == Some(&'*') {
        return Err(LexError {
            message: "Documentation comments using /** ... */ are not allowed".into(),
            position: self.position,
            line: self.line,
            column: self.column,
        });
    }

    self.skip_multiline_comment()?;
    self.next_token()
}

                    Some('/') => {
    self.advance(); // consume second /

    if self.peek() == Some('/') {
        return Err(LexError {
            message: "Triple-slash comments using /// are not allowed".into(),
            position: self.position,
            line: self.line,
            column: self.column,
        });
    }

    while let Some(ch) = self.peek() {
        self.advance();

        if ch == '\n' {
            self.line_start = true;
            break;
        }
    }

    self.next_token()
}

                    _ => {
                        Ok(Token::Slash)
                    }
                }
            }
            '=' => {
                self.advance();
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::EqualEqual)
                } else {
                    Ok(Token::Equal)
                }
            }
            '<' => {
    self.advance();
    if self.peek() == Some('=') {
        self.advance();
        Ok(Token::LessEqual)
    } else {
        Ok(Token::Less)
    }
}
'>' => {
    self.advance();
    if self.peek() == Some('=') {
        self.advance();
        Ok(Token::GreaterEqual)
    } else {
        Ok(Token::Greater)
    }
}
'!' => {
    self.advance();
    if self.peek() == Some('=') {
        self.advance();
        Ok(Token::NotEqual)
    } else {
        Ok(Token::Bang)
    }
}
            c if c.is_ascii_digit() => {
                self.read_number()
            }
                c if c.is_alphabetic() || c == '_' => {
                    let ident = self.read_identifier();
                    match ident.as_str() {
                        "const" => Ok(Token::Const),
                        "enum" => Ok(Token::Enum),
                        "fn" => Ok(Token::Fn),
                        "defer" => Ok(Token::Defer),
                        "if" => Ok(Token::If),
                        "and" => Ok(Token::And),
                        "or" => Ok(Token::Or),
                        "not" => Ok(Token::Not),
                        "for" => Ok(Token::For),
                        "in" => Ok(Token::In),
                        "else" => Ok(Token::Else),
                        "while" => Ok(Token::While),
                        "break" => Ok(Token::Break),
                        "continue" => Ok(Token::Continue),
                        "struct" => Ok(Token::Struct),
                        "trait" => Ok(Token::Trait),
                        "impl" => Ok(Token::Impl),
                        "match" => Ok(Token::Match),
                        "import" => Ok(Token::Import),
                        "from" => Ok(Token::From),
                        "async" => Ok(Token::Async),
                        "await" => Ok(Token::Await),
                        "return" => Ok(Token::Return),
                        "true" => Ok(Token::Boolean(true)),
                        "false" => Ok(Token::Boolean(false)),
                        _ => {
    Ok(Token::Identifier(ident))
}
                    }       
                }
                _ => Err(LexError {
                    message: format!("Unexpected character '{}'", ch),
                    position: self.position,
                    line: self.line,
                    column: self.column,
                }),
            }
        }

    pub fn tokenize(&mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        loop {
            let token = self.next_token()?;
            let end = token == Token::Eof;
            tokens.push(token);
            if end {
                 break;
            }
        }
    Ok(tokens)
    }
    fn read_number(&mut self) -> Result<Token, LexError> {
    let mut value = String::new();
    let mut has_dot = false;
    while let Some(ch) = self.peek() {
        if ch.is_ascii_digit() {
            value.push(ch);
            self.advance();
        }
        else if ch == '.' {
    if self.input.get(self.position + 1) == Some(&'.') {
        break;
    }
    if has_dot {
        return Err(LexError {
            message: "Multiple decimal points in number".into(),
            position: self.position,
            line: self.line,
            column: self.column,
        });
    }
    has_dot = true;
    value.push(ch);
    self.advance();
}
        else {
            break;
        }
    }
    if has_dot {
        match value.parse::<f64>() {
            Ok(v) => Ok(Token::Float(v)),
            Err(_) => Err(LexError {
                message: "Invalid float literal".into(),
                position: self.position,
                line: self.line,
                column: self.column,
            })
        }
    }
    else {
        match value.parse::<i64>() {
            Ok(v) => Ok(Token::Num(v)),
            Err(_) => Err(LexError {
                message: "Invalid number literal".into(),
                position: self.position,
                line: self.line,
                column: self.column,
            })
        }
    }
}
}