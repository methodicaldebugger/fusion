
//contents of parser.rs
use crate::lexer::Token;
use crate::ast::*;
pub struct Parser {
    tokens: Vec<Token>,
    position: usize,
}
impl Parser {
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
    fn parse_arguments(&mut self) -> Option<Vec<Expression>> {
    let mut arguments = Vec::new();
    if !self.consume(&Token::LeftParen) {
        return None;
    }
    while self.current() != &Token::RightParen
        && self.current() != &Token::Eof
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
    fn is_type_name(name:&str)->bool {
        matches!(name,"int" |"float" |"bool" |"string")
    }
    fn parse_block(&mut self) -> Vec<Statement> {
    let mut statements = Vec::new();
    if self.current() == &Token::Indent {
        self.advance();
    }
    else {
        return statements;
    }
    while self.current() != &Token::Dedent
        && self.current() != &Token::Eof
    {
        if self.current() == &Token::NewLine {
            self.advance();
            continue;
        }
        if let Some(statement) = self.parse_statement() {
            println!("BLOCK FOUND: {:?}", statement);
            statements.push(statement);
        }
        else {
    panic!(
        "Invalid statement in block near token: {:?}",
        self.current()
    );
}
    }
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
    println!("START PARAM PARSE");
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
    println!("FUNCTION NAME: {}", name);
    println!("FUNCTION BODY: {:?}", body);
    Some(Statement::Function {name,parameters,return_type,body,})}

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
            let next = self.tokens.get(self.position + 1);
            match next {
                Some(Token::Equal) => {
                    self.advance(); // identifier
                    self.advance(); // '='
                    let value = self.parse_expression()?;
                    Some(Statement::Assignment {
                        name: first_name,
                        declared_type: None,
                        value,
                    })
                }
                Some(Token::LeftParen) => {
                    let expression = self.parse_expression()?;
                    Some(Statement::Call(expression))
                }
                Some(Token::Identifier(_))
                    if Parser::is_type_name(&first_name) =>
                {
                    self.advance(); // type
                    let name = match self.current() {
                        Token::Identifier(name) => {
                            let n = name.clone();
                            self.advance();
                            n
                        }
                        _ => return None,
                    };
                    if !self.consume(&Token::Equal) {
                        return None;
                    }
                    let value = self.parse_expression()?;
                    Some(Statement::Assignment {
                        name,
                        declared_type: Some(first_name),
                        value,
                    })
                }
                _ => {
                    let expression = self.parse_expression()?;
                    Some(Statement::Expression(expression))
                }
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
fn parse_primary(&mut self) -> Option<Expression> {
    match self.current() {
        Token::Character(value) => {
            let value = *value;
            self.advance();
            Some(Expression::Character(value))
        }
        Token::LeftParen => {
            self.advance();
            let expr = self.parse_expression()?;
            if !self.consume(&Token::RightParen) {
                panic!("Expected ')'");
            }
            Some(expr)
        }
        Token::LeftBracket => {
    self.advance();
    let mut values=Vec::new();
    while self.current()!=&Token::RightBracket {
        let value =
            self.parse_expression()?;
        values.push(value);
        if self.current()==&Token::Comma {
            self.advance();
        }
    }
    self.advance();
    Some(
        Expression::Array(values)
    )

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
    let arguments =
        self.parse_arguments()?;
    Some(
        Expression::Call {
            name,
            arguments,
        }
    )
}
    else {
        Some(Expression::Identifier(name))
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
