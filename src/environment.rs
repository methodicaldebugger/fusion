// contents of environment.rs
use std::collections::HashMap;

use crate::ast::Expression;
use crate::value::Value;

#[derive(Clone)]
pub struct Binding {
    pub value: Value,
    pub mutable: bool,
}

#[derive(Clone)]
pub struct Scope {
    pub variables: HashMap<String, Binding>,
    pub deferred: Vec<Expression>,
}

#[derive(Clone)]
pub struct Environment {
    scopes: Vec<Scope>,
}

impl Environment {
    pub fn new() -> Self {
        Self {
            scopes: vec![
                Scope {
                    variables: HashMap::new(),
                    deferred: Vec::new(),
                }
            ],
        }
    }

    pub fn push_scope(&mut self) {
        self.scopes.push(
            Scope {
                variables: HashMap::new(),
                deferred: Vec::new(),
            }
        );
    }

    pub fn pop_scope(&mut self) -> Vec<Expression> {
        match self.scopes.pop() {
            Some(scope) => scope.deferred,
            None => Vec::new(),
        }
    }

    pub fn set(
    &mut self,
    name: String,
    value: Value,
) {
    if let Some(scope) = self.scopes.last_mut() {
        scope.variables.insert(
            name,
            Binding {
                value,
                mutable: true,
            },
        );
    }
}

    pub fn assign(
    &mut self,
    name: &str,
    value: Value,
) -> Result<(), String> {
    for scope in self.scopes.iter_mut().rev() {
        if let Some(binding) = scope.variables.get_mut(name) {
            if !binding.mutable {
                return Err(
                    format!(
                        "Cannot assign to constant '{}'",
                        name
                    )
                );
            }

            binding.value = value;
            return Ok(());
        }
    }

    Err(format!(
        "Unknown variable '{}'",
        name
    ))
}

    pub fn declare(
    &mut self,
    name: String,
    value: Value,
    mutable: bool,
) {
    if let Some(scope) = self.scopes.last_mut() {
        scope.variables.insert(
            name,
            Binding {
                value,
                mutable,
            },
        );
    }
}

    pub fn get(
    &self,
    name: &str,
) -> Option<&Value> {
    for scope in self.scopes.iter().rev() {
        if let Some(binding) = scope.variables.get(name) {
            return Some(&binding.value);
        }
    }

    None
}

    pub fn get_mut(
    &mut self,
    name: &str,
) -> Option<&mut Value> {
    for scope in self.scopes.iter_mut().rev() {
        if let Some(binding) =
            scope.variables.get_mut(name)
        {
            return Some(&mut binding.value);
        }
    }

    None
}

    pub fn add_defer(
        &mut self,
        expression: Expression,
    ) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.deferred.push(expression);
        }
    }
}