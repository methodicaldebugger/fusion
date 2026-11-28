//! Construction of backend-oriented HIR bodies.
//!
//! This is deliberately a side-by-side representation during Phase 3: source
//! HIR remains lossless enough for diagnostics while this layer provides local
//! identities, expression type annotations, and explicit basic-block edges.

use crate::hir::*;
use crate::span::Span;
use std::collections::HashMap;

pub fn build_project_bodies(
    modules: &[HirModule],
    resolved_locals: &[HirResolvedLocalType],
) -> Vec<HirBody> {
    build_project_bodies_with_calls(modules, resolved_locals, &[])
}

pub fn build_project_bodies_with_calls(
    modules: &[HirModule],
    resolved_locals: &[HirResolvedLocalType],
    resolved_calls: &[HirResolvedCall],
) -> Vec<HirBody> {
    let mut bodies = Vec::new();
    for module in modules {
        for statement in &module.statements {
            match statement {
                HirStatement::Function(function) => bodies.push(build_body(
                    &module.name,
                    &function.name,
                    function.identity.clone(),
                    &function.parameters,
                    &function.body,
                    resolved_locals,
                    resolved_calls,
                )),
                HirStatement::Impl { methods, .. } => {
                    for method in methods {
                        bodies.push(build_body(
                            &module.name,
                            &method.name,
                            method.identity.clone(),
                            &method.parameters,
                            &method.body,
                            resolved_locals,
                            resolved_calls,
                        ));
                    }
                }
                HirStatement::Main { body, .. } => bodies.push(build_body(
                    &module.name,
                    "main",
                    Some(HirDefinitionId::scoped(
                        module.name.clone(),
                        ["<module-entry>".to_string()],
                        "main",
                        HirDefinitionKind::Function,
                    )),
                    &[],
                    body,
                    resolved_locals,
                    resolved_calls,
                )),
                _ => {}
            }
        }
    }
    bodies
}

fn empty_block(id: u32) -> HirBasicBlock {
    HirBasicBlock {
        id,
        operations: Vec::new(),
        terminator: HirTerminator::Open,
        terminator_expression_types: Vec::new(),
        terminator_local_references: Vec::new(),
        terminator_call_references: Vec::new(),
    }
}

fn build_body(
    module: &str,
    name: &str,
    owner: Option<HirDefinitionId>,
    parameters: &[HirParameter],
    statements: &[HirStatement],
    resolved_locals: &[HirResolvedLocalType],
    resolved_calls: &[HirResolvedCall],
) -> HirBody {
    let mut builder = BodyBuilder {
        module,
        resolved_locals,
        resolved_calls,
        locals: Vec::new(),
        blocks: vec![empty_block(0)],
        loop_targets: Vec::new(),
    };
    let mut environment = HashMap::new();
    for parameter in parameters {
        let ty = builder.lookup_type(&parameter.name, parameter.span);
        let id = builder.add_local(&parameter.name, ty.clone(), parameter.span, true);
        environment.insert(parameter.name.clone(), (id, ty));
    }
    let entry = 0;
    let exit = builder.new_block();
    let end = builder.lower_sequence(statements, entry, &mut environment);
    if matches!(
        &builder.blocks[end as usize].terminator,
        HirTerminator::Open
    ) {
        builder.blocks[end as usize].terminator = HirTerminator::Goto(exit);
    }
    builder.blocks[exit as usize].terminator = HirTerminator::Return(None);
    HirBody {
        module: module.to_string(),
        name: name.to_string(),
        owner,
        entry,
        exit,
        locals: builder.locals,
        blocks: builder.blocks,
    }
}

struct BodyBuilder<'a> {
    module: &'a str,
    resolved_locals: &'a [HirResolvedLocalType],
    resolved_calls: &'a [HirResolvedCall],
    locals: Vec<HirBodyLocal>,
    blocks: Vec<HirBasicBlock>,
    loop_targets: Vec<(u32, u32)>, // (break, continue)
}

