//contents of parser.rs

/*
Parser — understands the grammatical structure
The parser receives tokens from the lexer
 and checks whether they follow the language's grammar.

The parser determines how the tokens fit together:
which tokens form a variable declaration, which form an expression,
and which operations belong inside other operations.

Its most common output is an AST.
Think of it as: understanding how the words in a sentence
fit together according to grammar.

For example, this is valid:
let x = 10 + 20;
But this might be invalid:
let = + 10;
*/

use crate::ast::*;
use crate::errors::ParseError;
use crate::lexer::Token;
use crate::span::{Span, Spanned};
use crate::type_expr::TypeExpr;

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
    require_main: bool,
    pending_block_styles: Vec<BlockStyle>,
    current_function_return_type: Option<Option<String>>,
    function_depth: usize,
    loop_depth: usize,
    /// Additional declarations produced by a single `extern` block.
    pending_statements: Vec<Statement>,
    /// Type parameters of the struct, enum or impl being parsed. They are erased to `unknown`
    /// wherever they appear in a type annotation (see `TypeExpr::erase_generics`).
    erased_type_parameters: Vec<String>,
    /// Anonymous function bodies are hoisted, while closure expressions retain capture metadata.
    hoisted_functions: Vec<Statement>,
    lambda_counter: usize,
    /// Numbers the temporaries introduced by the `?` operator.
    try_counter: usize,
}

fn collect_expression_identifiers(expression: &Expression, output: &mut Vec<String>) {
    match expression {
        Expression::Identifier { name, .. } => output.push(name.clone()),
        Expression::Closure { captures, .. } => output.extend(captures.iter().cloned()),
        Expression::Array { elements, .. } | Expression::Tuple { elements, .. } => {
            for element in elements {
                collect_expression_identifiers(element, output);
            }
        }
        Expression::Index { array, index, .. } => {
            collect_expression_identifiers(array, output);
            collect_expression_identifiers(index, output);
        }
        Expression::Property { object, .. }
        | Expression::Await { expression: object, .. }
        | Expression::Unary { expression: object, .. }
        | Expression::Conversion { expression: object, .. } => {
            collect_expression_identifiers(object, output);
        }
        Expression::MethodCall { object, arguments, .. } => {
            collect_expression_identifiers(object, output);
            for argument in arguments {
                collect_expression_identifiers(argument, output);
            }
        }
        Expression::Call { name, arguments, .. } => {
            // A call target may be a local function-valued binding. Built-ins are resolved
            // independently and do not need to be part of the lexical capture environment.
            if !crate::stdlib_ext::is_builtin(name)
                && !matches!(name.as_str(), "print" | "input" | "open" | "log_debug" | "log_info" | "log_warn" | "log_error" | "Some" | "None" | "Ok" | "Err")
            {
                output.push(name.clone());
            }
            for argument in arguments {
                collect_expression_identifiers(argument, output);
            }
        }
        Expression::EnumConstructor { arguments, .. } => {
            for argument in arguments {
                collect_expression_identifiers(argument, output);
            }
        }
        Expression::StructConstructor { fields, .. } => {
            for (_, value) in fields {
                collect_expression_identifiers(value, output);
            }
        }
        Expression::Binary { left, right, .. } => {
            collect_expression_identifiers(left, output);
            collect_expression_identifiers(right, output);
        }
        Expression::Number { .. }
        | Expression::WideNumber { .. }
        | Expression::Float { .. }
        | Expression::Boolean { .. }
        | Expression::String { .. } => {}
    }
}

impl Parser {
    pub fn new(tokens: Vec<Spanned<Token>>) -> Self {
        Self {
            tokens,
            position: 0,
            block_style: BlockStyle::Unknown,
            pending_block_styles: Vec::new(),
            seen_main: false,
            require_main: true,
            current_function_return_type: None,
            function_depth: 0,
            loop_depth: 0,
            pending_statements: Vec::new(),
            erased_type_parameters: Vec::new(),
            hoisted_functions: Vec::new(),
            lambda_counter: 0,
            try_counter: 0,
        }
    }

