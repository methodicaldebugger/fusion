//! Structured type expressions.
//!
//! Before this module existed, a type annotation was parsed once by the parser, flattened back
//! into a `String`, and then re-parsed by every consumer with `starts_with("Option<")` style
//! prefix tests.  That made the grammar impossible to extend (function types, `Option<T>[]`,
//! `T[][]`) and meant every consumer carried its own slightly different parser.
//!
//! `TypeExpr` is the parser and semantic front-end's structural representation of a written
//! type. The compatibility AST still stores its canonical `Display` spelling in legacy String
//! fields; this module is the single grammar used to produce and consume those spellings. The
//! staged AST migration will replace those compatibility fields with `TypeExpr` directly.
//!
//! `TypeExpr::parse(&expr.to_string()) == Ok(expr)` for every expression the parser can build.
//!
//! Grammar (postfix `[]` binds tighter than anything else):
//!
//! ```text
//! type     := primary ( "[" "]" )*
//! primary  := "*opaque"
//!           | "(" type ("," type)+ [","] ")"        tuple (two or more elements)
//!           | "(" function-type ")"                 grouping, so that arrays of functions can be written
//!           | "fn" "(" [type ("," type)* [","]] ")" [ "->" type ]
//!           | name [ "<" type ("," type)* ">" ]     name may be dotted: geometry.Point
//! ```
//!
//! A function type without `->` returns `void`.

use std::fmt;

/// A written type, as a tree.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeExpr {
    /// A (possibly dotted) type name with optional generic arguments:
    /// `num`, `geometry.Point`, `Option<num>`, `HashMap<string, num>`.
    Named { name: String, args: Vec<TypeExpr> },
    /// `T[]`
    Array(Box<TypeExpr>),
    /// `(A, B, ...)` with at least two elements.
    Tuple(Vec<TypeExpr>),
    /// `fn(A, B) -> R`.  Parsed and printed, but not yet accepted by the type checker.
    Function {
        params: Vec<TypeExpr>,
        ret: Box<TypeExpr>,
    },
    /// `*opaque`, the only pointer type allowed at a foreign boundary.
    OpaquePointer,
}

/// Generic type constructors that the compiler itself gives meaning to. Any other generic
/// spelling (`Stack<num>`) names a user-declared generic type.
pub const BUILTIN_GENERIC_CONSTRUCTORS: &[&str] =
    &["Array", "Iterator", "Option", "Result", "HashMap", "Task"];

/// A malformed type spelling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeParseError {
    pub message: String,
}

impl TypeParseError {
    fn new(message: impl Into<String>) -> Self {
        TypeParseError {
            message: message.into(),
        }
    }
}

impl fmt::Display for TypeParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TypeParseError {}

impl TypeExpr {
    /// A plain name with no generic arguments.
    pub fn named(name: &str) -> TypeExpr {
        TypeExpr::Named {
            name: name.to_string(),
            args: Vec::new(),
        }
    }

    /// A name applied to generic arguments.
    pub fn generic(name: &str, args: Vec<TypeExpr>) -> TypeExpr {
        TypeExpr::Named {
            name: name.to_string(),
            args,
        }
    }

    /// Parse the canonical source spelling of a type.
    pub fn parse(source: &str) -> Result<TypeExpr, TypeParseError> {
        let tokens = tokenize(source)?;
        let mut parser = StringTypeParser {
            tokens,
            position: 0,
        };
        let expr = parser.parse_type()?;
        if parser.position != parser.tokens.len() {
            return Err(TypeParseError::new(format!(
                "Unexpected trailing input in type '{}'",
                source.trim()
            )));
        }
        Ok(expr)
    }

