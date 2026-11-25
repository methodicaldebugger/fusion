//! Built-in `Option` / `Result` values: constructors `Some(x)`, `None`, `Ok(x)`, `Err(e)`, `match`
//! patterns on them, exhaustiveness checking, and the helper functions.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn run_single_file(source: &str) -> Vec<String> {
    let program = fusion::compile_source(source).expect("program should compile");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    interpreter.output().to_vec()
}

/// The same program through the project pipeline (module system and HIR passes), which is what
/// `fusion run` uses.
fn run_as_project(source: &str) -> Vec<String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "fusion-option-result-test-{}-{nonce}",
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

const OPTION: &str = "fn show(o: Option<num>):
    match o:
        Some(x) => print(x)
        None => print(\"none\")

main:
    show(Some(5))
    show(None)
    maybe: Option<num> = None
    print(option_is_none(maybe))
    print(option_is_some(Some(1)))
    print(option_unwrap_or(None, 9))
";

const RESULT: &str = "fn parse_positive(n: num) -> Result<num, string>:
    if n > 0:
        return Ok(n)
    return Err(\"not positive\")

fn report(r: Result<num, string>):
    match r:
        Ok(v) => print(v)
        Err(e) => print(e)

main:
    report(parse_positive(7))
    report(parse_positive(-1))
    print(result_is_ok(parse_positive(1)))
    print(result_is_err(parse_positive(0)))
    print(result_unwrap_or(parse_positive(0), 3))
    print(result_unwrap(parse_positive(4)))
    print(result_unwrap_err(parse_positive(0)))
";

#[test]
fn option_constructors_patterns_and_helpers() {
    assert_eq!(
        run_single_file(OPTION),
        vec!["5", "none", "true", "true", "9"]
    );
}

#[test]
fn result_constructors_patterns_and_helpers() {
    assert_eq!(
        run_single_file(RESULT),
        vec!["7", "not positive", "true", "true", "3", "4", "not positive"]
    );
}

#[test]
fn qualified_patterns_wildcards_and_nesting() {
    let source = "fn show(r: Result<num, string>):
    match r:
        Result::Ok(v) => print(v)
        Result::Err(e) => print(e)

fn deep(o: Option<Result<num, string>>):
    match o:
        Some(Ok(v)) => print(v)
        Some(Err(e)) => print(e)
        _ => print(\"other\")

main:
    show(Ok(1))
    show(Err(\"bad\"))
    deep(Some(Ok(2)))
    deep(Some(Err(\"worse\")))
    deep(None)
";
    assert_eq!(
        run_single_file(source),
        vec!["1", "bad", "2", "worse", "other"]
    );
}

#[test]
fn builtin_option_and_result_survive_the_project_pipeline() {
    assert_eq!(
        run_as_project(OPTION),
        vec!["5", "none", "true", "true", "9"]
    );
    assert_eq!(
        run_as_project(RESULT),
        vec!["7", "not positive", "true", "true", "3", "4", "not positive"]
    );
}

#[test]
fn missing_variants_are_reported() {
    let option = "fn f(o: Option<num>):\n    match o:\n        Some(x) => print(x)\n\nmain:\n    f(Some(1))\n";
    let error = fusion::compile_source(option).expect_err("None is not covered");
    assert!(
        error.contains("Non-exhaustive match: missing variant 'None'"),
        "{error}"
    );

    let result = "fn f(r: Result<num, string>):\n    match r:\n        Err(e) => print(e)\n\nmain:\n    f(Ok(1))\n";
    let error = fusion::compile_source(result).expect_err("Ok is not covered");
    assert!(
        error.contains("Non-exhaustive match: missing variant 'Ok'"),
        "{error}"
    );
}

#[test]
fn a_pattern_for_the_wrong_type_is_rejected() {
    let source = "fn f(o: Option<num>):\n    match o:\n        Ok(x) => print(x)\n        _ => print(\"other\")\n\nmain:\n    f(None)\n";
    let error = fusion::compile_source(source).expect_err("Ok cannot match an Option");
    assert!(error.contains("cannot match"), "{error}");
}

#[test]
fn variant_arity_is_checked() {
    let source = "fn f(o: Option<num>):\n    match o:\n        Some(a, b) => print(a)\n        None => print(\"none\")\n\nmain:\n    f(None)\n";
    let error = fusion::compile_source(source).expect_err("Some takes one binding");
    assert!(error.contains("expects 1 bindings, found 2"), "{error}");
}

#[test]
#[should_panic(expected = "result_unwrap() called on Err")]
fn unwrapping_an_err_panics_with_the_payload() {
    run_single_file("main:\n    print(result_unwrap(Err(\"boom\")))\n");
}
