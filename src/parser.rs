
//contents of parser.rs
use crate::lexer::Token;
use crate::ast::*;
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


fn parse_style_block(&mut self) -> Option<Vec<Statement>> {
    let style = match self.current() {
        Token::Colon => {
            self.advance();
            BlockStyle::Indentation
        }

        Token::LeftBrace => BlockStyle::Braces,

        _ => return None,
    };

    // Every block in the program must use the same global style.
    self.use_block_style(style);

    match style {
        BlockStyle::Indentation => {
            while self.current() == &Token::NewLine {
                self.advance();
            }

            Some(self.parse_indentation_block())
        }

        BlockStyle::Braces => {
            Some(self.parse_brace_block())
        }

        BlockStyle::Unknown => unreachable!(),
    }
}


fn use_block_style(&mut self, style: BlockStyle) {
    match self.block_style {
        BlockStyle::Unknown => {
            // main has not appeared yet.
            //
            // Remember every style used before main.
            // main must eventually agree with all of them.
            self.pending_block_styles.push(style);
        }

        existing if existing != style => {
            panic!(
                "Block style mismatch: program uses {:?}, but this construct uses {:?}",
                existing,
                style
            );
        }

        _ => {}
    }
}



fn establish_main_style(&mut self, style: BlockStyle) {
    if self.seen_main {
        panic!("Multiple 'main' declarations are not allowed");
    }

    // main establishes the single global block style.
    //
    // Every block that appeared before main must already use
    // exactly the same style.
    for previous in &self.pending_block_styles {
        if *previous != style {
            panic!(
                "Block style mismatch: main uses {:?}, but an earlier construct uses {:?}",
                style,
                previous
            );
        }
    }

    self.pending_block_styles.clear();

    self.block_style = style;
    self.seen_main = true;
}



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


    fn parse_statement(&mut self) -> Option<Statement> {
    match self.current() {
        Token::NewLine => {
            self.advance();

            if self.current() == &Token::Eof {
                return None;
            }

            self.parse_statement()
        }

Token::Main => {
    self.advance();

    // `main` is intentionally strict.
    //
    // These are the only valid forms:
    //
    //     main:
    //     main {
    //
    // `main` followed by anything else is invalid.
    let style = match self.current() {
        Token::Colon => {
            self.advance();
            BlockStyle::Indentation
        }

        Token::LeftBrace => {
            BlockStyle::Braces
        }

        _ => return None,
    };

    // main establishes the global program-wide block style.
    self.establish_main_style(style);

    let body = match style {
        BlockStyle::Indentation => {
            while self.current() == &Token::NewLine {
                self.advance();
            }

            self.parse_indentation_block()
        }

        BlockStyle::Braces => {
            self.parse_brace_block()
        }

        BlockStyle::Unknown => unreachable!(),
    };

    Some(Statement::Main { body })
}



        Token::Fn => self.parse_function(),

        Token::Defer => {
    self.advance();

    let expression = self.parse_expression()?;

    Some(Statement::Defer(expression))
}

        Token::Struct => self.parse_struct(),

        Token::Enum => self.parse_enum(),

        Token::Match => self.parse_match(),

        Token::While => {
    self.advance();

    let previous = self.allow_struct_constructor;
    self.allow_struct_constructor = false;

    let condition = self.parse_expression()?;

    self.allow_struct_constructor = previous;

    let body = self.parse_style_block()?;

    Some(Statement::While {
        condition,
        body,
    })
}

        Token::If => {
    self.advance();

    let previous = self.allow_struct_constructor;
    self.allow_struct_constructor = false;

    let condition = self.parse_expression()?;

    self.allow_struct_constructor = previous;

    let body = self.parse_style_block()?;

    let else_body = if self.current() == &Token::Else {
        self.advance();

        Some(self.parse_style_block()?)
    } else {
        None
    };

    Some(Statement::If {
        condition,
        body,
        else_body,
    })
}

        Token::For => self.parse_for(),

        Token::Break => {
            self.advance();
            Some(Statement::Break)
        }

        Token::Continue => {
            self.advance();
            Some(Statement::Continue)
        }

        Token::Return => {
            self.advance();

            let value = self.parse_expression()?;

            Some(Statement::Return(value))
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
        _ => return None,
    };

            let declared_type = if self.consume(&Token::Colon) {
                Some(self.parse_type()?)
            } else {
                None
            };

            if !self.consume(&Token::Equal) {
                return None;
            }

            let value = self.parse_expression()?;

            Some(Statement::ConstDeclaration {
    name,
    name_span,
    declared_type,
    value,
    span: Span::new(
        statement_start,
        self.current_span().end,
    ),
})
        }


        Token::Identifier(first_name) => {
            let first_name = first_name.clone();

            // -------------------------------------------------
            // Typed variable declaration
            //
            // num x = 10
            // string name = "Bob"
            // Color c = Color::Red
            // Result a = Result::Ok(42)
            // -------------------------------------------------

            if let Some(Token::Identifier(_)) =
                self.peek_at(self.position + 1)
            {
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

                                if let Some(token) = self.tokens.get(lookahead) {
                                    name_spans.push(token.span);
                                }

                                lookahead += 1;
                            }

                            _ => return None,
                        }
                    }

                    if matches!(
                        self.peek_at(lookahead),
                        Some(Token::Equal)
                        ) {
                        // Move from the type name to the first variable.
                        self.advance();

                        // Consume variable names.
                        for _ in 0..names.len() {
                            self.advance();

                            if self.current() == &Token::Comma {
                                self.advance();
                            }
                        }

                        // Consume '='.
                        if !self.consume(&Token::Equal) {
                            return None;
                        }

                        let mut values = Vec::new();

                        loop {
                            let value = self.parse_expression()?;
                            values.push(value);

                            if self.current() == &Token::Comma {
                                self.advance();
                            } else {
                                break;
                            }
                        }

                        if names.len() != values.len() {
                            return None;
                        }

                        let declarations = names
                            .into_iter()
                            .zip(name_spans.into_iter())
                            .zip(values.into_iter())
                            .map(|((name, name_span), value)| {
                            VariableDeclaration {
                                name,
                                name_span,
                                declared_type: Some(first_name.clone()),
                                value,
                                span: Span::new(
                                    statement_start,
                                    self.current_span().end,
                                    ),
                                }
                            })
                        .collect();

                        return Some(
                            Statement::VariableDeclarations {
                            declarations,
                            span: Span::new(
                                statement_start,
                                self.current_span().end,
                            ),
                        }
                    );
                    }
                }
            }

            // -------------------------------------------------
            // Normal expression / assignment
            // -------------------------------------------------

            let expression = self.parse_expression()?;

            if self.current() == &Token::Equal {
                self.advance();

                let value = self.parse_expression()?;

                Some(Statement::Assignment {
                    target: expression,
                    value,
                })
            } else {
                Some(Statement::Expression(expression))
            }
        }

        _ => None,
    }
}

    fn parse_for(&mut self) -> Option<Statement> {
    self.advance();

    let variable = match self.current() {
        Token::Identifier(name) => {
            let n = name.clone();
            self.advance();
            n
        }

        _ => return None,
    };

    if !self.consume(&Token::In) {
        return None;
    }

    let start = self.parse_expression()?;

    if !self.consume(&Token::DotDot) {
        return None;
    }

    let end = self.parse_expression()?;

    let body = self.parse_style_block()?;

    Some(Statement::For {
        variable,
        start,
        end,
        body,
    })
}



    fn parse_struct(&mut self) -> Option<Statement> {
    self.advance(); // consume `struct`

    let name = match self.current() {
        Token::Identifier(name) => {
            let name = name.clone();
            self.advance();
            name
        }
        _ => return None,
    };

    let style = match self.current() {
        Token::Colon => {
            self.advance();
            BlockStyle::Indentation
        }

        Token::LeftBrace => BlockStyle::Braces,

        _ => return None,
    };

    // Struct declarations must use the same block style
    // established by `main`.


    self.use_block_style(style);

    let fields = match style {
        BlockStyle::Indentation => {
            while self.current() == &Token::NewLine {
                self.advance();
            }

            self.parse_indentation_struct_fields()?
        }

        BlockStyle::Braces => {
            self.parse_brace_struct_fields()?
        }

        BlockStyle::Unknown => unreachable!(),
    };

    Some(Statement::Struct {
        name,
        fields,
    })
}

    fn parse_indentation_struct_fields(&mut self) -> Option<Vec<StructField>> {
    let mut fields = Vec::new();

    if !self.consume(&Token::Indent) {
        return None;
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
            _ => return None,
        };

        if !self.consume(&Token::Colon) {
            return None;
        }

        let type_name = self.parse_type()?;

        fields.push(StructField {
            name: field_name,
            name_span: field_name_span,
            type_name,
        });

        // Optional comma.
        if self.current() == &Token::Comma {
            self.advance();
        }

        // Newline is consumed naturally by the loop.
        if self.current() == &Token::NewLine {
            self.advance();
        }
    }

    if self.current() == &Token::Dedent {
        self.advance();
    }

    Some(fields)
}

    fn parse_brace_struct_fields(&mut self) -> Option<Vec<StructField>> {
    if !self.consume(&Token::LeftBrace) {
        return None;
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
            _ => return None,
        };

        if !self.consume(&Token::Colon) {
            return None;
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
        return None;
    }

    Some(fields)
}