    /// Resolve a parenthesised list of types.  Shared by the token parser and the string parser
    /// so both apply the same rule: two or more elements form a tuple, and a single function
    /// type is plain grouping.
    pub fn from_parenthesized(mut items: Vec<TypeExpr>) -> Result<TypeExpr, &'static str> {
        if items.len() == 1 && matches!(items[0], TypeExpr::Function { .. }) {
            return Ok(items.remove(0));
        }
        if items.len() < 2 {
            return Err("Tuple types require at least two elements");
        }
        Ok(TypeExpr::Tuple(items))
    }

    /// Replace every bare use of the type name `from` (a `Named` with no arguments) by `to`,
    /// at any depth.  Used to turn `Self` into the implementing type.
    pub fn substitute_name(&self, from: &str, to: &str) -> TypeExpr {
        match self {
            TypeExpr::Named { name, args } => {
                if args.is_empty() && name == from {
                    TypeExpr::named(to)
                } else {
                    TypeExpr::Named {
                        name: name.clone(),
                        args: args.iter().map(|arg| arg.substitute_name(from, to)).collect(),
                    }
                }
            }
            TypeExpr::Array(inner) => TypeExpr::Array(Box::new(inner.substitute_name(from, to))),
            TypeExpr::Tuple(items) => TypeExpr::Tuple(
                items
                    .iter()
                    .map(|item| item.substitute_name(from, to))
                    .collect(),
            ),
            TypeExpr::Function { params, ret } => TypeExpr::Function {
                params: params
                    .iter()
                    .map(|param| param.substitute_name(from, to))
                    .collect(),
                ret: Box::new(ret.substitute_name(from, to)),
            },
            TypeExpr::OpaquePointer => TypeExpr::OpaquePointer,
        }
    }

    /// Replace declaration-local type parameters with `unknown` while preserving the complete
    /// written type structure, including arguments to user-defined generic types.
    ///
    /// This is still a compatibility operation: generic user types are not yet represented as
    /// parameterized semantic `Type` values, so the checker currently resolves their nominal
    /// constructor while retaining the arguments in the AST spelling for later generic checking.
    pub fn erase_generics(&self, parameters: &[String]) -> TypeExpr {
        match self {
            TypeExpr::Named { name, args } => {
                if parameters.iter().any(|parameter| parameter == name) {
                    return TypeExpr::named("unknown");
                }
                if args.is_empty() {
                    return self.clone();
                }
                TypeExpr::Named {
                    name: name.clone(),
                    args: args
                        .iter()
                        .map(|arg| arg.erase_generics(parameters))
                        .collect(),
                }
            }
            TypeExpr::Array(inner) => TypeExpr::Array(Box::new(inner.erase_generics(parameters))),
            TypeExpr::Tuple(items) => TypeExpr::Tuple(
                items
                    .iter()
                    .map(|item| item.erase_generics(parameters))
                    .collect(),
            ),
            TypeExpr::Function { params, ret } => TypeExpr::Function {
                params: params
                    .iter()
                    .map(|param| param.erase_generics(parameters))
                    .collect(),
                ret: Box::new(ret.erase_generics(parameters)),
            },
            TypeExpr::OpaquePointer => TypeExpr::OpaquePointer,
        }
    }

    /// True if `symbol` is used anywhere in this type, as a name or inside any argument.
    pub fn mentions(&self, symbol: &str) -> bool {
        match self {
            TypeExpr::Named { name, args } => {
                name == symbol || args.iter().any(|arg| arg.mentions(symbol))
            }
            TypeExpr::Array(inner) => inner.mentions(symbol),
            TypeExpr::Tuple(items) => items.iter().any(|item| item.mentions(symbol)),
            TypeExpr::Function { params, ret } => {
                params.iter().any(|param| param.mentions(symbol)) || ret.mentions(symbol)
            }
            TypeExpr::OpaquePointer => false,
        }
    }
}

fn write_joined(f: &mut fmt::Formatter<'_>, items: &[TypeExpr]) -> fmt::Result {
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{}", item)?;
    }
    Ok(())
}