impl BodyBuilder<'_> {
    fn new_block(&mut self) -> u32 {
        let id = self.blocks.len() as u32;
        self.blocks.push(empty_block(id));
        id
    }

    fn add_local(&mut self, name: &str, ty: HirType, span: Span, parameter: bool) -> HirLocalId {
        let id = HirLocalId(self.locals.len() as u32);
        self.locals.push(HirBodyLocal {
            id,
            name: name.to_string(),
            ty,
            declaration_span: span,
            parameter,
        });
        id
    }

    fn lookup_type(&self, name: &str, span: Span) -> HirType {
        self.resolved_locals
            .iter()
            .filter(|local| local.module == self.module && local.name == name)
            .min_by_key(|local| {
                local
                    .span
                    .start
                    .abs_diff(span.start)
                    .saturating_add(local.span.end.abs_diff(span.end))
            })
            .map(|local| local.ty.clone())
            .unwrap_or(HirType::Unknown)
    }

    fn lower_sequence(
        &mut self,
        statements: &[HirStatement],
        mut current: u32,
        environment: &mut HashMap<String, (HirLocalId, HirType)>,
    ) -> u32 {
        for statement in statements {
            if !matches!(
                &self.blocks[current as usize].terminator,
                HirTerminator::Open
            ) {
                current = self.new_block();
            }
            match statement {
                HirStatement::Block { statements, .. } => {
                    let mut nested = environment.clone();
                    current = self.lower_sequence(statements, current, &mut nested);
                }
                HirStatement::If {
                    condition,
                    body,
                    else_body,
                    ..
                } => {
                    let then_block = self.new_block();
                    let else_block = self.new_block();
                    let merge_block = self.new_block();
                    self.record_terminator_expressions(current, &[condition], environment);
                    self.blocks[current as usize].terminator = HirTerminator::Branch {
                        condition: condition.clone(),
                        then_block,
                        else_block,
                    };
                    let mut then_env = environment.clone();
                    let then_end = self.lower_sequence(body, then_block, &mut then_env);
                    if matches!(
                        &self.blocks[then_end as usize].terminator,
                        HirTerminator::Open
                    ) {
                        self.blocks[then_end as usize].terminator =
                            HirTerminator::Goto(merge_block);
                    }
                    let mut else_env = environment.clone();
                    let else_end = if let Some(other) = else_body {
                        self.lower_sequence(other, else_block, &mut else_env)
                    } else {
                        else_block
                    };
                    if matches!(
                        &self.blocks[else_end as usize].terminator,
                        HirTerminator::Open
                    ) {
                        self.blocks[else_end as usize].terminator =
                            HirTerminator::Goto(merge_block);
                    }
                    let merged_environment = merge_environments(environment, &then_env, &else_env);
                    *environment = merged_environment;
                    current = merge_block;
                }
                HirStatement::While {
                    condition, body, ..
                } => {
                    let condition_block = self.new_block();
                    let body_block = self.new_block();
                    let exit_block = self.new_block();
                    self.blocks[current as usize].terminator = HirTerminator::Goto(condition_block);
                    self.record_terminator_expressions(condition_block, &[condition], environment);
                    self.blocks[condition_block as usize].terminator = HirTerminator::Branch {
                        condition: condition.clone(),
                        then_block: body_block,
                        else_block: exit_block,
                    };
                    self.loop_targets.push((exit_block, condition_block));
                    let mut loop_env = environment.clone();
                    let body_end = self.lower_sequence(body, body_block, &mut loop_env);
                    self.loop_targets.pop();
                    if matches!(
                        &self.blocks[body_end as usize].terminator,
                        HirTerminator::Open
                    ) {
                        self.blocks[body_end as usize].terminator =
                            HirTerminator::Goto(condition_block);
                    }
                    current = exit_block;
                }
                HirStatement::For {
                    variable,
                    start,
                    end,
                    body,
                    span,
                } => {
                    let body_block = self.new_block();
                    let exit_block = self.new_block();
                    let ty = self.lookup_type(variable, *span);
                    let id = self.add_local(variable, ty.clone(), *span, false);
                    self.record_terminator_expressions(current, &[start, end], environment);
                    self.blocks[current as usize].terminator = HirTerminator::For {
                        variable: variable.clone(),
                        local: id,
                        start: start.clone(),
                        end: end.clone(),
                        body: body_block,
                        exit: exit_block,
                    };
                    self.loop_targets.push((exit_block, current));
                    let mut loop_env = environment.clone();
                    loop_env.insert(variable.clone(), (id, ty));
                    let body_end = self.lower_sequence(body, body_block, &mut loop_env);
                    self.loop_targets.pop();
                    if matches!(
                        &self.blocks[body_end as usize].terminator,
                        HirTerminator::Open
                    ) {
                        self.blocks[body_end as usize].terminator = HirTerminator::Goto(current);
                    }
                    current = exit_block;
                }
                HirStatement::ForEach {
                    variable,
                    iterable,
                    body,
                    span,
                } => {
                    let body_block = self.new_block();
                    let exit_block = self.new_block();
                    let ty = self.lookup_type(variable, *span);
                    let id = self.add_local(variable, ty.clone(), *span, false);
                    self.record_terminator_expressions(current, &[iterable], environment);
                    self.blocks[current as usize].terminator = HirTerminator::ForEach {
                        variable: variable.clone(),
                        local: id,
                        iterable: iterable.clone(),
                        body: body_block,
                        exit: exit_block,
                    };
                    self.loop_targets.push((exit_block, current));
                    let mut loop_env = environment.clone();
                    loop_env.insert(variable.clone(), (id, ty));
                    let body_end = self.lower_sequence(body, body_block, &mut loop_env);
                    self.loop_targets.pop();
                    if matches!(
                        &self.blocks[body_end as usize].terminator,
                        HirTerminator::Open
                    ) {
                        self.blocks[body_end as usize].terminator = HirTerminator::Goto(current);
                    }
                    current = exit_block;
                }
                HirStatement::Match {
                    expression, arms, ..
                } => {
                    let fallback = self.new_block();
                    self.record_terminator_expressions(current, &[expression], environment);
                    let mut arm_targets = Vec::new();
                    let merge = self.new_block();
                    for arm in arms {
                        let target = self.new_block();
                        let mut arm_env = environment.clone();
                        self.bind_pattern(&arm.pattern, arm.span, &mut arm_env);
                        let mut guard_expression_types = Vec::new();
                        let mut guard_local_references = Vec::new();
                        let mut guard_call_references = Vec::new();
                        if let Some(guard) = arm.guard.as_ref() {
                            let mut guard_path = vec![0];
                            self.walk_expression(
                                guard,
                                &mut guard_path,
                                &arm_env,
                                &mut guard_expression_types,
                                &mut guard_local_references,
                            );
                            collect_call_references(
                                guard,
                                &guard_path,
                                self.module,
                                self.resolved_calls,
                                &mut guard_call_references,
                            );
                        }
                        let mut binding_paths = Vec::new();
                        collect_pattern_binding_paths(
                            &arm.pattern,
                            &mut Vec::new(),
                            &mut binding_paths,
                        );
                        let bindings = binding_paths
                            .into_iter()
                            .filter_map(|(name, path)| {
                                arm_env.get(&name).map(|(local, _)| HirPatternBinding {
                                    name,
                                    local: *local,
                                    path,
                                })
                            })
                            .collect();
                        arm_targets.push(HirMatchTarget {
                            pattern: arm.pattern.clone(),
                            bindings,
                            guard: arm.guard.clone(),
                            guard_expression_types,
                            guard_local_references,
                            guard_call_references,
                            block: target,
                        });
                        let arm_end = self.lower_sequence(&arm.body, target, &mut arm_env);
                        if matches!(
                            &self.blocks[arm_end as usize].terminator,
                            HirTerminator::Open
                        ) {
                            self.blocks[arm_end as usize].terminator = HirTerminator::Goto(merge);
                        }
                    }
                    self.blocks[current as usize].terminator = HirTerminator::Match {
                        value: expression.clone(),
                        arms: arm_targets,
                        fallback,
                    };
                    self.blocks[fallback as usize].terminator = HirTerminator::Unreachable;
                    current = merge;
                }
                HirStatement::Return { value, .. } => {
                    if let Some(value) = value.as_ref() {
                        self.record_terminator_expressions(current, &[value], environment);
                    }
                    self.blocks[current as usize].terminator = HirTerminator::Return(value.clone());
                }
                HirStatement::Break { .. } => {
                    let target = self.loop_targets.last().map(|targets| targets.0);
                    self.blocks[current as usize].terminator = if target.is_some() {
                        HirTerminator::Break(target)
                    } else {
                        HirTerminator::Unreachable
                    };
                }
                HirStatement::Continue { .. } => {
                    let target = self.loop_targets.last().map(|targets| targets.1);
                    self.blocks[current as usize].terminator = if target.is_some() {
                        HirTerminator::Continue(target)
                    } else {
                        HirTerminator::Unreachable
                    };
                }
                HirStatement::VariableDeclaration { name, span, .. } => {
                    self.record_operation(current, statement.clone(), environment);
                    let ty = self.lookup_type(name, *span);
                    let id = self.add_local(name, ty.clone(), *span, false);
                    environment.insert(name.clone(), (id, ty));
                }
                HirStatement::UninitializedVariable { name, span, .. } => {
                    let ty = self.lookup_type(name, *span);
                    let id = self.add_local(name, ty.clone(), *span, false);
                    environment.insert(name.clone(), (id, ty));
                    self.record_operation(current, statement.clone(), environment);
                }
                HirStatement::ConstDeclaration { name, span, .. } => {
                    self.record_operation(current, statement.clone(), environment);
                    let ty = self.lookup_type(name, *span);
                    let id = self.add_local(name, ty.clone(), *span, false);
                    environment.insert(name.clone(), (id, ty));
                }
                _ => self.record_operation(current, statement.clone(), environment),
            }
        }
        current
    }

    fn bind_pattern(
        &mut self,
        pattern: &HirPattern,
        span: Span,
        env: &mut HashMap<String, (HirLocalId, HirType)>,
    ) {
        match pattern {
            HirPattern::Identifier(name) | HirPattern::At { name, .. } => {
                let ty = self.lookup_type(name, span);
                let id = self.add_local(name, ty.clone(), span, false);
                env.insert(name.clone(), (id, ty));
                if let HirPattern::At { pattern, .. } = pattern {
                    self.bind_pattern(pattern, span, env);
                }
            }
            HirPattern::Variant { bindings, .. } | HirPattern::Tuple(bindings) => {
                for binding in bindings {
                    self.bind_pattern(binding, span, env);
                }
            }
            HirPattern::Or(alternatives) => {
                if let Some(first) = alternatives.first() {
                    let mut names = Vec::new();
                    collect_pattern_names(first, &mut names);
                    names.sort();
                    names.dedup();
                    for name in names {
                        let ty = self.lookup_type(&name, span);
                        let id = self.add_local(&name, ty.clone(), span, false);
                        env.insert(name, (id, ty));
                    }
                }
            }
            HirPattern::Struct { fields, .. } => {
                for (_, binding) in fields {
                    self.bind_pattern(binding, span, env);
                }
            }
            _ => {}
        }
    }

    fn record_terminator_expressions(
        &mut self,
        block: u32,
        expressions: &[&HirExpression],
        env: &HashMap<String, (HirLocalId, HirType)>,
    ) {
        let mut types = Vec::new();
        let mut references = Vec::new();
        let mut call_references = Vec::new();
        for (root_index, expression) in expressions.iter().enumerate() {
            let mut path = vec![root_index as u32];
            self.walk_expression(expression, &mut path, env, &mut types, &mut references);
            collect_call_references(
                expression,
                &path,
                self.module,
                self.resolved_calls,
                &mut call_references,
            );
        }
        self.blocks[block as usize].terminator_expression_types = types;
        self.blocks[block as usize].terminator_local_references = references;
        self.blocks[block as usize].terminator_call_references = call_references;
    }

    fn record_operation(
        &mut self,
        block: u32,
        statement: HirStatement,
        env: &HashMap<String, (HirLocalId, HirType)>,
    ) {
        let roots = statement_expressions(&statement);
        let mut expression_types = Vec::new();
        let mut local_references = Vec::new();
        let mut call_references = Vec::new();
        for (root_index, expression) in roots.iter().enumerate() {
            let mut path = vec![root_index as u32];
            self.walk_expression(
                expression,
                &mut path,
                env,
                &mut expression_types,
                &mut local_references,
            );
            collect_call_references(
                expression,
                &path,
                self.module,
                self.resolved_calls,
                &mut call_references,
            );
        }
        self.blocks[block as usize]
            .operations
            .push(HirBodyOperation {
                statement,
                expression_types,
                local_references,
                call_references,
            });
    }

    fn walk_expression(
        &self,
        expression: &HirExpression,
        path: &mut Vec<u32>,
        env: &HashMap<String, (HirLocalId, HirType)>,
        types: &mut Vec<HirTypedExpression>,
        references: &mut Vec<HirLocalReference>,
    ) -> HirType {
        let ty = match expression {
            HirExpression::Number(_) | HirExpression::WideNumber(_) => HirType::Num,
            HirExpression::Float(_) => HirType::Float,
            HirExpression::Boolean(_) => HirType::Bool,
            HirExpression::String(_) => HirType::String,
            HirExpression::Identifier(name) => {
                if let Some((id, ty)) = env.get(name) {
                    references.push(HirLocalReference {
                        path: path.clone(),
                        local: *id,
                    });
                    ty.clone()
                } else {
                    HirType::Unknown
                }
            }
            HirExpression::Array(values) => {
                let mut element: Option<HirType> = None;
                let mut inconsistent = false;
                for (index, value) in values.iter().enumerate() {
                    path.push(index as u32);
                    let actual = self.walk_expression(value, path, env, types, references);
                    path.pop();
                    if actual == HirType::Unknown {
                        continue;
                    }
                    if let Some(previous) = element.as_ref() {
                        if previous != &actual {
                            inconsistent = true;
                        }
                    } else {
                        element = Some(actual);
                    }
                }
                HirType::Array(Box::new(if inconsistent {
                    HirType::Unknown
                } else {
                    element.unwrap_or(HirType::Unknown)
                }))
            }
            HirExpression::Tuple(values) => {
                let mut elements = Vec::new();
                for (index, value) in values.iter().enumerate() {
                    path.push(index as u32);
                    elements.push(self.walk_expression(value, path, env, types, references));
                    path.pop();
                }
                HirType::Tuple(elements)
            }
            HirExpression::Index { array, index } => {
                path.push(0);
                let array_ty = self.walk_expression(array, path, env, types, references);
                path.pop();
                path.push(1);
                self.walk_expression(index, path, env, types, references);
                path.pop();
                match array_ty {
                    HirType::Array(inner) | HirType::Iterator(inner) => *inner,
                    HirType::Tuple(elements) => match index.as_ref() {
                        HirExpression::Number(position) if *position >= 0 => elements
                            .get(*position as usize)
                            .cloned()
                            .unwrap_or(HirType::Unknown),
                        _ => HirType::Unknown,
                    },
                    _ => HirType::Unknown,
                }
            }
            HirExpression::Property { object, .. } => {
                path.push(0);
                self.walk_expression(object, path, env, types, references);
                path.pop();
                HirType::Unknown
            }
            HirExpression::MethodCall {
                object, arguments, ..
            } => {
                path.push(0);
                self.walk_expression(object, path, env, types, references);
                path.pop();
                for (index, arg) in arguments.iter().enumerate() {
                    path.push(index as u32 + 1);
                    self.walk_expression(arg, path, env, types, references);
                    path.pop();
                }
                HirType::Unknown
            }
            HirExpression::Call { arguments, .. } => {
                for (index, arg) in arguments.iter().enumerate() {
                    path.push(index as u32);
                    self.walk_expression(arg, path, env, types, references);
                    path.pop();
                }
                HirType::Unknown
            }
            HirExpression::Await(value) => {
                path.push(0);
                let inner = self.walk_expression(value, path, env, types, references);
                path.pop();
                match inner {
                    HirType::Task(value) => *value,
                    _ => HirType::Unknown,
                }
            }
            HirExpression::Conversion {
                expression,
                target_type,
                ..
            } => {
                path.push(0);
                self.walk_expression(expression, path, env, types, references);
                path.pop();
                match target_type.to_string().as_str() {
                    "Num" | "num" => HirType::Num,
                    "Float" | "float" => HirType::Float,
                    "Bool" | "bool" => HirType::Bool,
                    "String" | "string" => HirType::String,
                    _ => HirType::Unknown,
                }
            }
            HirExpression::StructConstructor { fields, .. } => {
                for (index, (_, value)) in fields.iter().enumerate() {
                    path.push(index as u32);
                    self.walk_expression(value, path, env, types, references);
                    path.pop();
                }
                HirType::Unknown
            }
            HirExpression::EnumConstructor { arguments, .. } => {
                for (index, value) in arguments.iter().enumerate() {
                    path.push(index as u32);
                    self.walk_expression(value, path, env, types, references);
                    path.pop();
                }
                HirType::Unknown
            }
            HirExpression::Binary {
                left,
                operator,
                right,
            } => {
                path.push(0);
                let left_ty = self.walk_expression(left, path, env, types, references);
                path.pop();
                path.push(1);
                let right_ty = self.walk_expression(right, path, env, types, references);
                path.pop();
                match operator {
                    HirOperator::Equal
                    | HirOperator::NotEqual
                    | HirOperator::Less
                    | HirOperator::LessEqual
                    | HirOperator::Greater
                    | HirOperator::GreaterEqual
                    | HirOperator::And
                    | HirOperator::Or => HirType::Bool,
                    _ if left_ty == right_ty => left_ty,
                    _ => HirType::Unknown,
                }
            }
            HirExpression::Unary {
                operator,
                expression,
            } => {
                path.push(0);
                let inner = self.walk_expression(expression, path, env, types, references);
                path.pop();
                match operator {
                    HirUnaryOperator::Not => HirType::Bool,
                    _ => inner,
                }
            }
        };
        types.push(HirTypedExpression {
            path: path.clone(),
            ty: ty.clone(),
        });
        ty
    }
}

