
use std::collections::HashMap;
use crate::ast::{Program, Statement, Expression};
use crate::span::Span;
use crate::errors::FusionError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DefinitionId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefinitionKind {
    Variable,
    Constant,
    Function,
    Parameter,
    Struct,
    Enum,
    EnumVariant,
}

#[derive(Debug, Clone)]
pub struct Definition {
    pub id: DefinitionId,
    pub name: String,
    pub span: Span,
    pub kind: DefinitionKind,
}

#[derive(Debug, Default, Clone)]
pub struct Scope {
    pub bindings: HashMap<String, DefinitionId>,
}

#[derive(Debug, Default)]
pub struct Resolver {
    scopes: Vec<Scope>,
    definitions: Vec<Definition>,
}

impl Resolver {
    pub fn new() -> Self {
        Self { scopes: vec![Scope::default()], definitions: Vec::new() }
    }

    fn push(&mut self) { self.scopes.push(Scope::default()); }
    fn pop(&mut self) { if self.scopes.len() > 1 { self.scopes.pop(); } }

    fn define(&mut self, name: &str, span: Span, kind: DefinitionKind) -> Result<DefinitionId, FusionError> {
        if self.scopes.last().unwrap().bindings.contains_key(name) {
            return Err(FusionError::Syntax {
                message: format!("duplicate definition '{}'", name),
                span,
            });
        }
        let id = DefinitionId(self.definitions.len() as u32);
        self.definitions.push(Definition {
            id, name: name.to_string(), span, kind,
        });
        self.scopes.last_mut().unwrap().bindings.insert(name.to_string(), id);
        Ok(id)
    }

    fn lookup(&self, name: &str) -> Option<DefinitionId> {
        self.scopes.iter().rev().find_map(|s| s.bindings.get(name).copied())
    }

    pub fn resolve(&mut self, program: &Program) -> Result<Vec<Definition>, FusionError> {
        self.scopes.clear();
        self.scopes.push(Scope::default());
        self.definitions.clear();

        // Collect top-level definitions first so forward references work.
        for stmt in &program.statements {
            match stmt {
                Statement::Function { name, span, .. } =>
                    { self.define(name, *span, DefinitionKind::Function)?; }
                Statement::Struct { name, span, .. } =>
                    { self.define(name, *span, DefinitionKind::Struct)?; }
                Statement::Enum { name, span, .. } =>
                    { self.define(name, *span, DefinitionKind::Enum)?; }
                _ => {}
            }
        }

        for stmt in &program.statements {
            self.resolve_statement(stmt)?;
        }
        Ok(self.definitions.clone())
    }

    fn resolve_statement(&mut self, stmt: &Statement) -> Result<(), FusionError> {
        match stmt {
            Statement::Function { parameters, body, .. } => {
                self.push();
                for p in parameters {
                    self.define(&p.name, p.name_span, DefinitionKind::Parameter)?;
                }
                for s in body { self.resolve_statement(s)?; }
                self.pop();
            }
            Statement::Main { body, .. } => {
                self.push();
                for s in body { self.resolve_statement(s)?; }
                self.pop();
            }
            Statement::VariableDeclarations { declarations, .. } => {
                for d in declarations {
                    if let Some(v) = &d.value { self.resolve_expression(v)?; }
                    self.define(&d.name, d.name_span, DefinitionKind::Variable)?;
                }
            }
            Statement::ConstDeclaration { name, name_span, value, .. } => {
                self.resolve_expression(value)?;
                self.define(name, *name_span, DefinitionKind::Constant)?;
            }
            Statement::Assignment { target, value, .. } => {
                self.resolve_expression(target)?;
                self.resolve_expression(value)?;
            }
            Statement::Expression { expression, .. } | Statement::Call { expression, .. } =>
                self.resolve_expression(expression)?,
            Statement::If { condition, body, else_body, .. } => {
                self.resolve_expression(condition)?;
                self.push(); for s in body { self.resolve_statement(s)?; } self.pop();
                if let Some(body) = else_body { self.push(); for s in body { self.resolve_statement(s)?; } self.pop(); }
            }
            Statement::While { condition, body, .. } => {
                self.resolve_expression(condition)?;
                self.push(); for s in body { self.resolve_statement(s)?; } self.pop();
            }
            Statement::For { start, end, body, variable, span } => {
                self.resolve_expression(start)?; self.resolve_expression(end)?;
                self.push();
                self.define(variable, *span, DefinitionKind::Variable)?;
                for s in body { self.resolve_statement(s)?; }
                self.pop();
            }
            Statement::ForEach { iterable, body, variable, span } => {
                self.resolve_expression(iterable)?;
                self.push();
                self.define(variable, *span, DefinitionKind::Variable)?;
                for s in body { self.resolve_statement(s)?; }
                self.pop();
            }
            Statement::Match { expression, arms, .. } => {
                self.resolve_expression(expression)?;
                for arm in arms {
                    self.push();
                    if let crate::ast::PatternKind::Identifier(n) = &arm.pattern.kind {
                        self.define(n, arm.pattern.span, DefinitionKind::Variable)?;
                    }
                    for s in &arm.body { self.resolve_statement(s)?; }
                    self.pop();
                }
            }
            Statement::Defer { expression, .. } | Statement::Return { value: Some(expression), .. } =>
                self.resolve_expression(expression)?,
            Statement::Return { value: None, .. } |
            Statement::Break { .. } | Statement::Continue { .. } |
            Statement::Struct { .. } | Statement::Enum { .. } |
            Statement::Trait { .. } | Statement::Impl { .. } => {}
        }
        Ok(())
    }

    fn resolve_expression(&mut self, expr: &Expression) -> Result<(), FusionError> {
        match expr {
            Expression::Identifier { name, span } => {
                if self.lookup(name).is_none() && name != "print" {
                    return Err(FusionError::UnknownVariable { name: name.clone(), span: *span });
                }
            }
            Expression::Array { elements, .. } => for e in elements { self.resolve_expression(e)?; },
            Expression::Index { array, index, .. } => { self.resolve_expression(array)?; self.resolve_expression(index)?; }
            Expression::Property { object, .. } => self.resolve_expression(object)?,
            Expression::MethodCall { object, arguments, .. } => {
                self.resolve_expression(object)?; for a in arguments { self.resolve_expression(a)?; }
            }
            Expression::Await { expression, .. } => self.resolve_expression(expression)?,
            Expression::Call { arguments, .. } => for a in arguments { self.resolve_expression(a)?; },
            Expression::StructConstructor { fields, .. } => for (_, e) in fields { self.resolve_expression(e)?; },
            Expression::EnumConstructor { arguments, .. } => for e in arguments { self.resolve_expression(e)?; },
            Expression::Binary { left, right, .. } => { self.resolve_expression(left)?; self.resolve_expression(right)?; }
            Expression::Unary { expression, .. } => self.resolve_expression(expression)?,
            Expression::Number { .. } | Expression::Float { .. } |
            Expression::Boolean { .. } | Expression::String { .. } => {}
        }
        Ok(())
    }
}
