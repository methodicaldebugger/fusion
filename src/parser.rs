//contents of parser.rs

use crate::ast::*;
use crate::errors::ParseError;
use crate::lexer::Token;
use crate::span::{Span, Spanned};

#[derive(Debug, Clone, Copy, PartialEq)]
enum BlockStyle {
    Unknown,
    Indentation,
    Braces,
}

pub struct Parser {
    tokens: Vec<Spanned<Token>>,
    position: usize,
    allow_struct_constructor: bool,

    block_style: BlockStyle,
    seen_main: bool,
    pending_block_styles: Vec<BlockStyle>,
}

impl Parser {
    pub fn new(tokens: Vec<Spanned<Token>>) -> Self {
        Self {
            tokens,
            position: 0,
            allow_struct_constructor: true,
            block_style: BlockStyle::Unknown,
            seen_main: false,
            pending_block_styles: Vec::new(),
        }
    }

    fn error<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError::new(message, self.current_span()))
    }

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

    fn peek_at(&self, position: usize) -> Option<&Token> {
        self.tokens.get(position).map(|token| &token.node)
    }

    fn advance(&mut self) {
        if self.position + 1 < self.tokens.len() {
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

    fn skip_newlines(&mut self) {
        while self.current() == &Token::NewLine {
            self.advance();
        }
    }

    // ------------------------------------------------------------
    // Block style
    // ------------------------------------------------------------

    fn use_block_style(&mut self, style: BlockStyle) -> Result<(), ParseError> {
        match self.block_style {
            BlockStyle::Unknown => {
                self.pending_block_styles.push(style);
            }

            existing if existing != style => {
                return Err(ParseError::new(
                    format!(
                        "Block style mismatch: program uses {:?}, but this construct uses {:?}",
                        existing, style
                    ),
                    self.current_span(),
                ));
            }

            _ => {}
        }

        Ok(())
    }

    fn establish_main_style(
        &mut self,
        style: BlockStyle,
    ) -> Result<(), ParseError> {
        if self.seen_main {
            return self.error("Multiple 'main' declarations are not allowed");
        }

        for previous in &self.pending_block_styles {
            if *previous != style {
                return Err(ParseError::new(
                    format!(
                        "Block style mismatch: main uses {:?}, but an earlier construct uses {:?}",
                        style, previous
                    ),
                    self.current_span(),
                ));
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

            Token::Main => {
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

                Ok(Some(Statement::Main { body }))
            }

            Token::Fn => Ok(Some(self.parse_function()?)),

            Token::Defer => {
                self.advance();

                let expression = self.parse_expression()?;

                Ok(Some(Statement::Defer(expression)))
            }

            Token::Struct => Ok(Some(self.parse_struct()?)),

            Token::Enum => Ok(Some(self.parse_enum()?)),

            Token::Match => Ok(Some(self.parse_match()?)),

            Token::While => {
                self.advance();

                let previous = self.allow_struct_constructor;
                self.allow_struct_constructor = false;

                let condition_result = self.parse_expression();

                self.allow_struct_constructor = previous;

                let condition = condition_result?;

                let body = self.parse_style_block()?;

                Ok(Some(Statement::While {
                    condition,
                    body,
                }))
            }

            Token::If => {
                self.advance();

                let previous = self.allow_struct_constructor;
                self.allow_struct_constructor = false;

                let condition_result = self.parse_expression();

                self.allow_struct_constructor = previous;

                let condition = condition_result?;

                let body = self.parse_style_block()?;

                let else_body = if self.current() == &Token::Else {
                    self.advance();
                    Some(self.parse_style_block()?)
                } else {
                    None
                };

                Ok(Some(Statement::If {
                    condition,
                    body,
                    else_body,
                }))
            }

            Token::For => Ok(Some(self.parse_for()?)),

            Token::Break => {
                self.advance();
                Ok(Some(Statement::Break))
            }

            Token::Continue => {
                self.advance();
                Ok(Some(Statement::Continue))
            }

            Token::Return => {
                self.advance();

                let value = self.parse_expression()?;

                Ok(Some(Statement::Return(value)))
            }

            Token::Const => {
                let statement_start = self.current_span().start;

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

                Ok(Some(Statement::ConstDeclaration {
                    name,
                    name_span,
                    declared_type,
                    value,
                    span: Span::new(
                        statement_start,
                        self.current_span().end,
                    ),
                }))
            }

            Token::Identifier(first_name) => {
                let first_name = first_name.clone();

                /*
                 * Typed declaration:
                 *
                 * num x = 10
                 * string name = "Bob"
                 * Person p = Person { ... }
                 *
                 * Multiple declarations:
                 *
                 * num x, y = 1, 2
                 */
                if matches!(
                    self.peek_at(self.position + 1),
                    Some(Token::Identifier(_))
                ) {
                    let statement_start = self.current_span().start;

                    let mut lookahead = self.position + 1;
                    let mut names = Vec::new();
                    let mut name_spans = Vec::new();

                    if let Some(Token::Identifier(name)) =
                        self.peek_at(lookahead)
                    {
                        names.push(name.clone());

                        if let Some(token) = self.tokens.get(lookahead) {
                            name_spans.push(token.span);
                        }

                        lookahead += 1;

                        while matches!(
                            self.peek_at(lookahead),
                            Some(Token::Comma)
                        ) {
                            lookahead += 1;

                            match self.peek_at(lookahead) {
                                Some(Token::Identifier(name)) => {
                                    names.push(name.clone());

                                    if let Some(token) =
                                        self.tokens.get(lookahead)
                                    {
                                        name_spans.push(token.span);
                                    }

                                    lookahead += 1;
                                }

                                _ => {
                                    return self.error(
                                        "Expected identifier after ','",
                                    );
                                }
                            }
                        }

                        if matches!(
                            self.peek_at(lookahead),
                            Some(Token::Equal)
                        ) {
                            self.advance();

                            for i in 0..names.len() {
                                self.advance();

                                if i + 1 < names.len()
                                    && !self.consume(&Token::Comma)
                                {
                                    return self.error(
                                        "Expected ',' between variable names",
                                    );
                                }
                            }

                            if !self.consume(&Token::Equal) {
                                return self.error(
                                    "Expected '=' in variable declaration",
                                );
                            }

                            let mut values = Vec::new();

                            loop {
                                values.push(self.parse_expression()?);

                                if self.current() == &Token::Comma {
                                    self.advance();
                                } else {
                                    break;
                                }
                            }

                            if names.len() != values.len() {
                                return self.error(
                                    "Number of variables does not match number of values",
                                );
                            }

                            let declarations = names
                                .into_iter()
                                .zip(name_spans.into_iter())
                                .zip(values.into_iter())
                                .map(|((name, name_span), value)| {
                                    VariableDeclaration {
                                        name,
                                        name_span,
                                        declared_type: Some(
                                            first_name.clone(),
                                        ),
                                        value,
                                        span: Span::new(
                                            statement_start,
                                            self.current_span().end,
                                        ),
                                    }
                                })
                                .collect();

                            return Ok(Some(
                                Statement::VariableDeclarations {
                                    declarations,
                                    span: Span::new(
                                        statement_start,
                                        self.current_span().end,
                                    ),
                                },
                            ));
                        }
                    }
                }

                let expression = self.parse_expression()?;

                if self.current() == &Token::Equal {
                    self.advance();

                    let value = self.parse_expression()?;

                    Ok(Some(Statement::Assignment {
                        target: expression,
                        value,
                    }))
                } else {
                    Ok(Some(Statement::Expression(expression)))
                }
            }

            _ => Ok(None),
        }
    }

    // ------------------------------------------------------------
    // for
    // ------------------------------------------------------------

    fn parse_for(&mut self) -> Result<Statement, ParseError> {
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

        let start = self.parse_expression()?;

        if !self.consume(&Token::DotDot) {
            return self.error("Expected '..' in for loop");
        }

        let end = self.parse_expression()?;

        let body = self.parse_style_block()?;

        Ok(Statement::For {
            variable,
            start,
            end,
            body,
        })
    }

    // ------------------------------------------------------------
    // struct
    // ------------------------------------------------------------

    fn parse_struct(&mut self) -> Result<Statement, ParseError> {
        self.advance();

        let name = match self.current() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                name
            }

            _ => {
                return self.error("Expected identifier after 'struct'");
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

            BlockStyle::Braces => self.parse_brace_struct_fields()?,

            BlockStyle::Unknown => unreachable!(),
        };

        Ok(Statement::Struct { name, fields })
    }

    fn parse_indentation_struct_fields(
        &mut self,
    ) -> Result<Vec<StructField>, ParseError> {
        let mut fields = Vec::new();

        if !self.consume(&Token::Indent) {
            return self.error(
                "Expected indentation after struct declaration",
            );
        }

        while self.current() != &Token::Dedent
            && self.current() != &Token::Eof
        {
            if self.current() == &Token::NewLine {
                self.advance();
                continue;
            }

            let field_name_span = self.current_span();

            let field_name = match self.current() {
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

            fields.push(StructField {
                name: field_name,
                name_span: field_name_span,
                type_name,
            });

            if self.current() == &Token::Comma {
                self.advance();
            }

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
            return self.error(
                "Expected '{' after struct name",
            );
        }

        let mut fields = Vec::new();

        while self.current() != &Token::RightBrace
            && self.current() != &Token::Eof
        {
            if self.current() == &Token::NewLine {
                self.advance();
                continue;
            }

            let field_name_span = self.current_span();

            let field_name = match self.current() {
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

            fields.push(StructField {
                name: field_name,
                name_span: field_name_span,
                type_name,
            });

            if self.current() == &Token::Comma {
                self.advance();
            }
        }

        if !self.consume(&Token::RightBrace) {
            return self.error(
                "Expected '}' after struct fields",
            );
        }

        Ok(fields)
    }

    // ------------------------------------------------------------
    // enum
    // ------------------------------------------------------------

    fn parse_enum(&mut self) -> Result<Statement, ParseError> {
        self.advance();

        let name = match self.current() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                name
            }

            _ => {
                return self.error(
                    "Expected identifier after 'enum'",
                );
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
                    "Expected ':' or '{' after enum name",
                );
            }
        };

        self.use_block_style(style)?;

        let variants = match style {
            BlockStyle::Indentation => {
                self.skip_newlines();
                self.parse_indentation_enum_variants()?
            }

            BlockStyle::Braces => {
                self.parse_brace_enum_variants()?
            }

            BlockStyle::Unknown => unreachable!(),
        };

        Ok(Statement::Enum {
            name,
            variants,
        })
    }

    fn parse_brace_enum_variants(
        &mut self,
    ) -> Result<Vec<EnumVariant>, ParseError> {
        if !self.consume(&Token::LeftBrace) {
            return self.error(
                "Expected '{' after enum name",
            );
        }

        let mut variants = Vec::new();

        while self.current() != &Token::RightBrace
            && self.current() != &Token::Eof
        {
            if self.current() == &Token::NewLine {
                self.advance();
                continue;
            }

            let variant_name_span = self.current_span();

            let variant_name = match self.current() {
                Token::Identifier(name) => {
                    let name = name.clone();
                    self.advance();
                    name
                }

                _ => {
                    return self.error(
                        "Expected enum variant name",
                    );
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
                    return self.error(
                        "Expected ')' after enum variant fields",
                    );
                }
            }

            variants.push(EnumVariant {
                name: variant_name,
                name_span: variant_name_span,
                fields,
            });

            if self.current() == &Token::Comma {
                self.advance();
            }

            self.skip_newlines();
        }

        if !self.consume(&Token::RightBrace) {
            return self.error(
                "Expected '}' after enum variants",
            );
        }

        Ok(variants)
    }

    fn parse_indentation_enum_variants(
        &mut self,
    ) -> Result<Vec<EnumVariant>, ParseError> {
        let mut variants = Vec::new();

        self.skip_newlines();

        if !self.consume(&Token::Indent) {
            return self.error(
                "Expected indentation after enum declaration",
            );
        }

        while self.current() != &Token::Dedent
            && self.current() != &Token::Eof
        {
            if self.current() == &Token::NewLine {
                self.advance();
                continue;
            }

            let variant_name_span = self.current_span();

            let variant_name = match self.current() {
                Token::Identifier(name) => {
                    let name = name.clone();
                    self.advance();
                    name
                }

                _ => {
                    return self.error(
                        "Expected enum variant name",
                    );
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
                    return self.error(
                        "Expected ')' after enum variant fields",
                    );
                }
            }

            variants.push(EnumVariant {
                name: variant_name,
                name_span: variant_name_span,
                fields,
            });

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
        match self.current() {
            Token::Identifier(name) => {
                let first = name.clone();
                self.advance();

                if first == "_" {
                    return Ok(Pattern::Wildcard);
                }

                if self.consume(&Token::DoubleColon) {
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

                                    continue;
                                }

                                break;
                            }
                        }

                        if !self.consume(&Token::RightParen) {
                            return self.error(
                                "Expected ')' after pattern bindings",
                            );
                        }
                    }

                    return Ok(Pattern::Variant {
                        name: format!("{}::{}", first, variant),
                        bindings,
                    });
                }

                Ok(Pattern::Identifier(first))
            }

            Token::Num(value) => {
                let value = *value;
                self.advance();
                Ok(Pattern::Number(value))
            }

            Token::Float(value) => {
                let value = *value;
                self.advance();
                Ok(Pattern::Float(value))
            }

            Token::String(value) => {
                let value = value.clone();
                self.advance();
                Ok(Pattern::String(value))
            }

            Token::Boolean(value) => {
                let value = value.clone();
                self.advance();
                Ok(Pattern::Boolean(value))
            }

            _ => self.error("Expected match pattern"),
        }
    }

    fn parse_match(&mut self) -> Result<Statement, ParseError> {
        self.advance();

        let previous = self.allow_struct_constructor;
        self.allow_struct_constructor = false;

        let expression_result = self.parse_expression();

        self.allow_struct_constructor = previous;

        let expression = expression_result?;

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
            BlockStyle::Indentation => {
                self.parse_indentation_match_arms()?
            }

            BlockStyle::Braces => {
                self.parse_brace_match_arms()?
            }

            BlockStyle::Unknown => unreachable!(),
        };

        Ok(Statement::Match {
            expression,
            arms,
        })
    }

    fn parse_indentation_match_arms(
        &mut self,
    ) -> Result<Vec<MatchArm>, ParseError> {
        let mut arms = Vec::new();

        self.skip_newlines();

        if !self.consume(&Token::Indent) {
            return self.error(
                "Expected indentation after match",
            );
        }

        while self.current() != &Token::Dedent
            && self.current() != &Token::Eof
        {
            if self.current() == &Token::NewLine {
                self.advance();
                continue;
            }

            let pattern = self.parse_pattern()?;

            if !self.consume(&Token::FatArrow) {
                return self.error(
                    "Expected '=>' after match pattern",
                );
            }

            if self.current() == &Token::Colon {
                return self.error(
                    "':' is not allowed after '=>' in indentation mode",
                );
            }

            let body = if self.current() == &Token::NewLine {
                self.advance();
                self.skip_newlines();

                self.parse_indentation_block()?
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

            arms.push(MatchArm {
                pattern,
                body,
            });

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
            return self.error(
                "Expected '{' after match expression",
            );
        }

        let mut arms = Vec::new();

        while self.current() != &Token::RightBrace
            && self.current() != &Token::Eof
        {
            if self.current() == &Token::NewLine {
                self.advance();
                continue;
            }

            let pattern = self.parse_pattern()?;

            if !self.consume(&Token::FatArrow) {
                return self.error(
                    "Expected '=>' after match pattern",
                );
            }

            let body = if self.current() == &Token::LeftBrace {
                if self.block_style != BlockStyle::Braces {
                    return self.error(format!(
                        "Block style mismatch: program uses {:?}, but match arm uses braces",
                        self.block_style
                    ));
                }

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

            arms.push(MatchArm {
                pattern,
                body,
            });

            self.skip_newlines();
        }

        if !self.consume(&Token::RightBrace) {
            return self.error(
                "Expected '}' after match arms",
            );
        }

        Ok(arms)
    }

    // ------------------------------------------------------------
    // Blocks
    // ------------------------------------------------------------

    fn parse_indentation_block(
        &mut self,
    ) -> Result<Vec<Statement>, ParseError> {
        let mut statements = Vec::new();

        if !self.consume(&Token::Indent) {
            return self.error("Expected indentation block");
        }

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
        let mut statements = Vec::new();

        if !self.consume(&Token::LeftBrace) {
            return self.error(
                "Expected '{' at beginning of block",
            );
        }

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
            return self.error(
                "Expected '}' at end of block",
            );
        }

        Ok(statements)
    }

    // ------------------------------------------------------------
    // Function
    // ------------------------------------------------------------

    fn parse_function(&mut self) -> Result<Statement, ParseError> {
        self.advance();

        let name = match self.current() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                name
            }

            _ => {
                return self.error(
                    "Expected function name after 'fn'",
                );
            }
        };

        if !self.consume(&Token::LeftParen) {
            return self.error(
                "Expected '(' after function name",
            );
        }

        let mut parameters = Vec::new();

        while self.current() != &Token::RightParen
            && self.current() != &Token::Eof
        {
            let name_span = self.current_span();

            let name = match self.current() {
                Token::Identifier(name) => {
                    let name = name.clone();
                    self.advance();
                    name
                }

                _ => {
                    return self.error(
                        "Expected parameter name",
                    );
                }
            };

            let type_name = if self.consume(&Token::Colon) {
                Some(self.parse_type()?)
            } else {
                None
            };

            parameters.push(Parameter {
                name,
                name_span,
                type_name,
            });

            if self.current() == &Token::Comma {
                self.advance();

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

        let body = match style {
            BlockStyle::Indentation => {
                self.skip_newlines();
                self.parse_indentation_block()?
            }

            BlockStyle::Braces => self.parse_brace_block()?,

            BlockStyle::Unknown => unreachable!(),
        };

        Ok(Statement::Function {
            name,
            generic_parameters: Vec::new(),
            parameters,
            return_type,
            body,
        })
    }

    // ------------------------------------------------------------
    // Types
    // ------------------------------------------------------------

    fn parse_type(&mut self) -> Result<String, ParseError> {
        match self.current() {
            Token::NumType => {
                self.advance();
                Ok("num".into())
            }

            Token::FloatType => {
                self.advance();
                Ok("float".into())
            }

            Token::BoolType => {
                self.advance();
                Ok("bool".into())
            }

            Token::StringType => {
                self.advance();
                Ok("string".into())
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

            left = Expression::Binary {
                left: Box::new(left),
                operator,
                right: Box::new(right),
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

            left = Expression::Binary {
                left: Box::new(left),
                operator,
                right: Box::new(right),
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

            left = Expression::Binary {
                left: Box::new(left),
                operator,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_addition(
        &mut self,
    ) -> Result<Expression, ParseError> {
        let mut left = self.parse_multiplication()?;

        loop {
            let operator = match self.current() {
                Token::Plus => Operator::Plus,
                Token::Minus => Operator::Minus,
                _ => break,
            };

            self.advance();

            let right = self.parse_multiplication()?;

            left = Expression::Binary {
                left: Box::new(left),
                operator,
                right: Box::new(right),
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

            left = Expression::Binary {
                left: Box::new(left),
                operator,
                right: Box::new(right),
            };
        }

        Ok(left)
    }

    fn parse_unary(
        &mut self,
    ) -> Result<Expression, ParseError> {
        match self.current() {
            Token::Minus => {
                self.advance();

                let expression = self.parse_unary()?;

                Ok(Expression::Unary {
                    operator: UnaryOperator::Negate,
                    expression: Box::new(expression),
                })
            }

            Token::Bang | Token::Not => {
                self.advance();

                let expression = self.parse_unary()?;

                Ok(Expression::Unary {
                    operator: UnaryOperator::Not,
                    expression: Box::new(expression),
                })
            }

            _ => self.parse_primary(),
        }
    }

    // ------------------------------------------------------------
    // Arguments
    // ------------------------------------------------------------

    fn parse_arguments(
        &mut self,
    ) -> Result<Vec<Expression>, ParseError> {
        let mut arguments = Vec::new();

        if !self.consume(&Token::LeftParen) {
            return self.error("Expected '('");
        }

        if self.current() == &Token::RightParen {
            self.advance();
            return Ok(arguments);
        }

        loop {
            arguments.push(self.parse_expression()?);

            if self.consume(&Token::Comma) {
                if self.current() == &Token::RightParen {
                    break;
                }

                continue;
            }

            break;
        }

        if !self.consume(&Token::RightParen) {
            return self.error(
                "Expected ')' after arguments",
            );
        }

        Ok(arguments)
    }

    // ------------------------------------------------------------
    // Struct constructor fields
    // ------------------------------------------------------------

    fn parse_struct_fields(
        &mut self,
    ) -> Result<Vec<(String, Expression)>, ParseError> {
        if !self.consume(&Token::LeftBrace) {
            return self.error(
                "Expected '{' after struct name",
            );
        }

        let mut fields = Vec::new();

        while self.current() != &Token::RightBrace
            && self.current() != &Token::Eof
        {
            if self.current() == &Token::NewLine {
                self.advance();
                continue;
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

            if self.current() == &Token::Comma {
                self.advance();
            } else if self.current() != &Token::RightBrace {
                return self.error(
                    "Expected ',' or '}' after struct constructor field",
                );
            }
        }

        if !self.consume(&Token::RightBrace) {
            return self.error(
                "Expected '}' after struct constructor fields",
            );
        }

        Ok(fields)
    }

    // ------------------------------------------------------------
    // Primary expressions
    // ------------------------------------------------------------

    fn parse_primary(
        &mut self,
    ) -> Result<Expression, ParseError> {
        let mut expression = match self.current() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();

                if self.current() == &Token::DoubleColon {
                    self.advance();

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

                    Expression::EnumConstructor {
                        enum_name: name,
                        variant,
                        arguments,
                    }
                } else if self.current() == &Token::LeftParen {
                    let arguments = self.parse_arguments()?;

                    Expression::Call {
                        name,
                        arguments,
                        generic_arguments: Vec::new(),
                    }
                } else if self.current() == &Token::LeftBrace
                    && self.allow_struct_constructor
                {
                    let fields = self.parse_struct_fields()?;

                    Expression::StructConstructor {
                        name,
                        fields,
                    }
                } else {
                    Expression::Identifier(name)
                }
            }

            Token::Num(value) => {
                let value = *value;
                self.advance();
                Expression::Number(value)
            }

            Token::Float(value) => {
                let value = *value;
                self.advance();
                Expression::Float(value)
            }

            Token::String(value) => {
                let value = value.clone();
                self.advance();
                Expression::String(value)
            }

            Token::Boolean(value) => {
                let value = value.clone();
                self.advance();
                Expression::Boolean(value)
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
                self.advance();

                let mut values = Vec::new();

                while self.current() != &Token::RightBracket
                    && self.current() != &Token::Eof
                {
                    values.push(self.parse_expression()?);

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

                Expression::Array(values)
            }

            _ => {
                return self.error(
                    format!(
                        "Expected expression, found {:?}",
                        self.current()
                    ),
                );
            }
        };

        // Postfix expressions:
        //
        // object.property
        // object.method(...)
        // array[index]
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

                    if self.current() == &Token::LeftParen {
                        let arguments = self.parse_arguments()?;

                        expression = Expression::MethodCall {
                            object: Box::new(expression),
                            method: name,
                            arguments,
                        };
                    } else {
                        expression = Expression::Property {
                            object: Box::new(expression),
                            name,
                        };
                    }
                }

                Token::LeftBracket => {
                    self.advance();

                    let index = self.parse_expression()?;

                    if !self.consume(&Token::RightBracket) {
                        return self.error(
                            "Expected ']' after index",
                        );
                    }

                    expression = Expression::Index {
                        array: Box::new(expression),
                        index: Box::new(index),
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
        let mut statements = Vec::new();

        while self.current() != &Token::Eof {
            match self.parse_statement()? {
                Some(statement) => statements.push(statement),

                None => {
                    if self.current() == &Token::Eof {
                        break;
                    }

                    return self.error(format!(
                        "Unexpected token at top level: {:?}",
                        self.current()
                    ));
                }
            }
        }

        if !self.seen_main {
            return self.error(
                "Program must contain a 'main' declaration",
            );
        }

        Ok(Program {
            statements,
        })
    }
}