impl fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeExpr::Named { name, args } => {
                f.write_str(name)?;
                if !args.is_empty() {
                    f.write_str("<")?;
                    write_joined(f, args)?;
                    f.write_str(">")?;
                }
                Ok(())
            }
            TypeExpr::Array(inner) => {
                if matches!(**inner, TypeExpr::Function { .. }) {
                    write!(f, "({})[]", inner)
                } else {
                    write!(f, "{}[]", inner)
                }
            }
            TypeExpr::Tuple(items) => {
                f.write_str("(")?;
                write_joined(f, items)?;
                f.write_str(")")
            }
            TypeExpr::Function { params, ret } => {
                f.write_str("fn(")?;
                write_joined(f, params)?;
                write!(f, ") -> {}", ret)
            }
            TypeExpr::OpaquePointer => f.write_str("*opaque"),
        }
    }
}

// ---------------------------------------------------------------------------
// String parser
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Word(String),
    Lt,
    Gt,
    Comma,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Star,
    Arrow,
}

fn is_word_stop(chars: &[char], index: usize) -> bool {
    let c = chars[index];
    c.is_whitespace()
        || matches!(c, '<' | '>' | ',' | '(' | ')' | '[' | ']' | '*')
        || (c == '-' && index + 1 < chars.len() && chars[index + 1] == '>')
}

fn tokenize(source: &str) -> Result<Vec<Tok>, TypeParseError> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        let c = chars[index];
        if c.is_whitespace() {
            index += 1;
            continue;
        }
        match c {
            '<' => {
                tokens.push(Tok::Lt);
                index += 1;
            }
            '>' => {
                tokens.push(Tok::Gt);
                index += 1;
            }
            ',' => {
                tokens.push(Tok::Comma);
                index += 1;
            }
            '(' => {
                tokens.push(Tok::LParen);
                index += 1;
            }
            ')' => {
                tokens.push(Tok::RParen);
                index += 1;
            }
            '[' => {
                tokens.push(Tok::LBracket);
                index += 1;
            }
            ']' => {
                tokens.push(Tok::RBracket);
                index += 1;
            }
            '*' => {
                tokens.push(Tok::Star);
                index += 1;
            }
            '-' if index + 1 < chars.len() && chars[index + 1] == '>' => {
                tokens.push(Tok::Arrow);
                index += 2;
            }
            _ => {
                // A name: names in this compiler are alphanumeric plus '_' and '.', but being
                // permissive here keeps internal mangled names parseable.
                let start = index;
                while index < chars.len() && !is_word_stop(&chars, index) {
                    index += 1;
                }
                if index == start {
                    return Err(TypeParseError::new(format!(
                        "Unexpected character '{}' in type '{}'",
                        c,
                        source.trim()
                    )));
                }
                tokens.push(Tok::Word(chars[start..index].iter().collect()));
            }
        }
    }
    Ok(tokens)
}

struct StringTypeParser {
    tokens: Vec<Tok>,
    position: usize,
}

impl StringTypeParser {
    fn peek(&self) -> Option<&Tok> {
        self.tokens.get(self.position)
    }