fn parse_enum(&mut self) -> Option<Statement> {
    self.advance(); // consume `enum`

    let name = match self.current() {
        Token::Identifier(name) => {
            let name = name.clone();
            self.advance();
            name
        }

        _ => return None,
    };

    let style = match self.current() {
        Token::Colon => {
            self.advance();
            BlockStyle::Indentation
        }

        Token::LeftBrace => BlockStyle::Braces,

        _ => return None,
    };

    // Enum declarations must use the single global block style.
    self.use_block_style(style);

    let variants = match style {
        BlockStyle::Indentation => {
            while self.current() == &Token::NewLine {
                self.advance();
            }

            self.parse_indentation_enum_variants()?
        }

        BlockStyle::Braces => {
            self.parse_brace_enum_variants()?
        }

        BlockStyle::Unknown => unreachable!(),
    };

    Some(Statement::Enum {
        name,
        variants,
    })
}


fn parse_brace_enum_variants(&mut self) -> Option<Vec<EnumVariant>> {
    if !self.consume(&Token::LeftBrace) {
        return None;
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

            _ => return None,
        };

        let mut fields = Vec::new();

        // Tuple-style enum variant:
        //
        // Rgb(num, num, num)
        if self.consume(&Token::LeftParen) {
            while self.current() != &Token::RightParen
                && self.current() != &Token::Eof
            {
                let field_type = self.parse_type()?;
                fields.push(field_type);

                if self.current() == &Token::Comma {
                    self.advance();
                }
            }

            if !self.consume(&Token::RightParen) {
                return None;
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

        while self.current() == &Token::NewLine {
            self.advance();
        }
    }

    if !self.consume(&Token::RightBrace) {
        return None;
    }

    Some(variants)
}



fn parse_indentation_enum_variants(&mut self) -> Option<Vec<EnumVariant>> {
    let mut variants = Vec::new();

    while self.current() == &Token::NewLine {
        self.advance();
    }

    if !self.consume(&Token::Indent) {
        return None;
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

            _ => return None,
        };

        let mut fields = Vec::new();

        // Tuple-style enum variant:
        //
        // Rgb(num, num, num)
        if self.consume(&Token::LeftParen) {
            while self.current() != &Token::RightParen
                && self.current() != &Token::Eof
            {
                let field_type = self.parse_type()?;
                fields.push(field_type);

                if self.current() == &Token::Comma {
                    self.advance();
                }
            }

            if !self.consume(&Token::RightParen) {
                return None;
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

        while self.current() == &Token::NewLine {
            self.advance();
        }
    }

    if self.current() == &Token::Dedent {
        self.advance();
    }

    Some(variants)
}



    fn parse_pattern(&mut self) -> Option<Pattern> {
    match self.current() {
        Token::Identifier(name) => {
    let first = name.clone();
    self.advance();

    if first == "_" {
        return Some(Pattern::Wildcard);
    }

    if self.consume(&Token::DoubleColon) {
        let variant = match self.current() {
            Token::Identifier(name) => {
                let variant = name.clone();
                self.advance();
                variant
            }
            _ => return None,
        };

        let mut bindings = Vec::new();

        if self.consume(&Token::LeftParen) {
            while self.current() != &Token::RightParen
                && self.current() != &Token::Eof
            {
                match self.current() {
                    Token::Identifier(name) => {
                        bindings.push(name.clone());
                        self.advance();
                    }
                    _ => return None,
                }

                if self.consume(&Token::Comma) {
                    continue;
                }

                if self.current() != &Token::RightParen {
                    return None;
                }
            }

            if !self.consume(&Token::RightParen) {
                return None;
            }
        }

        return Some(Pattern::Variant {
            name: format!("{}::{}", first, variant),
            bindings,
        });
    }

    Some(Pattern::Identifier(first))
}

        Token::Num(value) => {
            let value = *value;
            self.advance();
            Some(Pattern::Number(value))
        }

        Token::Float(value) => {
            let value = *value;
            self.advance();
            Some(Pattern::Float(value))
        }

        Token::String(value) => {
            let value = value.clone();
            self.advance();
            Some(Pattern::String(value))
        }

        Token::Boolean(value) => {
    let value = value.clone();
    self.advance();
    Some(Pattern::Boolean(value))
}

        _ => None,
    }
}


    fn parse_match(&mut self) -> Option<Statement> {
    self.advance(); // consume `match`

    // The `{` after a match expression is a match block delimiter,
    // not a struct constructor.
    let previous = self.allow_struct_constructor;
    self.allow_struct_constructor = false;

    let expression = self.parse_expression()?;

    self.allow_struct_constructor = previous;

    let style = match self.current() {
        Token::Colon => {
            self.advance();
            BlockStyle::Indentation
        }

        Token::LeftBrace => BlockStyle::Braces,

        _ => return None,
    };

    // Match blocks must obey the same global block style as main.


    self.use_block_style(style);

    let arms = match style {
        BlockStyle::Indentation => self.parse_indentation_match_arms()?,

        BlockStyle::Braces => self.parse_brace_match_arms()?,

        BlockStyle::Unknown => unreachable!(),
    };

    Some(Statement::Match {
        expression,
        arms,
    })
}

    fn parse_indentation_match_arms(&mut self) -> Option<Vec<MatchArm>> {
    let mut arms = Vec::new();

    // Skip blank lines before the first arm.
    while self.current() == &Token::NewLine {
        self.advance();
    }

    // The match arms themselves form an indentation block.
    if !self.consume(&Token::Indent) {
        return None;
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
    return None;
}

// `=>:` is invalid in indentation mode.
if self.current() == &Token::Colon {
    return None;
}

let body = if self.current() == &Token::NewLine {
    self.advance();

    while self.current() == &Token::NewLine {
        self.advance();
    }

    self.parse_indentation_block()
} else {
    let statement = self.parse_statement()?;
    vec![statement]
};

        arms.push(MatchArm {
            pattern,
            body,
        });

        // Consume blank lines between arms.
        while self.current() == &Token::NewLine {
            self.advance();
        }
    }

    if self.current() == &Token::Dedent {
        self.advance();
    }

    Some(arms)
}

    fn parse_brace_match_arms(&mut self) -> Option<Vec<MatchArm>> {
    if !self.consume(&Token::LeftBrace) {
        return None;
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
            return None;
        }

        let body = if self.current() == &Token::LeftBrace {
            self.parse_brace_block()
        } else {
            let statement = self.parse_statement()?;
            vec![statement]
        };

        arms.push(MatchArm {
            pattern,
            body,
        });

        while self.current() == &Token::NewLine {
            self.advance();
        }
    }

    if !self.consume(&Token::RightBrace) {
        return None;
    }

    Some(arms)
}


    fn parse_arguments(&mut self) -> Option<Vec<Expression>> {
    let mut arguments = Vec::new();

    if !self.consume(&Token::LeftParen) {
        return None;
    }

    if self.current() == &Token::RightParen {
        self.advance();
        return Some(arguments);
    }

    loop {
        let argument = self.parse_expression()?;
        arguments.push(argument);

        if self.current() == &Token::Comma {
            self.advance();
            continue;
        }

        break;
    }

    if !self.consume(&Token::RightParen) {
        return None;
    }

    Some(arguments)
}




    fn parse_indentation_block(&mut self) -> Vec<Statement> {
    let mut statements = Vec::new();

    if !self.consume(&Token::Indent) {
        panic!("Expected indentation block");
    }

    while self.current() != &Token::Dedent
        && self.current() != &Token::Eof
    {
        if self.current() == &Token::NewLine {
            self.advance();
            continue;
        }

        if let Some(statement) = self.parse_statement() {
            statements.push(statement);
        } else {
            panic!(
                "Invalid statement in indentation block near token: {:?}",
                self.current()
            );
        }
    }

    if self.current() == &Token::Dedent {
        self.advance();
    }

    statements
}



    fn parse_brace_block(&mut self) -> Vec<Statement> {
        let mut statements = Vec::new();
        // consume {
        if !self.consume(&Token::LeftBrace) {
            return statements;
        }
        while self.current() != &Token::RightBrace
            && self.current() != &Token::Eof
        {
            // Ignore newlines inside brace blocks
            if self.current() == &Token::NewLine {
                self.advance();
                continue;
            }
        if let Some(statement) = self.parse_statement() {
            statements.push(statement);
        } else {
            panic!(
                "Invalid statement in brace block near token: {:?}",
                self.current()
                );
            }
        }
        if !self.consume(&Token::RightBrace) {
            panic!("{}", "Expected '}' at end of block");
        }
    statements
    }


    fn current(&self) -> &Token {
        &self.tokens[self.position].node
    }

    fn peek_at(&self, position: usize) -> Option<&Token> {
    self.tokens.get(position).map(|token| &token.node)
}

    fn current_span(&self) -> Span {
        self.tokens[self.position].span
    }

    fn advance(&mut self) {
        if self.position < self.tokens.len() - 1 {
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

    pub fn parse(&mut self) -> Program {
    let mut statements = Vec::new();

    while self.current() != &Token::Eof {
        let result = self.parse_statement();

        if let Some(statement) = result {
            statements.push(statement);
        } else {
            if self.current() == &Token::Eof {
                break;
            }

            panic!(
                "Unexpected token at top level: {:?}",
                self.current()
            );
        }
    }

    if !self.seen_main {
        panic!("Program must contain a 'main' declaration");
    }

    Program {
        statements,
    }
}


    fn parse_type(&mut self) -> Option<String> {
    match self.current() {
        Token::NumType => {
            self.advance();
            Some("num".into())
        }

        Token::FloatType => {
            self.advance();
            Some("float".into())
        }

        Token::BoolType => {
            self.advance();
            Some("bool".into())
        }

        Token::StringType => {
            self.advance();
            Some("string".into())
        }

        Token::Identifier(name) => {
            let name = name.clone();
            self.advance();
            Some(name)
        }

        _ => None,
    }
}





    fn parse_function(&mut self) -> Option<Statement> {
    self.advance();

    let name = match self.current() {
        Token::Identifier(name) => {
            let name = name.clone();
            self.advance();
            name
        }
        _ => return None,
    };

    if !self.consume(&Token::LeftParen) {
        return None;
    }

    let mut parameters = Vec::new();

    while self.current() != &Token::RightParen
        && self.current() != &Token::Eof
    {
        if self.current() == &Token::Comma {
            self.advance();
            continue;
        }

        if let Token::Identifier(param) = self.current() {
            let name_span = self.current_span();
            let name = param.clone();
            self.advance();

            let mut type_name = None;

            if self.current() == &Token::Colon {
                self.advance();
                type_name = self.parse_type();

                if type_name.is_none() {
                    return None;
                }
            }

            parameters.push(Parameter {
                name,
                name_span,
                type_name,
            });
        }

        if self.current() == &Token::Comma {
            self.advance();
        }
    }

    if !self.consume(&Token::RightParen) {
        return None;
    }

    let mut return_type = None;

    if self.current() == &Token::Arrow {
        self.advance();
        return_type = self.parse_type();

        if return_type.is_none() {
            return None;
        }
    }

    let style = match self.current() {
        Token::Colon => {
            self.advance();
            BlockStyle::Indentation
        }

        Token::LeftBrace => BlockStyle::Braces,

        _ => return None,
    };

    self.use_block_style(style);

    let body = match style {
        BlockStyle::Indentation => {
            while self.current() == &Token::NewLine {
                self.advance();
            }

            self.parse_indentation_block()
        }

        BlockStyle::Braces => {
            self.parse_brace_block()
        }

        BlockStyle::Unknown => unreachable!(),
    };

    Some(Statement::Function {
        name,
        generic_parameters: Vec::new(),
        parameters,
        return_type,
        body,
    })
}









fn parse_or(&mut self)->Option<Expression>{
    let mut left=self.parse_and()?;
    loop {
        let operator=match self.current(){
            Token::OrOr |
            Token::Or =>
            Operator::Or,
            _=>break,
        };
        self.advance();
        let right=self.parse_and()?;
        left=Expression::Binary{
            left:Box::new(left),
            operator,
            right:Box::new(right),
        };
    }
    Some(left)
}











fn parse_and(&mut self)->Option<Expression>{
    let mut left=self.parse_comparison()?;
    loop {
        let operator=match self.current(){
            Token::AndAnd |
            Token::And =>
                Operator::And,
            _=>break,
        };
        self.advance();
        let right=self.parse_comparison()?;
        left=Expression::Binary{
            left:Box::new(left),
            operator,
            right:Box::new(right),
        };
    }
    Some(left)
}









fn parse_expression(&mut self)-> Option<Expression>
{
    self.parse_or()
}









fn parse_unary(&mut self) -> Option<Expression> {
    match self.current() {
        Token::Minus => {
            self.advance();
            let expression = self.parse_unary()?;
            Some(Expression::Unary {
                    operator: UnaryOperator::Negate,
                    expression: Box::new(expression),
                })
        }
        Token::Bang | Token::Not => {
            self.advance();
            let expression = self.parse_unary()?;
            Some(
                Expression::Unary {
                    operator: UnaryOperator::Not,
                    expression: Box::new(expression),
                }
            )
        }
        _ => {
            self.parse_primary()
        }
    }
}




    fn parse_struct_fields(
    &mut self
) -> Option<Vec<(String, Expression)>> {
    let mut fields = Vec::new();

    if !self.consume(&Token::LeftBrace) {
        return None;
    }

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
            _ => return None,
        };

        if !self.consume(&Token::Colon) {
            return None;
        }

        let value = self.parse_expression()?;

        fields.push((name, value));

        if self.current() == &Token::Comma {
            self.advance();
        }
    }

    if !self.consume(&Token::RightBrace) {
        return None;
    }

    Some(fields)
}






    fn parse_primary(&mut self) -> Option<Expression> {
        let mut expression = match self.current() {

            Token::Identifier(name) => {
    let name = name.clone();
    self.advance();

    // Enum constructor:
    //
    // Color::Red
    // Color::Rgb(255, 0, 0)
    //
    if self.current() == &Token::DoubleColon {
        self.advance();

        let variant = match self.current() {
            Token::Identifier(variant) => {
                let variant = variant.clone();
                self.advance();
                variant
            }
            _ => return None,
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
            let value = value.clone();
            self.advance();
            Expression::Number(value)
        }

        Token::Float(value) => {
            let value = value.clone();
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
        return None;
    }

    expression
}

Token::LeftBracket => {
    self.advance();

    let mut values = Vec::new();

    while self.current() != &Token::RightBracket
        && self.current() != &Token::Eof
    {
        let value = self.parse_expression()?;
        values.push(value);

        if self.current() == &Token::Comma {
            self.advance();
        } else if self.current() != &Token::RightBracket {
            return None;
        }
    }

    if !self.consume(&Token::RightBracket) {
        return None;
    }

    Expression::Array(values)
}

_ => return None,
    };

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
        _ => return None,
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
                    return None;
                }

                expression = Expression::Index {
                    array: Box::new(expression),
                    index: Box::new(index),
                };
            }

            _ => break,
        }
    }

    Some(expression)
}















fn parse_comparison(&mut self) -> Option<Expression> {
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
    Some(left)
}







fn parse_addition(&mut self) -> Option<Expression> {
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
    Some(left)
}









fn parse_multiplication(&mut self) -> Option<Expression> {
 let mut left= self.parse_unary()?;
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
    Some(left)
}
}