fn collect_call_references(
    expression: &HirExpression,
    path: &[u32],
    module: &str,
    resolved_calls: &[HirResolvedCall],
    output: &mut Vec<HirCallReference>,
) {
    fn recurse(
        value: &HirExpression,
        index: u32,
        path: &[u32],
        module: &str,
        resolved_calls: &[HirResolvedCall],
        output: &mut Vec<HirCallReference>,
    ) {
        let mut next = path.to_vec();
        next.push(index);
        collect_call_references(value, &next, module, resolved_calls, output);
    }
    match expression {
        HirExpression::Call {
            arguments, span, ..
        } => {
            if let Some(call) = resolved_calls
                .iter()
                .find(|call| call.module == module && call.span == *span)
            {
                output.push(HirCallReference {
                    path: path.to_vec(),
                    call: call.clone(),
                });
            }
            for (index, argument) in arguments.iter().enumerate() {
                recurse(argument, index as u32, path, module, resolved_calls, output);
            }
        }
        HirExpression::MethodCall {
            object,
            arguments,
            span,
            ..
        } => {
            if let Some(call) = resolved_calls
                .iter()
                .find(|call| call.module == module && call.span == *span)
            {
                output.push(HirCallReference {
                    path: path.to_vec(),
                    call: call.clone(),
                });
            }
            recurse(object, 0, path, module, resolved_calls, output);
            for (index, argument) in arguments.iter().enumerate() {
                recurse(
                    argument,
                    index as u32 + 1,
                    path,
                    module,
                    resolved_calls,
                    output,
                );
            }
        }
        HirExpression::Array(values) | HirExpression::Tuple(values) => {
            for (index, value) in values.iter().enumerate() {
                recurse(value, index as u32, path, module, resolved_calls, output);
            }
        }
        HirExpression::Index { array, index } => {
            recurse(array, 0, path, module, resolved_calls, output);
            recurse(index, 1, path, module, resolved_calls, output);
        }
        HirExpression::Property { object, .. } => {
            recurse(object, 0, path, module, resolved_calls, output)
        }
        HirExpression::Await(value)
        | HirExpression::Unary {
            expression: value, ..
        }
        | HirExpression::Conversion {
            expression: value, ..
        } => recurse(value, 0, path, module, resolved_calls, output),
        HirExpression::StructConstructor { fields, .. } => {
            for (index, (_, value)) in fields.iter().enumerate() {
                recurse(value, index as u32, path, module, resolved_calls, output);
            }
        }
        HirExpression::EnumConstructor { arguments, .. } => {
            for (index, value) in arguments.iter().enumerate() {
                recurse(value, index as u32, path, module, resolved_calls, output);
            }
        }
        HirExpression::Binary { left, right, .. } => {
            recurse(left, 0, path, module, resolved_calls, output);
            recurse(right, 1, path, module, resolved_calls, output);
        }
        HirExpression::Number(_)
        | HirExpression::WideNumber(_)
        | HirExpression::Float(_)
        | HirExpression::Boolean(_)
        | HirExpression::String(_)
        | HirExpression::Identifier(_) => {}
    }
}

