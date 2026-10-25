// contents of hir_lower

/*
lower_hir — converts another representation into HIR
lower_hir is generally a function or compiler stage, 
rather than a standardized compiler term.
Its job is to take an earlier representation—often an AST—and lower it into HIR.

Lowering is not necessarily the same as optimization. 
Its primary purpose is to transform the representation
 and make implicit language features explicit.
*/

use crate::ast::*;
use crate::hir::*;
use crate::span::Span;

pub struct HirLowerer {
    /// Lexical scopes of variable names declared so far. Used to tell Fusion's
    /// inferred declaration form (`x = 10` where `x` is new) apart from a real
    /// re-assignment of an existing variable.
    scopes: Vec<std::collections::HashSet<String>>,
}

impl HirLowerer {
    pub fn new() -> Self {
        Self {
            scopes: vec![std::collections::HashSet::new()],
        }
    }

    fn is_declared(&self, name: &str) -> bool {
        self.scopes.iter().any(|scope| scope.contains(name))
    }

    fn declare(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string());
        }
    }

    pub fn lower_program(&mut self, program: &Program) -> Result<HirProgram, String> {
        let mut statements = Vec::new();

        for statement in &program.statements {
            statements.push(self.lower_statement(statement)?);
        }

        Ok(HirProgram {
            statements,
            span: program.span,
        })
    }

    fn lower_statement(&mut self, statement: &Statement) -> Result<HirStatement, String> {
        match statement {
            Statement::Import { .. } => Ok(HirStatement::Expression {
                expression: HirExpression::Number(0),
                span: Span::new(0, 0),
            }),

            Statement::Expression { expression, span } => Ok(HirStatement::Expression {
                expression: self.lower_expression(expression)?,
                span: *span,
            }),

            Statement::Assignment {
                target,
                value,
                span,
            } => {
                // `x = expr` where `x` has not been declared yet is Fusion's
                // inferred declaration form (the type checker treats it the
                // same way), so lower it to a variable declaration.
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
                value: match value {
                    Some(value) => Some(self.lower_expression(value)?),
                    None => None,
                },
                span: *span,
            }),

            Statement::Break { span } => Ok(HirStatement::Break { span: *span }),

            Statement::Continue { span } => Ok(HirStatement::Continue { span: *span }),

            Statement::VariableDeclarations { declarations, span } => {
                // Initially we can lower multiple declarations
                // into separate HIR statements.
                //
                // A later pass can enforce/normalize semantics.

                if declarations.len() != 1 {
                    return Err(
                        "Multiple variable declarations are not yet supported by HIR".to_string(),
                    );
                }

                let declaration = &declarations[0];

                let value = match &declaration.value {
                    Some(value) => self.lower_expression(value)?,
                    None => {
                        return Err(format!(
                            "variable '{}' has no initializer; HIR requires a value",
                            declaration.name
                        ));
                    }
                };
                self.declare(&declaration.name);

                Ok(HirStatement::VariableDeclaration {
                    name: declaration.name.clone(),
                    declared_type: declaration.declared_type.clone(),
                    value,
                    span: *span,
                })
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
                    name: name.clone(),
                    declared_type: declared_type.clone(),
                    value,
                    span: *span,
                })
            }

            Statement::If {
                condition,
                body,
                else_body,
                span,
            } => Ok(HirStatement::If {
                condition: self.lower_expression(condition)?,
                body: self.lower_statements(body)?,
                else_body: match else_body {
                    Some(body) => Some(self.lower_statements(body)?),
                    None => None,
                },
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
                self.scopes.push(std::collections::HashSet::new());
                self.declare(variable);
                let body = self.lower_statements(body);
                self.scopes.pop();
                Ok(HirStatement::For {
                    variable: variable.clone(),
                    start,
                    end,
                    body: body?,
                    span: *span,
                })
            }

            Statement::Main { body, span } => Ok(HirStatement::Main {
                body: self.lower_statements(body)?,
                span: *span,
            }),

            Statement::Struct { name, fields, derives, span } => Ok(HirStatement::Struct {
                name: name.clone(),
                fields: fields.iter().map(|field| HirStructField {
                    name: field.name.clone(),
                    type_name: field.type_name.clone(),
                    span: field.span,
                }).collect(),
                derives: derives.clone(),
                span: *span,
            }),

            Statement::Defer { expression, span } => Ok(HirStatement::Defer {
                expression: self.lower_expression(expression)?,
                span: *span,
            }),

            _ => Err(format!(
                "HIR lowering not yet implemented for {:?}",
                statement
            )),
        }
    }

    fn lower_statements(&mut self, statements: &[Statement]) -> Result<Vec<HirStatement>, String> {
        // Every nested block gets its own lexical scope.
        self.scopes.push(std::collections::HashSet::new());
        let result = statements
            .iter()
            .map(|statement| self.lower_statement(statement))
            .collect();
        self.scopes.pop();
        result
    }

    fn lower_expression(&mut self, expression: &Expression) -> Result<HirExpression, String> {
        match expression {
            Expression::Number { value, .. } => Ok(HirExpression::Number(*value)),

            Expression::Float { value, .. } => Ok(HirExpression::Float(*value)),

            Expression::Boolean { value, .. } => Ok(HirExpression::Boolean(*value)),

            Expression::String { value, .. } => Ok(HirExpression::String(value.clone())),

            Expression::Identifier { name, .. } => Ok(HirExpression::Identifier(name.clone())),

            Expression::Array { elements, .. } => Ok(HirExpression::Array(
                elements.iter().map(|e| self.lower_expression(e)).collect::<Result<_, _>>()?,
            )),
            Expression::Tuple { elements, .. } => Ok(HirExpression::Tuple(
                elements.iter().map(|element| self.lower_expression(element)).collect::<Result<_, _>>()?,
            )),

            Expression::Await { .. } => {
                // HIR does not yet model an async state machine. Keep this as an
                // explicit lowering error rather than silently changing semantics.
                Err("await is parsed but async lowering is not implemented yet".to_string())
            }

            Expression::Call {
                name, arguments, ..
            } => Ok(HirExpression::Call {
                name: name.clone(),
                arguments: arguments
                    .iter()
                    .map(|e| self.lower_expression(e))
                    .collect::<Result<_, _>>()?,
            }),

            Expression::StructConstructor { name, fields, .. } => {
                Ok(HirExpression::StructConstructor {
                    name: name.clone(),
                    fields: fields
                        .iter()
                        .map(|(name, value)| Ok((name.clone(), self.lower_expression(value)?)))
                        .collect::<Result<_, String>>()?,
                })
            }

            Expression::EnumConstructor {
                enum_name,
                variant,
                arguments,
                ..
            } => Ok(HirExpression::EnumConstructor {
                enum_name: enum_name.clone(),
                variant: variant.clone(),
                arguments: arguments
                    .iter()
                    .map(|e| self.lower_expression(e))
                    .collect::<Result<_, _>>()?,
            }),

            Expression::Binary {
                left,
                operator,
                right,
                ..
            } => Ok(HirExpression::Binary {
                left: Box::new(self.lower_expression(left)?),
                operator: self.lower_operator(*operator),
                right: Box::new(self.lower_expression(right)?),
            }),

            Expression::Conversion { .. } => {
                Err("conversions are not yet lowered to HIR".to_string())
            }

            Expression::Unary {
                operator,
                expression,
                ..
            } => Ok(HirExpression::Unary {
                operator: self.lower_unary_operator(*operator),
                expression: Box::new(self.lower_expression(expression)?),
            }),

            Expression::Index { array, index, .. } => Ok(HirExpression::Index {
                array: Box::new(self.lower_expression(array)?),
                index: Box::new(self.lower_expression(index)?),
            }),

            Expression::Property { object, name, .. } => Ok(HirExpression::Property {
                object: Box::new(self.lower_expression(object)?),
                name: name.clone(),
            }),

            Expression::MethodCall {
                object,
                method,
                arguments,
                ..
            } => Ok(HirExpression::MethodCall {
                object: Box::new(self.lower_expression(object)?),
                method: method.clone(),
                arguments: arguments
                    .iter()
                    .map(|e| self.lower_expression(e))
                    .collect::<Result<_, _>>()?,
            }),
        }
    }

    fn lower_operator(&self, op: Operator) -> HirOperator {
        match op {
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

    fn lower_unary_operator(&self, op: UnaryOperator) -> HirUnaryOperator {
        match op {
            UnaryOperator::Negate => HirUnaryOperator::Negate,
            UnaryOperator::Not => HirUnaryOperator::Not,
        }
    }
}
