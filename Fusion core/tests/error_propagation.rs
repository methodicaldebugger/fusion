//! The `?` operator: `value = parse(text)?` returns early with the `Err` and otherwise continues
//! with the `Ok` payload.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn run_single_file(source: &str) -> Vec<String> {
    let program = fusion::compile_source(source).expect("program should compile");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    interpreter.output().to_vec()
}

fn run_as_project(source: &str) -> Vec<String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "fusion-try-test-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("main.fusion");
    fs::write(&entry, source).unwrap();
    let result = fusion::compile_project(&entry);
    let _ = fs::remove_dir_all(&dir);
    let program = result.expect("project should compile");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    interpreter.output().to_vec()
}

const PROGRAM: &str = "fn parse_positive(n: num) -> Result<num, string>:
    if n > 0:
        return Ok(n)
    return Err(\"not positive\")

fn validate(n: num) -> Result<bool, string>:
    if n > 10:
        return Err(\"too big\")
    return Ok(true)

fn double_positive(n: num) -> Result<num, string>:
    value = parse_positive(n)?
    return Ok(value * 2)

fn sum_two(a: num, b: num) -> Result<num, string>:
    x = parse_positive(a)?
    y = parse_positive(b)?
    return Ok(x + y)

fn typed(n: num) -> Result<num, string>:
    value: num = parse_positive(n)?
    return Ok(value + 1)

fn run(n: num) -> Result<num, string>:
    validate(n)?
    return Ok(n)

fn total(items: num[]) -> Result<num, string>:
    sum = 0
    i = 0
    while i < items.get_length():
        value = parse_positive(items.access(i))?
        sum = sum + value
        i = i + 1
    return Ok(sum)

main:
    print(result_unwrap(double_positive(4)))
    print(result_unwrap_err(double_positive(-1)))
    print(result_unwrap(sum_two(1, 2)))
    print(result_unwrap_err(sum_two(1, -2)))
    print(result_unwrap(typed(9)))
    print(result_is_ok(run(5)))
    print(result_unwrap_err(run(50)))
    print(result_unwrap(total([1, 2, 3])))
    print(result_unwrap_err(total([1, -1, 3])))
";

const EXPECTED: &[&str] = &[
    "8",
    "not positive",
    "3",
    "not positive",
    "10",
    "true",
    "too big",
    "6",
    "not positive",
];

#[test]
fn question_mark_returns_the_error_or_continues_with_the_value() {
    assert_eq!(run_single_file(PROGRAM), EXPECTED);
}

#[test]
fn question_mark_survives_the_project_pipeline() {
    assert_eq!(run_as_project(PROGRAM), EXPECTED);
}

#[test]
fn question_mark_in_the_middle_of_an_expression_is_rejected() {
    let source = "fn parse_positive(n: num) -> Result<num, string>:
    return Ok(n)

fn bad(n: num) -> Result<num, string>:
    return Ok(parse_positive(n)? + 1)

main:
    print(1)
";
    let error = fusion::compile_source(source).expect_err("`?` must end a statement");
    assert!(error.contains("The '?' operator can only end"), "{error}");
}
