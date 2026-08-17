// contents of ast.rs
#[derive(Debug, Clone)]
pub struct Program {
    pub statements: Vec<Statement>,
}
#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub type_name: Option<String>,
}
#[derive(Debug, Clone)]
pub enum Statement {
    VariableDeclaration {
        name: String,
        declared_type: Option<String>,
        value: Expression,
    },
    ConstDeclaration {
        name: String,
        value: Expression,
    },
    Assignment {
        name: String,
        declared_type:Option<String>,
        value: Expression,
    },
    Function {
        name: String,
        parameters: Vec<Parameter>,
        return_type: Option<String>,
        body: Vec<Statement>,
    },
    If {
        condition: Expression,
        body: Vec<Statement>,
        else_body: Option<Vec<Statement>>,
    },
    While {
        condition: Expression,
        body: Vec<Statement>,
    },
    For {
        variable: String,
        start: Expression,
        end: Expression,
        body: Vec<Statement>,
    },
    Return(Expression),
    Break,
    Continue,
    Expression(Expression),
    Call(Expression),
}
#[derive(Debug, Clone)]
pub enum Expression {
    Number(i64),Float(f64),Boolean(bool),
    String(String),Array(Vec<Expression>),Identifier(String),

    Index {
        array: Box<Expression>,
        index: Box<Expression>,
    },
    Property {
        object: Box<Expression>,
        name:String,
    },
    MethodCall {
        object:Box<Expression>,
        method:String,
        arguments:Vec<Expression>,
    },
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
        name:String,
        arguments:Vec<Expression>,
    },
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Operator {
    Plus,Minus,Multiply,Divide,
    Equal,NotEqual,Less,LessEqual,Greater,GreaterEqual,And,Or,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOperator {
    Negate,Not,
}

//contents of environment.rs
use std::collections::HashMap;
use crate::value::Value;
#[derive(Clone)]
pub struct Environment {
    scopes: Vec<HashMap<String, Value>>,
}
impl Environment {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }
    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }
    pub fn set(
        &mut self,
        name: String,
        value: Value,
    ) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, value);
        }
    }
    pub fn get(
        &self,
        name: &str,
    ) -> Option<&Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Some(value);
            }
        }
        None
    }
}