// A lexer (also called a lexical analyzer or tokenizer)
// is the first stage of a compiler or interpreter
// It's job is to read raw source code and break it into meaningful pieces called tokens.

// The lexer produces something like:
// Source -> Token
// 42 -> Integer or number
// = -> Assign
// + -> PLUS
// x -> Identifier
// : -> Colon

// example:
// if x > 10:
//    print("Hello")

/*IF
IDENTIFIER(x)
GREATER_THAN
INTEGER(10)
COLON
INDENT
IDENTIFIER(print)
LEFT_PAREN
STRING("Hello")
RIGHT_PAREN
DEDENT
*/

// Later add braces <{}> to indentation
// Later add comments with <//> and </**/>

use std::collections::VecDeque;

#[derive(Debug)]
pub struct LexError {
    message: String,
    position: usize,
    line: usize,
    column: usize,
}

    impl std::fmt::Display for LexError {

    fn fmt(
        &self,
        f: &mut std::fmt::Formatter
    ) -> std::fmt::Result {

        write!(
            f,
            "{} at line {}, column {}",
            self.message,
            self.line,
            self.column
        )
    }
}

impl std::error::Error for LexError {}

#[derive(Debug, Clone, PartialEq)]
pub enum Token {

    // Keywords
    Fn,
    Struct,
    Class,
    Trait,
    Impl,
    Match,
    Async,
    Await,
    Import,
    From,
    Return,
    Let,
    Mut,
    If,

    // Identifiers
    Identifier(String),

    // Literals
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Pipe,
    EqualEqual,
    NotEqual,

    Less,
    LessEqual,

    Greater,
    GreaterEqual,

    Equal,
    Arrow,
    Colon,

    // Indentation
    Indent,
    Dedent,
    NewLine,

    // Delimiters
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,

    Comma,
    Dot,

    Eof,
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
}



impl Lexer {

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
    }
}

    fn handle_indentation(&mut self) -> Result<(), LexError> {

    let mut spaces = 0;

    while let Some(ch) = self.peek() {

        if ch == ' ' {
            spaces += 1;
            self.advance();
        }

        else if ch == '\t' {
            spaces += 4;
            self.advance();
        }

        else {
            break;
        }
    }


    let current = *self.indentation_stack.last().unwrap();

if spaces > current {
    self.indentation_stack.push(spaces);
    self.pending_tokens.push_back(Token::Indent);
}

else if spaces < current {

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
            if self.at_file_start {
                self.skip_spaces();
                self.at_file_start = false;
            } else {
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
                if self.indentation_stack.len() > 1 {
                    self.indentation_stack.pop();
                    return Ok(Token::Dedent);
                }

                return Ok(Token::Eof);
            }
        };

        match ch {

            '\n' => {
                self.advance();
                self.line_start = true;
                Ok(Token::NewLine)
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
                Ok(Token::Pipe)
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
                Ok(Token::Dot)
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
                Ok(Token::Slash)
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
        Err(LexError {
            message: "Unexpected '!'".into(),
            position: self.position,
            line: self.line,
            column: self.column,
        })
    }
}

            c if c.is_ascii_digit() => {
                self.read_number()
            }

                c if c.is_alphabetic() || c == '_' => {
                    let ident = self.read_identifier();

                    match ident.as_str() {

                        "fn" => Ok(Token::Fn),
                        "if" => Ok(Token::If),
                        "struct" => Ok(Token::Struct),
                        "class" => Ok(Token::Class),
                        "trait" => Ok(Token::Trait),
                        "impl" => Ok(Token::Impl),
                        "match" => Ok(Token::Match),
                        "import" => Ok(Token::Import),
                        "from" => Ok(Token::From),

                        "async" => Ok(Token::Async),
                        "await" => Ok(Token::Await),

                        "return" => Ok(Token::Return),

                        "let" => Ok(Token::Let),
                        "mut" => Ok(Token::Mut),

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
            Ok(v) => Ok(Token::Integer(v)),
            Err(_) => Err(LexError {
                message: "Invalid integer literal".into(),
                position: self.position,
                line: self.line,
                column: self.column,
            })
        }
    }
}
}