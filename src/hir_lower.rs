// contents of hir_lower

use crate::ast::*;
use crate::hir::*;
use crate::errors::ParseError;

pub struct HirLowerer;

impl HirLowerer {
    pub fn new() -> Self {
        Self
    }

    pub fn lower_program(
        &mut self,
        program: &Program,
    ) -> Result<HirProgram, String> {
        let mut statements = Vec::new();

        for statement in &program.statements {
            statements.push(self.lower_statement(statement)?);
        }

        Ok(HirProgram {
            statements,
            span: program.span,
        })
    }

    fn lower_statement(
        &mut self,
        statement: &Statement,
    ) -> Result<HirStatement, String> {
        match statement {
            Statement::Expression {
                expression,
                span,
            } => Ok(HirStatement::Expression {
                expression: self.lower_expression(expression)?,
                span: *span,
            }),

            Statement::Assignment {
                target,
                value,
                span,
            } => Ok(HirStatement::Assignment {
                target: self.lower_expression(target)?,
                value: self.lower_expression(value)?,
                span: *span,
            }),

            Statement::Return { value, span } => {
                Ok(HirStatement::Return {
                    value: Some(self.lower_expression(value)?),
                    span: *span,
                })
            }

            Statement::Break { span } => {
                Ok(HirStatement::Break { span: *span })
            }

            Statement::Continue { span } => {
                Ok(HirStatement::Continue { span: *span })
            }

            Statement::VariableDeclarations {
                declarations,
                span,
            } => {
                // Initially we can lower multiple declarations
                // into separate HIR statements.
                //
                // A later pass can enforce/normalize semantics.

                if declarations.len() != 1 {
                    return Err(
                        "Multiple variable declarations are not yet supported by HIR"
                            .to_string()
                    );
                }

                let declaration = &declarations[0];

                Ok(HirStatement::VariableDeclaration {
                    name: declaration.name.clone(),
                    declared_type: declaration.declared_type.clone(),
                    value: self.lower_expression(&declaration.value)?,
                    span: *span,
                })
            }

            Statement::ConstDeclaration {
                name,
                declared_type,
                value,
                span,
                ..
            } => Ok(HirStatement::ConstDeclaration {
                name: name.clone(),
                declared_type: declared_type.clone(),
                value: self.lower_expression(value)?,
                span: *span,
            }),

            Statement::If {
                condition,
                body,
                else_body,
                span,
            } => {
                Ok(HirStatement::If {
                    condition: self.lower_expression(condition)?,
                    body: self.lower_statements(body)?,
                    else_body: match else_body {
                        Some(body) => Some(self.lower_statements(body)?),
                        None => None,
                    },
                    span: *span,
                })
            }

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
            } => Ok(HirStatement::For {
                variable: variable.clone(),
                start: self.lower_expression(start)?,
                end: self.lower_expression(end)?,
                body: self.lower_statements(body)?,
                span: *span,
            }),

            Statement::Defer {
                expression,
                span,
            } => Ok(HirStatement::Defer {
                expression: self.lower_expression(expression)?,
                span: *span,
            }),

            _ => Err(format!(
                "HIR lowering not yet implemented for {:?}",
                statement
            )),
        }
    }

    fn lower_statements(
        &mut self,
        statements: &[Statement],
    ) -> Result<Vec<HirStatement>, String> {
        statements
            .iter()
            .map(|statement| self.lower_statement(statement))
            .collect()
    }

    fn lower_expression(
        &mut self,
        expression: &Expression,
    ) -> Result<HirExpression, String> {
        match expression {
            Expression::Number { value, .. } => {
                Ok(HirExpression::Number(*value))
            }

            Expression::Float { value, .. } => {
                Ok(HirExpression::Float(*value))
            }

            Expression::Boolean { value, .. } => {
                Ok(HirExpression::Boolean(*value))
            }

            Expression::String { value, .. } => {
                Ok(HirExpression::String(value.clone()))
            }

            Expression::Identifier { name, .. } => {
                Ok(HirExpression::Identifier(name.clone()))
            }

            Expression::Array { elements, .. } => {
                Ok(HirExpression::Array(
                    elements
                        .iter()
                        .map(|e| self.lower_expression(e))
                        .collect::<Result<_, _>>()?,
                ))
            }

            Expression::Call {
                name,
                arguments,
                ..
            } => Ok(HirExpression::Call {
                name: name.clone(),
                arguments: arguments
                    .iter()
                    .map(|e| self.lower_expression(e))
                    .collect::<Result<_, _>>()?,
            }),

            Expression::StructConstructor {
                name,
                fields,
                ..
            } => Ok(HirExpression::StructConstructor {
                name: name.clone(),
                fields: fields
                    .iter()
                    .map(|(name, value)| {
                        Ok((
                            name.clone(),
                            self.lower_expression(value)?,
                        ))
                    })
                    .collect::<Result<_, String>>()?,
            }),

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

            Expression::Unary {
                operator,
                expression,
                ..
            } => Ok(HirExpression::Unary {
                operator: self.lower_unary_operator(*operator),
                expression: Box::new(
                    self.lower_expression(expression)?
                ),
            }),

            Expression::Index {
                array,
                index,
                ..
            } => Ok(HirExpression::Index {
                array: Box::new(self.lower_expression(array)?),
                index: Box::new(self.lower_expression(index)?),
            }),

            Expression::Property {
                object,
                name,
                ..
            } => Ok(HirExpression::Property {
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

    fn lower_unary_operator(
        &self,
        op: UnaryOperator,
    ) -> HirUnaryOperator {
        match op {
            UnaryOperator::Negate => HirUnaryOperator::Negate,
            UnaryOperator::Not => HirUnaryOperator::Not,
        }
    }
}