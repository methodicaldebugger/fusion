// contents of name_resolution.rs

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DefinitionId(pub u32);

#[derive(Debug, Clone)]
pub struct Definition {
    pub id: DefinitionId,
    pub name: String,
    pub span: Span,
    pub kind: DefinitionKind,
}

pub enum DefinitionKind {
    Variable,
    Constant,
    Function,
    Parameter,
    Struct,
    Enum,
    EnumVariant,
}

pub struct Scope {
    pub bindings: HashMap<String, DefinitionId>,
}

pub struct Resolver {
    scopes: Vec<Scope>,
    definitions: Vec<Definition>,
}