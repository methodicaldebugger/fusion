#[derive(Debug, Clone, PartialEq)]
pub enum Expression {

    Integer(i64),

    Float(f64),

    String(String),

    Boolean(bool),

    Identifier(String),

    Binary {
        left: Box<Expression>,
        operator: Operator,
        right: Box<Expression>,
    },

    Unary {
        operator: UnaryOperator,
        expression: Box<Expression>,
    },

    Call {
    name: String,
    arguments: Vec<Expression>,
},
}


#[derive(Debug, Clone, PartialEq)]
pub enum Operator {

    Plus,
    Minus,

    Multiply,
    Divide,

    Equal,
    NotEqual,

    Less,
    LessEqual,

    Greater,
    GreaterEqual,
}


#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOperator {
    Negate,
    Not,
}


#[derive(Debug, Clone, PartialEq)]
pub enum Statement {

    
    Assignment {
        name: String,
        value: Expression,
    },

    Call(Expression),

    Return(Expression),

    If {
    condition: Expression,
    body: Vec<Statement>,
    else_body: Vec<Statement>,
},

    Function {
        name: String,
        parameters: Vec<String>,
        return_type: Option<String>,
        body: Vec<Statement>,
    },
}


#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub type_name: Option<String>,
}