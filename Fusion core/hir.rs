//contents of hir.rs

/*
HIR — a higher-level intermediate representation
HIR stands for High-level Intermediate Representation.

It is another representation of the program,
usually designed to be easier for later compiler stages to analyze
and transform than the original AST.

Why have one?
Source code often contains convenient syntax
 that is complicated to implement repeatedly throughout the compiler.
 HIR can simplify that syntax while preserving the program's meaning.

 HIR can also include information that the AST lacks or leaves unresolved, such as:
- Resolved variable and function references.
- Unique IDs for declarations.
- Explicit type information.
- Desugared syntax.
- More uniform representations of expressions and control flow.

 */

use crate::span::Span;

#[derive(Debug, Clone)]
pub struct HirProgram {
    pub statements: Vec<HirStatement>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirProject {
    pub entry_module: String,
    pub modules: Vec<HirModule>,
    /// Resolved call sites, keyed by source module and span. Backend stages
    /// consume declaration identities rather than repeating name lookup.
    pub resolved_calls: Vec<HirResolvedCall>,
    /// Structural types inferred for parameters and local bindings.
    pub resolved_local_types: Vec<HirResolvedLocalType>,
    /// Backend-oriented control-flow bodies derived from the source-preserving HIR.
    pub bodies: Vec<HirBody>,
}

/// A function, method, or module entry body in a backend-oriented control-flow form.
/// Source-preserving HIR remains available alongside this representation during migration.
#[derive(Debug, Clone)]
pub struct HirBody {
    pub module: String,
    pub name: String,
    pub owner: Option<HirDefinitionId>,
    pub entry: u32,
    pub exit: u32,
    pub locals: Vec<HirBodyLocal>,
    pub blocks: Vec<HirBasicBlock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HirLocalId(pub u32);

#[derive(Debug, Clone)]
pub struct HirBodyLocal {
    pub id: HirLocalId,
    pub name: String,
    pub ty: HirType,
    pub declaration_span: Span,
    pub parameter: bool,
}

#[derive(Debug, Clone)]
pub struct HirBasicBlock {
    pub id: u32,
    pub operations: Vec<HirBodyOperation>,
    pub terminator: HirTerminator,
    pub terminator_expression_types: Vec<HirTypedExpression>,
    pub terminator_local_references: Vec<HirLocalReference>,
    pub terminator_call_references: Vec<HirCallReference>,
}

impl HirBasicBlock {
    /// Successor block IDs in deterministic source order.
    pub fn successors(&self) -> Vec<u32> {
        match &self.terminator {
            HirTerminator::Open | HirTerminator::Return(_) | HirTerminator::Unreachable => {
                Vec::new()
            }
            HirTerminator::Goto(target) => vec![*target],
            HirTerminator::Branch {
                then_block,
                else_block,
                ..
            } => vec![*then_block, *else_block],
            HirTerminator::Match { arms, fallback, .. } => arms
                .iter()
                .map(|arm| arm.block)
                .chain(std::iter::once(*fallback))
                .collect(),
            HirTerminator::For { body, exit, .. } | HirTerminator::ForEach { body, exit, .. } => {
                vec![*body, *exit]
            }
            HirTerminator::Break(target) | HirTerminator::Continue(target) => {
                target.iter().copied().collect()
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct HirBodyOperation {
    /// The non-control-flow HIR operation. Control flow is represented by the
    /// block terminator and successor blocks instead of nested statement lists.
    pub statement: HirStatement,
    pub expression_types: Vec<HirTypedExpression>,
    pub local_references: Vec<HirLocalReference>,
    pub call_references: Vec<HirCallReference>,
}

#[derive(Debug, Clone)]
pub struct HirTypedExpression {
    /// Child-index path from the operation's expression roots to this expression.
    /// The first index selects the root expression in the operation.
    pub path: Vec<u32>,
    pub ty: HirType,
}

#[derive(Debug, Clone)]
pub struct HirLocalReference {
    pub path: Vec<u32>,
    pub local: HirLocalId,
}

/// Resolved call attached to an expression-tree path in a backend HIR body.
/// The declaration identity and structural signature are authoritative; the
/// source spelling on the expression is retained only for diagnostics.
#[derive(Debug, Clone)]
pub struct HirCallReference {
    pub path: Vec<u32>,
    pub call: HirResolvedCall,
}

#[derive(Debug, Clone)]
pub struct HirMatchTarget {
    pub pattern: HirPattern,
    /// Lexical locals introduced by this pattern. Paths are relative to the
    /// matched value; an empty path binds the whole scrutinee.
    pub bindings: Vec<HirPatternBinding>,
    /// If false, dispatch continues to the next ordered arm before fallback.
    pub guard: Option<HirExpression>,
    pub guard_expression_types: Vec<HirTypedExpression>,
    pub guard_local_references: Vec<HirLocalReference>,
    pub guard_call_references: Vec<HirCallReference>,
    pub block: u32,
}

#[derive(Debug, Clone)]
pub struct HirPatternBinding {
    pub name: String,
    pub local: HirLocalId,
    pub path: Vec<u32>,
}

#[derive(Debug, Clone)]
pub enum HirTerminator {
    Open,
    Goto(u32),
    Branch {
        condition: HirExpression,
        then_block: u32,
        else_block: u32,
    },
    Match {
        value: HirExpression,
        arms: Vec<HirMatchTarget>,
        fallback: u32,
    },
    For {
        variable: String,
        local: HirLocalId,
        start: HirExpression,
        end: HirExpression,
        body: u32,
        exit: u32,
    },
    ForEach {
        variable: String,
        local: HirLocalId,
        iterable: HirExpression,
        body: u32,
        exit: u32,
    },
    Return(Option<HirExpression>),
    Break(Option<u32>),
    Continue(Option<u32>),
    Unreachable,
}

#[derive(Debug, Clone)]
pub struct HirModule {
    pub name: String,
    pub path: String,
    pub imports: Vec<HirImport>,
    pub public_symbols: Vec<String>,
    pub local_public_symbols: Vec<String>,
    pub private_symbols: Vec<String>,
    pub statements: Vec<HirStatement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HirCallKind {
    Function,
    Method,
}

/// Fully resolved call-site information. Source spelling is retained only for
/// diagnostics; `declaration` is the authoritative callee identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirResolvedCall {
    pub module: String,
    pub span: Span,
    pub kind: HirCallKind,
    pub declaration: HirDefinitionId,
    pub receiver_type: Option<HirDefinitionId>,
    pub parameter_types: Vec<Option<HirDefinitionId>>,
    pub argument_types: Vec<Option<HirDefinitionId>>,
    pub return_type: Option<HirDefinitionId>,
    pub generic_parameters: Vec<String>,
    pub is_async: bool,
    /// Complete structural signature types. The legacy nominal-only fields above
    /// remain during migration and are derived from these where possible.
    pub receiver_hir_type: Option<HirType>,
    pub parameter_hir_types: Vec<HirType>,
    pub argument_hir_types: Vec<HirType>,
    pub return_hir_type: HirType,
}

/// Structural type assigned to a parameter or local declaration. The source span
/// identifies the binding site, while `ty` preserves primitives and generic shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirResolvedLocalType {
    pub module: String,
    pub name: String,
    pub span: Span,
    pub ty: HirType,
}

/// Resolved HIR type algebra. Unlike source type strings, this representation
/// preserves primitive, container, tuple, generic and nominal distinctions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirType {
    Num,
    Float,
    Bool,
    String,
    Void,
    File,
    TcpStream,
    Array(Box<HirType>),
    Tuple(Vec<HirType>),
    Iterator(Box<HirType>),
    HashMap(Box<HirType>, Box<HirType>),
    Option(Box<HirType>),
    Result(Box<HirType>, Box<HirType>),
    Task(Box<HirType>),
    Nominal(HirDefinitionId),
    Applied {
        constructor: Box<HirType>,
        arguments: Vec<HirType>,
    },
    Generic(String),
    Unknown,
}

#[derive(Debug, Clone)]
pub struct HirImport {
    pub path: String,
    pub alias: Option<String>,
    pub selected: Vec<String>,
    pub re_exported: bool,
}

#[derive(Debug, Clone)]
pub struct HirParameter {
    pub name: String,
    pub type_name: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirFunction {
    pub identity: Option<HirDefinitionId>,
    pub name: String,
    pub generic_parameters: Vec<String>,
    pub parameters: Vec<HirParameter>,
    pub return_type: Option<String>,
    pub is_async: bool,
    pub body: Vec<HirStatement>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirTraitMethod {
    pub identity: Option<HirDefinitionId>,
    pub name: String,
    pub parameters: Vec<HirParameter>,
    pub return_type: Option<String>,
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

    UninitializedVariable {
        name: String,
        declared_type: Option<String>,
        span: Span,
    },

    ConstDeclaration {
        identity: Option<HirDefinitionId>,
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

    Block {
        statements: Vec<HirStatement>,
        span: Span,
    },

    Function(HirFunction),

    Trait {
        identity: Option<HirDefinitionId>,
        name: String,
        methods: Vec<HirTraitMethod>,
        span: Span,
    },

    Impl {
        trait_name: Option<String>,
        type_name: String,
        methods: Vec<HirFunction>,
        span: Span,
    },

    Import {
        path: String,
        alias: Option<String>,
        selected: Vec<String>,
        re_exported: bool,
        span: Span,
    },

    Main {
        body: Vec<HirStatement>,
        span: Span,
    },

    Struct {
        identity: Option<HirDefinitionId>,
        name: String,
        fields: Vec<HirStructField>,
        derives: Vec<String>,
        span: Span,
    },

    Enum {
        identity: Option<HirDefinitionId>,
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

    ForEach {
        variable: String,
        iterable: HirExpression,
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
    pub identity: Option<HirDefinitionId>,
    pub name: String,
    pub type_name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirEnumVariant {
    pub identity: Option<HirDefinitionId>,
    pub name: String,
    pub fields: Vec<String>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct HirMatchArm {
    pub pattern: HirPattern,
    pub guard: Option<HirExpression>,
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
        name: String,
        bindings: Vec<HirPattern>,
    },
    Tuple(Vec<HirPattern>),
    Struct {
        name: String,
        fields: Vec<(String, HirPattern)>,
        has_rest: bool,
    },
    At {
        name: String,
        pattern: Box<HirPattern>,
    },
    Or(Vec<HirPattern>),
    Range {
        start: i64,
        end: i64,
        inclusive: bool,
    },
}

#[derive(Debug, Clone)]
pub enum HirExpression {
    Number(i64),
    WideNumber(u128),
    Float(f64),
    Boolean(bool),
    String(String),

    Identifier(String),

    Array(Vec<HirExpression>),

    /// Heterogeneous, fixed-length tuple value. Element order and types are preserved.
    Tuple(Vec<HirExpression>),

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
        generic_arguments: Vec<String>,
        span: Span,
    },

    Call {
        name: String,
        arguments: Vec<HirExpression>,
        generic_arguments: Vec<String>,
        span: Span,
    },

    Await(Box<HirExpression>),

    Conversion {
        kind: HirConversionKind,
        expression: Box<HirExpression>,
        target_type: String,
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
pub enum HirConversionKind {
    As,
    From,
    Try,
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
    Deref,
}

/// Stable identity for a declaration before backend-specific name mangling.
/// The local spelling is intentionally not globally unique: two modules may
/// both declare `Point` without referring to the same type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HirDefinitionId {
    pub module: String,
    pub scope: Vec<String>,
    pub name: String,
    pub kind: HirDefinitionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HirDefinitionKind {
    Function,
    Struct,
    Enum,
    Trait,
    Constant,
    Method,
    Field,
    Variant,
}

impl HirDefinitionId {
    pub fn new(
        module: impl Into<String>,
        name: impl Into<String>,
        kind: HirDefinitionKind,
    ) -> Self {
        Self {
            module: module.into(),
            scope: Vec::new(),
            name: name.into(),
            kind,
        }
    }

    pub fn scoped(
        module: impl Into<String>,
        scope: impl IntoIterator<Item = String>,
        name: impl Into<String>,
        kind: HirDefinitionKind,
    ) -> Self {
        Self {
            module: module.into(),
            scope: scope.into_iter().collect(),
            name: name.into(),
            kind,
        }
    }
}

/// A compiler-independent declaration index for the HIR/module-resolution
/// transition. Resolution should carry these IDs; source spellings are only
/// used for lookup and diagnostics, never as global symbol identity.
#[derive(Debug, Clone, Default)]
pub struct HirSymbolIndex {
    pub definitions: std::collections::BTreeSet<HirDefinitionId>,
}

impl HirSymbolIndex {
    pub fn insert(&mut self, id: HirDefinitionId) -> bool {
        self.definitions.insert(id)
    }

    pub fn contains(&self, id: &HirDefinitionId) -> bool {
        self.definitions.contains(id)
    }
}

/// Source-level type declaration metadata, kept separate from the legacy
/// name-keyed type tables. This registry can represent same-named types in
/// different modules without making either declaration overwrite the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirTypeShape {
    Struct {
        fields: Vec<HirTypeField>,
        derives: Vec<String>,
    },
    Enum {
        variants: Vec<HirTypeVariant>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirTypeField {
    pub name: String,
    /// Source type spelling; this becomes a `HirDefinitionId` during type-name
    /// resolution rather than being guessed while the module graph is loaded.
    pub type_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirTypeVariant {
    pub name: String,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirTypeDefinition {
    pub id: HirDefinitionId,
    pub is_public: bool,
    pub shape: HirTypeShape,
}

#[derive(Debug, Clone, Default)]
pub struct HirTypeRegistry {
    pub definitions: std::collections::BTreeMap<HirDefinitionId, HirTypeDefinition>,
}

impl HirTypeRegistry {
    pub fn insert(&mut self, definition: HirTypeDefinition) -> bool {
        self.definitions
            .insert(definition.id.clone(), definition)
            .is_none()
    }

    pub fn get(&self, id: &HirDefinitionId) -> Option<&HirTypeDefinition> {
        self.definitions.get(id)
    }

    pub fn find_local(
        &self,
        module: &str,
        name: &str,
        kind: HirDefinitionKind,
    ) -> Option<&HirTypeDefinition> {
        self.definitions.values().find(|definition| {
            definition.id.module == module
                && definition.id.name == name
                && definition.id.kind == kind
        })
    }
}

#[cfg(test)]
mod module_identity_tests {
    use super::*;

    #[test]
    fn same_type_name_in_distinct_modules_has_distinct_identity() {
        let geometry = HirDefinitionId::new("geometry", "Point", HirDefinitionKind::Struct);
        let graphics = HirDefinitionId::new("graphics", "Point", HirDefinitionKind::Struct);
        assert_ne!(geometry, graphics);
        let mut index = HirSymbolIndex::default();
        assert!(index.insert(geometry.clone()));
        assert!(index.insert(graphics.clone()));
        assert_eq!(index.definitions.len(), 2);
        assert!(!index.insert(geometry));
    }

    #[test]
    fn implementation_methods_are_scoped_to_their_impl_target() {
        let point = HirDefinitionId::scoped(
            "geometry",
            ["Point".to_string()],
            "distance",
            HirDefinitionKind::Method,
        );
        let vector = HirDefinitionId::scoped(
            "geometry",
            ["Vector".to_string()],
            "distance",
            HirDefinitionKind::Method,
        );
        assert_ne!(point, vector);
    }
}

#[cfg(test)]
mod type_registry_tests {
    use super::*;

    #[test]
    fn type_registry_keeps_same_spelling_in_distinct_modules() {
        let mut registry = HirTypeRegistry::default();
        let geometry = HirDefinitionId::new("geometry", "Point", HirDefinitionKind::Struct);
        let graphics = HirDefinitionId::new("graphics", "Point", HirDefinitionKind::Struct);
        assert!(registry.insert(HirTypeDefinition {
            id: geometry.clone(),
            is_public: true,
            shape: HirTypeShape::Struct {
                fields: vec![HirTypeField {
                    name: "x".into(),
                    type_name: "num".into()
                }],
                derives: vec!["Debug".into()],
            },
        }));
        assert!(registry.insert(HirTypeDefinition {
            id: graphics.clone(),
            is_public: false,
            shape: HirTypeShape::Struct {
                fields: vec![HirTypeField {
                    name: "rgba".into(),
                    type_name: "u32".into()
                }],
                derives: Vec::new(),
            },
        }));
        assert_eq!(registry.definitions.len(), 2);
        assert_eq!(
            registry.get(&geometry).unwrap().shape,
            HirTypeShape::Struct {
                fields: vec![HirTypeField {
                    name: "x".into(),
                    type_name: "num".into()
                }],
                derives: vec!["Debug".into()],
            }
        );
        assert!(!registry.get(&graphics).unwrap().is_public);
    }
}
