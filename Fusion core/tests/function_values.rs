//! First-class functions: named functions as values, function types, anonymous functions
//! (`fn(x: num) -> num => x * 2`), calling function values, and the higher-order built-ins.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn run_single_file(source: &str) -> Vec<String> {
    let program = fusion::compile_source(source).expect("program should compile");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    interpreter.output().to_vec()
}

/// Compile a project from `(file name, source)` pairs; the first file is the entry point.
fn run_project(files: &[(&str, &str)]) -> Vec<String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "fusion-function-values-test-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    for (name, source) in files {
        fs::write(dir.join(name), source).unwrap();
    }
    let result = fusion::compile_project(dir.join(files[0].0));
    let _ = fs::remove_dir_all(&dir);
    let program = result.expect("project should compile");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    interpreter.output().to_vec()
}

const NAMED: &str = "fn double(x: num) -> num:
    return x * 2

fn apply(f: fn(num) -> num, x: num) -> num:
    return f(x)

main:
    print(apply(double, 21))
    g = double
    print(g(4))
    h: fn(num) -> num = double
    print(h(5))
    print(double)
";

const HIGHER_ORDER: &str = "fn square(x: num) -> num:
    return x * x

main:
    xs = [1, 2, 3, 4]
    squares = array_map(xs, square)
    print(squares.access(3))
    tens = array_map(xs, fn(x: num) -> num => x * 10)
    print(tens.access(1))
    big = array_filter(xs, fn(x: num) -> bool => x > 2)
    print(big.get_length())
    total = array_fold(xs, 0, fn(acc: num, x: num) -> num => acc + x)
    print(total)
    print(array_any(xs, fn(x: num) -> bool => x > 3))
    print(array_all(xs, fn(x: num) -> bool => x > 3))
    descending = array_sort_by(xs, fn(a: num, b: num) -> num => b - a)
    print(descending.access(0))
";

const OPTION_RESULT: &str = "fn inc(x: num) -> num:
    return x + 1

main:
    print(option_unwrap(option_map(Some(1), inc)))
    print(result_unwrap(result_map(Ok(1), inc)))
    print(result_unwrap_err(result_map_err(Err(5), inc)))
    print(option_is_none(option_map(None, inc)))
    print(option_is_some(option_and_then(Some(2), fn(x: num) -> Option<num> => Some(x + 1))))
";

#[test]
fn named_functions_are_values() {
    assert_eq!(
        run_single_file(NAMED),
        vec!["42", "8", "10", "<function double>"]
    );
}

#[test]
fn higher_order_builtins_with_named_and_anonymous_functions() {
    assert_eq!(
        run_single_file(HIGHER_ORDER),
        vec!["16", "20", "2", "10", "true", "false", "4"]
    );
}

#[test]
fn option_and_result_combinators() {
    assert_eq!(
        run_single_file(OPTION_RESULT),
        vec!["2", "2", "6", "true", "true"]
    );
}

#[test]
fn function_values_survive_the_project_pipeline() {
    assert_eq!(
        run_project(&[("main.fusion", NAMED)]),
        vec!["42", "8", "10", "<function double>"]
    );
    assert_eq!(
        run_project(&[("main.fusion", HIGHER_ORDER)]),
        vec!["16", "20", "2", "10", "true", "false", "4"]
    );
    assert_eq!(
        run_project(&[("main.fusion", OPTION_RESULT)]),
        vec!["2", "2", "6", "true", "true"]
    );
}

#[test]
fn function_values_resolve_in_the_module_that_names_them() {
    // Both modules declare `add_one` and contain one anonymous function, which the parser names
    // `__lambda_0` in each; every reference must resolve inside its own module.
    let math = "fn add_one(x: num) -> num:
    return x + 1

pub fn bump_all(xs: num[]) -> num[]:
    return array_map(xs, add_one)

pub fn bump_by_lambda(xs: num[]) -> num[]:
    return array_map(xs, fn(x: num) -> num => x + 100)
";
    let main = "import math

fn add_one(x: num) -> num:
    return x + 5

main:
    print(math.bump_all([1, 2]).access(0))
    print(math.bump_by_lambda([1, 2]).access(0))
    print(array_map([1], add_one).access(0))
    print(array_map([1], fn(x: num) -> num => x + 1000).access(0))
";
    assert_eq!(
        run_project(&[("main.fusion", main), ("math.fusion", math)]),
        vec!["2", "101", "6", "1001"]
    );
}

#[test]
fn anonymous_functions_capture_local_values_by_value() {
    let source = "main:
    k = 3
    f = fn(x: num) -> num => x + k
    k = 100
    print(f(4))
    values = array_map([1, 2], f)
    print(values.access(1))
";
    assert_eq!(run_single_file(source), vec!["7", "5"]);
}

#[test]
fn closures_can_capture_and_call_local_function_values() {
    let source = "fn double(value: num) -> num:
    return value * 2

main:
    callback = double
    apply = fn(value: num) -> num => callback(value)
    print(apply(21))
";
    assert_eq!(run_single_file(source), vec!["42"]);
}

#[test]
fn anonymous_functions_can_escape_their_creation_scope() {
    let source = "fn make_adder(base: num) -> fn(num) -> num:
    return fn(value: num) -> num => value + base

main:
    add_five = make_adder(5)
    print(add_five(7))
";
    assert_eq!(run_single_file(source), vec!["12"]);
}

#[test]
fn nested_anonymous_functions_capture_outer_parameters() {
    let source = "fn make_adder(base: num) -> fn(num) -> num:
    return fn(value: num) -> num => value + base

main:
    add_two = make_adder(2)
    add_three = make_adder(3)
    print(add_two(10))
    print(add_three(10))
";
    assert_eq!(run_single_file(source), vec!["12", "13"]);
}

#[test]
fn nested_closures_capture_enclosing_lambda_parameters() {
    let source = "fn make_adder(base: num) -> fn(num) -> fn(num) -> num:
    return fn(left: num) -> fn(num) -> num => fn(right: num) -> num => left + right + base

main:
    add_from_five = make_adder(5)
    add_eight = add_from_five(3)
    print(add_eight(4))
";
    assert_eq!(run_single_file(source), vec!["12"]);
}

#[test]
fn captured_closures_survive_the_project_pipeline() {
    let source = "fn make_multiplier(factor: num) -> fn(num) -> num:
    return fn(value: num) -> num => value * factor

main:
    triple = make_multiplier(3)
    print(triple(14))
";
    assert_eq!(run_project(&[("main.fusion", source)]), vec!["42"]);
}

#[test]
fn a_function_of_the_wrong_type_is_rejected() {
    let source = "fn add(a: num, b: num) -> num:
    return a + b

fn apply(f: fn(num) -> num, x: num) -> num:
    return f(x)

main:
    print(apply(add, 1))
";
    assert!(fusion::compile_source(source).is_err());
}

#[test]
fn calling_a_function_value_checks_the_argument_count() {
    let source = "fn double(x: num) -> num:
    return x * 2

fn apply(f: fn(num) -> num) -> num:
    return f(1, 2)

main:
    print(apply(double))
";
    let error = fusion::compile_source(source).expect_err("wrong argument count");
    assert!(error.contains("expects 1 arguments, found 2"), "{error}");
}

#[test]
fn function_expression_parameters_need_types() {
    let error = fusion::compile_source("main:\n    f = fn(x) -> num => x\n    print(1)\n")
        .expect_err("untyped parameter");
    assert!(error.contains("need a type"), "{error}");
}
