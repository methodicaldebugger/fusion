//contents of ast.rs

use crate::span::Span;

#[derive(Debug, Clone)]
pub struct Program {
    pub statements: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub name_span: Span,
    pub type_name: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct VariableDeclaration {
    pub name: String,
    pub name_span: Span,
    pub declared_type: Option<String>,
    pub value: Expression,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub name: String,
    pub name_span: Span,
    pub type_name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumVariant {
    pub name: String,
    pub name_span: Span,
    pub fields: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct TraitMethod {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Pattern {
    pub kind: PatternKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum PatternKind {
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
        span: Span,
    },

    ConstDeclaration {
        name: String,
        name_span: Span,
        declared_type: Option<String>,
        value: Expression,
        span: Span,
    },

    Assignment {
        target: Expression,
        value: Expression,
        span: Span,
    },

    Function {
        name: String,
        generic_parameters: Vec<String>,
        parameters: Vec<Parameter>,
        return_type: Option<String>,
        body: Vec<Statement>,
        span: Span,
    },

    Struct {
        name: String,
        fields: Vec<StructField>,
        span: Span,
    },

    Enum {
        name: String,
        variants: Vec<EnumVariant>,
        span: Span,
    },

    Main {
        body: Vec<Statement>,
        span: Span,
    },

    Trait {
        name: String,
        methods: Vec<TraitMethod>,
        span: Span,
    },

    Impl {
        trait_name: Option<String>,
        type_name: String,
        methods: Vec<Statement>,
        span: Span,
    },

    Match {
        expression: Expression,
        arms: Vec<MatchArm>,
        span: Span,
    },

    Defer {
        expression: Expression,
        span: Span,
    },

    If {
        condition: Expression,
        body: Vec<Statement>,
        else_body: Option<Vec<Statement>>,
        span: Span,
    },

    While {
        condition: Expression,
        body: Vec<Statement>,
        span: Span,
    },

    For {
        variable: String,
        start: Expression,
        end: Expression,
        body: Vec<Statement>,
        span: Span,
    },

    Return {
        value: Expression,
        span: Span,
    },

    Break {
        span: Span,
    },

    Continue {
        span: Span,
    },

    Expression {
        expression: Expression,
        span: Span,
    },

    Call {
        expression: Expression,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub enum Expression {
    Number {
        value: i64,
        span: Span,
    },

    Float {
        value: f64,
        span: Span,
    },

    Boolean {
        value: bool,
        span: Span,
    },

    String {
        value: String,
        span: Span,
    },

    Array {
        elements: Vec<Expression>,
        span: Span,
    },

    Identifier {
        name: String,
        span: Span,
    },

    Index {
        array: Box<Expression>,
        index: Box<Expression>,
        span: Span,
    },

    Property {
        object: Box<Expression>,
        name: String,
        span: Span,
    },

    MethodCall {
        object: Box<Expression>,
        method: String,
        arguments: Vec<Expression>,
        span: Span,
    },

    Call {
        name: String,
        arguments: Vec<Expression>,
        generic_arguments: Vec<String>,
        span: Span,
    },

    StructConstructor {
        name: String,
        fields: Vec<(String, Expression)>,
        span: Span,
    },

    EnumConstructor {
        enum_name: String,
        variant: String,
        arguments: Vec<Expression>,
        span: Span,
    },

    Binary {
        left: Box<Expression>,
        operator: Operator,
        right: Box<Expression>,
        span: Span,
    },

    Unary {
        operator: UnaryOperator,
        expression: Box<Expression>,
        span: Span,
    },
}

impl Expression {
    pub fn span(&self) -> Span {
        match self {
            Expression::Number { span, .. }
            | Expression::Float { span, .. }
            | Expression::Boolean { span, .. }
            | Expression::String { span, .. }
            | Expression::Array { span, .. }
            | Expression::Identifier { span, .. }
            | Expression::Index { span, .. }
            | Expression::Property { span, .. }
            | Expression::MethodCall { span, .. }
            | Expression::Call { span, .. }
            | Expression::StructConstructor { span, .. }
            | Expression::EnumConstructor { span, .. }
            | Expression::Binary { span, .. }
            | Expression::Unary { span, .. } => *span,
        }
    }
}

impl Statement {
    pub fn span(&self) -> Span {
        match self {
            Statement::VariableDeclarations { span, .. }
            | Statement::ConstDeclaration { span, .. }
            | Statement::Assignment { span, .. }
            | Statement::Function { span, .. }
            | Statement::Struct { span, .. }
            | Statement::Enum { span, .. }
            | Statement::Main { span, .. }
            | Statement::Trait { span, .. }
            | Statement::Impl { span, .. }
            | Statement::Match { span, .. }
            | Statement::Defer { span, .. }
            | Statement::If { span, .. }
            | Statement::While { span, .. }
            | Statement::For { span, .. }
            | Statement::Return { span, .. }
            | Statement::Break { span, .. }
            | Statement::Continue { span, .. }
            | Statement::Expression { span, .. }
            | Statement::Call { span, .. } => *span,
        }
    }
}

impl Pattern {
    pub fn span(&self) -> Span {
        self.span
    }
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