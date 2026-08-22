
//contents of parser.rs
use crate::lexer::Token;
use crate::ast::*;
pub struct Parser {
    tokens: Vec<Token>,
    position: usize,
}
impl Parser { // contains many functions for parsing different constructs in the language
   


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
        if !self.consume(&Token::Colon) {
            return None;
        }
        while self.current() == &Token::NewLine {
            self.advance();
        }
        let body = self.parse_block();
        Some(Statement::For {variable,start,end,body,})
    }



    fn parse_struct(&mut self) -> Option<Statement> {
    self.advance(); // consume 'struct'

    let name = match self.current() {
        Token::Identifier(name) => {
            let name = name.clone();
            self.advance();
            name
        }
        _ => return None,
    };

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
            type_name,
        });

        // optional comma
        if self.current() == &Token::Comma {
            self.advance();
        }
    }

    if !self.consume(&Token::RightBrace) {
        return None;
    }

    Some(Statement::Struct {
        name,
        fields,
    })
}



    fn parse_arguments(&mut self) -> Option<Vec<Expression>> {
        let mut arguments = Vec::new();
        if !self.consume(&Token::LeftParen) {
            return None;
        }
        while self.current() != &Token::RightParen&& self.current() != &Token::Eof
        {
            let argument =
            self.parse_expression()?;
            arguments.push(argument);
            if self.current() == &Token::Comma {
                self.advance();
            }
        }
        if !self.consume(&Token::RightParen) {
            return None;
        }
        Some(arguments)
    }




    fn is_type_name(name: &str) -> bool {
        matches!(
            name,
            "num" | "float" | "bool" | "string"
        )
    }




    fn parse_block(&mut self) -> Vec<Statement> {
        match self.current() {
            Token::Indent => self.parse_indentation_block(),
            Token::LeftBrace => self.parse_brace_block(),
            _ => Vec::new(),
        }
    }




    fn parse_indentation_block(&mut self) -> Vec<Statement> {
        let mut statements = Vec::new();

        // consume Indent
        self.advance();

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
            let result = self.parse_statement();
            if let Some(statement) = result {
                statements.push(statement);
            }
            else {
                if self.current() == &Token::Eof {
                    break;
                }
            panic!(
                "Unexpected token at top level: {:?}",
                self.current()
                );
            }
        }
        Program {
            statements,
        }
    }




    fn parse_type(&mut self) -> Option<String> {
        match self.current() {
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
                parameters.push(Parameter {name,type_name,});
                } else {
                    self.advance();
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
        if !self.consume(&Token::Colon) {
            return None;
        }
        if self.current() == &Token::NewLine {
            self.advance();
        }
        let body = self.parse_block();
        Some(Statement::Function {
            name,generic_parameters: Vec::new(),parameters,return_type,body,
        })
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
            Token::Fn => self.parse_function(), 
            Token::Struct => self.parse_struct(),
            Token::While => {
                self.advance();
                let condition = self.parse_expression()?;
                if !self.consume(&Token::Colon) {
                    return None;
                }
                while self.current() == &Token::NewLine {
                    self.advance();
                }
                let body = self.parse_block();
                Some(Statement::While {
                    condition,
                    body,
                    })
                }
            Token::Const => {
                self.advance();
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
            declared_type,
            value,
        })
    }
        Token::If => {
            self.advance();
            let condition = self.parse_expression()?;
            if !self.consume(&Token::Colon) {
                return None;
            }
            while self.current() == &Token::NewLine {
                self.advance();
            }
            let body = self.parse_block();
            let else_body = if self.current() == &Token::Else {
                self.advance();
                if !self.consume(&Token::Colon) {
                    return None;
                }
                while self.current() == &Token::NewLine {
                    self.advance();
                }
                Some(self.parse_block())
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
        Token::Identifier(first_name) => {
    let first_name = first_name.clone();

    // Type declaration:
    // Person person = ...
    // num age = ...
    if let Some(Token::Identifier(_)) =
        self.tokens.get(self.position + 1)
    {
        if matches!(
            self.tokens.get(self.position + 2),
            Some(Token::Equal)
        ) {
            self.advance(); // type

            let name = match self.current() {
                Token::Identifier(name) => {
                    let name = name.clone();
                    self.advance();
                    name
                }
                _ => return None,
            };

            self.advance(); // '='

            let value = self.parse_expression()?;

            return Some(Statement::VariableDeclaration {
                name,
                declared_type: Some(first_name),
                value,
            });
        }
    }

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

                if self.current() == &Token::LeftParen {
                    let arguments = self.parse_arguments()?;

                    Expression::Call {
                        name,
                        arguments,
                        generic_arguments: Vec::new(),
                    }
                } else if self.current() == &Token::LeftBrace {
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
            let value = *value;
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

                expression = Expression::Property {
                    object: Box::new(expression),
                    name,
                };
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
