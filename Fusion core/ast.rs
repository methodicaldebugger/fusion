// contents of ast.rs

/*
AST — represents the program as a tree
AST stands for Abstract Syntax Tree.

It is a data structure, not necessarily a separate processing stage.
The parser usually constructs it.
It understands operator precendece, multiplication has precedence over addition.
Real ASTs often also contain source locations, which help produce useful error messages.
*/

use crate::span::Span;
use crate::type_expr::TypeExpr;

#[derive(Debug, Clone)]
pub struct Program {
    pub statements: Vec<Statement>,
    pub span: Span,
    /// Project module metadata retained alongside the compatibility flattened AST.
    /// Ranges refer to `statements[start..end]`; standalone source has no module entries.
    pub modules: Vec<ModuleInfo>,
    /// Structured type syntax collected from legacy annotation fields. This is the
    /// migration bridge: consumers can use parsed trees without reparsing strings.
    pub type_annotations: Vec<StructuredTypeAnnotation>,
}


/// Identifies the role of a source-level type annotation in the AST.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeAnnotationRole {
    Parameter { owner: String, name: String },
    Return { owner: String },
    Variable { name: String },
    Constant { name: String },
    StructField { owner: String, name: String },
    EnumPayload { owner: String, variant: String, index: usize },
    TraitParameter { trait_name: String, method: String, name: String },
    TraitReturn { trait_name: String, method: String },
    ImplTarget { owner: String },
}

/// A structured type annotation retained in the AST with its source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredTypeAnnotation {
    pub role: TypeAnnotationRole,
    pub span: Span,
    pub expression: TypeExpr,
}

impl Program {
    /// Rebuild the structured annotation index from compatibility AST fields.
    /// Invalid spellings are skipped here because the parser/type checker report them
    /// through their normal diagnostics; successfully parsed source annotations are
    /// represented exactly once per AST occurrence.
    pub fn rebuild_type_annotations(&mut self) {
        let mut annotations = Vec::new();
        collect_statement_type_annotations(&self.statements, &mut annotations);
        self.type_annotations = annotations;
    }

    /// Find a structured annotation by its role and source span.
    pub fn type_annotation(
        &self,
        role: &TypeAnnotationRole,
        span: Span,
    ) -> Option<&StructuredTypeAnnotation> {
        self.type_annotations
            .iter()
            .find(|item| item.role == *role && item.span == span)
    }
}

fn push_type_annotation(
    output: &mut Vec<StructuredTypeAnnotation>,
    role: TypeAnnotationRole,
    span: Span,
    source: &str,
) {
    if let Ok(expression) = TypeExpr::parse(source) {
        output.push(StructuredTypeAnnotation {
            role,
            span,
            expression,
        });
    }
}

