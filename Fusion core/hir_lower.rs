// AST-to-HIR lowering. HIR preserves source declarations while assigning
// module-qualified identities before compatibility/backend symbol mangling.

use crate::ast::*;
use crate::hir::*;
use crate::span::Span;
use std::collections::{HashMap, HashSet};

pub struct HirLowerer {
    /// Lexical scopes used to distinguish inferred declarations from assignment.
    scopes: Vec<HashSet<String>>,
    module: Option<String>,
    impl_scopes: HashMap<(String, Option<String>), Vec<String>>,
}

impl HirLowerer {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashSet::new()],
            module: None,
            impl_scopes: HashMap::new(),
        }
    }

    fn for_module(
        module: impl Into<String>,
        impl_scopes: HashMap<(String, Option<String>), Vec<String>>,
    ) -> Self {
        Self {
            scopes: vec![HashSet::new()],
            module: Some(module.into()),
            impl_scopes,
        }
    }

    fn is_declared(&self, name: &str) -> bool {
        self.scopes.iter().rev().any(|scope| scope.contains(name))
    }

    fn declare(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string());
        }
    }

    fn identity(&self, name: &str, kind: HirDefinitionKind) -> Option<HirDefinitionId> {
        self.module
            .as_ref()
            .map(|module| HirDefinitionId::new(module.clone(), name, kind))
    }

    pub fn lower_project(project: &crate::module_system::Project) -> Result<HirProject, String> {
        // HIR lowering is also used directly by project-aware backends. Keep
        // trait conformance and dispatch validation at this boundary so callers
        // cannot bypass it by skipping the higher-level compile_project path.
        crate::module_system::validate_hir_trait_implementations(project)?;
        let mut modules = Vec::with_capacity(project.modules.len());
        for module in &project.modules {
            let mut impl_scopes = HashMap::new();
            for statement in &module.statements {
                if let Statement::Impl {
                    trait_name,
                    type_name,
                    ..
                } = statement
                {
                    let receiver_scope = crate::module_system::resolve_hir_type_name(
                        project,
                        &module.name,
                        type_name,
                    )
                    .ok()
                    .flatten()
                    .map(|identity| format!("{}::{}", identity.module, identity.name))
                    .unwrap_or_else(|| type_name.clone());
                    let trait_scope = if let Some(trait_name) = trait_name {
                        let resolved = if let Some((qualifier, name)) = trait_name.split_once('.') {
                            crate::module_system::resolve_qualified_definition(
                                project,
                                &module.name,
                                qualifier,
                                name,
                                HirDefinitionKind::Trait,
                            )
                            .ok()
                        } else {
                            crate::module_system::resolve_unqualified_definition(
                                project,
                                &module.name,
                                trait_name,
                                HirDefinitionKind::Trait,
                            )
                            .ok()
                        };
                        resolved
                            .map(|identity| format!("{}::{}", identity.module, identity.name))
                            .unwrap_or_else(|| trait_name.clone())
                    } else {
                        "inherent".to_string()
                    };
                    impl_scopes.insert(
                        (type_name.clone(), trait_name.clone()),
                        vec![trait_scope, receiver_scope],
                    );
                }
            }
            let mut lowerer = Self::for_module(module.name.clone(), impl_scopes);
            let mut statements = Vec::with_capacity(module.statements.len());
            for statement in &module.statements {
                statements.push(lowerer.lower_statement(statement)?);
            }
            modules.push(HirModule {
                name: module.name.clone(),
                path: module.path.to_string_lossy().into_owned(),
                imports: module
                    .imports
                    .iter()
                    .map(|import| HirImport {
                        path: import.path.clone(),
                        alias: import.alias.clone(),
                        selected: import.selected.clone(),
                        re_exported: import.re_exported,
                    })
                    .collect(),
                public_symbols: {
                    let mut values: Vec<_> = module.public_symbols.iter().cloned().collect();
                    values.sort();
                    values
                },
                local_public_symbols: {
                    let mut values: Vec<_> = module.local_public_symbols.iter().cloned().collect();
                    values.sort();
                    values
                },
                private_symbols: {
                    let mut values: Vec<_> = module.private_symbols.iter().cloned().collect();
                    values.sort();
                    values
                },
                statements,
            });
        }
        let resolved_calls = crate::module_system::resolve_hir_calls(project)?;
        let resolved_local_types = crate::module_system::resolve_hir_local_types(project)?;
        let bodies = crate::hir_body::build_project_bodies_with_calls(
            &modules,
            &resolved_local_types,
            &resolved_calls,
        );
        Ok(HirProject {
            entry_module: project.entry_module.clone(),
            modules,
            resolved_calls,
            resolved_local_types,
            bodies,
        })
    }

    pub fn lower_program(&mut self, program: &Program) -> Result<HirProgram, String> {
        let mut statements = Vec::with_capacity(program.statements.len());
        for statement in &program.statements {
            statements.push(self.lower_statement(statement)?);
        }
        Ok(HirProgram {
            statements,
            span: program.span,
        })
    }

    fn lower_function(
        &mut self,
        name: &str,
        generic_parameters: &[String],
        parameters: &[Parameter],
        return_type: &Option<String>,
        body: &[Statement],
        is_async: bool,
        span: Span,
        identity: Option<HirDefinitionId>,
    ) -> Result<HirFunction, String> {
        self.scopes.push(HashSet::new());
        let lowered_parameters = parameters
            .iter()
            .map(|parameter| {
                self.declare(&parameter.name);
                HirParameter {
                    name: parameter.name.clone(),
                    type_name: parameter.type_name.clone(),
                    span: parameter.span,
                }
            })
            .collect();
        let lowered_body = self.lower_statements(body);
        self.scopes.pop();
        Ok(HirFunction {
            identity,
            name: name.to_string(),
            generic_parameters: generic_parameters.to_vec(),
            parameters: lowered_parameters,
            return_type: return_type.clone(),
            is_async,
            body: lowered_body?,
            span,
        })
    }

    fn lower_statement(&mut self, statement: &Statement) -> Result<HirStatement, String> {
        match statement {
            Statement::Import {
                path,
                alias,
                selected,
                span,
            } => Ok(HirStatement::Import {
                path: path.clone(),
                alias: alias.clone(),
                selected: selected.clone(),
                re_exported: false,
                span: *span,
            }),
            Statement::Expression { expression, span } => Ok(HirStatement::Expression {
                expression: self.lower_expression(expression)?,
                span: *span,
            }),
            Statement::Call { expression, span } => Ok(HirStatement::Expression {
                expression: self.lower_expression(expression)?,
                span: *span,
            }),
            Statement::Assignment {
                target,
                value,
                span,
            } => {
                if let Expression::Identifier { name, .. } = target {
                    if !self.is_declared(name) {
                        let value = self.lower_expression(value)?;
                        self.declare(name);
                        return Ok(HirStatement::VariableDeclaration {
                            name: name.clone(),
                            declared_type: None,
                            value,
                            span: *span,
                        });
                    }
                }
                Ok(HirStatement::Assignment {
                    target: self.lower_expression(target)?,
                    value: self.lower_expression(value)?,
                    span: *span,
                })
            }
            Statement::Return { value, span } => Ok(HirStatement::Return {
                value: value
                    .as_ref()
                    .map(|value| self.lower_expression(value))
                    .transpose()?,
                span: *span,
            }),
            Statement::Break { span } => Ok(HirStatement::Break { span: *span }),
            Statement::Continue { span } => Ok(HirStatement::Continue { span: *span }),
            Statement::VariableDeclarations { declarations, span } => {
                if declarations.is_empty() {
                    return Err(format!(
                        "empty variable declaration at {}..{}",
                        span.start, span.end
                    ));
                }
                let mut lowered = Vec::with_capacity(declarations.len());
                for declaration in declarations {
                    let statement = if let Some(value) = declaration.value.as_ref() {
                        let value = self.lower_expression(value)?;
                        self.declare(&declaration.name);
                        HirStatement::VariableDeclaration {
                            name: declaration.name.clone(),
                            declared_type: declaration.declared_type.clone(),
                            value,
                            span: declaration.span,
                        }
                    } else {
                        self.declare(&declaration.name);
                        HirStatement::UninitializedVariable {
                            name: declaration.name.clone(),
                            declared_type: declaration.declared_type.clone(),
                            span: declaration.span,
                        }
                    };
                    lowered.push(statement);
                }
                if lowered.len() == 1 {
                    Ok(lowered.remove(0))
                } else {
                    Ok(HirStatement::Block {
                        statements: lowered,
                        span: *span,
                    })
                }
            }
            Statement::ConstDeclaration {
                name,
                declared_type,
                value,
                span,
                ..
            } => {
                let value = self.lower_expression(value)?;
                self.declare(name);
                Ok(HirStatement::ConstDeclaration {
                    identity: self.identity(name, HirDefinitionKind::Constant),
                    name: name.clone(),
                    declared_type: declared_type.clone(),
                    value,
                    span: *span,
                })
            }
            Statement::Function {
                name,
                generic_parameters,
                parameters,
                return_type,
                body,
                is_async,
                span,
            } => {
                let identity = self.identity(name, HirDefinitionKind::Function);
                let function = self.lower_function(
                    name,
                    generic_parameters,
                    parameters,
                    return_type,
                    body,
                    *is_async,
                    *span,
                    identity,
                )?;
                self.declare(name);
                Ok(HirStatement::Function(function))
            }
            Statement::Struct {
                name,
                fields,
                derives,
                span,
            } => Ok(HirStatement::Struct {
                identity: self.identity(name, HirDefinitionKind::Struct),
                name: name.clone(),
                fields: fields
                    .iter()
                    .map(|field| HirStructField {
                        identity: self.module.as_ref().map(|module| {
                            HirDefinitionId::scoped(
                                module.clone(),
                                [name.clone()],
                                field.name.clone(),
                                HirDefinitionKind::Field,
                            )
                        }),
                        name: field.name.clone(),
                        type_name: field.type_name.clone(),
                        span: field.span,
                    })
                    .collect(),
                derives: derives.clone(),
                span: *span,
            }),
            Statement::Enum {
                name,
                variants,
                span,
            } => Ok(HirStatement::Enum {
                identity: self.identity(name, HirDefinitionKind::Enum),
                name: name.clone(),
                variants: variants
                    .iter()
                    .map(|variant| HirEnumVariant {
                        identity: self.module.as_ref().map(|module| {
                            HirDefinitionId::scoped(
                                module.clone(),
                                [name.clone()],
                                variant.name.clone(),
                                HirDefinitionKind::Variant,
                            )
                        }),
                        name: variant.name.clone(),
                        fields: variant.fields.clone(),
                        span: variant.span,
                    })
                    .collect(),
                span: *span,
            }),
            Statement::Trait {
                name,
                methods,
                span,
            } => {
                let trait_identity = self.identity(name, HirDefinitionKind::Trait);
                let lowered_methods = methods
                    .iter()
                    .map(|method| HirTraitMethod {
                        identity: self.module.as_ref().map(|module| {
                            HirDefinitionId::scoped(
                                module.clone(),
                                [format!("{}::{}", module, name)],
                                method.name.clone(),
                                HirDefinitionKind::Method,
                            )
                        }),
                        name: method.name.clone(),
                        parameters: method
                            .parameters
                            .iter()
                            .map(|parameter| HirParameter {
                                name: parameter.name.clone(),
                                type_name: parameter.type_name.clone(),
                                span: parameter.span,
                            })
                            .collect(),
                        return_type: method.return_type.clone(),
                        span: method.span,
                    })
                    .collect();
                Ok(HirStatement::Trait {
                    identity: trait_identity,
                    name: name.clone(),
                    methods: lowered_methods,
                    span: *span,
                })
            }
            Statement::Impl {
                trait_name,
                type_name,
                methods,
                span,
            } => {
                let mut lowered_methods = Vec::new();
                for method in methods {
                    let Statement::Function {
                        name,
                        generic_parameters,
                        parameters,
                        return_type,
                        body,
                        is_async,
                        span: method_span,
                    } = method
                    else {
                        return Err(
                            "impl blocks may contain only function declarations in HIR".into()
                        );
                    };
                    let scope = self
                        .impl_scopes
                        .get(&(type_name.clone(), trait_name.clone()))
                        .cloned()
                        .unwrap_or_else(|| {
                            vec![
                                trait_name.clone().unwrap_or_else(|| "inherent".into()),
                                type_name.clone(),
                            ]
                        });
                    let identity = self.module.as_ref().map(|module| {
                        HirDefinitionId::scoped(
                            module.clone(),
                            scope,
                            name.clone(),
                            HirDefinitionKind::Method,
                        )
                    });
                    lowered_methods.push(self.lower_function(
                        name,
                        generic_parameters,
                        parameters,
                        return_type,
                        body,
                        *is_async,
                        *method_span,
                        identity,
                    )?);
                }
                Ok(HirStatement::Impl {
                    trait_name: trait_name.clone(),
                    type_name: type_name.clone(),
                    methods: lowered_methods,
                    span: *span,
                })
            }
            Statement::Main { body, span } => Ok(HirStatement::Main {
                body: self.lower_statements(body)?,
                span: *span,
            }),
            Statement::If {
                condition,
                body,
                else_body,
                span,
            } => Ok(HirStatement::If {
                condition: self.lower_expression(condition)?,
                body: self.lower_statements(body)?,
                else_body: else_body
                    .as_ref()
                    .map(|body| self.lower_statements(body))
                    .transpose()?,
                span: *span,
            }),
            Statement::While {
                condition,
                body,
                span,
            } => Ok(HirStatement::While {
                condition: self.lower_expression(condition)?,
                body: self.lower_statements(body)?,
                span: *span,
            }),
            Statement::For {
                variable,
                start,
                end,
                body,
                span,
            } => {
                let start = self.lower_expression(start)?;
                let end = self.lower_expression(end)?;
                self.scopes.push(HashSet::new());
                self.declare(variable);
                let lowered_body = self.lower_statements(body);
                self.scopes.pop();
                Ok(HirStatement::For {
                    variable: variable.clone(),
                    start,
                    end,
                    body: lowered_body?,
                    span: *span,
                })
            }
            Statement::ForEach {
                variable,
                iterable,
                body,
                span,
            } => {
                let iterable = self.lower_expression(iterable)?;
                self.scopes.push(HashSet::new());
                self.declare(variable);
                let lowered_body = self.lower_statements(body);
                self.scopes.pop();
                Ok(HirStatement::ForEach {
                    variable: variable.clone(),
                    iterable,
                    body: lowered_body?,
                    span: *span,
                })
            }
            Statement::Match {
                expression,
                arms,
                span,
            } => {
                let expression = self.lower_expression(expression)?;
                let mut lowered_arms = Vec::with_capacity(arms.len());
                for arm in arms {
                    self.scopes.push(HashSet::new());
                    let pattern = self.lower_pattern(&arm.pattern)?;
                    let guard = arm
                        .guard
                        .as_ref()
                        .map(|guard| self.lower_expression(guard))
                        .transpose()?;
                    let body = self.lower_statements(&arm.body);
                    self.scopes.pop();
                    lowered_arms.push(HirMatchArm {
                        pattern,
                        guard,
                        body: body?,
                        span: arm.span,
                    });
                }
                Ok(HirStatement::Match {
                    expression,
                    arms: lowered_arms,
                    span: *span,
                })
            }
            Statement::Defer { expression, span } => Ok(HirStatement::Defer {
                expression: self.lower_expression(expression)?,
                span: *span,
            }),
        }
    }

    fn lower_statements(&mut self, statements: &[Statement]) -> Result<Vec<HirStatement>, String> {
        self.scopes.push(HashSet::new());
        let result = statements
            .iter()
            .map(|statement| self.lower_statement(statement))
            .collect();
        self.scopes.pop();
        result
    }

    fn lower_pattern(&mut self, pattern: &Pattern) -> Result<HirPattern, String> {
        Ok(match &pattern.kind {
            PatternKind::Wildcard => HirPattern::Wildcard,
            PatternKind::Identifier(name) => {
                self.declare(name);
                HirPattern::Identifier(name.clone())
            }
            PatternKind::Number(value) => HirPattern::Number(*value),
            PatternKind::Float(value) => HirPattern::Float(*value),
            PatternKind::String(value) => HirPattern::String(value.clone()),
            PatternKind::Boolean(value) => HirPattern::Boolean(*value),
            PatternKind::Variant { name, bindings } => HirPattern::Variant {
                name: name.clone(),
                bindings: bindings
                    .iter()
                    .map(|binding| self.lower_pattern(binding))
                    .collect::<Result<_, _>>()?,
            },
            PatternKind::Tuple(items) => HirPattern::Tuple(
                items
                    .iter()
                    .map(|item| self.lower_pattern(item))
                    .collect::<Result<_, _>>()?,
            ),
            PatternKind::Struct {
                name,
                fields,
                has_rest,
            } => HirPattern::Struct {
                name: name.clone(),
                fields: fields
                    .iter()
                    .map(|(field, pattern)| Ok((field.clone(), self.lower_pattern(pattern)?)))
                    .collect::<Result<_, String>>()?,
                has_rest: *has_rest,
            },
            PatternKind::At { name, pattern } => {
                self.declare(name);
                HirPattern::At {
                    name: name.clone(),
                    pattern: Box::new(self.lower_pattern(pattern)?),
                }
            }
            PatternKind::Or(alternatives) => {
                let mut lowered = Vec::with_capacity(alternatives.len());
                let mut alternative_bindings = Vec::new();
                for alternative in alternatives {
                    self.scopes.push(HashSet::new());
                    let lowered_alternative = self.lower_pattern(alternative)?;
                    let names = self.scopes.last().cloned().unwrap_or_default();
                    self.scopes.pop();
                    alternative_bindings.push(names);
                    lowered.push(lowered_alternative);
                }
                if let Some(first) = alternative_bindings.first() {
                    for name in first {
                        if alternative_bindings
                            .iter()
                            .all(|bindings| bindings.contains(name))
                        {
                            self.declare(name);
                        }
                    }
                }
                HirPattern::Or(lowered)
            }
            PatternKind::Range {
                start,
                end,
                inclusive,
            } => HirPattern::Range {
                start: *start,
                end: *end,
                inclusive: *inclusive,
            },
        })
    }

    fn lower_expression(&mut self, expression: &Expression) -> Result<HirExpression, String> {
        Ok(match expression {
            Expression::Number { value, .. } => HirExpression::Number(*value),
            Expression::WideNumber { value, .. } => HirExpression::WideNumber(*value),
            Expression::Float { value, .. } => HirExpression::Float(*value),
            Expression::Boolean { value, .. } => HirExpression::Boolean(*value),
            Expression::String { value, .. } => HirExpression::String(value.clone()),
            Expression::Identifier { name, .. } => HirExpression::Identifier(name.clone()),
            Expression::Array { elements, .. } => HirExpression::Array(
                elements
                    .iter()
                    .map(|element| self.lower_expression(element))
                    .collect::<Result<_, _>>()?,
            ),
            Expression::Tuple { elements, .. } => HirExpression::Tuple(
                elements
                    .iter()
                    .map(|element| self.lower_expression(element))
                    .collect::<Result<_, _>>()?,
            ),
            Expression::Await { expression, .. } => {
                HirExpression::Await(Box::new(self.lower_expression(expression)?))
            }
            Expression::Call {
                name,
                arguments,
                generic_arguments,
                span,
            } => HirExpression::Call {
                name: name.clone(),
                arguments: arguments
                    .iter()
                    .map(|argument| self.lower_expression(argument))
                    .collect::<Result<_, _>>()?,
                generic_arguments: generic_arguments.clone(),
                span: *span,
            },
            Expression::StructConstructor { name, fields, .. } => {
                HirExpression::StructConstructor {
                    name: name.clone(),
                    fields: fields
                        .iter()
                        .map(|(name, value)| Ok((name.clone(), self.lower_expression(value)?)))
                        .collect::<Result<_, String>>()?,
                }
            }
            Expression::EnumConstructor {
                enum_name,
                variant,
                arguments,
                ..
            } => HirExpression::EnumConstructor {
                enum_name: enum_name.clone(),
                variant: variant.clone(),
                arguments: arguments
                    .iter()
                    .map(|argument| self.lower_expression(argument))
                    .collect::<Result<_, _>>()?,
            },
            Expression::Binary {
                left,
                operator,
                right,
                ..
            } => HirExpression::Binary {
                left: Box::new(self.lower_expression(left)?),
                operator: self.lower_operator(*operator),
                right: Box::new(self.lower_expression(right)?),
            },
            Expression::Unary {
                operator,
                expression,
                ..
            } => HirExpression::Unary {
                operator: self.lower_unary_operator(*operator),
                expression: Box::new(self.lower_expression(expression)?),
            },
            Expression::Index { array, index, .. } => HirExpression::Index {
                array: Box::new(self.lower_expression(array)?),
                index: Box::new(self.lower_expression(index)?),
            },
            Expression::Property { object, name, .. } => HirExpression::Property {
                object: Box::new(self.lower_expression(object)?),
                name: name.clone(),
            },
            Expression::MethodCall {
                object,
                method,
                arguments,
                generic_arguments,
                span,
            } => HirExpression::MethodCall {
                object: Box::new(self.lower_expression(object)?),
                method: method.clone(),
                arguments: arguments
                    .iter()
                    .map(|argument| self.lower_expression(argument))
                    .collect::<Result<_, _>>()?,
                generic_arguments: generic_arguments.clone(),
                span: *span,
            },
            Expression::Conversion {
                kind,
                expression,
                target_type,
                ..
            } => HirExpression::Conversion {
                kind: match kind {
                    ConversionKind::As => HirConversionKind::As,
                    ConversionKind::From => HirConversionKind::From,
                    ConversionKind::Try => HirConversionKind::Try,
                },
                expression: Box::new(self.lower_expression(expression)?),
                target_type: target_type.clone(),
            },
        })
    }

    fn lower_operator(&self, operator: Operator) -> HirOperator {
        match operator {
            Operator::Plus => HirOperator::Plus,
            Operator::Minus => HirOperator::Minus,
            Operator::Multiply => HirOperator::Multiply,
            Operator::Divide => HirOperator::Divide,
            Operator::Equal => HirOperator::Equal,
            Operator::NotEqual => HirOperator::NotEqual,
            Operator::Less => HirOperator::Less,
            Operator::LessEqual => HirOperator::LessEqual,
            Operator::Greater => HirOperator::Greater,
            Operator::GreaterEqual => HirOperator::GreaterEqual,
            Operator::And => HirOperator::And,
            Operator::Or => HirOperator::Or,
        }
    }

    fn lower_unary_operator(&self, operator: UnaryOperator) -> HirUnaryOperator {
        match operator {
            UnaryOperator::Negate => HirUnaryOperator::Negate,
            UnaryOperator::Not => HirUnaryOperator::Not,
            UnaryOperator::Deref => HirUnaryOperator::Deref,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_lowering_assigns_module_qualified_function_and_method_ids() {
        let span = Span::new(0, 0);
        let function = Statement::Function {
            name: "value".into(),
            generic_parameters: vec![],
            parameters: vec![Parameter {
                name: "x".into(),
                name_span: span,
                type_name: Some("num".into()),
                span,
            }],
            return_type: Some("num".into()),
            body: vec![Statement::Return {
                value: Some(Expression::Identifier {
                    name: "x".into(),
                    span,
                }),
                span,
            }],
            is_async: false,
            span,
        };
        let structure = Statement::Struct {
            name: "Point".into(),
            fields: vec![StructField {
                name: "x".into(),
                name_span: span,
                type_name: "num".into(),
                span,
            }],
            derives: vec![],
            span,
        };
        let method = Statement::Function {
            name: "length".into(),
            generic_parameters: vec![],
            parameters: vec![],
            return_type: Some("num".into()),
            body: vec![Statement::Return {
                value: Some(Expression::Number { value: 1, span }),
                span,
            }],
            is_async: false,
            span,
        };
        let implementation = Statement::Impl {
            trait_name: None,
            type_name: "Point".into(),
            methods: vec![method],
            span,
        };
        let enumeration = Statement::Enum {
            name: "Choice".into(),
            variants: vec![EnumVariant {
                name: "Ready".into(),
                name_span: span,
                fields: vec!["num".into()],
                span,
            }],
            span,
        };
        let trait_decl = Statement::Trait {
            name: "Measure".into(),
            methods: vec![TraitMethod {
                name: "measure".into(),
                parameters: vec![],
                return_type: Some("num".into()),
                span,
            }],
            span,
        };
        let statements = vec![function, structure, implementation, enumeration, trait_decl];
        let program = Program {
            statements: statements.clone(),
            span,
            modules: vec![],
        };
        let project = crate::module_system::Project {
            entry_module: "crate".into(),
            modules: vec![crate::module_system::ModuleUnit {
                name: "crate".into(),
                path: "main.fusion".into(),
                imports: vec![],
                public_symbols: HashSet::new(),
                local_public_symbols: HashSet::new(),
                function_symbols: HashSet::from(["value".into()]),
                private_symbols: HashSet::new(),
                statements,
                statement_start: 0,
                statement_end: 5,
            }],
            program,
        };
        let hir = HirLowerer::lower_project(&project).unwrap();
        let statements = &hir.modules[0].statements;
        let function = match &statements[0] {
            HirStatement::Function(function) => function,
            other => panic!("unexpected {other:?}"),
        };
        assert_eq!(function.identity.as_ref().unwrap().module, "crate");
        assert_eq!(
            function.identity.as_ref().unwrap().kind,
            HirDefinitionKind::Function
        );
        let implementation = statements
            .iter()
            .find_map(|statement| match statement {
                HirStatement::Impl { methods, .. } => Some(methods),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            implementation[0].identity.as_ref().unwrap().kind,
            HirDefinitionKind::Method
        );
        assert_eq!(
            implementation[0].identity.as_ref().unwrap().scope,
            vec!["inherent".to_string(), "crate::Point".to_string()]
        );
        let enum_variant = statements
            .iter()
            .find_map(|statement| match statement {
                HirStatement::Enum { variants, .. } => Some(&variants[0]),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            enum_variant.identity.as_ref().unwrap().kind,
            HirDefinitionKind::Variant
        );
        let trait_method = statements
            .iter()
            .find_map(|statement| match statement {
                HirStatement::Trait { methods, .. } => Some(&methods[0]),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            trait_method.identity.as_ref().unwrap().scope,
            vec!["crate::Measure".to_string()]
        );
        assert_ne!(implementation[0].identity, trait_method.identity);
    }

    #[test]
    fn lowerer_preserves_match_guards_nested_patterns_and_for_each() {
        let span = Span::new(0, 0);
        let number = |value| Expression::Number { value, span };
        let arms = vec![
            MatchArm {
                pattern: Pattern {
                    kind: PatternKind::Or(vec![
                        Pattern {
                            kind: PatternKind::Number(1),
                            span,
                        },
                        Pattern {
                            kind: PatternKind::Number(2),
                            span,
                        },
                    ]),
                    span,
                },
                guard: Some(Expression::Boolean { value: true, span }),
                body: vec![Statement::Continue { span }],
                span,
            },
            MatchArm {
                pattern: Pattern {
                    kind: PatternKind::Wildcard,
                    span,
                },
                guard: None,
                body: vec![Statement::Break { span }],
                span,
            },
        ];
        let program = Program {
            statements: vec![Statement::Main {
                body: vec![Statement::ForEach {
                    variable: "item".into(),
                    iterable: Expression::Array {
                        elements: vec![number(1), number(2)],
                        span,
                    },
                    body: vec![Statement::Match {
                        expression: Expression::Identifier {
                            name: "item".into(),
                            span,
                        },
                        arms,
                        span,
                    }],
                    span,
                }],
                span,
            }],
            span,
            modules: vec![],
        };
        let mut lowerer = HirLowerer::new();
        let hir = lowerer.lower_program(&program).unwrap();
        let main = match &hir.statements[0] {
            HirStatement::Main { body, .. } => body,
            other => panic!("unexpected {other:?}"),
        };
        let foreach = match &main[0] {
            HirStatement::ForEach { body, .. } => body,
            other => panic!("unexpected {other:?}"),
        };
        let match_arms = match &foreach[0] {
            HirStatement::Match { arms, .. } => arms,
            other => panic!("unexpected {other:?}"),
        };
        assert!(match_arms[0].guard.is_some());
        assert!(matches!(match_arms[0].pattern, HirPattern::Or(_)));
    }

    #[test]
    fn same_named_functions_in_separate_modules_keep_distinct_hir_ids() {
        let span = Span::new(0, 0);
        let function = Statement::Function {
            name: "parse".into(),
            generic_parameters: vec![],
            parameters: vec![],
            return_type: None,
            body: vec![],
            is_async: false,
            span,
        };
        let make_module = |name: &str| crate::module_system::ModuleUnit {
            name: name.into(),
            path: format!("{}.fusion", name).into(),
            imports: vec![],
            public_symbols: HashSet::new(),
            local_public_symbols: HashSet::new(),
            function_symbols: HashSet::from(["parse".into()]),
            private_symbols: HashSet::new(),
            statements: vec![function.clone()],
            statement_start: 0,
            statement_end: 1,
        };
        let project = crate::module_system::Project {
            entry_module: "alpha".into(),
            modules: vec![make_module("alpha"), make_module("beta")],
            program: Program {
                statements: vec![function],
                span,
                modules: vec![],
            },
        };
        let lowered = HirLowerer::lower_project(&project).unwrap();
        let alpha = match &lowered.modules[0].statements[0] {
            HirStatement::Function(function) => function.identity.as_ref().unwrap(),
            _ => panic!("expected function"),
        };
        let beta = match &lowered.modules[1].statements[0] {
            HirStatement::Function(function) => function.identity.as_ref().unwrap(),
            _ => panic!("expected function"),
        };
        assert_ne!(alpha, beta);
        assert_eq!(alpha.name, beta.name);
        assert_ne!(alpha.module, beta.module);
        assert_eq!(lowered.bodies.len(), 2);
        assert!(lowered.bodies.iter().all(|body| body.owner.is_some()));
    }

    #[test]
    fn project_lowering_resolves_calls_and_checks_nominal_signatures() {
        let span = Span::new(10, 20);
        let point = Statement::Struct {
            name: "Point".into(),
            fields: vec![StructField {
                name: "x".into(),
                name_span: span,
                type_name: "num".into(),
                span,
            }],
            derives: vec![],
            span,
        };
        let consume = Statement::Function {
            name: "consume".into(),
            generic_parameters: vec![],
            parameters: vec![Parameter {
                name: "value".into(),
                name_span: span,
                type_name: Some("Point".into()),
                span,
            }],
            return_type: None,
            body: vec![],
            is_async: false,
            span,
        };
        let main = Statement::Main {
            body: vec![
                Statement::VariableDeclarations {
                    declarations: vec![VariableDeclaration {
                        name: "p".into(),
                        name_span: span,
                        declared_type: None,
                        value: Some(Expression::StructConstructor {
                            name: "Point".into(),
                            fields: vec![],
                            span,
                        }),
                        span,
                    }],
                    span,
                },
                Statement::Expression {
                    expression: Expression::Call {
                        name: "consume".into(),
                        arguments: vec![Expression::Identifier {
                            name: "p".into(),
                            span,
                        }],
                        generic_arguments: vec![],
                        span,
                    },
                    span,
                },
            ],
            span,
        };
        let statements = vec![point, consume, main];
        let project = crate::module_system::Project {
            entry_module: "app".into(),
            modules: vec![crate::module_system::ModuleUnit {
                name: "app".into(),
                path: "main.fusion".into(),
                imports: vec![],
                public_symbols: HashSet::new(),
                local_public_symbols: HashSet::new(),
                function_symbols: HashSet::from(["consume".into()]),
                private_symbols: HashSet::new(),
                statements: statements.clone(),
                statement_start: 0,
                statement_end: statements.len(),
            }],
            program: Program {
                statements,
                span,
                modules: vec![],
            },
        };
        let lowered = HirLowerer::lower_project(&project).unwrap();
        assert_eq!(lowered.resolved_calls.len(), 1);
        let call = &lowered.resolved_calls[0];
        assert_eq!(call.declaration.module, "app");
        assert_eq!(call.declaration.name, "consume");
        assert_eq!(call.kind, HirCallKind::Function);
        assert_eq!(call.parameter_types[0].as_ref().unwrap().name, "Point");
        assert_eq!(call.argument_types[0].as_ref().unwrap().name, "Point");
    }

    #[test]
    fn project_lowering_rejects_known_nominal_argument_mismatch() {
        let span = Span::new(0, 1);
        let make_struct = |name: &str| Statement::Struct {
            name: name.into(),
            fields: vec![],
            derives: vec![],
            span,
        };
        let consume = Statement::Function {
            name: "consume".into(),
            generic_parameters: vec![],
            parameters: vec![Parameter {
                name: "value".into(),
                name_span: span,
                type_name: Some("Point".into()),
                span,
            }],
            return_type: None,
            body: vec![],
            is_async: false,
            span,
        };
        let main = Statement::Main {
            body: vec![Statement::Expression {
                expression: Expression::Call {
                    name: "consume".into(),
                    arguments: vec![Expression::StructConstructor {
                        name: "Vector".into(),
                        fields: vec![],
                        span,
                    }],
                    generic_arguments: vec![],
                    span,
                },
                span,
            }],
            span,
        };
        let statements = vec![make_struct("Point"), make_struct("Vector"), consume, main];
        let project = crate::module_system::Project {
            entry_module: "app".into(),
            modules: vec![crate::module_system::ModuleUnit {
                name: "app".into(),
                path: "main.fusion".into(),
                imports: vec![],
                public_symbols: HashSet::new(),
                local_public_symbols: HashSet::new(),
                function_symbols: HashSet::from(["consume".into()]),
                private_symbols: HashSet::new(),
                statements: statements.clone(),
                statement_start: 0,
                statement_end: statements.len(),
            }],
            program: Program {
                statements,
                span,
                modules: vec![],
            },
        };
        let error = HirLowerer::lower_project(&project).unwrap_err();
        assert!(error.contains("nominal type"));
    }

    #[test]
    fn project_lowering_resolves_nominal_method_calls_to_method_ids() {
        let span = Span::new(3, 8);
        let point = Statement::Struct {
            name: "Point".into(),
            fields: vec![],
            derives: vec![],
            span,
        };
        let method = Statement::Function {
            name: "duplicate".into(),
            generic_parameters: vec![],
            parameters: vec![Parameter {
                name: "self".into(),
                name_span: span,
                type_name: None,
                span,
            }],
            return_type: Some("Self".into()),
            body: vec![],
            is_async: false,
            span,
        };
        let implementation = Statement::Impl {
            trait_name: None,
            type_name: "Point".into(),
            methods: vec![method],
            span,
        };
        let main = Statement::Main {
            body: vec![
                Statement::VariableDeclarations {
                    declarations: vec![VariableDeclaration {
                        name: "point".into(),
                        name_span: span,
                        declared_type: None,
                        value: Some(Expression::StructConstructor {
                            name: "Point".into(),
                            fields: vec![],
                            span,
                        }),
                        span,
                    }],
                    span,
                },
                Statement::Expression {
                    expression: Expression::MethodCall {
                        object: Box::new(Expression::Identifier {
                            name: "point".into(),
                            span,
                        }),
                        method: "duplicate".into(),
                        arguments: vec![],
                        generic_arguments: vec![],
                        span,
                    },
                    span,
                },
            ],
            span,
        };
        let statements = vec![point, implementation, main];
        let project = crate::module_system::Project {
            entry_module: "app".into(),
            modules: vec![crate::module_system::ModuleUnit {
                name: "app".into(),
                path: "main.fusion".into(),
                imports: vec![],
                public_symbols: HashSet::new(),
                local_public_symbols: HashSet::new(),
                function_symbols: HashSet::new(),
                private_symbols: HashSet::new(),
                statements: statements.clone(),
                statement_start: 0,
                statement_end: statements.len(),
            }],
            program: Program {
                statements,
                span,
                modules: vec![],
            },
        };
        let lowered = HirLowerer::lower_project(&project).unwrap();
        assert_eq!(lowered.resolved_calls.len(), 1);
        let call = &lowered.resolved_calls[0];
        assert_eq!(call.kind, HirCallKind::Method);
        assert_eq!(call.declaration.kind, HirDefinitionKind::Method);
        assert_eq!(call.declaration.name, "duplicate");
        assert_eq!(call.receiver_type.as_ref().unwrap().name, "Point");
        assert_eq!(call.return_type.as_ref().unwrap().name, "Point");
    }

    #[test]
    fn project_lowering_records_builtin_calls_in_reserved_namespace() {
        let span = Span::new(1, 4);
        let main = Statement::Main {
            body: vec![Statement::Expression {
                expression: Expression::Call {
                    name: "print".into(),
                    arguments: vec![Expression::String {
                        value: "hello".into(),
                        span,
                    }],
                    generic_arguments: vec![],
                    span,
                },
                span,
            }],
            span,
        };
        let statements = vec![main];
        let project = crate::module_system::Project {
            entry_module: "app".into(),
            modules: vec![crate::module_system::ModuleUnit {
                name: "app".into(),
                path: "main.fusion".into(),
                imports: vec![],
                public_symbols: HashSet::new(),
                local_public_symbols: HashSet::new(),
                function_symbols: HashSet::new(),
                private_symbols: HashSet::new(),
                statements: statements.clone(),
                statement_start: 0,
                statement_end: statements.len(),
            }],
            program: Program {
                statements,
                span,
                modules: vec![],
            },
        };
        let lowered = HirLowerer::lower_project(&project).unwrap();
        assert_eq!(lowered.resolved_calls.len(), 1);
        assert_eq!(lowered.resolved_calls[0].declaration.module, "builtin");
        assert_eq!(lowered.resolved_calls[0].declaration.name, "print");
    }
}