fn collect_pattern_binding_paths(
    pattern: &HirPattern,
    path: &mut Vec<u32>,
    output: &mut Vec<(String, Vec<u32>)>,
) {
    match pattern {
        HirPattern::Identifier(name) => output.push((name.clone(), path.clone())),
        HirPattern::At { name, pattern } => {
            output.push((name.clone(), path.clone()));
            collect_pattern_binding_paths(pattern, path, output);
        }
        HirPattern::Tuple(items) => {
            for (index, item) in items.iter().enumerate() {
                path.push(index as u32);
                collect_pattern_binding_paths(item, path, output);
                path.pop();
            }
        }
        HirPattern::Or(alternatives) => {
            if let Some(first) = alternatives.first() {
                collect_pattern_binding_paths(first, path, output);
            }
        }
        // Nominal payload and named-field paths require HIR layout projection
        // metadata. They remain on the compatibility backend path for now.
        HirPattern::Variant { .. }
        | HirPattern::Struct { .. }
        | HirPattern::Wildcard
        | HirPattern::Number(_)
        | HirPattern::Float(_)
        | HirPattern::String(_)
        | HirPattern::Boolean(_)
        | HirPattern::Range { .. } => {}
    }
}

fn collect_pattern_names(pattern: &HirPattern, names: &mut Vec<String>) {
    match pattern {
        HirPattern::Identifier(name) => names.push(name.clone()),
        HirPattern::At { name, pattern } => {
            names.push(name.clone());
            collect_pattern_names(pattern, names);
        }
        HirPattern::Variant { bindings, .. }
        | HirPattern::Tuple(bindings)
        | HirPattern::Or(bindings) => {
            if let HirPattern::Or(alternatives) = pattern {
                if let Some(first) = alternatives.first() {
                    collect_pattern_names(first, names);
                }
            } else {
                for binding in bindings {
                    collect_pattern_names(binding, names);
                }
            }
        }
        HirPattern::Struct { fields, .. } => {
            for (_, binding) in fields {
                collect_pattern_names(binding, names);
            }
        }
        _ => {}
    }
}