fn collect_statement_type_annotations(
    statements: &[Statement],
    output: &mut Vec<StructuredTypeAnnotation>,
) {
    for statement in statements {
        match statement {
            Statement::VariableDeclarations { declarations, .. } => {
                for declaration in declarations {
                    if let Some(ty) = declaration.declared_type.as_deref() {
                        push_type_annotation(
                            output,
                            TypeAnnotationRole::Variable {
                                name: declaration.name.clone(),
                            },
                            declaration.span,
                            ty,
                        );
                    }
                }
            }
            Statement::ConstDeclaration {
                name,
                declared_type,
                span,
                ..
            } => {
                if let Some(ty) = declared_type.as_deref() {
                    push_type_annotation(
                        output,
                        TypeAnnotationRole::Constant { name: name.clone() },
                        *span,
                        ty,
                    );
                }
            }
            Statement::Function {
                name,
                parameters,
                return_type,
                body,
                span,
                ..
            } => {
                for parameter in parameters {
                    if let Some(ty) = parameter.type_name.as_deref() {
                        push_type_annotation(
                            output,
                            TypeAnnotationRole::Parameter {
                                owner: name.clone(),
                                name: parameter.name.clone(),
                            },
                            parameter.span,
                            ty,
                        );
                    }
                }
                if let Some(ty) = return_type.as_deref() {
                    push_type_annotation(
                        output,
                        TypeAnnotationRole::Return { owner: name.clone() },
                        *span,
                        ty,
                    );
                }
                collect_statement_type_annotations(body, output);
            }
            Statement::Struct { name, fields, .. } => {
                for field in fields {
                    push_type_annotation(
                        output,
                        TypeAnnotationRole::StructField {
                            owner: name.clone(),
                            name: field.name.clone(),
                        },
                        field.span,
                        &field.type_name,
                    );
                }
            }
            Statement::Enum { name, variants, .. } => {
                for variant in variants {
                    for (index, ty) in variant.fields.iter().enumerate() {
                        push_type_annotation(
                            output,
                            TypeAnnotationRole::EnumPayload {
                                owner: name.clone(),
                                variant: variant.name.clone(),
                                index,
                            },
                            variant.span,
                            ty,
                        );
                    }
                }
            }
            Statement::Trait { name, methods, .. } => {
                for method in methods {
                    for parameter in &method.parameters {
                        if let Some(ty) = parameter.type_name.as_deref() {
                            push_type_annotation(
                                output,
                                TypeAnnotationRole::TraitParameter {
                                    trait_name: name.clone(),
                                    method: method.name.clone(),
                                    name: parameter.name.clone(),
                                },
                                parameter.span,
                                ty,
                            );
                        }
                    }
                    if let Some(ty) = method.return_type.as_deref() {
                        push_type_annotation(
                            output,
                            TypeAnnotationRole::TraitReturn {
                                trait_name: name.clone(),
                                method: method.name.clone(),
                            },
                            method.span,
                            ty,
                        );
                    }
                    if let Some(body) = &method.default_body {
                        collect_statement_type_annotations(body, output);
                    }
                }
            }
            Statement::Impl {
                type_name,
                methods,
                span,
                ..
            } => {
                push_type_annotation(
                    output,
                    TypeAnnotationRole::ImplTarget {
                        owner: type_name.clone(),
                    },
                    *span,
                    type_name,
                );
                collect_statement_type_annotations(methods, output);
            }
            Statement::Main { body, .. } => {
                collect_statement_type_annotations(body, output);
            }
            Statement::Match { arms, .. } => {
                for arm in arms {
                    collect_statement_type_annotations(&arm.body, output);
                }
            }
            Statement::If {
                body, else_body, ..
            } => {
                collect_statement_type_annotations(body, output);
                if let Some(body) = else_body {
                    collect_statement_type_annotations(body, output);
                }
            }
            Statement::While { body, .. }
            | Statement::For { body, .. }
            | Statement::ForEach { body, .. } => {
                collect_statement_type_annotations(body, output);
            }
            Statement::Assignment { .. }
            | Statement::Import { .. }
            | Statement::Defer { .. }
            | Statement::Return { .. }
            | Statement::Break { .. }
            | Statement::Continue { .. }
            | Statement::Expression { .. }
            | Statement::Call { .. } => {}
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModuleImportInfo {
    pub path: String,
    pub alias: Option<String>,
    /// Empty means the whole module is imported; otherwise these are selective imports.
    pub selected: Vec<String>,
    pub re_exported: bool,
}

#[derive(Debug, Clone)]
pub struct ModuleInfo {
    pub name: String,
    pub path: String,
    pub statement_start: usize,
    pub statement_end: usize,
    pub imports: Vec<ModuleImportInfo>,
    pub public_symbols: Vec<String>,
    pub private_symbols: Vec<String>,
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
    pub value: Option<Expression>,
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
    /// Body of a default method; `None` for a required method. Default bodies are copied into
    /// each implementing `impl` block by `trait_defaults::expand_trait_defaults`.
    pub default_body: Option<Vec<Statement>>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expression>,
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
        bindings: Vec<Pattern>,
    },
    Tuple(Vec<Pattern>),
    Struct {
        name: String,
        fields: Vec<(String, Pattern)>,
        has_rest: bool,
    },
    At {
        name: String,
        pattern: Box<Pattern>,
    },
    Or(Vec<Pattern>),
    Range {
        start: i64,
        end: i64,
        inclusive: bool,
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
        is_async: bool,
        /// Present for a declaration supplied by a foreign ABI rather than a Fusion body.
        foreign_abi: Option<String>,
        /// Native linker symbol; defaults to the source-level function name.
        link_name: Option<String>,
        span: Span,
    },

    Struct {
        name: String,
        fields: Vec<StructField>,
        derives: Vec<String>,
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

    Import {
        path: String,
        alias: Option<String>,
        selected: Vec<String>,
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

    ForEach {
        variable: String,
        iterable: Expression,
        body: Vec<Statement>,
        span: Span,
    },

    Return {
        value: Option<Expression>,
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

    /// Integer literal above `i64::MAX` (up to `u128::MAX`). Negative literals are
    /// written with unary minus, so the stored magnitude is never negative.
    WideNumber {
        value: u128,
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

    Tuple {
        elements: Vec<Expression>,
        span: Span,
    },

    Identifier {
        name: String,
        span: Span,
    },

    /// A closure expression backed by a private hoisted function and a by-value capture list.
    Closure {
        name: String,
        captures: Vec<String>,
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
        generic_arguments: Vec<String>,
        span: Span,
    },

    Await {
        expression: Box<Expression>,
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

    Conversion {
        kind: ConversionKind,
        expression: Box<Expression>,
        target_type: String,
        span: Span,
    },
}

impl Expression {
    pub fn span(&self) -> Span {
        match self {
            Expression::Number { span, .. }
            | Expression::WideNumber { span, .. }
            | Expression::Float { span, .. }
            | Expression::Boolean { span, .. }
            | Expression::String { span, .. }
            | Expression::Array { span, .. }
            | Expression::Tuple { span, .. }
            | Expression::Identifier { span, .. }
            | Expression::Closure { span, .. }
            | Expression::Index { span, .. }
            | Expression::Property { span, .. }
            | Expression::MethodCall { span, .. }
            | Expression::Await { span, .. }
            | Expression::Call { span, .. }
            | Expression::StructConstructor { span, .. }
            | Expression::EnumConstructor { span, .. }
            | Expression::Binary { span, .. }
            | Expression::Unary { span, .. }
            | Expression::Conversion { span, .. } => *span,
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
            | Statement::Import { span, .. }
            | Statement::Trait { span, .. }
            | Statement::Impl { span, .. }
            | Statement::Match { span, .. }
            | Statement::Defer { span, .. }
            | Statement::If { span, .. }
            | Statement::While { span, .. }
            | Statement::For { span, .. }
            | Statement::ForEach { span, .. }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionKind {
    As,
    From,
    Try,
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
    Deref,
}