    fn eat(&mut self, expected: &Tok) -> bool {
        if self.peek() == Some(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn parse_type(&mut self) -> Result<TypeExpr, TypeParseError> {
        let mut base = self.parse_primary()?;
        while self.peek() == Some(&Tok::LBracket) {
            self.position += 1;
            if !self.eat(&Tok::RBracket) {
                return Err(TypeParseError::new("Expected ']' after array type"));
            }
            base = TypeExpr::Array(Box::new(base));
        }
        Ok(base)
    }

    fn parse_primary(&mut self) -> Result<TypeExpr, TypeParseError> {
        match self.peek().cloned() {
            Some(Tok::Star) => {
                self.position += 1;
                match self.peek().cloned() {
                    Some(Tok::Word(word)) if word == "opaque" => {
                        self.position += 1;
                        Ok(TypeExpr::OpaquePointer)
                    }
                    _ => Err(TypeParseError::new(
                        "Only '*opaque' pointer types are supported",
                    )),
                }
            }
            Some(Tok::LParen) => {
                self.position += 1;
                let items = self.parse_list(&Tok::RParen)?;
                TypeExpr::from_parenthesized(items).map_err(TypeParseError::new)
            }
            Some(Tok::Word(word)) => {
                self.position += 1;
                if word == "fn" {
                    self.parse_function()
                } else {
                    self.parse_named(word)
                }
            }
            _ => Err(TypeParseError::new("Expected type")),
        }
    }

    /// Parse `type, type, ... [,] <close>` where the opening token is already consumed.  The
    /// list may be empty.  Consumes the closing token.
    fn parse_list(&mut self, close: &Tok) -> Result<Vec<TypeExpr>, TypeParseError> {
        let mut items = Vec::new();
        if self.peek() != Some(close) {
            loop {
                items.push(self.parse_type()?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
                if self.peek() == Some(close) {
                    break;
                }
            }
        }
        if !self.eat(close) {
            return Err(TypeParseError::new("Unterminated type list"));
        }
        Ok(items)
    }

    fn parse_function(&mut self) -> Result<TypeExpr, TypeParseError> {
        if !self.eat(&Tok::LParen) {
            return Err(TypeParseError::new("Expected '(' after 'fn' in function type"));
        }
        let params = self.parse_list(&Tok::RParen)?;
        let ret = if self.eat(&Tok::Arrow) {
            self.parse_type()?
        } else {
            TypeExpr::named("void")
        };
        Ok(TypeExpr::Function {
            params,
            ret: Box::new(ret),
        })
    }

    fn parse_named(&mut self, name: String) -> Result<TypeExpr, TypeParseError> {
        let mut args = Vec::new();
        if self.eat(&Tok::Lt) {
            loop {
                args.push(self.parse_type()?);
                if self.eat(&Tok::Comma) {
                    continue;
                }
                break;
            }
            if !self.eat(&Tok::Gt) {
                return Err(TypeParseError::new(
                    "Expected '>' after generic type arguments",
                ));
            }
        }
        Ok(TypeExpr::Named { name, args })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(source: &str) -> TypeExpr {
        let parsed = TypeExpr::parse(source).unwrap_or_else(|e| panic!("{source}: {e}"));
        assert_eq!(parsed.to_string(), source, "canonical spelling must round-trip");
        assert_eq!(TypeExpr::parse(&parsed.to_string()), Ok(parsed.clone()));
        parsed
    }

    #[test]
    fn simple_and_dotted_names() {
        assert_eq!(roundtrip("num"), TypeExpr::named("num"));
        assert_eq!(roundtrip("geometry.Point"), TypeExpr::named("geometry.Point"));
        assert_eq!(roundtrip("*opaque"), TypeExpr::OpaquePointer);
    }

    #[test]
    fn generics_and_arrays_compose_in_any_order() {
        roundtrip("Option<num>");
        roundtrip("HashMap<string, num[]>");
        roundtrip("Option<num>[]");
        roundtrip("HashMap<string, num>[]");
        roundtrip("num[][]");
        roundtrip("Result<Option<num>[], string>");
        roundtrip("Task<Result<num, string>>");
    }

    #[test]
    fn tuples() {
        assert_eq!(
            roundtrip("(num, string)"),
            TypeExpr::Tuple(vec![TypeExpr::named("num"), TypeExpr::named("string")])
        );
        roundtrip("(num[], (bool, float))");
        roundtrip("(num, string)[]");
        assert!(TypeExpr::parse("(num)").is_err());
        assert!(TypeExpr::parse("()").is_err());
        assert!(TypeExpr::parse("(num, string,)").is_ok());
    }

    #[test]
    fn function_types() {
        assert_eq!(
            roundtrip("fn(num, num) -> num"),
            TypeExpr::Function {
                params: vec![TypeExpr::named("num"), TypeExpr::named("num")],
                ret: Box::new(TypeExpr::named("num")),
            }
        );
        // No arrow means a void return, and the canonical spelling makes it explicit.
        assert_eq!(
            TypeExpr::parse("fn(num)").unwrap().to_string(),
            "fn(num) -> void"
        );
        roundtrip("fn() -> void");
        roundtrip("fn(fn(num) -> num) -> fn(num) -> num");
        roundtrip("Option<fn(string) -> bool>");
        roundtrip("(fn(num) -> num)[]");
        // The return type extends as far as possible, so this is a function returning num[].
        assert_eq!(
            TypeExpr::parse("fn(num) -> num[]").unwrap(),
            TypeExpr::Function {
                params: vec![TypeExpr::named("num")],
                ret: Box::new(TypeExpr::Array(Box::new(TypeExpr::named("num")))),
            }
        );
    }

    #[test]
    fn malformed_spellings_are_rejected() {
        for bad in ["", "Option<", "Option<num", "num[", "fn num", "*ptr", "num num", "<num>"] {
            assert!(TypeExpr::parse(bad).is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn mangled_internal_names_survive() {
        let mangled = "__fusion_m4_74657374_t5_506f696e74";
        assert_eq!(roundtrip(mangled), TypeExpr::named(mangled));
        roundtrip("Option<__fusion_m4_74657374_t5_506f696e74>[]");
    }

    #[test]
    fn substitute_name_replaces_bare_names_at_any_depth() {
        let expr = TypeExpr::parse("Option<Self>[]").unwrap();
        assert_eq!(expr.substitute_name("Self", "Point").to_string(), "Option<Point>[]");
        let tuple = TypeExpr::parse("(Self, fn(Self) -> Self, num)").unwrap();
        assert_eq!(
            tuple.substitute_name("Self", "Point").to_string(),
            "(Point, fn(Point) -> Point, num)"
        );
        // A name that merely contains the text, or takes arguments, is left alone.
        let other = TypeExpr::parse("SelfLike<num>").unwrap();
        assert_eq!(other.substitute_name("Self", "Point"), other);
    }

    #[test]
    fn erase_generics_replaces_parameters_and_preserves_user_type_arguments() {
        let params = vec!["T".to_string(), "E".to_string()];
        let erase = |source: &str| {
            TypeExpr::parse(source)
                .unwrap()
                .erase_generics(&params)
                .to_string()
        };
        assert_eq!(erase("T"), "unknown");
        assert_eq!(erase("T[]"), "unknown[]");
        assert_eq!(erase("num"), "num");
        assert_eq!(erase("Stack<num>"), "Stack<num>");
        assert_eq!(erase("Stack<Option<T>>[]"), "Stack<Option<unknown>>[]");
        // Both built-in and user-defined constructors preserve recursively transformed args.
        assert_eq!(erase("Option<T>"), "Option<unknown>");
        assert_eq!(erase("Result<T, E>"), "Result<unknown, unknown>");
        assert_eq!(erase("HashMap<string, Stack<T>>"), "HashMap<string, Stack<unknown>>");
        assert_eq!(erase("(T, Pair<T, E>)"), "(unknown, Pair<unknown, unknown>)");
        assert_eq!(erase("fn(T) -> Stack<T>"), "fn(unknown) -> Stack<unknown>");
        // Names that are not parameters are untouched.
        assert_eq!(erase("Item"), "Item");
    }

    #[test]
    fn mentions_finds_symbols_at_any_depth() {
        let expr = TypeExpr::parse("HashMap<string, (Point, fn(Shape) -> num)[]>").unwrap();
        assert!(expr.mentions("Point"));
        assert!(expr.mentions("Shape"));
        assert!(expr.mentions("HashMap"));
        assert!(!expr.mentions("Circle"));
    }
}
