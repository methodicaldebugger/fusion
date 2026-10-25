//! Source-level expansion for Fusion's built-in `#[derive(...)]` traits.
//!
//! Expansion happens after parsing and before name resolution/type checking. The
//! generated implementations are ordinary `Statement::Impl` nodes, so they pass
//! through the same trait validation and method registration as handwritten impls.

use crate::ast::{Parameter, Program, Statement, TraitMethod};
use crate::span::Span;

const BUILTIN_DERIVES: &[&str] = &["Debug", "Clone", "PartialEq"];

fn parameter(name: &str, ty: &str, span: Span) -> Parameter {
    Parameter {
        name: name.to_string(),
        name_span: span,
        type_name: Some(ty.to_string()),
        span,
    }
}

fn trait_method(name: &str, params: Vec<Parameter>, return_type: &str, span: Span) -> TraitMethod {
    TraitMethod {
        name: name.to_string(),
        parameters: params,
        return_type: Some(return_type.to_string()),
        span,
    }
}

fn builtin_trait(name: &str, span: Span) -> Statement {
    let self_param = || parameter("self", "Self", span);
    let methods = match name {
        "Debug" => vec![trait_method("debug", vec![self_param()], "string", span)],
        "Clone" => vec![trait_method("clone", vec![self_param()], "Self", span)],
        "PartialEq" => vec![trait_method("eq", vec![self_param(), parameter("other", "Self", span)], "bool", span)],
        _ => unreachable!("only built-in derive traits are synthesized"),
    };
    Statement::Trait { name: name.to_string(), methods, span }
}

fn generated_impl(trait_name: &str, type_name: &str, span: Span) -> Statement {
    let self_param = parameter("self", type_name, span);
    let self_expr = crate::ast::Expression::Identifier { name: "self".into(), span };
    let (method_name, parameters, return_type, value) = match trait_name {
        "Clone" => (
            "clone",
            vec![self_param],
            type_name.to_string(),
            self_expr,
        ),
        "Debug" => (
            "debug",
            vec![self_param],
            "string".to_string(),
            crate::ast::Expression::Call {
                name: "__fusion_derive_debug".into(),
                arguments: vec![self_expr],
                generic_arguments: Vec::new(),
                span,
            },
        ),
        "PartialEq" => {
            let other = crate::ast::Expression::Identifier { name: "other".into(), span };
            (
                "eq",
                vec![self_param, parameter("other", type_name, span)],
                "bool".to_string(),
                crate::ast::Expression::Call {
                    name: "__fusion_derive_eq".into(),
                    arguments: vec![self_expr, other],
                    generic_arguments: Vec::new(),
                    span,
                },
            )
        }
        _ => unreachable!("only built-in derive traits are generated"),
    };
    let function = Statement::Function {
        name: method_name.into(),
        generic_parameters: Vec::new(),
        parameters,
        return_type: Some(return_type),
        body: vec![Statement::Return { value: Some(value), span }],
        is_async: false,
        span,
    };
    Statement::Impl {
        trait_name: Some(trait_name.to_string()),
        type_name: type_name.to_string(),
        methods: vec![function],
        span,
    }
}

/// Expand derives into regular trait declarations (when needed) and ordinary
/// trait implementation blocks. This pass is invoked once by the parser; the
/// original derive list is retained so the checker can diagnose unsupported and
/// duplicate attributes.
pub fn expand_derives(mut program: Program) -> Program {
    let span = program.span;
    let existing_traits = program.statements.iter().filter_map(|statement| {
        if let Statement::Trait { name, .. } = statement { Some(name.clone()) } else { None }
    }).collect::<std::collections::HashSet<_>>();

    let mut generated_traits = Vec::new();
    let mut generated_impls = Vec::new();

    for statement in &mut program.statements {
        if let Statement::Struct { name, derives, span: struct_span, .. } = statement {
            let requested = derives.clone();
            for trait_name in requested {
                if !BUILTIN_DERIVES.iter().any(|known| *known == trait_name.as_str()) {
                    // Keep unsupported names for the type checker to report with the
                    // struct's source span, rather than silently dropping them.
                    continue;
                }
                if !existing_traits.contains(&trait_name)
                    && !generated_traits.iter().any(|t| matches!(t, Statement::Trait { name, .. } if name == &trait_name))
                {
                    generated_traits.push(builtin_trait(&trait_name, span));
                }
                generated_impls.push(generated_impl(&trait_name, name, *struct_span));
            }
        }
    }

    // Put generated impls before user statements so that an explicit inherent
    // implementation later in the source can override a derived method in the
    // interpreter's method table. Trait declarations are globally collected by
    // the checker before any impl is validated, so declaration order is safe.
    let mut expanded = generated_traits;
    expanded.extend(generated_impls);
    expanded.append(&mut program.statements);
    program.statements = expanded;
    program
}