fn statement_expressions(statement: &HirStatement) -> Vec<&HirExpression> {
    match statement {
        HirStatement::VariableDeclaration { value, .. }
        | HirStatement::ConstDeclaration { value, .. } => vec![value],
        HirStatement::Assignment { target, value, .. } => vec![target, value],
        HirStatement::Expression { expression, .. } | HirStatement::Defer { expression, .. } => {
            vec![expression]
        }
        HirStatement::Return {
            value: Some(value), ..
        } => vec![value],
        _ => Vec::new(),
    }
}

fn merge_environments(
    original: &HashMap<String, (HirLocalId, HirType)>,
    left: &HashMap<String, (HirLocalId, HirType)>,
    right: &HashMap<String, (HirLocalId, HirType)>,
) -> HashMap<String, (HirLocalId, HirType)> {
    // Only bindings that existed before the branch can escape it. Newly declared
    // branch-local names remain local to their predecessor blocks; a later HIR
    // pass can introduce explicit phi locals where the language permits it.
    let mut merged = original.clone();
    for (name, (original_id, original_ty)) in original {
        if let (Some((_, left_ty)), Some((_, right_ty))) = (left.get(name), right.get(name)) {
            let merged_ty = if left_ty == right_ty {
                left_ty.clone()
            } else if left_ty == original_ty {
                right_ty.clone()
            } else if right_ty == original_ty {
                left_ty.clone()
            } else {
                HirType::Unknown
            };
            merged.insert(name.clone(), (*original_id, merged_ty));
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_explicit_branch_blocks_and_resolved_local_references() {
        let parameter_span = Span::new(0, 1);
        let local_span = Span::new(5, 6);
        let parameter = HirParameter {
            name: "enabled".into(),
            type_name: Some("Bool".into()),
            span: parameter_span,
        };
        let function = HirFunction {
            identity: Some(HirDefinitionId::new(
                "app",
                "run",
                HirDefinitionKind::Function,
            )),
            name: "run".into(),
            generic_parameters: Vec::new(),
            parameters: vec![parameter],
            return_type: None,
            is_async: false,
            body: vec![HirStatement::If {
                condition: HirExpression::Identifier("enabled".into()),
                body: vec![HirStatement::VariableDeclaration {
                    name: "copy".into(),
                    declared_type: None,
                    value: HirExpression::Identifier("enabled".into()),
                    span: local_span,
                }],
                else_body: Some(vec![HirStatement::Expression {
                    expression: HirExpression::Boolean(false),
                    span: Span::new(7, 8),
                }]),
                span: Span::new(2, 9),
            }],
            span: Span::new(0, 10),
        };
        let module = HirModule {
            name: "app".into(),
            path: "app.fus".into(),
            imports: Vec::new(),
            public_symbols: Vec::new(),
            local_public_symbols: Vec::new(),
            private_symbols: Vec::new(),
            statements: vec![HirStatement::Function(function)],
        };
        let resolved = vec![
            HirResolvedLocalType {
                module: "app".into(),
                name: "enabled".into(),
                span: parameter_span,
                ty: HirType::Bool,
            },
            HirResolvedLocalType {
                module: "app".into(),
                name: "copy".into(),
                span: local_span,
                ty: HirType::Bool,
            },
        ];
        let bodies = build_project_bodies(&[module], &resolved);
        assert_eq!(bodies.len(), 1);
        let body = &bodies[0];
        assert!(body.blocks.len() >= 4);
        assert!(matches!(
            &body.blocks[body.entry as usize].terminator,
            HirTerminator::Branch { .. }
        ));
        assert_eq!(
            body.blocks[body.entry as usize].terminator_local_references[0].local,
            HirLocalId(0)
        );
        assert_eq!(body.locals[0].ty, HirType::Bool);
        assert!(body
            .blocks
            .iter()
            .flat_map(|block| &block.operations)
            .any(|operation| operation
                .local_references
                .iter()
                .any(|reference| reference.local == HirLocalId(0))));
    }

    #[test]
    fn lowers_loop_control_to_explicit_targets() {
        let span = Span::new(0, 1);
        let module = HirModule {
            name: "app".into(),
            path: "app.fus".into(),
            imports: Vec::new(),
            public_symbols: Vec::new(),
            local_public_symbols: Vec::new(),
            private_symbols: Vec::new(),
            statements: vec![HirStatement::Main {
                body: vec![HirStatement::While {
                    condition: HirExpression::Boolean(true),
                    body: vec![HirStatement::If {
                        condition: HirExpression::Boolean(false),
                        body: vec![HirStatement::Break { span }],
                        else_body: Some(vec![HirStatement::Continue { span }]),
                        span,
                    }],
                    span,
                }],
                span,
            }],
        };
        let bodies = build_project_bodies(&[module], &[]);
        let body = &bodies[0];
        assert!(body
            .blocks
            .iter()
            .any(|block| matches!(&block.terminator, HirTerminator::Branch { .. })));
        assert!(body
            .blocks
            .iter()
            .any(|block| matches!(&block.terminator, HirTerminator::Break(Some(_)))));
        assert!(body
            .blocks
            .iter()
            .any(|block| matches!(&block.terminator, HirTerminator::Continue(Some(_)))));
    }

    #[test]
    fn annotates_tuple_expression_shapes() {
        let expression =
            HirExpression::Tuple(vec![HirExpression::Number(1), HirExpression::Boolean(true)]);
        let mut builder = BodyBuilder {
            module: "app",
            resolved_locals: &[],
            resolved_calls: &[],
            locals: Vec::new(),
            blocks: vec![empty_block(0)],
            loop_targets: Vec::new(),
        };
        let mut types = Vec::new();
        let mut refs = Vec::new();
        let ty = builder.walk_expression(
            &expression,
            &mut Vec::new(),
            &HashMap::new(),
            &mut types,
            &mut refs,
        );
        assert_eq!(ty, HirType::Tuple(vec![HirType::Num, HirType::Bool]));
        assert!(types.iter().any(|item| item.ty == HirType::Bool));
    }
    #[test]
    fn records_resolved_calls_by_expression_tree_path() {
        let call_span = Span::new(20, 28);
        let declaration = HirDefinitionId::new("math", "double", HirDefinitionKind::Function);
        let function = HirFunction {
            identity: Some(HirDefinitionId::new(
                "app",
                "caller",
                HirDefinitionKind::Function,
            )),
            name: "caller".into(),
            generic_parameters: Vec::new(),
            parameters: Vec::new(),
            return_type: Some("num".into()),
            is_async: false,
            body: vec![HirStatement::Return {
                value: Some(HirExpression::Call {
                    name: "double".into(),
                    arguments: vec![HirExpression::Number(4)],
                    generic_arguments: Vec::new(),
                    span: call_span,
                }),
                span: Span::new(10, 30),
            }],
            span: Span::new(0, 31),
        };
        let module = HirModule {
            name: "app".into(),
            path: "app.fus".into(),
            imports: Vec::new(),
            public_symbols: Vec::new(),
            local_public_symbols: Vec::new(),
            private_symbols: Vec::new(),
            statements: vec![HirStatement::Function(function)],
        };
        let resolved = HirResolvedCall {
            module: "app".into(),
            span: call_span,
            kind: HirCallKind::Function,
            declaration: declaration.clone(),
            receiver_type: None,
            parameter_types: vec![None],
            argument_types: vec![None],
            return_type: None,
            generic_parameters: Vec::new(),
            explicit_generic_arguments: Vec::new(),
            is_async: false,
            receiver_hir_type: None,
            parameter_hir_types: vec![HirType::Num],
            argument_hir_types: vec![HirType::Num],
            return_hir_type: HirType::Num,
        };
        let bodies = build_project_bodies_with_calls(&[module], &[], &[resolved]);
        let references = &bodies[0].blocks[0].terminator_call_references;
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].path, vec![0]);
        assert_eq!(references[0].call.declaration, declaration);
    }

    #[test]
    fn match_guard_call_metadata_survives_arm_body_termination() {
        let guard_span = Span::new(12, 24);
        let declaration = HirDefinitionId::new("checks", "allowed", HirDefinitionKind::Function);
        let guard = HirExpression::Call {
            name: "allowed".into(),
            arguments: Vec::new(),
            generic_arguments: Vec::new(),
            span: guard_span,
        };
        let function = HirFunction {
            identity: Some(HirDefinitionId::new(
                "app",
                "choose",
                HirDefinitionKind::Function,
            )),
            name: "choose".into(),
            generic_parameters: Vec::new(),
            parameters: Vec::new(),
            return_type: Some("num".into()),
            is_async: false,
            body: vec![HirStatement::Match {
                expression: HirExpression::Number(1),
                arms: vec![HirMatchArm {
                    pattern: HirPattern::Number(1),
                    guard: Some(guard),
                    body: vec![HirStatement::Return {
                        value: Some(HirExpression::Number(7)),
                        span: Span::new(25, 30),
                    }],
                    span: Span::new(10, 31),
                }],
                span: Span::new(5, 32),
            }],
            span: Span::new(0, 33),
        };
        let module = HirModule {
            name: "app".into(),
            path: "app.fus".into(),
            imports: Vec::new(),
            public_symbols: Vec::new(),
            local_public_symbols: Vec::new(),
            private_symbols: Vec::new(),
            statements: vec![HirStatement::Function(function)],
        };
        let resolved = HirResolvedCall {
            module: "app".into(),
            span: guard_span,
            kind: HirCallKind::Function,
            declaration: declaration.clone(),
            receiver_type: None,
            parameter_types: Vec::new(),
            argument_types: Vec::new(),
            return_type: None,
            generic_parameters: Vec::new(),
            explicit_generic_arguments: Vec::new(),
            is_async: false,
            receiver_hir_type: None,
            parameter_hir_types: Vec::new(),
            argument_hir_types: Vec::new(),
            return_hir_type: HirType::Bool,
        };
        let bodies = build_project_bodies_with_calls(&[module], &[], &[resolved]);
        let match_target = bodies[0]
            .blocks
            .iter()
            .find_map(|block| match &block.terminator {
                HirTerminator::Match { arms, .. } => arms.first(),
                _ => None,
            })
            .unwrap();
        assert_eq!(match_target.guard_call_references.len(), 1);
        assert_eq!(
            match_target.guard_call_references[0].call.declaration,
            declaration
        );
        let arm_block = bodies[0]
            .blocks
            .iter()
            .find(|block| block.id == match_target.block)
            .unwrap();
        assert!(matches!(
            &arm_block.terminator,
            HirTerminator::Return(Some(_))
        ));
    }
}
