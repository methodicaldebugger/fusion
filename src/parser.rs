//contents of parser.rs


use crate::ast::*;
use crate::errors::ParseError;
use crate::lexer::Token;
use crate::span::{Span, Spanned};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockStyle {
    Unknown,
    Indentation,
    Braces,
}

pub struct Parser {
    tokens: Vec<Spanned<Token>>,
    position: usize,
    block_style: BlockStyle,
    seen_main: bool,
    pending_block_styles: Vec<BlockStyle>,
    current_function_return_type: Option<Option<String>>,
}

impl Parser {
    pub fn new(tokens: Vec<Spanned<Token>>) -> Self {
    Self {
        tokens,
        position: 0,
        block_style: BlockStyle::Unknown,
        pending_block_styles: Vec::new(),
        seen_main: false,
        current_function_return_type: None,
    }
}

    // ------------------------------------------------------------
    // Token helpers
    // ------------------------------------------------------------

    fn current(&self) -> &Token {
        self.tokens
            .get(self.position)
            .map(|token| &token.node)
            .unwrap_or(&Token::Eof)
    }

    fn current_span(&self) -> Span {
        self.tokens
            .get(self.position)
            .map(|token| token.span)
            .unwrap_or_else(|| {
                self.tokens
                    .last()
                    .map(|token| token.span)
                    .unwrap_or_else(|| Span::new(0, 0))
            })
    }

    fn previous_span(&self) -> Span {
        self.position
            .checked_sub(1)
            .and_then(|pos| self.tokens.get(pos))
            .map(|token| token.span)
            .unwrap_or_else(|| Span::new(0, 0))
    }

    fn span_from(&self, start: usize) -> Span {
        Span::new(start, self.previous_span().end)
    }

    fn peek_at(&self, position: usize) -> Option<&Token> {
        self.tokens.get(position).map(|token| &token.node)
    }

    fn advance(&mut self) {
        if self.position < self.tokens.len() {
            self.position += 1;
        }
    }

    fn consume(&mut self, expected: &Token) -> bool {
        if self.current() == expected {
            self.advance();
            true
        } else {
            false
        }
    }

