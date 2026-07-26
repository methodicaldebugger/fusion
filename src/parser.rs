use crate::lexer::Token;
use crate::ast::*;

pub struct Parser {
    tokens: Vec<Token>,
    position: usize,
}


impl Parser {

    fn parse_block(&mut self) -> Vec<Statement> {

    let mut statements = Vec::new();

    // expect indentation
    if self.current() == &Token::Indent {
        self.advance();
    }
    else {
        return statements;
    }


    while self.current() != &Token::Dedent
        && self.current() != &Token::Eof
    {

        if let Some(statement) = self.parse_statement() {
    statements.push(statement);
}
else {

    if self.current() != &Token::Dedent {
        self.advance();
    }

}

    }


    // remove Dedent
    if self.current() == &Token::Dedent {
        self.advance();
    }

    statements
}

    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            position: 0,
        }
    }

    fn current(&self) -> &Token {
        &self.tokens[self.position]
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

        if let Some(statement) = self.parse_statement() {
            statements.push(statement);
        }
        else {
            self.advance();
        }

    }

    Program {
        statements,
    }
}

fn parse_function(&mut self) -> Option<Statement> {
     println!("ENTERED FUNCTION PARSER");
    // consume fn
    self.advance();


    // function name
    let name = match self.current() {

        Token::Identifier(name) => {
            let name = name.clone();
            self.advance();
            name
        }

        _ => return None,
    };


    // (
    if !self.consume(&Token::LeftParen) {
        return None;
    }


    let mut parameters = Vec::new();


    while self.current() != &Token::RightParen
        && self.current() != &Token::Eof
    {

        if let Token::Identifier(param) = self.current() {

            parameters.push(param.clone());
            self.advance();

        } else {
            self.advance();
        }


        // skip type annotations for now
        if self.current() == &Token::Colon {
            self.advance();
            self.advance();
        }


        if self.current() == &Token::Comma {
            self.advance();
        }

    }


    // )
    if !self.consume(&Token::RightParen) {
        return None;
    }


    let mut return_type = None;


    // ->
    if self.current() == &Token::Arrow {

        self.advance();


        if let Token::Identifier(type_name) = self.current() {

            return_type = Some(type_name.clone());
            self.advance();

        }
    }


    // :
    if !self.consume(&Token::Colon) {
        return None;
    }


    // newline
    if self.current() == &Token::NewLine {
        self.advance();
    }


    let body = self.parse_block();


    Some(
        Statement::Function {
            name,
            parameters,
            return_type,
            body,
        }
    )
}

fn parse_statement(&mut self) -> Option<Statement> {

    println!("CURRENT STATEMENT TOKEN = {:?}", self.current());

    match self.current() {

        Token::NewLine => {
    self.advance();
    None
}


        Token::Fn => {
            println!("FOUND FUNCTION");
            self.parse_function()
        }


        Token::If => {

    self.advance();

    let condition = self.parse_expression()?;


    if !self.consume(&Token::Colon) {
        return None;
    }


    if self.current() == &Token::NewLine {
        self.advance();
    }


    let body = self.parse_block();


    Some(
        Statement::If {
            condition,
            body,
        }
    )
}


        Token::Return => {

            self.advance();

            let value = self.parse_expression()?;

            Some(
                Statement::Return(value)
            )
        }


        Token::Identifier(name) => {

    let name = name.clone();

    self.advance();


    if self.current() == &Token::Equal {

        self.advance();

        let value = self.parse_expression()?;


        return Some(
            Statement::Assignment {
                name,
                value,
            }
        );
    }


    self.position -= 1;


    let expr = self.parse_expression()?;


    if let Expression::Call { .. } = expr {

        return Some(
            Statement::Call(expr)
        );
    }


    None
}


        _ => None,
    }
}

fn parse_expression(&mut self) -> Option<Expression> {
    self.parse_comparison()
}

fn parse_unary(&mut self) -> Option<Expression> {

    match self.current() {

        Token::Minus => {

            self.advance();

            let expression = self.parse_unary()?;

            Some(
                Expression::Unary {
                    operator: UnaryOperator::Negate,
                    expression: Box::new(expression),
                }
            )
        }


        Token::Pipe => {

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

fn parse_primary(&mut self) -> Option<Expression> {

    match self.current() {

        Token::LeftParen => {

            self.advance();

            let expr = self.parse_expression()?;

            if !self.consume(&Token::RightParen) {
                panic!("Expected ')'");
            }

            Some(expr)
        }

        Token::Integer(value) => {
            let value = *value;
            self.advance();
            Some(Expression::Integer(value))
        }

        Token::Float(value) => {
            let value = *value;
            self.advance();
            Some(Expression::Float(value))
        }

        Token::String(value) => {
            let value = value.clone();
            self.advance();
            Some(Expression::String(value))
        }

       Token::Boolean(value) => {
    let value = *value;
    self.advance();
    Some(Expression::Boolean(value))
}


Token::Identifier(name) => {

    let name = name.clone();

    self.advance();


    if self.current() == &Token::LeftParen {

        self.advance();

        let mut arguments = Vec::new();


        while self.current() != &Token::RightParen
            && self.current() != &Token::Eof
        {

            let argument = self.parse_expression()?;

            arguments.push(argument);


            if self.current() == &Token::Comma {
                self.advance();
            }
        }


        self.consume(&Token::RightParen);


        Some(
            Expression::Call {
                name,
                arguments,
            }
        )

    }

    else {

        Some(
            Expression::Identifier(name)
        )

    }
}


_ => None,
    }
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

    Some(left)
}

}