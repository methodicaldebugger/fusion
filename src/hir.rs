//contents of hir.rs

use crate::span::Span;

#[derive(Debug, Clone)]
pub struct HirProgram {
    pub statements: Vec<HirStatement>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirParameter {
    pub name: String,
    pub type_name: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirFunction {
    pub name: String,
    pub parameters: Vec<HirParameter>,
    pub return_type: Option<String>,
    pub body: Vec<HirStatement>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum HirStatement {
    VariableDeclaration {
        name: String,
        declared_type: Option<String>,
        value: HirExpression,
        span: Span,
    },

    ConstDeclaration {
        name: String,
        declared_type: Option<String>,
        value: HirExpression,
        span: Span,
    },

    Assignment {
        target: HirExpression,
        value: HirExpression,
        span: Span,
    },

    Function(HirFunction),

    Struct {
        name: String,
        fields: Vec<HirStructField>,
        span: Span,
    },

    Enum {
        name: String,
        variants: Vec<HirEnumVariant>,
        span: Span,
    },

    Expression {
        expression: HirExpression,
        span: Span,
    },

    Return {
        value: Option<HirExpression>,
        span: Span,
    },

    Break {
        span: Span,
    },

    Continue {
        span: Span,
    },

    If {
        condition: HirExpression,
        body: Vec<HirStatement>,
        else_body: Option<Vec<HirStatement>>,
        span: Span,
    },

    While {
        condition: HirExpression,
        body: Vec<HirStatement>,
        span: Span,
    },

    For {
        variable: String,
        start: HirExpression,
        end: HirExpression,
        body: Vec<HirStatement>,
        span: Span,
    },

    Match {
        expression: HirExpression,
        arms: Vec<HirMatchArm>,
        span: Span,
    },

    Defer {
        expression: HirExpression,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub struct HirStructField {
    pub name: String,
    pub type_name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirEnumVariant {
    pub name: String,
    pub fields: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirMatchArm {
    pub pattern: HirPattern,
    pub body: Vec<HirStatement>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum HirPattern {
    Wildcard,
    Identifier(String),
    Number(i64),
    Float(f64),
    String(String),
    Boolean(bool),

    Variant {
        enum_name: String,
        variant: String,
        bindings: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub enum HirExpression {
    Number(i64),
    Float(f64),
    Boolean(bool),
    String(String),

    Identifier(String),

    Array(Vec<HirExpression>),

    Index {
        array: Box<HirExpression>,
        index: Box<HirExpression>,
    },

    Property {
        object: Box<HirExpression>,
        name: String,
    },

    MethodCall {
        object: Box<HirExpression>,
        method: String,
        arguments: Vec<HirExpression>,
    },

    Call {
        name: String,
        arguments: Vec<HirExpression>,
    },

    StructConstructor {
        name: String,
        fields: Vec<(String, HirExpression)>,
    },

    EnumConstructor {
        enum_name: String,
        variant: String,
        arguments: Vec<HirExpression>,
    },

    Binary {
        left: Box<HirExpression>,
        operator: HirOperator,
        right: Box<HirExpression>,
    },

    Unary {
        operator: HirUnaryOperator,
        expression: Box<HirExpression>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirOperator {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirUnaryOperator {
    Negate,
    Not,
}