    fn error<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError::new(message, self.current_span()))
    }

    fn skip_newlines(&mut self) {
        while self.current() == &Token::NewLine {
            self.advance();
        }
    }

    fn is_statement_boundary(&self) -> bool {
        matches!(
            self.current(),
            Token::NewLine
                | Token::Dedent
                | Token::RightBrace
                | Token::Eof
        )
    }

    // ------------------------------------------------------------
    // Block style
    // ------------------------------------------------------------

    fn use_block_style(
    &mut self,
    style: BlockStyle,
) -> Result<(), ParseError> {
    match self.block_style {
        BlockStyle::Unknown => {
            self.pending_block_styles.push(style);
            Ok(())
        }

        existing if existing == style => Ok(()),

        existing => Err(ParseError {
            message: format!(
                "Mixed block styles are not allowed: expected {:?}, found {:?}",
                existing,
                style
            ),
            span: self.current_span(),
        }),
    }
}

    fn require_block_style(
    &self,
    style: BlockStyle,
) -> Result<(), ParseError> {
    if self.seen_main && self.block_style != style {
        return Err(ParseError {
            message: format!(
                "Mixed block styles are not allowed: expected {:?}, found {:?}",
                self.block_style,
                style
            ),
            span: self.current_span(),
        });
    }

    Ok(())
}

    fn establish_main_style(
    &mut self,
    style: BlockStyle,
) -> Result<(), ParseError> {
    if self.seen_main {
        return Err(ParseError {
            message: "Multiple main blocks are not allowed".into(),
            span: self.current_span(),
        });
    }

    for pending in &self.pending_block_styles {
        if *pending != style {
            return Err(ParseError {
                message:
                    "All blocks in a Fusion file must use the same block style"
                        .into(),
                span: self.current_span(),
            });
        }
    }

    self.pending_block_styles.clear();
    self.block_style = style;
    self.seen_main = true;

    Ok(())
}

    fn parse_style_block(&mut self) -> Result<Vec<Statement>, ParseError> {
    let style = match self.current() {
        Token::Colon => {
            self.advance();
            BlockStyle::Indentation
        }

        Token::LeftBrace => BlockStyle::Braces,

        _ => {
            return self.error("Expected ':' or '{' to begin block");
        }
    };

    self.use_block_style(style)?;

    match style {
        BlockStyle::Indentation => {
            self.skip_newlines();
            self.parse_indentation_block()
        }

        BlockStyle::Braces => self.parse_brace_block(),

        BlockStyle::Unknown => unreachable!(),
    }
}

    // ------------------------------------------------------------
    // Type helpers
    // ------------------------------------------------------------

    fn current_is_type_keyword(&self) -> bool {
        matches!(
            self.current(),
            Token::NumType
                | Token::FloatType
                | Token::BoolType
                | Token::StringType
        )
    }

    fn parse_type(&mut self) -> Result<String, ParseError> {
        match self.current() {
            Token::NumType => {
                self.advance();
                Ok("num".to_string())
            }

            Token::FloatType => {
                self.advance();
                Ok("float".to_string())
            }

            Token::BoolType => {
                self.advance();
                Ok("bool".to_string())
            }

            Token::StringType => {
                self.advance();
                Ok("string".to_string())
            }

            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                Ok(name)
            }

            _ => self.error("Expected type"),
        }
    }

    // ------------------------------------------------------------
    // Top-level statements
    // ------------------------------------------------------------

    fn parse_statement(&mut self) -> Result<Option<Statement>, ParseError> {
        match self.current() {
            Token::NewLine => {
                self.advance();

                if self.current() == &Token::Eof {
                    return Ok(None);
                }

                self.parse_statement()
            }

            Token::Main => Ok(Some(self.parse_main()?)),

            Token::Fn => Ok(Some(self.parse_function()?)),

            Token::Struct => Ok(Some(self.parse_struct()?)),

            Token::Enum => Ok(Some(self.parse_enum()?)),

            Token::Match => Ok(Some(self.parse_match()?)),

            Token::If => Ok(Some(self.parse_if()?)),

            Token::While => Ok(Some(self.parse_while()?)),

            Token::For => Ok(Some(self.parse_for()?)),

            Token::Defer => {
                let start = self.current_span().start;
                self.advance();

                let expression = self.parse_expression()?;

                Ok(Some(Statement::Defer {
                    expression,
                    span: self.span_from(start),
                }))
            }

            Token::Return => {
    let start = self.current_span().start;
    self.advance();

    let value = if self.is_statement_boundary() {
        None
    } else {
        Some(self.parse_expression()?)
    };

    Ok(Some(Statement::Return {
        value,
        span: self.span_from(start),
    }))
}

            Token::Break => {
                let start = self.current_span().start;
                self.advance();

                Ok(Some(Statement::Break {
                    span: self.span_from(start),
                }))
            }

            Token::Continue => {
                let start = self.current_span().start;
                self.advance();

                Ok(Some(Statement::Continue {
                    span: self.span_from(start),
                }))
            }

            Token::Const => Ok(Some(self.parse_const()?)),

            Token::Trait => Ok(Some(self.parse_trait()?)),

            Token::Impl => Ok(Some(self.parse_impl()?)),

            Token::Import | Token::From | Token::Async | Token::Await => {
                self.error(format!(
                    "Token {:?} is recognized by the lexer but is not yet supported by the AST",
                    self.current()
                ))
            }

            Token::Identifier(_) | Token::NumType | Token::FloatType
            | Token::BoolType | Token::StringType => {
                Ok(Some(self.parse_identifier_or_declaration()?))
            }

            _ => Ok(None),
        }
    }

    // ------------------------------------------------------------
    // main
    // ------------------------------------------------------------

    fn parse_main(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;
        self.advance();

        let style = match self.current() {
            Token::Colon => {
                self.advance();
                BlockStyle::Indentation
            }

            Token::LeftBrace => BlockStyle::Braces,

            _ => {
                return self.error("Expected ':' or '{' after 'main'");
            }
        };

        self.establish_main_style(style)?;

        let body = match style {
            BlockStyle::Indentation => {
                self.skip_newlines();
                self.parse_indentation_block()?
            }

            BlockStyle::Braces => self.parse_brace_block()?,

            BlockStyle::Unknown => unreachable!(),
        };

        Ok(Statement::Main {
            body,
            span: self.span_from(start),
        })
    }

    // ------------------------------------------------------------
    // const
    // ------------------------------------------------------------

    fn parse_const(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;
        self.advance();

        let name_span = self.current_span();

        let name = match self.current() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                name
            }

            _ => {
                return self.error("Expected identifier after 'const'");
            }
        };

        let declared_type = if self.consume(&Token::Colon) {
            Some(self.parse_type()?)
        } else {
            None
        };

        if !self.consume(&Token::Equal) {
            return self.error("Expected '=' in constant declaration");
        }

        let value = self.parse_expression()?;

        Ok(Statement::ConstDeclaration {
            name,
            name_span,
            declared_type,
            value,
            span: self.span_from(start),
        })
    }

    // ------------------------------------------------------------
    // Variable declarations / expressions
    // ------------------------------------------------------------

    fn looks_like_typed_declaration(&self) -> bool {
        match self.current() {
            Token::NumType
            | Token::FloatType
            | Token::BoolType
            | Token::StringType => {
                matches!(
                    self.peek_at(self.position + 1),
                    Some(Token::Identifier(_))
                )
            }

            Token::Identifier(_) => {
                matches!(
                    self.peek_at(self.position + 1),
                    Some(Token::Identifier(_))
                )
            }

            _ => false,
        }
    }

    fn parse_identifier_or_declaration(
        &mut self,
    ) -> Result<Statement, ParseError> {
        if self.looks_like_typed_declaration() {
            self.parse_variable_declaration()
        } else {
            self.parse_expression_statement()
        }
    }

    fn parse_variable_declaration(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;

        let declared_type = self.parse_type()?;

        let mut names = Vec::new();
        let mut name_spans = Vec::new();

        loop {
            let name_span = self.current_span();

            let name = match self.current() {
                Token::Identifier(name) => {
                    let name = name.clone();
                    self.advance();
                    name
                }

                _ => {
                    return self.error("Expected identifier in variable declaration");
                }
            };

            names.push(name);
            name_spans.push(name_span);

            if !self.consume(&Token::Comma) {
                break;
            }

            if !matches!(self.current(), Token::Identifier(_)) {
                return self.error("Expected identifier after ','");
            }
        }

        if !self.consume(&Token::Equal) {
            return self.error("Expected '=' in variable declaration");
        }

        let mut values = Vec::new();

        loop {
            values.push(self.parse_expression()?);

            if self.consume(&Token::Comma) {
                continue;
            }

            break;
        }

        if names.len() != values.len() {
            return self.error(format!(
                "Number of variables ({}) does not match number of values ({})",
                names.len(),
                values.len()
            ));
        }

        let end = self.previous_span().end;

        let declarations = names
            .into_iter()
            .zip(name_spans)
            .zip(values)
            .map(|((name, name_span), value)| {
                VariableDeclaration {
                    name,
                    name_span,
                    declared_type: Some(declared_type.clone()),
                    value,
                    span: Span::new(start, end),
                }
            })
            .collect();

        Ok(Statement::VariableDeclarations {
            declarations,
            span: Span::new(start, end),
        })
    }

    fn parse_expression_statement(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;

        let expression = self.parse_expression()?;

        if self.consume(&Token::Equal) {
            let value = self.parse_expression()?;

            return Ok(Statement::Assignment {
                target: expression,
                value,
                span: self.span_from(start),
            });
        }

        Ok(Statement::Expression {
            expression,
            span: self.span_from(start),
        })
    }

    // ------------------------------------------------------------
    // if
    // ------------------------------------------------------------

    fn parse_if(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;
        self.advance();

        let condition = self.parse_expression()?;

        let body = self.parse_style_block()?;

        let else_body = if self.current() == &Token::Else {
            self.advance();
            Some(self.parse_style_block()?)
        } else {
            None
        };

        Ok(Statement::If {
            condition,
            body,
            else_body,
            span: self.span_from(start),
        })
    }

    // ------------------------------------------------------------
    // while
    // ------------------------------------------------------------

    fn parse_while(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;
        self.advance();

        let condition = self.parse_expression()?;

        let body = self.parse_style_block()?;

        Ok(Statement::While {
            condition,
            body,
            span: self.span_from(start),
        })
    }

    // ------------------------------------------------------------
    // for
    // ------------------------------------------------------------

    fn parse_for(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;
        self.advance();

        let variable = match self.current() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                name
            }

            _ => {
                return self.error("Expected identifier after 'for'");
            }
        };

        if !self.consume(&Token::In) {
            return self.error("Expected 'in' in for loop");
        }

        let start_expression = self.parse_expression()?;

        if !self.consume(&Token::DotDot) {
            return self.error("Expected '..' in for loop");
        }

        let end_expression = self.parse_expression()?;

        let body = self.parse_style_block()?;

        Ok(Statement::For {
            variable,
            start: start_expression,
            end: end_expression,
            body,
            span: self.span_from(start),
        })
    }

    // ------------------------------------------------------------
    // struct
    // ------------------------------------------------------------

    fn parse_struct(&mut self) -> Result<Statement, ParseError> {
    let start = self.current_span().start;

    self.advance(); // consume 'struct'

    let name = match self.current() {
        Token::Identifier(name) => {
            let name = name.clone();
            self.advance();
            name
        }

        _ => {
            return self.error("Expected struct name after 'struct'");
        }
    };

    let style = match self.current() {
        Token::Colon => {
            self.advance();
            BlockStyle::Indentation
        }

        Token::LeftBrace => BlockStyle::Braces,

        _ => {
            return self.error(
                "Expected ':' or '{' after struct name",
            );
        }
    };

    self.use_block_style(style)?;

    let fields = match style {
        BlockStyle::Indentation => {
            self.skip_newlines();
            self.parse_indentation_struct_fields()?
        }

        BlockStyle::Braces => {
            self.parse_brace_struct_fields()?
        }

        BlockStyle::Unknown => unreachable!(),
    };

    Ok(Statement::Struct {
        name,
        fields,
        span: self.span_from(start),
    })
}

    fn parse_indentation_struct_fields(
    &mut self,
) -> Result<Vec<StructField>, ParseError> {
    if !self.consume(&Token::Indent) {
        return self.error("Expected indentation after struct declaration");
    }

    let mut fields = Vec::new();

    self.skip_newlines();

    while self.current() != &Token::Dedent
        && self.current() != &Token::Eof
    {
        let start = self.current_span().start;
        let name_span = self.current_span();

        let name = match self.current() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                name
            }

            _ => {
                return self.error(
                    "Expected field name in struct declaration",
                );
            }
        };

        if !self.consume(&Token::Colon) {
            return self.error(
                "Expected ':' after struct field name",
            );
        }

        let type_name = self.parse_type()?;

        let span = Span::new(start, self.previous_span().end);

        fields.push(StructField {
            name,
            name_span,
            type_name,
            span,
        });

        self.consume(&Token::Comma);
        self.skip_newlines();
    }

    if self.current() == &Token::Dedent {
        self.advance();
    }

    Ok(fields)
}

    fn parse_brace_struct_fields(
    &mut self,
) -> Result<Vec<StructField>, ParseError> {
    if !self.consume(&Token::LeftBrace) {
        return self.error("Expected '{' after struct name");
    }

    let mut fields = Vec::new();

    self.skip_newlines();

    while self.current() != &Token::RightBrace
        && self.current() != &Token::Eof
    {
        let start = self.current_span().start;
        let name_span = self.current_span();

        let name = match self.current() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                name
            }

            _ => {
                return self.error(
                    "Expected field name in struct declaration",
                );
            }
        };

        if !self.consume(&Token::Colon) {
            return self.error(
                "Expected ':' after struct field name",
            );
        }

        let type_name = self.parse_type()?;

        let span = Span::new(start, self.previous_span().end);

        fields.push(StructField {
            name,
            name_span,
            type_name,
            span,
        });

        // Newlines are valid separators in brace-style structs.
        self.skip_newlines();

        if self.consume(&Token::Comma) {
            self.skip_newlines();
        }
    }

    if !self.consume(&Token::RightBrace) {
        return self.error(
            "Expected '}' after struct declaration",
        );
    }

    Ok(fields)
}

    // ------------------------------------------------------------
    // enum
    // ------------------------------------------------------------

    fn parse_enum(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;
        self.advance();

        let name = match self.current() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                name
            }

            _ => {
                return self.error("Expected identifier after 'enum'");
            }
        };

        let style = match self.current() {
            Token::Colon => {
                self.advance();
                BlockStyle::Indentation
            }

            Token::LeftBrace => BlockStyle::Braces,

            _ => {
                return self.error("Expected ':' or '{' after enum name");
            }
        };

        self.use_block_style(style)?;

        let variants = match style {
            BlockStyle::Indentation => {
                self.skip_newlines();
                self.parse_indentation_enum_variants()?
            }

            BlockStyle::Braces => self.parse_brace_enum_variants()?,

            BlockStyle::Unknown => unreachable!(),
        };

        Ok(Statement::Enum {
            name,
            variants,
            span: self.span_from(start),
        })
    }

    fn parse_enum_variant(&mut self) -> Result<EnumVariant, ParseError> {
        let start = self.current_span().start;
        let name_span = self.current_span();

        let name = match self.current() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                name
            }

            _ => {
                return self.error("Expected enum variant name");
            }
        };

        let mut fields = Vec::new();

        if self.consume(&Token::LeftParen) {
            if self.current() != &Token::RightParen {
                loop {
                    fields.push(self.parse_type()?);

                    if self.consume(&Token::Comma) {
                        if self.current() == &Token::RightParen {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }

            if !self.consume(&Token::RightParen) {
                return self.error("Expected ')' after enum variant fields");
            }
        }

        Ok(EnumVariant {
            name,
            name_span,
            fields,
            span: self.span_from(start),
        })
    }

    fn parse_brace_enum_variants(
        &mut self,
    ) -> Result<Vec<EnumVariant>, ParseError> {
        if !self.consume(&Token::LeftBrace) {
            return self.error("Expected '{' after enum name");
        }

        let mut variants = Vec::new();

        while self.current() != &Token::RightBrace
            && self.current() != &Token::Eof
        {
            if self.current() == &Token::NewLine {
                self.advance();
                continue;
            }

            variants.push(self.parse_enum_variant()?);

            if self.consume(&Token::Comma) {
                self.skip_newlines();
            } else if self.current() != &Token::RightBrace {
                return self.error("Expected ',' or '}' after enum variant");
            }
        }

        if !self.consume(&Token::RightBrace) {
            return self.error("Expected '}' after enum variants");
        }

        Ok(variants)
    }

    fn parse_indentation_enum_variants(
        &mut self,
    ) -> Result<Vec<EnumVariant>, ParseError> {
        if !self.consume(&Token::Indent) {
            return self.error("Expected indentation after enum declaration");
        }

        let mut variants = Vec::new();

        while self.current() != &Token::Dedent
            && self.current() != &Token::Eof
        {
            if self.current() == &Token::NewLine {
                self.advance();
                continue;
            }

            variants.push(self.parse_enum_variant()?);

            if self.current() == &Token::Comma {
                self.advance();
            }

            self.skip_newlines();
        }

        if self.current() == &Token::Dedent {
            self.advance();
        }

        Ok(variants)
    }

    // ------------------------------------------------------------
    // match
    // ------------------------------------------------------------

    fn parse_pattern(&mut self) -> Result<Pattern, ParseError> {
        let start = self.current_span().start;

        let kind = match self.current() {
            Token::Identifier(name) => {
                let first = name.clone();
                self.advance();

                if first == "_" {
                    PatternKind::Wildcard
                } else if self.consume(&Token::DoubleColon) {
                    let variant = match self.current() {
                        Token::Identifier(name) => {
                            let variant = name.clone();
                            self.advance();
                            variant
                        }

                        _ => {
                            return self.error(
                                "Expected enum variant after '::'",
                            );
                        }
                    };

                    let mut bindings = Vec::new();

                    if self.consume(&Token::LeftParen) {
                        if self.current() != &Token::RightParen {
                            loop {
                                match self.current() {
                                    Token::Identifier(name) => {
                                        bindings.push(name.clone());
                                        self.advance();
                                    }

                                    _ => {
                                        return self.error(
                                            "Expected identifier in pattern binding",
                                        );
                                    }
                                }

                                if self.consume(&Token::Comma) {
                                    if self.current() == &Token::RightParen {
                                        break;
                                    }
                                } else {
                                    break;
                                }
                            }
                        }

                        if !self.consume(&Token::RightParen) {
                            return self.error(
                                "Expected ')' after pattern bindings",
                            );
                        }
                    }

                    PatternKind::Variant {
                        name: format!("{}::{}", first, variant),
                        bindings,
                    }
                } else {
                    PatternKind::Identifier(first)
                }
            }

            Token::Num(value) => {
                let value = *value;
                self.advance();
                PatternKind::Number(value)
            }

            Token::Float(value) => {
                let value = *value;
                self.advance();
                PatternKind::Float(value)
            }

            Token::String(value) => {
                let value = value.clone();
                self.advance();
                PatternKind::String(value)
            }

            Token::Boolean(value) => {
                let value = *value;
                self.advance();
                PatternKind::Boolean(value)
            }

            _ => {
                return self.error("Expected match pattern");
            }
        };

        Ok(Pattern {
            kind,
            span: self.span_from(start),
        })
    }

    fn parse_match(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;
        self.advance();

        let expression = self.parse_expression()?;

        let style = match self.current() {
            Token::Colon => {
                self.advance();
                BlockStyle::Indentation
            }

            Token::LeftBrace => BlockStyle::Braces,

            _ => {
                return self.error(
                    "Expected ':' or '{' after match expression",
                );
            }
        };

        self.use_block_style(style)?;

        let arms = match style {
            BlockStyle::Indentation => self.parse_indentation_match_arms()?,

            BlockStyle::Braces => self.parse_brace_match_arms()?,

            BlockStyle::Unknown => unreachable!(),
        };

        Ok(Statement::Match {
            expression,
            arms,
            span: self.span_from(start),
        })
    }

    fn parse_match_arm(&mut self) -> Result<MatchArm, ParseError> {
    let start = self.current_span().start;

    let pattern = self.parse_pattern()?;

    if !self.consume(&Token::FatArrow) {
        return self.error("Expected '=>' after match pattern");
    }

    if self.current() == &Token::Colon {
        return self.error(
            "':' is not allowed after '=>'; use a block style directly",
        );
    }

    let body = if self.current() == &Token::NewLine {
        self.advance();
        self.skip_newlines();

        self.parse_indentation_block()?
    } else if self.current() == &Token::LeftBrace {
        self.parse_brace_block()?
    } else {
        let statement = self
            .parse_statement()?
            .ok_or_else(|| {
                ParseError::new(
                    "Expected statement after '=>'",
                    self.current_span(),
                )
            })?;

        vec![statement]
    };

    Ok(MatchArm {
        pattern,
        body,
        span: self.span_from(start),
    })
}

    fn parse_indentation_match_arms(
        &mut self,
    ) -> Result<Vec<MatchArm>, ParseError> {
        self.skip_newlines();

        if !self.consume(&Token::Indent) {
            return self.error("Expected indentation after match");
        }

        let mut arms = Vec::new();

        while self.current() != &Token::Dedent
            && self.current() != &Token::Eof
        {
            if self.current() == &Token::NewLine {
                self.advance();
                continue;
            }

            arms.push(self.parse_match_arm()?);

            self.skip_newlines();
        }

        if self.current() == &Token::Dedent {
            self.advance();
        }

        Ok(arms)
    }

    fn parse_brace_match_arms(
        &mut self,
    ) -> Result<Vec<MatchArm>, ParseError> {
        if !self.consume(&Token::LeftBrace) {
            return self.error("Expected '{' after match expression");
        }

        let mut arms = Vec::new();

        while self.current() != &Token::RightBrace
            && self.current() != &Token::Eof
        {
            if self.current() == &Token::NewLine {
                self.advance();
                continue;
            }

            arms.push(self.parse_match_arm()?);

            if self.current() == &Token::Comma {
                self.advance();
            }

            self.skip_newlines();
        }

        if !self.consume(&Token::RightBrace) {
            return self.error("Expected '}' after match arms");
        }

        Ok(arms)
    }

    // ------------------------------------------------------------
    // Blocks
    // ------------------------------------------------------------

    fn parse_indentation_block(
        &mut self,
    ) -> Result<Vec<Statement>, ParseError> {
        if !self.consume(&Token::Indent) {
            return self.error("Expected indentation block");
        }

        let mut statements = Vec::new();

        while self.current() != &Token::Dedent
            && self.current() != &Token::Eof
        {
            if self.current() == &Token::NewLine {
                self.advance();
                continue;
            }

            match self.parse_statement()? {
                Some(statement) => statements.push(statement),

                None => {
                    return self.error(format!(
                        "Invalid statement in indentation block near token: {:?}",
                        self.current()
                    ));
                }
            }
        }

        if self.current() == &Token::Dedent {
            self.advance();
        }

        Ok(statements)
    }

    fn parse_brace_block(
        &mut self,
    ) -> Result<Vec<Statement>, ParseError> {
        if !self.consume(&Token::LeftBrace) {
            return self.error("Expected '{' at beginning of block");
        }

        let mut statements = Vec::new();

        while self.current() != &Token::RightBrace
            && self.current() != &Token::Eof
        {
            if self.current() == &Token::NewLine {
                self.advance();
                continue;
            }

            match self.parse_statement()? {
                Some(statement) => statements.push(statement),

                None => {
                    return self.error(format!(
                        "Invalid statement in brace block near token: {:?}",
                        self.current()
                    ));
                }
            }
        }

        if !self.consume(&Token::RightBrace) {
            return self.error("Expected '}' at end of block");
        }

        Ok(statements)
    }

    // ------------------------------------------------------------
    // Function
    // ------------------------------------------------------------

    fn parse_function(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;
        self.advance();

        let name = match self.current() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                name
            }

            _ => {
                return self.error("Expected function name after 'fn'");
            }
        };

        if !self.consume(&Token::LeftParen) {
            return self.error("Expected '(' after function name");
        }

        let mut parameters = Vec::new();

        while self.current() != &Token::RightParen
            && self.current() != &Token::Eof
        {
            let parameter_start = self.current_span().start;
            let name_span = self.current_span();

            let parameter_name = match self.current() {
                Token::Identifier(name) => {
                    let name = name.clone();
                    self.advance();
                    name
                }

                _ => {
                    return self.error("Expected parameter name");
                }
            };

            let type_name = if self.consume(&Token::Colon) {
                Some(self.parse_type()?)
            } else {
                None
            };

            parameters.push(Parameter {
                name: parameter_name,
                name_span,
                type_name,
                span: self.span_from(parameter_start),
            });

            if self.consume(&Token::Comma) {
                if self.current() == &Token::RightParen {
                    break;
                }
            } else if self.current() != &Token::RightParen {
                return self.error(
                    "Expected ',' or ')' after parameter",
                );
            }
        }

        if !self.consume(&Token::RightParen) {
            return self.error(
                "Expected ')' after function parameters",
            );
        }

        let return_type = if self.consume(&Token::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };

        let style = match self.current() {
            Token::Colon => {
                self.advance();
                BlockStyle::Indentation
            }

            Token::LeftBrace => BlockStyle::Braces,

            _ => {
                return self.error(
                    "Expected ':' or '{' to begin function body",
                );
            }
        };

        self.use_block_style(style)?;

        let previous_return_type = self.current_function_return_type.clone();
self.current_function_return_type = Some(return_type.clone());

let body_result = match style {
    BlockStyle::Indentation => {
        self.skip_newlines();
        self.parse_indentation_block()
    }

    BlockStyle::Braces => self.parse_brace_block(),

    BlockStyle::Unknown => unreachable!(),
};

self.current_function_return_type = previous_return_type;

let body = body_result?;

        Ok(Statement::Function {
            name,
            generic_parameters: Vec::new(),
            parameters,
            return_type,
            body,
            span: self.span_from(start),
        })
    }

    // ------------------------------------------------------------
    // Trait
    // ------------------------------------------------------------

    fn parse_trait(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;
        self.advance();

        let name = match self.current() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                name
            }

            _ => {
                return self.error("Expected trait name after 'trait'");
            }
        };

        let style = match self.current() {
            Token::Colon => {
                self.advance();
                BlockStyle::Indentation
            }

            Token::LeftBrace => BlockStyle::Braces,

            _ => {
                return self.error(
                    "Expected ':' or '{' after trait name",
                );
            }
        };

        self.use_block_style(style)?;

        let methods = match style {
            BlockStyle::Indentation => {
                self.skip_newlines();

                if !self.consume(&Token::Indent) {
                    return self.error(
                        "Expected indentation after trait declaration",
                    );
                }

                let mut methods = Vec::new();

                while self.current() != &Token::Dedent
                    && self.current() != &Token::Eof
                {
                    if self.current() == &Token::NewLine {
                        self.advance();
                        continue;
                    }

                    methods.push(self.parse_trait_method()?);
                    self.skip_newlines();
                }

                if self.current() == &Token::Dedent {
                    self.advance();
                }

                methods
            }

            BlockStyle::Braces => {
                if !self.consume(&Token::LeftBrace) {
                    return self.error(
                        "Expected '{' after trait name",
                    );
                }

                let mut methods = Vec::new();

                while self.current() != &Token::RightBrace
                    && self.current() != &Token::Eof
                {
                    if self.current() == &Token::NewLine {
                        self.advance();
                        continue;
                    }

                    methods.push(self.parse_trait_method()?);
                    self.skip_newlines();
                }

                if !self.consume(&Token::RightBrace) {
                    return self.error(
                        "Expected '}' after trait declaration",
                    );
                }

                methods
            }

            BlockStyle::Unknown => unreachable!(),
        };

        Ok(Statement::Trait {
            name,
            methods,
            span: self.span_from(start),
        })
    }

    fn parse_trait_method(&mut self) -> Result<TraitMethod, ParseError> {
        let start = self.current_span().start;

        if !self.consume(&Token::Fn) {
            return self.error(
                "Expected 'fn' in trait declaration",
            );
        }

        let name = match self.current() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                name
            }

            _ => {
                return self.error("Expected trait method name");
            }
        };

        if !self.consume(&Token::LeftParen) {
            return self.error(
                "Expected '(' after trait method name",
            );
        }

        let mut parameters = Vec::new();

        while self.current() != &Token::RightParen
            && self.current() != &Token::Eof
        {
            let parameter_start = self.current_span().start;
            let name_span = self.current_span();

            let parameter_name = match self.current() {
                Token::Identifier(name) => {
                    let name = name.clone();
                    self.advance();
                    name
                }

                _ => {
                    return self.error("Expected parameter name");
                }
            };

            let type_name = if self.consume(&Token::Colon) {
                Some(self.parse_type()?)
            } else {
                None
            };

            parameters.push(Parameter {
                name: parameter_name,
                name_span,
                type_name,
                span: self.span_from(parameter_start),
            });

            if self.consume(&Token::Comma) {
                if self.current() == &Token::RightParen {
                    break;
                }
            } else if self.current() != &Token::RightParen {
                return self.error(
                    "Expected ',' or ')' after parameter",
                );
            }
        }

        if !self.consume(&Token::RightParen) {
            return self.error(
                "Expected ')' after trait method parameters",
            );
        }

        let return_type = if self.consume(&Token::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };

        Ok(TraitMethod {
            name,
            parameters,
            return_type,
            span: self.span_from(start),
        })
    }

    // ------------------------------------------------------------
    // impl
    // ------------------------------------------------------------

    fn parse_impl(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;
        self.advance();

        let first_name = match self.current() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                name
            }

            _ => {
                return self.error("Expected type or trait name after 'impl'");
            }
        };

        let (trait_name, type_name) = if self.consume(&Token::For) {
            let type_name = match self.current() {
                Token::Identifier(name) => {
                    let name = name.clone();
                    self.advance();
                    name
                }

                _ => {
                    return self.error("Expected type name after 'for'");
                }
            };

            (Some(first_name), type_name)
        } else {
            (None, first_name)
        };

        let style = match self.current() {
            Token::Colon => {
                self.advance();
                BlockStyle::Indentation
            }

            Token::LeftBrace => BlockStyle::Braces,

            _ => {
                return self.error(
                    "Expected ':' or '{' after impl declaration",
                );
            }
        };

        self.use_block_style(style)?;

        let methods = match style {
            BlockStyle::Indentation => {
                self.skip_newlines();
                self.parse_indentation_block()?
            }

            BlockStyle::Braces => self.parse_brace_block()?,

            BlockStyle::Unknown => unreachable!(),
        };

        Ok(Statement::Impl {
            trait_name,
            type_name,
            methods,
            span: self.span_from(start),
        })
    }

    // ------------------------------------------------------------
    // Arguments
    // ------------------------------------------------------------

    fn parse_arguments(&mut self) -> Result<Vec<Expression>, ParseError> {
    if !self.consume(&Token::LeftParen) {
        return self.error("Expected '('");
    }

    self.skip_newlines();

    let mut arguments = Vec::new();

    if self.current() == &Token::RightParen {
        self.advance();
        return Ok(arguments);
    }

    loop {
        self.skip_newlines();

        arguments.push(self.parse_expression()?);

        self.skip_newlines();

        if self.consume(&Token::Comma) {
            self.skip_newlines();

            if self.current() == &Token::RightParen {
                break;
            }
        } else {
            break;
        }
    }

    if !self.consume(&Token::RightParen) {
        return self.error("Expected ')' after arguments");
    }

    Ok(arguments)
}

    // ------------------------------------------------------------
    // Struct constructor
    // ------------------------------------------------------------

    fn parse_struct_fields(
    &mut self,
) -> Result<Vec<(String, Expression)>, ParseError> {
    let mut fields = Vec::new();

    loop {
        if self.current() == &Token::RightBrace {
            break;
        }

        let name = match self.current() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                name
            }

            _ => {
                return self.error(
                    "Expected field name in struct constructor",
                );
            }
        };

        if !self.consume(&Token::Colon) {
            return self.error(
                "Expected ':' after struct constructor field name",
            );
        }

        let value = self.parse_expression()?;

        fields.push((name, value));

        if self.consume(&Token::Comma) {
            continue;
        }

        break;
    }

    if !self.consume(&Token::RightBrace) {
        return self.error(
            "Expected '}' after struct constructor fields",
        );
    }

    Ok(fields)
}

    // ------------------------------------------------------------
    // Expressions
    // ------------------------------------------------------------

    fn parse_expression(&mut self) -> Result<Expression, ParseError> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expression, ParseError> {
        let mut left = self.parse_and()?;

        loop {
            let operator = match self.current() {
                Token::OrOr | Token::Or => Operator::Or,
                _ => break,
            };

            self.advance();

            let right = self.parse_and()?;
            let span = left.span().merge(right.span());

            left = Expression::Binary {
                left: Box::new(left),
                operator,
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expression, ParseError> {
        let mut left = self.parse_comparison()?;

        loop {
            let operator = match self.current() {
                Token::AndAnd | Token::And => Operator::And,
                _ => break,
            };

            self.advance();

            let right = self.parse_comparison()?;
            let span = left.span().merge(right.span());

            left = Expression::Binary {
                left: Box::new(left),
                operator,
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn parse_comparison(
        &mut self,
    ) -> Result<Expression, ParseError> {
        let mut left = self.parse_addition()?;

        loop {
            let operator = match self.current() {
                Token::EqualEqual => Operator::Equal,
                Token::NotEqual => Operator::NotEqual,
                Token::Less => Operator::Less,
                Token::LessEqual => Operator::LessEqual,
                Token::Greater => Operator::Greater,
                Token::GreaterEqual => Operator::GreaterEqual,
                _ => break,
            };

            self.advance();

            let right = self.parse_addition()?;
            let span = left.span().merge(right.span());

            left = Expression::Binary {
                left: Box::new(left),
                operator,
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn parse_addition(&mut self) -> Result<Expression, ParseError> {
        let mut left = self.parse_multiplication()?;

        loop {
            let operator = match self.current() {
                Token::Plus => Operator::Plus,
                Token::Minus => Operator::Minus,
                _ => break,
            };

            self.advance();

            let right = self.parse_multiplication()?;
            let span = left.span().merge(right.span());

            left = Expression::Binary {
                left: Box::new(left),
                operator,
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn parse_multiplication(
        &mut self,
    ) -> Result<Expression, ParseError> {
        let mut left = self.parse_unary()?;

        loop {
            let operator = match self.current() {
                Token::Star => Operator::Multiply,
                Token::Slash => Operator::Divide,
                _ => break,
            };

            self.advance();

            let right = self.parse_unary()?;
            let span = left.span().merge(right.span());

            left = Expression::Binary {
                left: Box::new(left),
                operator,
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expression, ParseError> {
        match self.current() {
            Token::Minus => {
                let start = self.current_span().start;
                self.advance();

                let expression = self.parse_unary()?;
                let end = expression.span().end;

                Ok(Expression::Unary {
                    operator: UnaryOperator::Negate,
                    expression: Box::new(expression),
                    span: Span::new(start, end),
                })
            }

            Token::Bang | Token::Not => {
                let start = self.current_span().start;
                self.advance();

                let expression = self.parse_unary()?;
                let end = expression.span().end;

                Ok(Expression::Unary {
                    operator: UnaryOperator::Not,
                    expression: Box::new(expression),
                    span: Span::new(start, end),
                })
            }

            _ => self.parse_primary(),
        }
    }

    // ------------------------------------------------------------
    // Primary expressions
    // ------------------------------------------------------------

    fn parse_primary(
        &mut self,
    ) -> Result<Expression, ParseError> {
        let mut expression = match self.current() {
            Token::Identifier(name) => {
    let start = self.current_span().start;
    let name = name.clone();
    self.advance();

    if self.consume(&Token::DoubleColon) {
        let variant = match self.current() {
            Token::Identifier(variant) => {
                let variant = variant.clone();
                self.advance();
                variant
            }

            _ => {
                return self.error(
                    "Expected enum variant after '::'",
                );
            }
        };

        let arguments = if self.current() == &Token::LeftParen {
            self.parse_arguments()?
        } else {
            Vec::new()
        };

        let end = self.previous_span().end;

        Expression::EnumConstructor {
            enum_name: name,
            variant,
            arguments,
            span: Span::new(start, end),
        }
    } else if self.current() == &Token::LeftParen {
        let arguments = self.parse_arguments()?;
        let end = self.previous_span().end;

        Expression::Call {
            name,
            arguments,
            generic_arguments: Vec::new(),
            span: Span::new(start, end),
        }
    } else {
        Expression::Identifier {
            name,
            span: Span::new(start, self.previous_span().end),
        }
    }
}

            Token::Num(value) => {
                let span = self.current_span();
                let value = *value;
                self.advance();

                Expression::Number { value, span }
            }

            Token::Float(value) => {
                let span = self.current_span();
                let value = *value;
                self.advance();

                Expression::Float { value, span }
            }

            Token::Boolean(value) => {
                let span = self.current_span();
                let value = *value;
                self.advance();

                Expression::Boolean { value, span }
            }

            Token::String(value) => {
                let span = self.current_span();
                let value = value.clone();
                self.advance();

                Expression::String { value, span }
            }

            Token::LeftParen => {
                self.advance();

                let expression = self.parse_expression()?;

                if !self.consume(&Token::RightParen) {
                    return self.error(
                        "Expected ')' after expression",
                    );
                }

                expression
            }

            Token::LeftBracket => {
                let start = self.current_span().start;
                self.advance();

                let mut elements = Vec::new();

                while self.current() != &Token::RightBracket
                    && self.current() != &Token::Eof
                {
                    elements.push(self.parse_expression()?);

                    if self.consume(&Token::Comma) {
                        if self.current() == &Token::RightBracket {
                            break;
                        }
                    } else if self.current() != &Token::RightBracket {
                        return self.error(
                            "Expected ',' or ']' in array",
                        );
                    }
                }

                if !self.consume(&Token::RightBracket) {
                    return self.error(
                        "Expected ']' after array",
                    );
                }

                Expression::Array {
                    elements,
                    span: Span::new(
                        start,
                        self.previous_span().end,
                    ),
                }
            }

            _ => {
                return self.error(format!(
                    "Expected expression, found {:?}",
                    self.current()
                ));
            }
        };

        // --------------------------------------------------------
        // Postfix expressions
        // --------------------------------------------------------

        loop {
            match self.current() {
                Token::Dot => {
                    self.advance();

                    let name = match self.current() {
                        Token::Identifier(name) => {
                            let name = name.clone();
                            self.advance();
                            name
                        }

                        _ => {
                            return self.error(
                                "Expected identifier after '.'",
                            );
                        }
                    };

                    let start = expression.span().start;

                    if self.current() == &Token::LeftParen {
                        let arguments = self.parse_arguments()?;
                        let end = self.previous_span().end;

                        expression = Expression::MethodCall {
                            object: Box::new(expression),
                            method: name,
                            arguments,
                            span: Span::new(start, end),
                        };
                    } else {
                        let end = self.previous_span().end;

                        expression = Expression::Property {
                            object: Box::new(expression),
                            name,
                            span: Span::new(start, end),
                        };
                    }
                }

                Token::LeftBracket => {
                    let start = expression.span().start;
                    self.advance();

                    let index = self.parse_expression()?;

                    if !self.consume(&Token::RightBracket) {
                        return self.error(
                            "Expected ']' after index",
                        );
                    }

                    let end = self.previous_span().end;

                    expression = Expression::Index {
                        array: Box::new(expression),
                        index: Box::new(index),
                        span: Span::new(start, end),
                    };
                }

                _ => break,
            }
        }

        Ok(expression)
    }

    // ------------------------------------------------------------
    // Public parser entry point
    // ------------------------------------------------------------

    pub fn parse(&mut self) -> Result<Program, ParseError> {
        let program_start = self
            .tokens
            .first()
            .map(|token| token.span.start)
            .unwrap_or(0);

        let mut statements = Vec::new();

        self.skip_newlines();

        while self.current() != &Token::Eof {
            match self.parse_statement()? {
                Some(statement) => {
                    statements.push(statement);
                }

                None => {
                    return self.error(format!(
                        "Unexpected token at top level: {:?}",
                        self.current()
                    ));
                }
            }

            self.skip_newlines();
        }

        if !self.seen_main {
            return self.error(
                "Program must contain a 'main' declaration",
            );
        }

        let program_end = self
            .tokens
            .last()
            .map(|token| token.span.end)
            .unwrap_or(program_start);

        Ok(Program {
            statements,
            span: Span::new(program_start, program_end),
        })
    }
}
