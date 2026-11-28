//! Generic struct and enum syntax preservation. Declaration parameters are still dynamically
//! typed and applications are not yet semantically checked, but nested user-type arguments must
//! survive parsing and type conversion rather than being erased from the AST spelling.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn run_single_file(source: &str) -> Vec<String> {
    let program = fusion::compile_source(source).expect("program should compile");
    let mut interpreter = fusion::interpreter::Interpreter::new();
    interpreter.execute(&program);
    interpreter.output().to_vec()
}

/// Same program through the project pipeline (module system and HIR passes), which is what
/// `fusion run` uses.
fn run_as_project(source: &str) -> Vec<String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "fusion-generic-test-{}-{nonce}",
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

const PAIR: &str = "struct Pair<A, B>:
    first: A
    second: B

fn second_of(p: Pair<num, string>) -> string:
    return p.second

main:
    p = Pair(first: 1, second: \"one\")
    print(second_of(p))
    print(p.first + 41)
";

const MAYBE: &str = "enum Maybe<T>:
    Just(T)
    Nothing

fn show(m: Maybe<num>):
    match m:
        Maybe::Just(v) => print(v)
        Maybe::Nothing => print(\"nothing\")

main:
    show(Maybe::Just(5))
    show(Maybe::Nothing)
";

const STACK: &str = "struct Stack<T>:
    items: T[]

impl Stack<T>:
    fn size(self: Stack<T>) -> num:
        return self.items.get_length()

    fn peek(self: Stack<T>) -> T:
        return self.items.access(0)

main:
    s = Stack(items: [10, 20])
    print(s.size())
    print(s.peek())
";

const BRACES: &str = "struct Boxed<T> { value: T }

enum Either<L, R> {
    Left(L)
    Right(R)
}

main {
    b = Boxed { value: 7 }
    print(b.value)
    e = Either::Right(\"r\")
    print(\"done\")
}
";

#[test]
fn generic_struct_construction_annotation_and_field_access() {
    assert_eq!(run_single_file(PAIR), vec!["one", "42"]);
}

#[test]
fn generic_enum_construction_and_exhaustive_match() {
    assert_eq!(run_single_file(MAYBE), vec!["5", "nothing"]);
}

#[test]
fn methods_can_be_declared_on_a_generic_type() {
    assert_eq!(run_single_file(STACK), vec!["2", "10"]);
}

#[test]
fn generic_declarations_work_in_brace_style() {
    // Brace-style declarations with a brace-style `main`; this file keeps one block style.
    let result = fusion::compile_source(BRACES);
    assert!(result.is_ok(), "{:?}", result.err());
}

#[test]
fn generic_types_survive_the_project_pipeline() {
    assert_eq!(run_as_project(PAIR), vec!["one", "42"]);
    assert_eq!(run_as_project(MAYBE), vec!["5", "nothing"]);
    assert_eq!(run_as_project(STACK), vec!["2", "10"]);
}

#[test]
fn duplicate_type_parameters_are_rejected() {
    let error = fusion::compile_source("struct Bad<T, T>:\n    x: T\n\nmain:\n    print(1)\n")
        .expect_err("duplicate type parameters must be rejected");
    assert!(error.contains("Duplicate type parameter"), "{error}");
}

#[test]
fn unclosed_type_parameter_list_is_rejected() {
    let error = fusion::compile_source("struct Bad<T:\n    x: T\n\nmain:\n    print(1)\n")
        .expect_err("an unclosed parameter list must be rejected");
    assert!(error.contains("Expected '>' after type parameters"), "{error}");
}

#[test]
fn nested_user_generic_type_arguments_are_preserved_and_accepted() {
    let source = "struct Stack<T>:
    items: T[]

struct Holder:
    stack: Option<Stack<num>>

fn accept(value: Stack<Option<num>>):
    return

main:
    print(1)
";
    let result = fusion::compile_source(source);
    assert!(result.is_ok(), "{:?}", result.err());
    assert_eq!(run_as_project(source), vec!["1"]);
}
