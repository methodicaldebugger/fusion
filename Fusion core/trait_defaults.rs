//! Default methods in traits.
//!
//! A trait method written with a body is a *default method*:
//!
//! ```text
//! trait Greeter:
//!     fn name(self: Self) -> string
//!     fn greet(self: Self) -> string:
//!         return "hello " + self.name()
//! ```
//!
//! Like `#[derive]`, defaults are expanded right after parsing and before name resolution and
//! type checking: every `impl Trait for Type` that does not define a default method gets an
//! ordinary copy of it, with `Self` in the signature replaced by `Type`. Everything downstream
//! (checker, interpreter, HIR, LLVM backend) therefore only ever sees ordinary impl methods.
//!
//! Current limits, all reported by existing diagnostics rather than silently ignored:
//! - The trait and the `impl` must be in the same source file. An impl in another file still
//!   gets the usual "missing method" error for a default it did not copy.
//! - Default methods cannot be generic.
//! - A default body is type-checked only through the impls that receive it, and `Self` inside
//!   the body (as opposed to the signature) keeps the checker's existing loose treatment.

use std::collections::HashMap;

use crate::ast::{Parameter, Program, Statement, TraitMethod};
use crate::type_expr::TypeExpr;

fn substitute_self(annotation: &str, type_name: &str) -> String {
    match TypeExpr::parse(annotation) {
        Ok(expr) => expr.substitute_name("Self", type_name).to_string(),
        Err(_) => annotation.to_string(),
    }
}

/// Build the impl method for `type_name` from a trait's default method.
fn instantiate(method: &TraitMethod, body: &[Statement], type_name: &str) -> Statement {
    let parameters = method
        .parameters
        .iter()
        .map(|parameter| Parameter {
            name: parameter.name.clone(),
            name_span: parameter.name_span,
            type_name: parameter
                .type_name
                .as_ref()
                .map(|annotation| substitute_self(annotation, type_name)),
            span: parameter.span,
        })
        .collect();
    Statement::Function {
        name: method.name.clone(),
        generic_parameters: Vec::new(),
        parameters,
        return_type: method
            .return_type
            .as_ref()
            .map(|annotation| substitute_self(annotation, type_name)),
        body: body.to_vec(),
        is_async: false,
        foreign_abi: None,
        link_name: None,
        span: method.span,
    }
}

/// Copy each trait's default methods into the impls of that trait that do not override them.
pub fn expand_trait_defaults(mut program: Program) -> Program {
    let mut defaults: HashMap<String, Vec<TraitMethod>> = HashMap::new();
    for statement in &program.statements {
        if let Statement::Trait { name, methods, .. } = statement {
            let with_default: Vec<TraitMethod> = methods
                .iter()
                .filter(|method| method.default_body.is_some())
                .cloned()
                .collect();
            if !with_default.is_empty() {
                defaults.insert(name.clone(), with_default);
            }
        }
    }
    if defaults.is_empty() {
        return program;
    }

    for statement in &mut program.statements {
        if let Statement::Impl {
            trait_name: Some(trait_name),
            type_name,
            methods,
            ..
        } = statement
        {
            let Some(trait_defaults) = defaults.get(trait_name.as_str()) else {
                continue;
            };
            for method in trait_defaults {
                let overridden = methods.iter().any(
                    |existing| matches!(existing, Statement::Function { name, .. } if name == &method.name),
                );
                if overridden {
                    continue;
                }
                if let Some(body) = &method.default_body {
                    methods.push(instantiate(method, body, type_name.as_str()));
                }
            }
        }
    }
    program
}

#[cfg(test)]
mod tests {
    use crate::interpreter::Interpreter;
    use crate::lexer::{BlockMode, Lexer};
    use crate::parser::Parser;
    use crate::type_checker::TypeChecker;

    fn compile(source: &str) -> Result<crate::ast::Program, String> {
        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        let tokens = lexer.tokenize().map_err(|error| error.to_string())?;
        let program = Parser::new(tokens)
            .parse()
            .map_err(|error| error.to_string())?;
        TypeChecker::new()
            .check(&program)
            .map_err(|error| error.to_string())?;
        Ok(program)
    }

    fn run(source: &str) -> Vec<String> {
        let program = compile(source).expect("program should compile");
        let mut interpreter = Interpreter::new();
        interpreter.execute(&program);
        interpreter.output().to_vec()
    }

    const GREETER: &str = r#"struct Person:
    label: string

struct Robot:
    label: string

trait Greeter:
    fn who(self: Self) -> string
    fn greet(self: Self) -> string:
        return "hello " + self.who()

impl Greeter for Person:
    fn who(self: Person) -> string:
        return self.label

impl Greeter for Robot:
    fn who(self: Robot) -> string:
        return self.label
    fn greet(self: Robot) -> string:
        return "beep " + self.who()

main:
    p = Person(label: "Fusion")
    r = Robot(label: "R2")
    print(p.greet())
    print(r.greet())
"#;

    #[test]
    fn impls_inherit_defaults_and_may_override_them() {
        assert_eq!(run(GREETER), vec!["hello Fusion", "beep R2"]);
    }

    #[test]
    fn required_methods_without_defaults_are_still_required() {
        let source = GREETER.replace(
            "impl Greeter for Person:\n    fn who(self: Person) -> string:\n        return self.label\n",
            "impl Greeter for Person:\n    fn other(self: Person) -> string:\n        return self.label\n",
        );
        let error = compile(&source).expect_err("the required method is missing");
        assert!(error.contains("missing method 'who'"), "{error}");
    }

    #[test]
    fn a_default_method_may_call_another_default_method() {
        let source = r#"struct Counter:
    value: num

trait Stepper:
    fn current(self: Self) -> num
    fn plus_one(self: Self) -> num:
        return self.current() + 1
    fn plus_two(self: Self) -> num:
        return self.plus_one() + 1

impl Stepper for Counter:
    fn current(self: Counter) -> num:
        return self.value

main:
    c = Counter(value: 40)
    print(c.plus_two())
"#;
        assert_eq!(run(source), vec!["42"]);
    }

    #[test]
    fn brace_style_traits_support_defaults() {
        let source = r#"struct Point { x: num }

trait Describe {
    fn value(self: Self) -> num
    fn doubled(self: Self) -> num {
        return self.value() * 2
    }
}

impl Describe for Point {
    fn value(self: Point) -> num {
        return self.x
    }
}

main {
    p = Point { x: 21 }
    print(p.doubled())
}
"#;
        assert_eq!(run(source), vec!["42"]);
    }

    #[test]
    fn traits_without_defaults_are_unchanged() {
        let source = r#"struct Person:
    name: string

trait Greeter:
    fn greet(person: Person) -> string

impl Greeter for Person:
    fn greet(person: Person) -> string:
        return "hello " + person.name

main:
    p = Person(name: "Fusion")
    print(p.greet())
"#;
        assert_eq!(run(source), vec!["hello Fusion"]);
    }

    #[test]
    fn generic_default_methods_are_rejected_at_parse_time() {
        let source = "struct P:\n    x: num\n\ntrait Identity:\n    fn id<A>(self: Self, v: A) -> A:\n        return v\n\nmain:\n    print(1)\n";
        let error = compile(source).expect_err("generic defaults are not supported");
        assert!(error.contains("Generic default methods"), "{error}");
    }
}