    /// Construct a parser for an imported module, where a `main` declaration is optional.
    pub fn new_module(tokens: Vec<Spanned<Token>>) -> Self {
        let mut parser = Self::new(tokens);
        parser.require_main = false;
        parser
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
            Token::NewLine | Token::Dedent | Token::RightBrace | Token::Eof
        )
    }

    // ------------------------------------------------------------
    // Block style
    // ------------------------------------------------------------

    fn use_block_style(&mut self, style: BlockStyle) -> Result<(), ParseError> {
        match self.block_style {
            BlockStyle::Unknown => {
                self.pending_block_styles.push(style);
                Ok(())
            }

            existing if existing == style => Ok(()),

            existing => Err(ParseError {
                message: format!(
                    "Mixed block styles are not allowed: expected {:?}, found {:?}",
                    existing, style
                ),
                span: self.current_span(),
            }),
        }
    }

    fn establish_main_style(&mut self, style: BlockStyle) -> Result<(), ParseError> {
        if self.seen_main {
            return Err(ParseError {
                message: "Multiple main blocks are not allowed".into(),
                span: self.current_span(),
            });
        }

        for pending in &self.pending_block_styles {
            if *pending != style {
                return Err(ParseError {
                    message: "All blocks in a Fusion file must use the same block style".into(),
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

    fn parse_qualified_identifier(&mut self, expected: &str) -> Result<String, ParseError> {
        let mut name = match self.current() {
            Token::Identifier(name) => name.clone(),
            _ => return self.error(expected),
        };
        self.advance();
        while self.consume(&Token::Dot) {
            match self.current() {
                Token::Identifier(part) => {
                    name.push('.');
                    name.push_str(part);
                    self.advance();
                }
                _ => return self.error("Expected an identifier after '.'"),
            }
        }
        Ok(name)
    }

    /// Parse a type annotation and return its canonical source spelling.
    ///
    /// The AST still stores type annotations as strings (see `type_expr`), but they are always
    /// the `Display` form of a structurally parsed `TypeExpr`, never hand-assembled text.
    fn parse_type(&mut self) -> Result<String, ParseError> {
        let expr = self.parse_type_expr()?;
        Ok(expr.erase_generics(&self.erased_type_parameters).to_string())
    }

    /// Parse an optional `<T, U>` list of type parameters after a struct, enum or impl type
    /// name. Returns an empty list when there is none.
    ///
    /// Generic declarations are *erased*: inside the declaration each parameter is spelled
    /// `unknown`, and type arguments on user types (`Stack<num>`) are dropped wherever they are
    /// written. The arguments are accepted for readability and documentation but are not
    /// checked; see the README section "Generic structs and enums".
    fn parse_type_parameter_list(&mut self) -> Result<Vec<String>, ParseError> {
        let mut parameters: Vec<String> = Vec::new();
        if !self.consume(&Token::Less) {
            return Ok(parameters);
        }
        loop {
            match self.current() {
                Token::Identifier(name) => {
                    if parameters.iter().any(|existing| existing == name) {
                        return self.error("Duplicate type parameter");
                    }
                    parameters.push(name.clone());
                    self.advance();
                }
                _ => return self.error("Expected a type parameter name"),
            }
            if self.consume(&Token::Comma) {
                continue;
            }
            break;
        }
        if !self.consume(&Token::Greater) {
            return self.error("Expected '>' after type parameters");
        }
        Ok(parameters)
    }

    /// `type := primary ("[" "]")*`; see `type_expr` for the full grammar.
    fn parse_type_expr(&mut self) -> Result<TypeExpr, ParseError> {
        let mut base = self.parse_type_primary()?;
        while self.current() == &Token::LeftBracket {
            self.advance();
            if !self.consume(&Token::RightBracket) {
                return self.error("Expected ']' after array type");
            }
            base = TypeExpr::Array(Box::new(base));
        }
        Ok(base)
    }

    /// Parse `type, type, ... [,]` up to and including `close`; the opener is already consumed.
    fn parse_type_list(
        &mut self,
        close: &Token,
        what: &str,
    ) -> Result<Vec<TypeExpr>, ParseError> {
        let mut items = Vec::new();
        if self.current() != close {
            loop {
                items.push(self.parse_type_expr()?);
                if !self.consume(&Token::Comma) {
                    break;
                }
                if self.current() == close {
                    break;
                }
            }
        }
        if !self.consume(close) {
            return self.error(format!("Expected closing delimiter after {}", what));
        }
        Ok(items)
    }

    fn parse_type_primary(&mut self) -> Result<TypeExpr, ParseError> {
        if self.consume(&Token::Star) {
            return match self.current() {
                Token::Identifier(name) if name == "opaque" => {
                    self.advance();
                    Ok(TypeExpr::OpaquePointer)
                }
                _ => self.error("Only '*opaque' pointer types are supported in foreign declarations"),
            };
        }

        if self.consume(&Token::LeftParen) {
            let items = self.parse_type_list(&Token::RightParen, "tuple type")?;
            return match TypeExpr::from_parenthesized(items) {
                Ok(expr) => Ok(expr),
                Err(message) => self.error(message),
            };
        }

        if self.consume(&Token::Fn) {
            if !self.consume(&Token::LeftParen) {
                return self.error("Expected '(' after 'fn' in function type");
            }
            let params = self.parse_type_list(&Token::RightParen, "function type parameters")?;
            let ret = if self.consume(&Token::Arrow) {
                self.parse_type_expr()?
            } else {
                TypeExpr::named("void")
            };
            return Ok(TypeExpr::Function {
                params,
                ret: Box::new(ret),
            });
        }

        let token = self.current().clone();
        let name = match token {
            Token::NumType => {
                self.advance();
                return Ok(TypeExpr::named("num"));
            }
            Token::FloatType => {
                self.advance();
                return Ok(TypeExpr::named("float"));
            }
            Token::BoolType => {
                self.advance();
                return Ok(TypeExpr::named("bool"));
            }
            Token::StringType => {
                self.advance();
                return Ok(TypeExpr::named("string"));
            }
            Token::Identifier(first) => {
                self.advance();
                let mut name = first;
                // Qualified user-defined types use the same dotted module path
                // spelling as qualified value/function access (e.g. geometry.Point).
                while self.consume(&Token::Dot) {
                    match self.current() {
                        Token::Identifier(part) => {
                            name.push('.');
                            name.push_str(part);
                            self.advance();
                        }
                        _ => return self.error("Expected a type name after '.'"),
                    }
                }
                name
            }
            _ => return self.error("Expected type"),
        };

        let mut args = Vec::new();
        if self.consume(&Token::Less) {
            loop {
                args.push(self.parse_type_expr()?);
                if self.consume(&Token::Comma) {
                    continue;
                }
                break;
            }
            if !self.consume(&Token::Greater) {
                return self.error("Expected '>' after generic type arguments");
            }
        }
        Ok(TypeExpr::Named { name, args })
    }

    // ------------------------------------------------------------
    // Top-level statements
    // ------------------------------------------------------------

    fn parse_statement(&mut self) -> Result<Option<Statement>, ParseError> {
        // Statements queued by an earlier desugaring (extern blocks, `?`) come first.
        if !self.pending_statements.is_empty() {
            return Ok(Some(self.pending_statements.remove(0)));
        }
        match self.parse_statement_inner()? {
            Some(statement) => Ok(Some(self.desugar_try_statement(statement))),
            None => Ok(None),
        }
    }

    fn is_try_call(expression: &Expression) -> bool {
        matches!(
            expression,
            Expression::Call { name, arguments, .. } if name == "__try" && arguments.len() == 1
        )
    }

    /// Desugar the `?` operator.
    ///
    /// `?` is parsed as a postfix operator producing the marker call `__try(expr)`. When that
    /// marker is the whole value of a declaration, assignment, `return` or expression statement,
    /// the statement is rewritten into ordinary statements:
    ///
    /// ```text
    /// value = parse(text)?          __try_0 = parse(text)
    ///                        ==>    if result_is_err(__try_0):
    ///                                   return Err(result_unwrap_err(__try_0))
    ///                               value = result_unwrap(__try_0)
    /// ```
    ///
    /// A `?` anywhere else is left as the marker call, which the type checker reports.
    fn desugar_try_statement(&mut self, mut statement: Statement) -> Statement {
        let has_try = match &statement {
            Statement::Return {
                value: Some(value), ..
            } => Self::is_try_call(value),
            Statement::Assignment { value, .. } | Statement::ConstDeclaration { value, .. } => {
                Self::is_try_call(value)
            }
            Statement::Expression { expression, .. } | Statement::Call { expression, .. } => {
                Self::is_try_call(expression)
            }
            Statement::VariableDeclarations { declarations, .. } => {
                declarations.len() == 1
                    && declarations[0]
                        .value
                        .as_ref()
                        .map_or(false, Self::is_try_call)
            }
            _ => false,
        };
        if !has_try {
            return statement;
        }

        let slot: &mut Expression = match &mut statement {
            Statement::Return {
                value: Some(value), ..
            } => value,
            Statement::Assignment { value, .. } | Statement::ConstDeclaration { value, .. } => {
                value
            }
            Statement::Expression { expression, .. } | Statement::Call { expression, .. } => {
                expression
            }
            Statement::VariableDeclarations { declarations, .. } => declarations[0]
                .value
                .as_mut()
                .expect("a `?` declaration has a value"),
            _ => unreachable!("has_try is only true for the statement kinds above"),
        };
        let span = slot.span();
        let temp = format!("__try_{}", self.try_counter);
        self.try_counter += 1;

        let identifier = || Expression::Identifier {
            name: temp.clone(),
            span,
        };
        let call = |name: &str, argument: Expression| Expression::Call {
            name: name.to_string(),
            arguments: vec![argument],
            generic_arguments: Vec::new(),
            span,
        };

        let marker = std::mem::replace(slot, call("result_unwrap", identifier()));
        let operand = match marker {
            Expression::Call { mut arguments, .. } => arguments.remove(0),
            other => other,
        };

        let assign = Statement::Assignment {
            target: identifier(),
            value: operand,
            span,
        };
        let check = Statement::If {
            condition: call("result_is_err", identifier()),
            body: vec![Statement::Return {
                value: Some(call("Err", call("result_unwrap_err", identifier()))),
                span,
            }],
            else_body: None,
            span,
        };

        let is_bare_expression = matches!(
            statement,
            Statement::Expression { .. } | Statement::Call { .. }
        );
        let mut queued = vec![check];
        if !is_bare_expression {
            queued.push(statement);
        }
        for (index, queued_statement) in queued.into_iter().enumerate() {
            self.pending_statements.insert(index, queued_statement);
        }
        assign
    }

    fn parse_statement_inner(&mut self) -> Result<Option<Statement>, ParseError> {
        if !self.pending_statements.is_empty() {
            return Ok(Some(self.pending_statements.remove(0)));
        }

        match self.current() {
            Token::Pub => {
                self.advance();
                match self.parse_statement()? {
                    Some(
                        statement @ (Statement::Function { .. }
                        | Statement::Struct { .. }
                        | Statement::Enum { .. }
                        | Statement::Trait { .. }
                        | Statement::ConstDeclaration { .. }
                        | Statement::Import { .. }),
                    ) => Ok(Some(statement)),
                    Some(_) => {
                        self.error("'pub' can only be applied to a top-level declaration or import")
                    }
                    None => self.error("Expected a declaration after 'pub'"),
                }
            }
            Token::NewLine => {
                self.advance();

                if self.current() == &Token::Eof {
                    return Ok(None);
                }

                self.parse_statement()
            }

            Token::Main => Ok(Some(self.parse_main()?)),

            Token::Fn => {
                if self.function_depth > 0 {
                    return self.error("Nested function definitions are not allowed");
                }
                Ok(Some(self.parse_function()?))
            }

            Token::Hash => {
                self.advance();
                if !self.consume(&Token::LeftBracket) {
                    return self.error("Expected '[' after '#'");
                }
                // Test attributes (`#[test]`, `#[bench]`, `#[ignore]`, `#[should_panic]`) decorate the
                // next declaration; the test runner discovers them from source text.
                let test_attribute = match self.current() {
                    Token::Identifier(name)
                        if matches!(
                            name.as_str(),
                            "test" | "bench" | "ignore" | "should_panic"
                        ) =>
                    {
                        true
                    }
                    _ => false,
                };
                if test_attribute {
                    self.advance();
                    if self.current() == &Token::LeftParen {
                        let mut depth = 0i32;
                        loop {
                            match self.current() {
                                Token::LeftParen => depth += 1,
                                Token::RightParen => {
                                    depth -= 1;
                                    if depth == 0 {
                                        self.advance();
                                        break;
                                    }
                                }
                                Token::Eof => {
                                    return self.error("Unterminated attribute arguments")
                                }
                                _ => {}
                            }
                            self.advance();
                        }
                    }
                    if !self.consume(&Token::RightBracket) {
                        return self.error("Expected ']' after attribute");
                    }
                    self.skip_newlines();
                    return self.parse_statement();
                }
                match self.current() {
                    Token::Identifier(name) if name == "derive" => self.advance(),
                    _ => return self.error("Expected derive attribute"),
                }
                if !self.consume(&Token::LeftParen) {
                    return self.error("Expected '(' after derive");
                }
                let mut derives = Vec::new();
                loop {
                    match self.current() {
                        Token::Identifier(name) => {
                            derives.push(name.clone());
                            self.advance();
                        }
                        _ => return self.error("Expected trait name in derive list"),
                    }
                    if !self.consume(&Token::Comma) {
                        break;
                    }
                }
                if !self.consume(&Token::RightParen) || !self.consume(&Token::RightBracket) {
                    return self.error("Expected ')]' after derive list");
                }
                // The attribute is normally followed by a newline before the item it decorates.
                self.skip_newlines();
                if self.current() != &Token::Struct {
                    return self.error("derive attribute must precede a struct declaration");
                }
                let mut declaration = self.parse_struct()?;
                if let Statement::Struct {
                    derives: stored, ..
                } = &mut declaration
                {
                    *stored = derives;
                }
                Ok(Some(declaration))
            }
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
                if self.loop_depth == 0 {
                    return self.error("'break' is only valid inside a loop");
                }
                let start = self.current_span().start;
                self.advance();

                Ok(Some(Statement::Break {
                    span: self.span_from(start),
                }))
            }

            Token::Continue => {
                if self.loop_depth == 0 {
                    return self.error("'continue' is only valid inside a loop");
                }
                let start = self.current_span().start;
                self.advance();

                Ok(Some(Statement::Continue {
                    span: self.span_from(start),
                }))
            }

            Token::Const => Ok(Some(self.parse_const()?)),

            Token::Trait => Ok(Some(self.parse_trait()?)),

            Token::Impl => Ok(Some(self.parse_impl()?)),

            Token::Import => Ok(Some(self.parse_import()?)),

            Token::Async => Ok(Some(self.parse_async_function()?)),

            Token::From => Ok(Some(self.parse_selective_import()?)),

            Token::Identifier(name) if name == "extern" => {
                if self.function_depth > 0 {
                    return self.error("Foreign declarations are only allowed at the top level");
                }
                Ok(Some(self.parse_foreign_block()?))
            }

            Token::Await => self.error("'await' must be used as an expression"),

            Token::Identifier(_)
            | Token::NumType
            | Token::FloatType
            | Token::BoolType
            | Token::StringType => Ok(Some(self.parse_identifier_or_declaration()?)),

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

        // `main` is a function-like scope for parser restrictions (most
        // importantly, function declarations are only allowed at the
        // top level).  It is not represented as `Statement::Function`, so
        // keep the depth bookkeeping here.
        self.function_depth += 1;

        let body_result = match style {
            BlockStyle::Indentation => {
                self.skip_newlines();
                self.parse_indentation_block()
            }

            BlockStyle::Braces => self.parse_brace_block(),

            BlockStyle::Unknown => unreachable!(),
        };

        self.function_depth -= 1;

        let body = body_result?;

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
            Token::NumType | Token::FloatType | Token::BoolType | Token::StringType => {
                let mut position = self.position + 1;

                // Primitive array type: `num[] x`
                if self.peek_at(position) == Some(&Token::LeftBracket) {
                    if self.peek_at(position + 1) != Some(&Token::RightBracket) {
                        return false;
                    }

                    position += 2;
                }

                matches!(self.peek_at(position), Some(Token::Identifier(_)))
            }

            Token::Identifier(_) => {
                if matches!(self.peek_at(self.position + 1), Some(Token::Identifier(_))) {
                    return true;
                }

                if self.peek_at(self.position + 1) == Some(&Token::Less) {
                    let mut i = self.position + 2;
                    let mut depth = 1usize;

                    while let Some(token) = self.peek_at(i) {
                        match token {
                            Token::Less => depth += 1,

                            Token::Greater => {
                                depth -= 1;

                                if depth == 0 {
                                    return matches!(
                                        self.peek_at(i + 1),
                                        Some(Token::Identifier(_))
                                    );
                                }
                            }

                            Token::Eof | Token::NewLine | Token::RightBrace => break,

                            _ => {}
                        }

                        i += 1;
                    }
                }

                false
            }

            _ => false,
        }
    }

    fn parse_identifier_or_declaration(&mut self) -> Result<Statement, ParseError> {
        if matches!(self.current(), Token::Identifier(name) if name == "let") {
            return self.parse_inferred_variable_declaration();
        }
        if self.looks_like_colon_typed_declaration() {
            return self.parse_colon_typed_declaration();
        }
        if self.looks_like_typed_declaration() {
            self.parse_variable_declaration()
        } else {
            self.parse_expression_statement()
        }
    }

    // Parse the established `name: Type = value` spelling. This is distinct
    // from `Type name = value`, but both produce the same typed declaration AST.
    fn looks_like_colon_typed_declaration(&self) -> bool {
        if !matches!(self.current(), Token::Identifier(name) if name != "let")
            || self.peek_at(self.position + 1) != Some(&Token::Colon)
        {
            return false;
        }
        let mut i = self.position + 2;
        if !matches!(self.peek_at(i), Some(Token::Identifier(_) | Token::NumType | Token::FloatType | Token::BoolType | Token::StringType | Token::LeftParen | Token::Fn)) {
            return false;
        }
        let mut angle_depth = 0usize;
        while let Some(token) = self.peek_at(i) {
            match token {
                Token::Less => angle_depth += 1,
                Token::Greater if angle_depth > 0 => angle_depth -= 1,
                Token::Equal if angle_depth == 0 => return true,
                Token::NewLine | Token::Eof | Token::Dedent => return false,
                _ => {}
            }
            i += 1;
        }
        false
    }

    fn parse_colon_typed_declaration(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;
        let name_span = self.current_span();
        let name = match self.current() {
            Token::Identifier(name) => name.clone(),
            _ => return self.error("Expected variable name"),
        };
        self.advance();
        if !self.consume(&Token::Colon) {
            return self.error("Expected ':' after variable name");
        }
        let declared_type = self.parse_type()?;
        if !self.consume(&Token::Equal) {
            return self.error("Expected '=' after variable type annotation");
        }
        let value = self.parse_expression()?;
        let span = self.span_from(start);
        Ok(Statement::VariableDeclarations {
            declarations: vec![VariableDeclaration {
                name,
                name_span,
                declared_type: Some(declared_type),
                value: Some(value),
                span,
            }],
            span,
        })
    }

    /// Parse `let name = expression` declarations. `let` intentionally remains
    /// an identifier token for compatibility with the existing keyword table;
    /// recognizing it here preserves the parser's existing token model while
    /// producing the same untyped AST form used by assignment-style inference.
    fn parse_inferred_variable_declaration(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;
        self.advance(); // `let`
        // `let Type name = value` is the typed form; the untyped form is
        // `let name = value`. Keep both routed through the existing declaration
        // parsers so nominal and primitive annotations share one grammar.
        if self.looks_like_typed_declaration() {
            return self.parse_variable_declaration();
        }
        if self.looks_like_colon_typed_declaration() {
            return self.parse_colon_typed_declaration();
        }
        let mut declarations = Vec::new();
        loop {
            let name_span = self.current_span();
            let name = match self.current() {
                Token::Identifier(name) if name != "let" => {
                    let name = name.clone();
                    self.advance();
                    name
                }
                _ => return self.error("Expected identifier after 'let'"),
            };
            if !self.consume(&Token::Equal) {
                return self.error("Expected '=' and an initializer in inferred 'let' declaration");
            }
            let value = self.parse_expression()?;
            declarations.push(VariableDeclaration {
                name,
                name_span,
                declared_type: None,
                value: Some(value),
                span: self.span_from(start),
            });
            if !self.consume(&Token::Comma) {
                break;
            }
        }
        let span = self.span_from(start);
        for declaration in &mut declarations {
            declaration.span = span;
        }
        Ok(Statement::VariableDeclarations { declarations, span })
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

        let mut values = Vec::new();

        if self.consume(&Token::Equal) {
            loop {
                values.push(self.parse_expression()?);

                if !self.consume(&Token::Comma) {
                    break;
                }
            }
        }

        if values.len() > 1 {
            return self.error("A typed declaration list may have at most one initializer");
        }

        let name_count = names.len();
        let initializer = values.into_iter().next();

        let end = self.previous_span().end;

        let declarations = names
            .into_iter()
            .zip(name_spans)
            .enumerate()
            .map(|(index, (name, name_span))| {
                let value = if index + 1 == name_count {
                    initializer.clone()
                } else {
                    None
                };

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

            // `x = 10, y = 32` is shorthand for two
            // inferred variable declarations.
            if self.current() == &Token::Comma {
                let mut declarations = Vec::new();

                let first_name = match expression {
                    Expression::Identifier { name, span } => (name, span),
                    _ => {
                        return Ok(Statement::Assignment {
                            target: expression,
                            value,
                            span: self.span_from(start),
                        });
                    }
                };

                declarations.push(VariableDeclaration {
                    name: first_name.0,
                    name_span: first_name.1,
                    declared_type: None,
                    value: Some(value),
                    span: self.span_from(start),
                });

                while self.consume(&Token::Comma) {
                    let name_span = self.current_span();

                    let name = match self.current() {
                        Token::Identifier(name) => {
                            let name = name.clone();
                            self.advance();
                            name
                        }
                        _ => {
                            return self.error("Expected identifier after ','");
                        }
                    };

                    if !self.consume(&Token::Equal) {
                        return self.error("Expected '=' in variable declaration");
                    }

                    let value = self.parse_expression()?;

                    declarations.push(VariableDeclaration {
                        name,
                        name_span,
                        declared_type: None,
                        value: Some(value),
                        span: self.span_from(start),
                    });
                }

                return Ok(Statement::VariableDeclarations {
                    declarations,
                    span: self.span_from(start),
                });
            }

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

    fn parse_import(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;
        self.advance();
        let mut parts = Vec::new();
        match self.current() {
            Token::Identifier(name) => {
                parts.push(name.clone());
                self.advance();
            }
            // `main` is a keyword in declaration position, but it is also a
            // valid module filename (`main.fusion`) in an import path.
            Token::Main => {
                parts.push("main".to_string());
                self.advance();
            }
            _ => return self.error("Expected module name after 'import'"),
        }
        while self.consume(&Token::Dot) {
            match self.current() {
                Token::Identifier(name) => {
                    parts.push(name.clone());
                    self.advance();
                }
                Token::Main => {
                    parts.push("main".to_string());
                    self.advance();
                }
                _ => return self.error("Expected module name after '.'"),
            }
        }
        let alias = if self.current() == &Token::As {
            self.advance();
            match self.current() {
                Token::Identifier(name) => {
                    let alias = name.clone();
                    self.advance();
                    Some(alias)
                }
                _ => return self.error("Expected import alias after 'as'"),
            }
        } else {
            None
        };
        Ok(Statement::Import {
            path: parts.join("."),
            alias,
            selected: Vec::new(),
            span: self.span_from(start),
        })
    }

    /// Parse `from module.path import name, other` selective imports.
    fn parse_selective_import(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;
        self.advance(); // from
        let mut parts = Vec::new();
        match self.current() {
            Token::Identifier(name) => {
                parts.push(name.clone());
                self.advance();
            }
            _ => return self.error("Expected module name after 'from'"),
        }
        while self.consume(&Token::Dot) {
            match self.current() {
                Token::Identifier(name) => {
                    parts.push(name.clone());
                    self.advance();
                }
                _ => return self.error("Expected module name after '.'"),
            }
        }
        if !self.consume(&Token::Import) {
            return self.error("Expected 'import' in selective import");
        }
        let mut selected = Vec::new();
        loop {
            match self.current() {
                Token::Identifier(name) => {
                    selected.push(name.clone());
                    self.advance();
                }
                _ => return self.error("Expected imported symbol name"),
            }
            if !self.consume(&Token::Comma) {
                break;
            }
        }
        Ok(Statement::Import {
            path: parts.join("."),
            alias: None,
            selected,
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

        self.loop_depth += 1;
        let body_result = self.parse_style_block();
        self.loop_depth -= 1;
        let body = body_result?;

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

        self.loop_depth += 1;
        let body_result = self.parse_style_block();
        self.loop_depth -= 1;
        let body = body_result?;

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

        let type_parameters = self.parse_type_parameter_list()?;

        let style = match self.current() {
            Token::Colon => {
                self.advance();
                BlockStyle::Indentation
            }

            Token::LeftBrace => BlockStyle::Braces,

            _ => {
                return self.error("Expected ':' or '{' after struct name");
            }
        };

        // Struct field delimiters are part of the type declaration syntax, not
        // the file's executable block style. A module may therefore use
        // `struct Point { ... }` alongside indentation-style function bodies.
        let outer_type_parameters =
            std::mem::replace(&mut self.erased_type_parameters, type_parameters);
        let fields = match style {
            BlockStyle::Indentation => {
                self.skip_newlines();
                self.parse_indentation_struct_fields()?
            }

            BlockStyle::Braces => self.parse_brace_struct_fields()?,

            BlockStyle::Unknown => unreachable!(),
        };
        self.erased_type_parameters = outer_type_parameters;

        Ok(Statement::Struct {
            name,
            fields,
            derives: Vec::new(),
            span: self.span_from(start),
        })
    }

    fn parse_indentation_struct_fields(&mut self) -> Result<Vec<StructField>, ParseError> {
        if !self.consume(&Token::Indent) {
            return self.error("Expected indentation after struct declaration");
        }

        let mut fields = Vec::new();

        self.skip_newlines();

        while self.current() != &Token::Dedent && self.current() != &Token::Eof {
            let start = self.current_span().start;
            let name_span = self.current_span();

            let name = match self.current() {
                Token::Identifier(name) => {
                    let name = name.clone();
                    self.advance();
                    name
                }

                _ => {
                    return self.error("Expected field name in struct declaration");
                }
            };

            if !self.consume(&Token::Colon) {
                return self.error("Expected ':' after struct field name");
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

    fn parse_brace_struct_fields(&mut self) -> Result<Vec<StructField>, ParseError> {
        if !self.consume(&Token::LeftBrace) {
            return self.error("Expected '{' after struct name");
        }

        let mut fields = Vec::new();

        self.skip_newlines();

        while self.current() != &Token::RightBrace && self.current() != &Token::Eof {
            let start = self.current_span().start;
            let name_span = self.current_span();

            let name = match self.current() {
                Token::Identifier(name) => {
                    let name = name.clone();
                    self.advance();
                    name
                }

                _ => {
                    return self.error("Expected field name in struct declaration");
                }
            };

            if !self.consume(&Token::Colon) {
                return self.error("Expected ':' after struct field name");
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
            return self.error("Expected '}' after struct declaration");
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

        let type_parameters = self.parse_type_parameter_list()?;

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

        // Enum variant delimiters are likewise independent of executable block
        // style (see parse_struct).
        let outer_type_parameters =
            std::mem::replace(&mut self.erased_type_parameters, type_parameters);
        let variants = match style {
            BlockStyle::Indentation => {
                self.skip_newlines();
                self.parse_indentation_enum_variants()?
            }

            BlockStyle::Braces => self.parse_brace_enum_variants()?,

            BlockStyle::Unknown => unreachable!(),
        };
        self.erased_type_parameters = outer_type_parameters;

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

    fn parse_brace_enum_variants(&mut self) -> Result<Vec<EnumVariant>, ParseError> {
        if !self.consume(&Token::LeftBrace) {
            return self.error("Expected '{' after enum name");
        }

        let mut variants = Vec::new();

        while self.current() != &Token::RightBrace && self.current() != &Token::Eof {
            if self.current() == &Token::NewLine {
                self.advance();
                continue;
            }

            variants.push(self.parse_enum_variant()?);

            if self.consume(&Token::Comma) {
                self.skip_newlines();
            } else if self.current() == &Token::NewLine {
                // In brace-style enums, variants may be separated by commas or
                // by line breaks, matching the struct/statement formatting style.
                self.skip_newlines();
            } else if self.current() != &Token::RightBrace {
                return self.error("Expected ',', a newline, or '}' after enum variant");
            }
        }

        if !self.consume(&Token::RightBrace) {
            return self.error("Expected '}' after enum variants");
        }

        Ok(variants)
    }

    fn parse_indentation_enum_variants(&mut self) -> Result<Vec<EnumVariant>, ParseError> {
        if !self.consume(&Token::Indent) {
            return self.error("Expected indentation after enum declaration");
        }

        let mut variants = Vec::new();

        while self.current() != &Token::Dedent && self.current() != &Token::Eof {
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
        let first = self.parse_pattern_atom()?;
        let mut alternatives = vec![first];
        while self.consume(&Token::Pipe) {
            alternatives.push(self.parse_pattern_atom()?);
        }
        if alternatives.len() > 1 {
            Ok(Pattern {
                kind: PatternKind::Or(alternatives),
                span: self.span_from(start),
            })
        } else {
            Ok(alternatives.remove(0))
        }
    }

    fn parse_pattern_atom(&mut self) -> Result<Pattern, ParseError> {
        let start = self.current_span().start;

        let mut kind = match self.current() {
            Token::LeftParen => {
                self.advance();
                let mut patterns = Vec::new();
                if self.current() != &Token::RightParen {
                    loop {
                        patterns.push(self.parse_pattern()?);
                        if !self.consume(&Token::Comma) {
                            break;
                        }
                        if self.current() == &Token::RightParen {
                            break;
                        }
                    }
                }
                if !self.consume(&Token::RightParen) {
                    return self.error("Expected ')' after tuple pattern");
                }
                PatternKind::Tuple(patterns)
            }
            Token::Identifier(name) => {
                let first = name.clone();
                self.advance();
                // Patterns have no value-property semantics, so a dotted path
                // before `::` or `{` is unambiguously a qualified type path.
                let mut qualified = first.clone();
                while self.consume(&Token::Dot) {
                    match self.current() {
                        Token::Identifier(part) => {
                            qualified.push('.');
                            qualified.push_str(part);
                            self.advance();
                        }
                        _ => {
                            return self.error("Expected identifier after '.' in qualified pattern")
                        }
                    }
                }

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
                            return self.error("Expected enum variant after '::'");
                        }
                    };

                    let mut bindings = Vec::new();

                    if self.consume(&Token::LeftParen) {
                        if self.current() != &Token::RightParen {
                            loop {
                                bindings.push(self.parse_pattern()?);
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
                            return self.error("Expected ')' after variant pattern arguments");
                        }
                    }

                    PatternKind::Variant {
                        name: format!("{}::{}", qualified, variant),
                        bindings,
                    }
                } else if self.consume(&Token::LeftBrace) {
                    let mut fields = Vec::new();
                    let mut has_rest = false;
                    while self.current() != &Token::RightBrace && self.current() != &Token::Eof {
                        if self.consume(&Token::DotDot) {
                            has_rest = true;
                            if self.current() == &Token::Comma {
                                self.advance();
                            }
                            break;
                        }
                        let field_name = match self.current() {
                            Token::Identifier(field) => {
                                let field = field.clone();
                                self.advance();
                                field
                            }
                            _ => return self.error("Expected field name in struct pattern"),
                        };
                        let field_pattern = if self.consume(&Token::Colon) {
                            self.parse_pattern()?
                        } else {
                            Pattern {
                                kind: PatternKind::Identifier(field_name.clone()),
                                span: self.previous_span(),
                            }
                        };
                        fields.push((field_name, field_pattern));
                        if !self.consume(&Token::Comma) {
                            break;
                        }
                    }
                    if !self.consume(&Token::RightBrace) {
                        return self.error("Expected '}' after struct pattern");
                    }
                    PatternKind::Struct {
                        name: qualified,
                        fields,
                        has_rest,
                    }
                } else if qualified == first
                    && matches!(first.as_str(), "Some" | "Ok" | "Err")
                    && self.current() == &Token::LeftParen
                {
                    // Built-in Option/Result variants may be written without a path:
                    // `Some(x)`, `Ok(v)`, `Err(e)` mean `Option::Some(x)`, `Result::Ok(v)`, ...
                    self.advance();
                    let mut bindings = Vec::new();
                    if self.current() != &Token::RightParen {
                        loop {
                            bindings.push(self.parse_pattern()?);
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
                        return self.error("Expected ')' after variant pattern arguments");
                    }
                    let enum_name = if first == "Some" { "Option" } else { "Result" };
                    PatternKind::Variant {
                        name: format!("{}::{}", enum_name, first),
                        bindings,
                    }
                } else if qualified == first && first == "None" {
                    PatternKind::Variant {
                        name: "Option::None".to_string(),
                        bindings: Vec::new(),
                    }
                } else if self.consume(&Token::At) {
                    let nested = self.parse_pattern_atom()?;
                    PatternKind::At {
                        name: first,
                        pattern: Box::new(nested),
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

        if let PatternKind::Number(low) = kind.clone() {
            let inclusive = if self.consume(&Token::DotDotEq) {
                Some(true)
            } else if self.consume(&Token::DotDot) {
                Some(false)
            } else {
                None
            };
            if let Some(inclusive) = inclusive {
                let high = match self.current() {
                    Token::Num(n) => {
                        let n = *n;
                        self.advance();
                        n
                    }
                    _ => return self.error("Expected integer at end of range pattern"),
                };
                kind = PatternKind::Range {
                    start: low,
                    end: high,
                    inclusive,
                };
            } else {
                kind = PatternKind::Number(low);
            }
        }

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
                return self.error("Expected ':' or '{' after match expression");
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
        let guard = if self.consume(&Token::If) {
            Some(self.parse_expression()?)
        } else {
            None
        };

        let has_arrow = self.consume(&Token::FatArrow);
        let has_colon = if !has_arrow {
            self.consume(&Token::Colon)
        } else {
            false
        };

        if !has_arrow && !has_colon {
            return self.error("Expected '=>' or ':' after match pattern");
        }

        if has_arrow && self.current() == &Token::Colon {
            return self.error("':' is not allowed after '=>'; use a block style directly");
        }

        let body = if self.current() == &Token::NewLine {
            self.advance();
            self.skip_newlines();

            self.parse_indentation_block()?
        } else if self.current() == &Token::LeftBrace {
            self.parse_brace_block()?
        } else {
            let statement = self.parse_statement()?.ok_or_else(|| {
                ParseError::new("Expected statement after '=>'", self.current_span())
            })?;

            vec![statement]
        };

        Ok(MatchArm {
            pattern,
            guard,
            body,
            span: self.span_from(start),
        })
    }

    fn parse_indentation_match_arms(&mut self) -> Result<Vec<MatchArm>, ParseError> {
        self.skip_newlines();

        if !self.consume(&Token::Indent) {
            return self.error("Expected indentation after match");
        }

        let mut arms = Vec::new();

        while self.current() != &Token::Dedent && self.current() != &Token::Eof {
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

    fn parse_brace_match_arms(&mut self) -> Result<Vec<MatchArm>, ParseError> {
        if !self.consume(&Token::LeftBrace) {
            return self.error("Expected '{' after match expression");
        }

        let mut arms = Vec::new();

        while self.current() != &Token::RightBrace && self.current() != &Token::Eof {
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

    fn parse_indentation_block(&mut self) -> Result<Vec<Statement>, ParseError> {
        if !self.consume(&Token::Indent) {
            return self.error("Expected indentation block");
        }

        let mut statements = Vec::new();

        while self.current() != &Token::Dedent && self.current() != &Token::Eof {
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

    fn parse_brace_block(&mut self) -> Result<Vec<Statement>, ParseError> {
        if !self.consume(&Token::LeftBrace) {
            return self.error("Expected '{' at beginning of block");
        }

        let mut statements = Vec::new();

        while self.current() != &Token::RightBrace && self.current() != &Token::Eof {
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

    fn parse_async_function(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;

        self.advance(); // consume `async`

        if !self.consume(&Token::Fn) {
            return self.error("Expected 'fn' after 'async'");
        }

        let statement = self.parse_function_after_fn(start)?;

        match statement {
            Statement::Function {
                name,
                generic_parameters,
                parameters,
                return_type,
                body,
                span,
                ..
            } => Ok(Statement::Function {
                name,
                generic_parameters,
                parameters,
                return_type,
                body,
                is_async: true,
                foreign_abi: None,
                link_name: None,
                span: Span::new(start, span.end),
            }),

            _ => unreachable!(),
        }
    }

    /// Parse a block of C ABI declarations. Each prototype is represented as a
    /// function declaration in the AST; additional declarations are queued for
    /// the ordinary top-level parser loop.
    fn parse_foreign_block(&mut self) -> Result<Statement, ParseError> {
        let block_start = self.current_span().start;
        self.advance(); // extern

        let abi = match self.current() {
            Token::String(name) if name == "C" => {
                let name = name.clone();
                self.advance();
                name
            }
            Token::String(_) => return self.error("Only extern \"C\" is supported"),
            _ => return self.error("Expected ABI string after 'extern' (for example, extern \"C\")"),
        };

        if !self.consume(&Token::LeftBrace) {
            return self.error("Expected '{' after extern ABI");
        }

        self.skip_newlines();
        let mut declarations = Vec::new();

        while self.current() != &Token::RightBrace && self.current() != &Token::Eof {
            let declaration_start = self.current_span().start;
            if !self.consume(&Token::Fn) {
                return self.error("Expected a function prototype inside extern block");
            }

            let name = match self.current() {
                Token::Identifier(name) => {
                    let name = name.clone();
                    self.advance();
                    name
                }
                _ => return self.error("Expected foreign function name after 'fn'"),
            };

            if !self.consume(&Token::LeftParen) {
                return self.error("Expected '(' after foreign function name");
            }

            let mut parameters = Vec::new();
            while self.current() != &Token::RightParen && self.current() != &Token::Eof {
                let parameter_start = self.current_span().start;
                let name_span = self.current_span();
                let parameter_name = match self.current() {
                    Token::Identifier(name) => {
                        let name = name.clone();
                        self.advance();
                        name
                    }
                    _ => return self.error("Expected foreign parameter name"),
                };

                if !self.consume(&Token::Colon) {
                    return self.error("Foreign parameters require an explicit ABI type");
                }
                let type_name = self.parse_type()?;
                parameters.push(Parameter {
                    name: parameter_name,
                    name_span,
                    type_name: Some(type_name),
                    span: self.span_from(parameter_start),
                });

                if self.consume(&Token::Comma) {
                    if self.current() == &Token::RightParen {
                        break;
                    }
                } else if self.current() != &Token::RightParen {
                    return self.error("Expected ',' or ')' after foreign parameter");
                }
            }

            if !self.consume(&Token::RightParen) {
                return self.error("Expected ')' after foreign parameters");
            }

            let return_type = if self.consume(&Token::Arrow) {
                Some(self.parse_type()?)
            } else {
                Some("void".to_string())
            };

            if !self.consume(&Token::Semicolon) {
                return self.error("Expected ';' after foreign function prototype");
            }

            let span = self.span_from(declaration_start);
            declarations.push(Statement::Function {
                name: name.clone(),
                generic_parameters: Vec::new(),
                parameters,
                return_type,
                body: Vec::new(),
                is_async: false,
                foreign_abi: Some(abi.clone()),
                link_name: Some(name),
                span,
            });

            self.skip_newlines();
        }

        if !self.consume(&Token::RightBrace) {
            return self.error("Expected '}' to close extern block");
        }
        if declarations.is_empty() {
            return self.error("extern block must contain at least one function prototype");
        }

        let mut declarations = declarations.into_iter();
        let first = declarations.next().expect("non-empty declarations checked");
        self.pending_statements.extend(declarations);
        let _ = block_start;
        Ok(first)
    }

    fn parse_function(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;
        self.advance(); // consume `fn`

        self.parse_function_after_fn(start)
    }
    fn parse_function_after_fn(&mut self, start: usize) -> Result<Statement, ParseError> {
        let name = match self.current() {
            Token::Identifier(name) => {
                let name = name.clone();
                self.advance();
                name
            }
            _ => return self.error("Expected function name after 'fn'"),
        };

        let mut generic_parameters = Vec::new();

        if self.current() == &Token::Less {
            self.advance();

            while self.current() != &Token::Greater && self.current() != &Token::Eof {
                match self.current() {
                    Token::Identifier(name) => {
                        generic_parameters.push(name.clone());
                        self.advance();
                    }

                    _ => return self.error("Expected generic parameter name"),
                }

                if self.current() == &Token::Comma {
                    self.advance();
                }
            }

            if !self.consume(&Token::Greater) {
                return self.error("Expected '>' after generic parameters");
            }
        }

        if !self.consume(&Token::LeftParen) {
            return self.error("Expected '(' after function name");
        }

        let mut parameters = Vec::new();

        while self.current() != &Token::RightParen && self.current() != &Token::Eof {
            let parameter_start = self.current_span().start;
            let name_span = self.current_span();

            let (parameter_name, type_name) = match self.current() {
                // `name: type`
                Token::Identifier(first) => {
                    let first = first.clone();
                    self.advance();

                    if self.consume(&Token::Colon) {
                        let type_name = self.parse_type()?;
                        (first, Some(type_name))
                    } else if generic_parameters
                        .iter()
                        .any(|parameter| parameter == &first)
                        && matches!(self.current(), Token::Identifier(_))
                    {
                        // Generic type-first syntax: `T value`.
                        //
                        // Ordinary identifiers remain the existing untyped
                        // `name` form.  Restricting this interpretation to a
                        // declared generic parameter removes the ambiguity
                        // between `foo` and `T value`.
                        let parameter_name = match self.current() {
                            Token::Identifier(name) => {
                                let name = name.clone();
                                self.advance();
                                name
                            }
                            _ => unreachable!(),
                        };

                        (parameter_name, Some(first))
                    } else {
                        // Plain `name` = untyped parameter.
                        (first, None)
                    }
                }

                // Optional `type name` syntax.
                Token::NumType | Token::FloatType | Token::BoolType | Token::StringType => {
                    let type_name = self.parse_type()?;

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

                    (parameter_name, Some(type_name))
                }

                _ => {
                    return self.error("Expected parameter name");
                }
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
                return self.error("Expected ',' or ')' after parameter");
            }
        }

        if !self.consume(&Token::RightParen) {
            return self.error("Expected ')' after function parameters");
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
                return self.error("Expected ':' or '{' to begin function body");
            }
        };

        self.use_block_style(style)?;

        let previous_return_type = self.current_function_return_type.clone();
        self.current_function_return_type = Some(return_type.clone());
        self.function_depth += 1;

        let body_result = match style {
            BlockStyle::Indentation => {
                self.skip_newlines();
                self.parse_indentation_block()
            }

            BlockStyle::Braces => self.parse_brace_block(),

            BlockStyle::Unknown => unreachable!(),
        };

        self.function_depth -= 1;
        self.current_function_return_type = previous_return_type;

        let body = body_result?;

        Ok(Statement::Function {
            name,
            generic_parameters,
            parameters,
            return_type,
            body,
            is_async: false,
            foreign_abi: None,
            link_name: None,
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
                return self.error("Expected ':' or '{' after trait name");
            }
        };

        self.use_block_style(style)?;

        let methods = match style {
            BlockStyle::Indentation => {
                self.skip_newlines();

                if !self.consume(&Token::Indent) {
                    return self.error("Expected indentation after trait declaration");
                }

                let mut methods = Vec::new();

                while self.current() != &Token::Dedent && self.current() != &Token::Eof {
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
                    return self.error("Expected '{' after trait name");
                }

                let mut methods = Vec::new();

                while self.current() != &Token::RightBrace && self.current() != &Token::Eof {
                    if self.current() == &Token::NewLine {
                        self.advance();
                        continue;
                    }

                    methods.push(self.parse_trait_method()?);
                    self.skip_newlines();
                }

                if !self.consume(&Token::RightBrace) {
                    return self.error("Expected '}' after trait declaration");
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
        if self.current() == &Token::Fn && self.trait_method_has_body() {
            return self.parse_trait_default_method();
        }

        let start = self.current_span().start;

        if !self.consume(&Token::Fn) {
            return self.error("Expected 'fn' in trait declaration");
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
            return self.error("Expected '(' after trait method name");
        }

        let mut parameters = Vec::new();

        while self.current() != &Token::RightParen && self.current() != &Token::Eof {
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
                return self.error("Expected ',' or ')' after parameter");
            }
        }

        if !self.consume(&Token::RightParen) {
            return self.error("Expected ')' after trait method parameters");
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
            default_body: None,
            span: self.span_from(start),
        })
    }

    /// True if the trait method starting at the current `fn` token has a body (a default
    /// method) rather than being a bare signature. The signature ends after the parameter
    /// list and optional `-> type`; a body then starts with `:` or `{`, while a required
    /// method ends at a line break, `;`, or the end of the trait.
    fn trait_method_has_body(&self) -> bool {
        let mut position = self.position;
        let mut depth = 0i32;
        let mut parameters_done = false;
        loop {
            let token = match self.peek_at(position) {
                Some(token) => token,
                None => return false,
            };
            if parameters_done && depth <= 0 {
                match token {
                    Token::Colon | Token::LeftBrace => return true,
                    Token::NewLine
                    | Token::Semicolon
                    | Token::Dedent
                    | Token::RightBrace
                    | Token::Eof => return false,
                    _ => {}
                }
            }
            match token {
                Token::LeftParen | Token::LeftBracket | Token::Less => depth += 1,
                Token::RightBracket | Token::Greater => depth -= 1,
                Token::RightParen => {
                    depth -= 1;
                    if depth <= 0 {
                        parameters_done = true;
                    }
                }
                Token::Eof => return false,
                _ => {}
            }
            position += 1;
        }
    }

    /// Parse a trait method that has a body. The body is parsed exactly like a free function
    /// body and stored on the `TraitMethod` as a default implementation.
    fn parse_trait_default_method(&mut self) -> Result<TraitMethod, ParseError> {
        let start = self.current_span().start;
        let function = self.parse_function()?;
        match function {
            Statement::Function {
                name,
                generic_parameters,
                parameters,
                return_type,
                body,
                ..
            } => {
                if !generic_parameters.is_empty() {
                    return self.error("Generic default methods are not supported in traits yet");
                }
                Ok(TraitMethod {
                    name,
                    parameters,
                    return_type,
                    default_body: Some(body),
                    span: self.span_from(start),
                })
            }
            _ => self.error("Expected a method declaration in trait"),
        }
    }

    // ------------------------------------------------------------
    // impl
    // ------------------------------------------------------------

    fn parse_impl(&mut self) -> Result<Statement, ParseError> {
        let start = self.current_span().start;
        self.advance();

        let first_name =
            self.parse_qualified_identifier("Expected type or trait name after 'impl'")?;

        // `impl Stack<T>:` / `impl Trait for Stack<T>:` declare the type parameters used by
        // the methods; they are erased like the type's own (see `parse_type_parameter_list`).
        let mut type_parameters = self.parse_type_parameter_list()?;
        let (trait_name, type_name) = if self.consume(&Token::For) {
            let type_name = self.parse_qualified_identifier("Expected type name after 'for'")?;
            type_parameters.extend(self.parse_type_parameter_list()?);
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
                return self.error("Expected ':' or '{' after impl declaration");
            }
        };

        self.use_block_style(style)?;

        let outer_type_parameters =
            std::mem::replace(&mut self.erased_type_parameters, type_parameters);
        let methods = match style {
            BlockStyle::Indentation => {
                self.skip_newlines();
                self.parse_indentation_block()?
            }

            BlockStyle::Braces => self.parse_brace_block()?,

            BlockStyle::Unknown => unreachable!(),
        };
        self.erased_type_parameters = outer_type_parameters;

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

    fn looks_like_generic_call(&self) -> bool {
        if self.current() != &Token::Less {
            return false;
        }

        let mut position = self.position + 1;

        // There must be at least one generic type argument.
        if position >= self.tokens.len() {
            return false;
        }

        loop {
            let token = match self.peek_at(position) {
                Some(token) => token,
                None => return false,
            };

            match token {
                Token::NumType
                | Token::FloatType
                | Token::BoolType
                | Token::StringType
                | Token::Identifier(_) => {
                    position += 1;
                }

                _ => return false,
            }

            match self.peek_at(position) {
                Some(Token::Comma) => {
                    position += 1;
                }

                Some(Token::Greater) => {
                    position += 1;
                    break;
                }

                _ => return false,
            }
        }

        matches!(self.peek_at(position), Some(Token::LeftParen))
    }

    fn parse_generic_arguments(&mut self) -> Result<Vec<String>, ParseError> {
        if !self.consume(&Token::Less) {
            return self.error("Expected '<'");
        }

        let mut arguments = Vec::new();

        loop {
            arguments.push(self.parse_type()?);

            if self.consume(&Token::Comma) {
                continue;
            }

            break;
        }

        if !self.consume(&Token::Greater) {
            return self.error("Expected '>' after generic arguments");
        }

        Ok(arguments)
    }

    fn looks_like_named_struct_constructor(&self) -> bool {
        matches!(
            (
                self.peek_at(self.position + 1),
                self.peek_at(self.position + 2)
            ),
            (Some(Token::Identifier(_)), Some(Token::Colon))
        )
    }

    fn parse_named_struct_fields_braced(
        &mut self,
    ) -> Result<Vec<(String, Expression)>, ParseError> {
        if !self.consume(&Token::LeftBrace) {
            return self.error("Expected '{'");
        }
        let mut fields = Vec::new();
        while self.current() != &Token::RightBrace && self.current() != &Token::Eof {
            let name = match self.current() {
                Token::Identifier(name) => {
                    let name = name.clone();
                    self.advance();
                    name
                }
                _ => return self.error("Expected field name in struct constructor"),
            };
            if !self.consume(&Token::Colon) {
                return self.error("Expected ':' after struct constructor field name");
            }
            fields.push((name, self.parse_expression()?));
            if !self.consume(&Token::Comma) {
                break;
            }
        }
        if !self.consume(&Token::RightBrace) {
            return self.error("Expected '}' after struct constructor fields");
        }
        Ok(fields)
    }

    fn parse_named_struct_fields(&mut self) -> Result<Vec<(String, Expression)>, ParseError> {
        if !self.consume(&Token::LeftParen) {
            return self.error("Expected '('");
        }

        let mut fields = Vec::new();

        if self.current() == &Token::RightParen {
            self.advance();
            return Ok(fields);
        }

        loop {
            let name = match self.current() {
                Token::Identifier(name) => {
                    let name = name.clone();
                    self.advance();
                    name
                }
                _ => return self.error("Expected field name in struct constructor"),
            };

            if !self.consume(&Token::Colon) {
                return self.error("Expected ':' after struct constructor field name");
            }

            fields.push((name, self.parse_expression()?));

            if self.consume(&Token::Comma) {
                if self.current() == &Token::RightParen {
                    self.advance();
                    return Ok(fields);
                }
                continue;
            }

            if !self.consume(&Token::RightParen) {
                return self.error("Expected ')' after struct constructor fields");
            }
            return Ok(fields);
        }
    }

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

    fn parse_comparison(&mut self) -> Result<Expression, ParseError> {
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

    fn parse_multiplication(&mut self) -> Result<Expression, ParseError> {
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

        // Explicit conversion has lower precedence than arithmetic but higher
        // precedence than comparisons: `x + 1 as float` converts the complete
        // arithmetic expression.
        if self.current() == &Token::As {
            let start = left.span().start;
            self.advance();
            let target_type = self.parse_type()?;
            let end = self.previous_span().end;
            left = Expression::Conversion {
                kind: ConversionKind::As,
                expression: Box::new(left),
                target_type,
                span: Span::new(start, end),
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

            Token::Star => {
                let start = self.current_span().start;
                self.advance();
                let expression = self.parse_unary()?;
                let end = expression.span().end;
                Ok(Expression::Unary {
                    operator: UnaryOperator::Deref,
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

            Token::Await => {
                let start = self.current_span().start;
                self.advance();
                let expression = self.parse_unary()?;
                let end = expression.span().end;
                Ok(Expression::Await {
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

    /// `fn(x: num) -> num => x * 2`: an anonymous function with an expression body.
    /// The executable body is hoisted, but the expression retains the lexical names to snapshot
    /// into its closure environment when evaluated.
    fn parse_lambda_expression(&mut self) -> Result<Expression, ParseError> {
        let start = self.current_span().start;
        self.advance();
        if !self.consume(&Token::LeftParen) {
            return self.error("Expected '(' after 'fn' in a function expression");
        }
        let mut parameters = Vec::new();
        if self.current() != &Token::RightParen {
            loop {
                let (name, name_span) = match self.current() {
                    Token::Identifier(name) => (name.clone(), self.current_span()),
                    _ => return self.error("Expected a parameter name"),
                };
                let parameter_start = self.current_span().start;
                self.advance();
                if !self.consume(&Token::Colon) {
                    return self.error(
                        "Function expression parameters need a type, e.g. fn(x: num) -> num => x * 2",
                    );
                }
                let type_name = self.parse_type()?;
                parameters.push(Parameter {
                    name,
                    name_span,
                    type_name: Some(type_name),
                    span: self.span_from(parameter_start),
                });
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
            return self.error("Expected ')' after function expression parameters");
        }
        let return_type = if self.consume(&Token::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        if !self.consume(&Token::FatArrow) {
            return self.error("Expected '=>' before the body of a function expression");
        }
        let body = self.parse_expression()?;
        let span = self.span_from(start);
        let mut captures = Vec::new();
        collect_expression_identifiers(&body, &mut captures);
        let parameter_names: std::collections::HashSet<String> =
            parameters.iter().map(|parameter| parameter.name.clone()).collect();
        captures.retain(|capture| !parameter_names.contains(capture));
        let mut seen = std::collections::HashSet::new();
        captures.retain(|capture| seen.insert(capture.clone()));
        let name = format!("__lambda_{}", self.lambda_counter);
        self.lambda_counter += 1;
        let mut function_parameters = parameters;
        let mut function_body = Vec::new();
        for (index, capture) in captures.iter().enumerate() {
            let hidden_name = format!("{}__capture_{}", name, index);
            function_parameters.push(Parameter {
                name: hidden_name.clone(),
                name_span: span,
                type_name: Some("unknown".to_string()),
                span,
            });
            function_body.push(Statement::VariableDeclarations {
                declarations: vec![VariableDeclaration {
                    name: capture.clone(),
                    name_span: span,
                    declared_type: None,
                    value: Some(Expression::Identifier {
                        name: hidden_name,
                        span,
                    }),
                    span,
                }],
                span,
            });
        }
        function_body.push(Statement::Return {
            value: Some(body),
            span,
        });
        self.hoisted_functions.push(Statement::Function {
            name: name.clone(),
            generic_parameters: Vec::new(),
            parameters: function_parameters,
            return_type,
            body: function_body,
            is_async: false,
            foreign_abi: None,
            link_name: None,
            span,
        });
        Ok(Expression::Closure { name, captures, span })
    }

    fn parse_primary(&mut self) -> Result<Expression, ParseError> {
        let mut expression = match self.current() {
            Token::Identifier(name) => {
                let start = self.current_span().start;
                let name = name.clone();
                self.advance();

                // A dotted path followed by `::` denotes a qualified enum
                // constructor (for example `g.Color::Red`). Otherwise leave
                // dots to the ordinary property/method-call parser.
                let saved_position = self.position;
                let mut qualified_enum = name.clone();
                let mut had_dot = false;
                while self.consume(&Token::Dot) {
                    had_dot = true;
                    match self.current() {
                        Token::Identifier(part) => {
                            qualified_enum.push('.');
                            qualified_enum.push_str(part);
                            self.advance();
                        }
                        _ => {
                            self.position = saved_position;
                            break;
                        }
                    }
                }
                if had_dot
                    && self.current() != &Token::DoubleColon
                    && self.current() != &Token::LeftBrace
                    && !(self.current() == &Token::LeftParen
                        && self.looks_like_named_struct_constructor())
                {
                    self.position = saved_position;
                }

                if had_dot && self.current() == &Token::LeftBrace {
                    let fields = self.parse_named_struct_fields_braced()?;
                    Expression::StructConstructor {
                        name: qualified_enum,
                        fields,
                        span: self.span_from(start),
                    }
                } else if had_dot
                    && self.current() == &Token::LeftParen
                    && self.looks_like_named_struct_constructor()
                {
                    let fields = self.parse_named_struct_fields()?;
                    Expression::StructConstructor {
                        name: qualified_enum,
                        fields,
                        span: self.span_from(start),
                    }
                } else if self.current() == &Token::LeftBrace {
                    let fields = self.parse_named_struct_fields_braced()?;
                    Expression::StructConstructor {
                        name,
                        fields,
                        span: self.span_from(start),
                    }
                } else if self.current() == &Token::DoubleColon {
                    let enum_name = if had_dot {
                        qualified_enum
                    } else {
                        name.clone()
                    };
                    self.advance();

                    if matches!(self.current(), Token::From | Token::Try) {
                        let kind = match self.current() {
                            Token::From => ConversionKind::From,
                            Token::Try => ConversionKind::Try,
                            _ => unreachable!(),
                        };
                        self.advance();
                        let arguments = self.parse_arguments()?;
                        if arguments.len() != 1 {
                            return self.error("conversion requires exactly one argument");
                        }
                        return Ok(Expression::Conversion {
                            kind,
                            expression: Box::new(arguments.into_iter().next().unwrap()),
                            target_type: name,
                            span: self.span_from(start),
                        });
                    }

                    let variant = match self.current() {
                        Token::Identifier(variant) => {
                            let variant = variant.clone();
                            self.advance();
                            variant
                        }
                        _ => return self.error("Expected enum variant name"),
                    };

                    let arguments = if self.current() == &Token::LeftParen {
                        self.parse_arguments()?
                    } else {
                        Vec::new()
                    };

                    Expression::EnumConstructor {
                        enum_name,
                        variant,
                        arguments,
                        span: self.span_from(start),
                    }
                } else {
                    let generic_arguments = if self.looks_like_generic_call() {
                        self.parse_generic_arguments()?
                    } else {
                        Vec::new()
                    };

                    if self.current() == &Token::LeftParen {
                        if self.looks_like_named_struct_constructor() {
                            let fields = self.parse_named_struct_fields()?;
                            Expression::StructConstructor {
                                name,
                                fields,
                                span: self.span_from(start),
                            }
                        } else {
                            let arguments = self.parse_arguments()?;

                            Expression::Call {
                                name,
                                arguments,
                                generic_arguments,
                                span: self.span_from(start),
                            }
                        }
                    } else if name == "None" {
                        // `None` is the empty Option; it is the built-in call `None()`.
                        Expression::Call {
                            name,
                            arguments: Vec::new(),
                            generic_arguments: Vec::new(),
                            span: self.span_from(start),
                        }
                    } else {
                        Expression::Identifier {
                            name,
                            span: self.span_from(start),
                        }
                    }
                }
            }

            Token::NumType | Token::FloatType | Token::BoolType | Token::StringType => {
                let start = self.current_span().start;
                let target_type = match self.current() {
                    Token::NumType => "num",
                    Token::FloatType => "float",
                    Token::BoolType => "bool",
                    Token::StringType => "string",
                    _ => unreachable!(),
                }
                .to_string();
                self.advance();
                if self.current() != &Token::DoubleColon {
                    return self
                        .error("primitive type name is only valid here as a conversion target");
                }
                self.advance();
                let kind = match self.current() {
                    Token::From => ConversionKind::From,
                    Token::Try => ConversionKind::Try,
                    _ => return self.error("expected 'from' or 'try' after conversion target"),
                };
                self.advance();
                let arguments = self.parse_arguments()?;
                if arguments.len() != 1 {
                    return self.error("conversion requires exactly one argument");
                }
                Expression::Conversion {
                    kind,
                    expression: Box::new(arguments.into_iter().next().unwrap()),
                    target_type,
                    span: self.span_from(start),
                }
            }

            Token::Num(value) => {
                let span = self.current_span();
                let value = *value;
                self.advance();

                Expression::Number { value, span }
            }

            Token::WideNum(value) => {
                let span = self.current_span();
                let value = *value;
                self.advance();

                Expression::WideNumber { value, span }
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

            Token::Fn => self.parse_lambda_expression()?,

            Token::LeftParen => {
                let start = self.current_span().start;
                self.advance();
                let first = self.parse_expression()?;
                if self.consume(&Token::Comma) {
                    let mut elements = vec![first];
                    while self.current() != &Token::RightParen && self.current() != &Token::Eof {
                        elements.push(self.parse_expression()?);
                        if !self.consume(&Token::Comma) {
                            break;
                        }
                    }
                    if !self.consume(&Token::RightParen) {
                        return self.error("Expected ')' after tuple expression");
                    }
                    Expression::Tuple {
                        elements,
                        span: self.span_from(start),
                    }
                } else {
                    if !self.consume(&Token::RightParen) {
                        return self.error("Expected ')' after expression");
                    }
                    first
                }
            }

            Token::LeftBracket => {
                let start = self.current_span().start;
                self.advance();

                let mut elements = Vec::new();

                while self.current() != &Token::RightBracket && self.current() != &Token::Eof {
                    elements.push(self.parse_expression()?);

                    if self.consume(&Token::Comma) {
                        if self.current() == &Token::RightBracket {
                            break;
                        }
                    } else if self.current() != &Token::RightBracket {
                        return self.error("Expected ',' or ']' in array");
                    }
                }

                if !self.consume(&Token::RightBracket) {
                    return self.error("Expected ']' after array");
                }

                Expression::Array {
                    elements,
                    span: Span::new(start, self.previous_span().end),
                }
            }

            _ => {
                return self.error(format!("Expected expression, found {:?}", self.current()));
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
                            return self.error("Expected identifier after '.'");
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
                            generic_arguments: Vec::new(),
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
                        return self.error("Expected ']' after index");
                    }

                    let end = self.previous_span().end;

                    expression = Expression::Index {
                        array: Box::new(expression),
                        index: Box::new(index),
                        span: Span::new(start, end),
                    };
                }

                // Postfix `?`: the marker call `__try(expr)`, desugared by `parse_statement`.
                Token::Question => {
                    let start = expression.span().start;
                    self.advance();
                    let end = self.previous_span().end;
                    expression = Expression::Call {
                        name: "__try".to_string(),
                        arguments: vec![expression],
                        generic_arguments: Vec::new(),
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

        while self.current() != &Token::Eof || !self.pending_statements.is_empty() {
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

        if self.require_main && !self.seen_main {
            return self.error("Program must contain a 'main' declaration");
        }

        statements.extend(std::mem::take(&mut self.hoisted_functions));

        let program_end = self
            .tokens
            .last()
            .map(|token| token.span.end)
            .unwrap_or(program_start);

        let program = Program {
            statements,
            span: Span::new(program_start, program_end),
            modules: Vec::new(),
        };
        Ok(crate::trait_defaults::expand_trait_defaults(
            crate::derive_expansion::expand_derives(program),
        ))
    }
}

#[cfg(test)]
mod inferred_let_parser_tests {
    use super::*;
    use crate::lexer::{BlockMode, Lexer};

    #[test]
    fn let_initializer_is_an_inferred_variable_declaration() {
        let source = "main:\n    let answer = 42\n";
        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("source should lex");
        let mut parser = Parser::new(tokens);
        let program = parser.parse().expect("inferred let should parse");
        let Statement::Main { body, .. } = &program.statements[0] else {
            panic!("expected main block");
        };
        let Statement::VariableDeclarations { declarations, .. } = &body[0] else {
            panic!("expected variable declaration");
        };
        assert_eq!(declarations[0].name, "answer");
        assert!(declarations[0].declared_type.is_none());
        assert!(declarations[0].value.is_some());
    }

    #[test]
    fn let_nominal_type_annotation_is_a_typed_variable_declaration() {
        let source = "main:\n    let Point point = Point { x: 1 }\n";
        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("source should lex");
        let mut parser = Parser::new(tokens);
        let program = parser.parse().expect("typed let should parse");
        let Statement::Main { body, .. } = &program.statements[0] else {
            panic!("expected main block");
        };
        let Statement::VariableDeclarations { declarations, .. } = &body[0] else {
            panic!("expected variable declaration");
        };
        assert_eq!(declarations[0].name, "point");
        assert_eq!(declarations[0].declared_type.as_deref(), Some("Point"));
        assert!(declarations[0].value.is_some());
    }

    #[test]
    fn brace_struct_fields_can_coexist_with_indentation_function_blocks() {
        let source = "struct Point { x: num }\nfn get_x(point: Point) -> num:\n    return point.x\nmain:\n    let Point point = Point { x: 1 }\n    print(get_x(point))\n";
        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("expression/type braces should lex");
        let mut parser = Parser::new(tokens);
        parser.parse().expect("struct braces should not select file block style");
    }
}


#[cfg(test)]
mod foreign_declaration_parser_tests {
    use super::*;
    use crate::lexer::{BlockMode, Lexer};

    #[test]
    fn parses_multiple_c_abi_declarations_and_opaque_handles() {
        let source = r#"extern "C" {
    fn fusion_helper(value: i32) -> i32;
    fn fusion_release(handle: *opaque);
}
main:
    print("foreign declarations parsed")
"#;
        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("foreign source should lex");
        let mut parser = Parser::new(tokens);
        let program = parser.parse().expect("foreign declarations should parse");

        assert_eq!(program.statements.len(), 3);
        let Statement::Function {
            name,
            parameters,
            return_type,
            foreign_abi,
            link_name,
            ..
        } = &program.statements[0] else {
            panic!("expected first foreign function");
        };
        assert_eq!(name, "fusion_helper");
        assert_eq!(parameters[0].type_name.as_deref(), Some("i32"));
        assert_eq!(return_type.as_deref(), Some("i32"));
        assert_eq!(foreign_abi.as_deref(), Some("C"));
        assert_eq!(link_name.as_deref(), Some("fusion_helper"));

        let Statement::Function { parameters, return_type, .. } = &program.statements[1] else {
            panic!("expected second foreign function");
        };
        assert_eq!(parameters[0].type_name.as_deref(), Some("*opaque"));
        assert_eq!(return_type.as_deref(), Some("void"));
    }

    #[test]
    fn rejects_non_c_foreign_abis() {
        let source = "extern \"Python\" { fn run(); }\nmain:\n    print(1)\n";
        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("source should lex");
        let mut parser = Parser::new(tokens);
        assert!(parser.parse().is_err());
    }
}
