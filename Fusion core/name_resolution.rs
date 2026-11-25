use crate::ast::{Expression, Program, Statement};
use crate::errors::FusionError;
use crate::span::Span;
use std::collections::HashMap;

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
        Self {
            scopes: vec![Scope::default()],
            definitions: Vec::new(),
        }
    }

    fn push(&mut self) {
        self.scopes.push(Scope::default());
    }
    fn pop(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    fn define_pattern_bindings(
        &mut self,
        pattern: &crate::ast::Pattern,
    ) -> Result<(), FusionError> {
        use crate::ast::PatternKind;
        match &pattern.kind {
            PatternKind::Identifier(name) => {
                self.define(name, pattern.span, DefinitionKind::Variable)?;
            }
            PatternKind::At {
                name,
                pattern: nested,
            } => {
                self.define(name, pattern.span, DefinitionKind::Variable)?;
                self.define_pattern_bindings(nested)?;
            }
            PatternKind::Tuple(items) => {
                for item in items {
                    self.define_pattern_bindings(item)?;
                }
            }
            PatternKind::Struct { fields, .. } => {
                for (_, item) in fields {
                    self.define_pattern_bindings(item)?;
                }
            }
            PatternKind::Variant { bindings, .. } => {
                for nested in bindings {
                    self.define_pattern_bindings(nested)?;
                }
            }
            PatternKind::Or(items) => {
                if let Some(first) = items.first() {
                    self.define_pattern_bindings(first)?;
                }
            }
            PatternKind::Wildcard
            | PatternKind::Number(_)
            | PatternKind::Float(_)
            | PatternKind::String(_)
            | PatternKind::Boolean(_)
            | PatternKind::Range { .. } => {}
        }
        Ok(())
    }

    fn define(
        &mut self,
        name: &str,
        span: Span,
        kind: DefinitionKind,
    ) -> Result<DefinitionId, FusionError> {
        if self.scopes.last().unwrap().bindings.contains_key(name) {
            return Err(FusionError::Syntax {
                message: format!("duplicate definition '{}'", name),
                span,
            });
        }
        let id = DefinitionId(self.definitions.len() as u32);
        self.definitions.push(Definition {
            id,
            name: name.to_string(),
            span,
            kind,
        });
        self.scopes
            .last_mut()
            .unwrap()
            .bindings
            .insert(name.to_string(), id);
        Ok(id)
    }

    fn lookup(&self, name: &str) -> Option<DefinitionId> {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.bindings.get(name).copied())
    }

    pub fn resolve(&mut self, program: &Program) -> Result<Vec<Definition>, FusionError> {
        self.scopes.clear();
        self.scopes.push(Scope::default());
        self.definitions.clear();

        // Collect top-level definitions first so forward references work.
        for stmt in &program.statements {
            match stmt {
                Statement::Function { name, span, .. } => {
                    self.define(name, *span, DefinitionKind::Function)?;
                }
                Statement::Struct { name, span, .. } => {
                    self.define(name, *span, DefinitionKind::Struct)?;
                }
                Statement::Enum { name, span, .. } => {
                    self.define(name, *span, DefinitionKind::Enum)?;
                }
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
            Statement::Function {
                parameters, body, ..
            } => {
                self.push();
                for p in parameters {
                    self.define(&p.name, p.name_span, DefinitionKind::Parameter)?;
                }
                for s in body {
                    self.resolve_statement(s)?;
                }
                self.pop();
            }
            Statement::Main { body, .. } => {
                self.push();
                for s in body {
                    self.resolve_statement(s)?;
                }
                self.pop();
            }
            Statement::VariableDeclarations { declarations, .. } => {
                for d in declarations {
                    if let Some(v) = &d.value {
                        self.resolve_expression(v)?;
                    }
                    self.define(&d.name, d.name_span, DefinitionKind::Variable)?;
                }
            }
            Statement::ConstDeclaration {
                name,
                name_span,
                value,
                ..
            } => {
                self.resolve_expression(value)?;
                self.define(name, *name_span, DefinitionKind::Constant)?;
            }
            Statement::Assignment { target, value, .. } => match target {
                Expression::Identifier { name, span } => {
                    if self.lookup(name).is_none() {
                        self.resolve_expression(value)?;
                        self.define(name, *span, DefinitionKind::Variable)?;
                    } else {
                        self.resolve_expression(value)?;
                    }
                }
                _ => {
                    self.resolve_expression(target)?;
                    self.resolve_expression(value)?;
                }
            },
            Statement::Expression { expression, .. } | Statement::Call { expression, .. } => {
                self.resolve_expression(expression)?
            }
            Statement::If {
                condition,
                body,
                else_body,
                ..
            } => {
                self.resolve_expression(condition)?;
                self.push();
                for s in body {
                    self.resolve_statement(s)?;
                }
                self.pop();
                if let Some(body) = else_body {
                    self.push();
                    for s in body {
                        self.resolve_statement(s)?;
                    }
                    self.pop();
                }
            }
            Statement::While {
                condition, body, ..
            } => {
                self.resolve_expression(condition)?;
                self.push();
                for s in body {
                    self.resolve_statement(s)?;
                }
                self.pop();
            }
            Statement::For {
                start,
                end,
                body,
                variable,
                span,
            } => {
                self.resolve_expression(start)?;
                self.resolve_expression(end)?;
                self.push();
                self.define(variable, *span, DefinitionKind::Variable)?;
                for s in body {
                    self.resolve_statement(s)?;
                }
                self.pop();
            }
            Statement::ForEach {
                iterable,
                body,
                variable,
                span,
            } => {
                self.resolve_expression(iterable)?;
                self.push();
                self.define(variable, *span, DefinitionKind::Variable)?;
                for s in body {
                    self.resolve_statement(s)?;
                }
                self.pop();
            }
            Statement::Match {
                expression, arms, ..
            } => {
                self.resolve_expression(expression)?;
                for arm in arms {
                    self.push();
                    self.define_pattern_bindings(&arm.pattern)?;
                    for s in &arm.body {
                        self.resolve_statement(s)?;
                    }
                    self.pop();
                }
            }
            Statement::Defer { expression, .. }
            | Statement::Return {
                value: Some(expression),
                ..
            } => self.resolve_expression(expression)?,
            Statement::Return { value: None, .. }
            | Statement::Break { .. }
            | Statement::Continue { .. }
            | Statement::Struct { .. }
            | Statement::Enum { .. }
            | Statement::Trait { .. }
            | Statement::Impl { .. }
            | Statement::Import { .. } => {}
        }
        Ok(())
    }

    /// A bare function identifier is a value as well as a call target. Project
    /// compilation module-mangles function definitions, so the source spelling
    /// may not be present in the lexical scope table. Accept it here and leave
    /// the module-aware identity/type check to the type checker.
    fn is_function_value_name(&self, name: &str) -> bool {
        self.definitions.iter().any(|definition| {
            definition.kind == DefinitionKind::Function
                && crate::function_values::display_name(&definition.name) == name
        })
    }

    fn is_type_value_name(&self, name: &str) -> bool {
        self.definitions.iter().any(|definition| {
            matches!(definition.kind, DefinitionKind::Struct | DefinitionKind::Enum)
                && crate::function_values::display_name(&definition.name) == name
        })
    }

    fn resolve_expression(&mut self, expr: &Expression) -> Result<(), FusionError> {
        match expr {
            Expression::Identifier { name, span } => {
                if self.lookup(name).is_none()
                    && !self.is_function_value_name(name)
                    && name != "print"
                    && name != "input"
                    && name != "open"
                {
                    return Err(FusionError::UnknownVariable {
                        name: name.clone(),
                        suggestion: None,
                        span: *span,
                    });
                }
            }
            Expression::Array { elements, .. } | Expression::Tuple { elements, .. } => {
                for e in elements {
                    self.resolve_expression(e)?;
                }
            }
            Expression::Index { array, index, .. } => {
                self.resolve_expression(array)?;
                self.resolve_expression(index)?;
            }
            Expression::Property { object, .. } => self.resolve_expression(object)?,
            Expression::MethodCall {
                object, arguments, ..
            } => {
                self.resolve_expression(object)?;
                for a in arguments {
                    self.resolve_expression(a)?;
                }
            }
            Expression::Await { expression, .. } => self.resolve_expression(expression)?,
            Expression::Closure { captures, span, .. } => {
                for capture in captures {
                    if self.lookup(capture).is_none()
                        && !self.is_function_value_name(capture)
                        && !self.is_type_value_name(capture)
                    {
                        return Err(FusionError::UnknownVariable {
                            name: capture.clone(),
                            suggestion: None,
                            span: *span,
                        });
                    }
                }
            },
            Expression::Call { arguments, .. } => {
                for a in arguments {
                    self.resolve_expression(a)?;
                }
            }
            Expression::StructConstructor { fields, .. } => {
                for (_, e) in fields {
                    self.resolve_expression(e)?;
                }
            }
            Expression::EnumConstructor { arguments, .. } => {
                for e in arguments {
                    self.resolve_expression(e)?;
                }
            }
            Expression::Binary { left, right, .. } => {
                self.resolve_expression(left)?;
                self.resolve_expression(right)?;
            }
            Expression::Unary { expression, .. } => self.resolve_expression(expression)?,
            Expression::Conversion { expression, .. } => self.resolve_expression(expression)?,
            Expression::Number { .. }
            | Expression::WideNumber { .. }
            | Expression::Float { .. }
            | Expression::Boolean { .. }
            | Expression::String { .. } => {}
        }
        Ok(())
    }
}
