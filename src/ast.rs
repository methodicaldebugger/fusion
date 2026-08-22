//contents of ast.rs

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
pub struct VariableDeclaration {
    pub name: String,
    pub declared_type: Option<String>,
    pub value: Expression,
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone)]
pub enum Pattern {
    Wildcard,
    Identifier(String),
    Number(i64),
    Float(f64),
    String(String),
    Boolean(bool),

    Variant {
        name: String,
        bindings: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub enum Statement {

    VariableDeclarations {
        declarations: Vec<VariableDeclaration>,
    },

    ConstDeclaration {
        name: String,
        declared_type: Option<String>,
        value: Expression,
    },

    Assignment {
    target: Expression,
    value: Expression,
    },

    Function {
        name: String,
        generic_parameters: Vec<String>,
        parameters: Vec<Parameter>,
        return_type: Option<String>,
        body: Vec<Statement>,
    },

    Struct {
        name: String,
        fields: Vec<StructField>,
    },

    Enum {
        name: String,
        variants: Vec<EnumVariant>,
    },

    Trait {
        name: String,
        methods: Vec<TraitMethod>,
    },

    Impl {
        trait_name: Option<String>,
        type_name: String,
        methods: Vec<Statement>,
    },

    Match {
        expression: Expression,
        arms: Vec<MatchArm>,
    },

    Defer(Expression),

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
    Number(i64),
    Float(f64),
    Boolean(bool),
    String(String),
    Array(Vec<Expression>),
    Identifier(String),

    Index {
        array: Box<Expression>,
        index: Box<Expression>,
    },

    Property {
        object: Box<Expression>,
        name: String,
    },

    MethodCall {
        object: Box<Expression>,
        method: String,
        arguments: Vec<Expression>,
    },

    Call {
        name: String,
        arguments: Vec<Expression>,
        generic_arguments: Vec<String>,
    },

    StructConstructor {
        name: String,
        fields: Vec<(String, Expression)>,
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
}

#[derive(Debug, Clone, Copy, PartialEq)]
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
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOperator {
    Negate,
    Not,
}