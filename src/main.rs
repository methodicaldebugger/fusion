mod span;
mod lexer;
mod parser;
mod ast;
mod interpreter;
mod type_checker;
mod types;
mod errors;
mod value;
mod environment;

use std::env;
use std::fs;

use lexer::*;
use parser::*;
use type_checker::TypeChecker;
use interpreter::Interpreter;

fn parse_should_fail(source: &str) {
    let mut lexer = Lexer::new(source, BlockMode::Unknown);

    let tokens = lexer
        .tokenize()
        .expect("test source should lex successfully");

    let mut parser = Parser::new(tokens);

    assert!(
        parser.parse().is_err(),
        "expected parser to reject invalid source"
    );
}

fn source_from_args() -> Result<String, String> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        None => Ok(r#"main:
    x = 10
    print(x)

    if x > 5:
        y = 20
        print(y)

    x = 40
    print(x)
"#.to_string()),
        Some(path) => fs::read_to_string(&path)
            .map_err(|error| format!("error: could not read `{}`: {}", path, error)),
    }
}

    fn parse_program(tokens: Vec<span::Spanned<Token>>) -> Result<ast::Program, String> {
    let mut parser = Parser::new(tokens);

    parser
        .parse()
        .map_err(|error| format!("error: parser: {}", error))
}

fn main() {
    println!("Fusion Compiler 0.1");

    let source = match source_from_args() {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{}", error);
            return;
        }
    };

    let mut lexer = Lexer::new(&source, BlockMode::Unknown);
    let tokens = match lexer.tokenize() {
        Ok(tokens) => tokens,
        Err(error) => {
            eprintln!("error: lexer: {}", error);
            return;
        }
    };

    let program = match parse_program(tokens) {
        Ok(program) => program,
        Err(error) => {
            eprintln!("{}", error);
            return;
        }
    };

    let mut checker = TypeChecker::new();
    if let Err(error) = checker.check(&program) {
        eprintln!("{}", error);
        return;
    }

    println!("Type checking passed.");

    let mut interpreter = Interpreter::new();
    interpreter.execute(&program);
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // Test helpers
    // -------------------------------------------------------------------------

    fn compile_test_program(source: &str) -> ast::Program {
        let mut lexer = Lexer::new(source, BlockMode::Unknown);

        let tokens = lexer
            .tokenize()
            .expect("test source should lex successfully");

        let mut parser = Parser::new(tokens);

        // Parser::parse() currently returns Program directly and uses
        // panic-based error handling.
        let program = parser
    .parse()
    .expect("test source should parse successfully");

        let mut checker = TypeChecker::new();

        checker
            .check(&program)
            .expect("test source should type-check successfully");

        program
    }

    fn run_program(source: &str) -> Vec<String> {
        let program = compile_test_program(source);

        let mut interpreter = Interpreter::new();
        interpreter.execute(&program);

        interpreter.output().to_vec()
    }

    fn parse_should_fail(source: &str) {
        let mut lexer = Lexer::new(source, BlockMode::Unknown);

        let tokens = match lexer.tokenize() {
            Ok(tokens) => tokens,
            Err(_) => return,
        };

        let result = std::panic::catch_unwind(
            std::panic::AssertUnwindSafe(|| {
                let mut parser = Parser::new(tokens);
                parser.parse()
            }),
        );

        assert!(
            result.is_err(),
            "expected parser to reject invalid source"
        );
    }

    fn type_check_should_fail(source: &str) {
        let mut lexer = Lexer::new(source, BlockMode::Unknown);

        let tokens = lexer
            .tokenize()
            .expect("test source should lex successfully");

        let mut parser = Parser::new(tokens);

        let program = parser
    .parse()
    .expect("source should parse before type-checking");

        let mut checker = TypeChecker::new();

        assert!(
            checker.check(&program).is_err(),
            "expected type checker to reject invalid program"
        );
    }

    // =========================================================================
    // Lexer
    // =========================================================================


 #[test]
fn indentation_match_rejects_colon_after_arm_arrow() {
    parse_should_fail(
        r#"main:
    x = 2

    match x:
        1 =>:
            print(10)
        _ =>:
            print(30)
"#,
    );
}


    #[test]
fn indentation_match_executes_selected_arm_3() {
    assert_eq!(
        run_program(
            r#"main:
    x = 2

    match x:
        1 => print(10)

        2 => print(20)

        _ => print(30)
"#
        ),
        vec!["20"]
    );
}


    #[test]
    fn unknown_variable_inside_if_is_rejected() {
        type_check_should_fail(
            r#"main{
    if missing {
        print(1)
    }
}
"#,
        );
    }

    #[test]
fn indentation_match_executes_selected_arm_without_space_after_arrow() {
    assert_eq!(
        run_program(
            r#"main:
    x = 2

    match x:
        1 =>print(10)

        2 =>print(20)

        _ =>print(30)
"#
        ),
        vec!["20"]
    );
}


    #[test]
    fn non_boolean_if_condition_is_rejected_again() {
        type_check_should_fail(
            r#"main{
    x = 10

    if x {
        print(1)
    }
}
"#,
        );
    }

    #[test]
    fn non_boolean_while_condition_is_rejected_again() {
        type_check_should_fail(
            r#"main{
    x = 10

    while x {
        print(1)
    }
}
"#,
        );
    }

    // =========================================================================
    // Functions
    // =========================================================================

    #[test]
    fn function_without_parameters_executes() {
        assert_eq!(
            run_program(
                r#"main:
    greet()

fn greet():
    print(42)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn function_with_one_parameter_executes() {
        assert_eq!(
            run_program(
                r#"main:
    print_double(21)

fn print_double(x):
    print(x + x)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn function_with_multiple_parameters_executes() {
        assert_eq!(
            run_program(
                r#"main:
    add(10, 32)

fn add(a, b):
    print(a + b)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn function_can_return_value() {
        assert_eq!(
            run_program(
                r#"main:
    x = add(20, 22)
    print(x)

fn add(a, b):
    return a + b
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn function_return_value_can_be_used_in_expression() {
        assert_eq!(
            run_program(
                r#"main:
    x = double(21) + 1
    print(x)

fn double(x):
    return x * 2
"#
            ),
            vec!["43"]
        );
    }

    #[test]
fn function_without_return_type_allows_bare_return() {
    compile_test_program(
        r#"main:
    foo()

fn foo():
    return
"#,
    );
}

#[test]
fn function_with_return_type_allows_value_return() {
    compile_test_program(
        r#"main:
    x = foo()
    print(x)

fn foo() -> num:
    return 42
"#,
    );
}

#[test]
fn function_with_return_type_rejects_bare_return() {
    parse_should_fail(
        r#"main:
    foo()

fn foo() -> num:
    return
"#,
    );
}

#[test]
fn function_without_return_type_rejects_value_return() {
    parse_should_fail(
        r#"main:
    foo()

fn foo():
    return 42
"#,
    );
}

    #[test]
    fn function_can_be_called_multiple_times() {
        assert_eq!(
            run_program(
                r#"main:
    print(double(10))
    print(double(20))

fn double(x):
    return x * 2
"#
            ),
            vec!["20", "40"]
        );
    }

    #[test]
    fn function_can_call_another_function() {
        assert_eq!(
            run_program(
                r#"main:
    print(double_and_add(20))

fn double_and_add(x):
    return double(x) + 2

fn double(x):
    return x * 2
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn function_parameters_can_have_types() {
        assert_eq!(
            run_program(
                r#"main:
    print(add(20, 22))

fn add(a: num, b: num):
    return a + b
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn function_can_have_return_type() {
        assert_eq!(
            run_program(
                r#"main:
    x = add(20, 22)
    print(x)

fn add(a: num, b: num) -> num:
    return a + b
"#
            ),
            vec!["42"]
        );
    }

    // =========================================================================
    // Function type-checking failures
    // =========================================================================

    #[test]
    fn calling_unknown_function_is_rejected() {
        type_check_should_fail(
            r#"main:
    does_not_exist()
"#,
        );
    }

    #[test]
    fn function_argument_type_mismatch_is_rejected() {
        type_check_should_fail(
            r#"main:
    add("hello", 2)

fn add(a: num, b: num):
    return a + b
"#,
        );
    }

    #[test]
    fn function_return_type_mismatch_is_rejected() {
        type_check_should_fail(
            r#"main:
    x = get_number()
    print(x)

fn get_number() -> num:
    return "not a number"
"#,
        );
    }

    // =========================================================================
    // Structs
    // =========================================================================

    #[test]
    fn struct_can_be_declared_and_constructed() {
        assert_eq!(
            run_program(
                r#"struct Person {
    name: string
    age: num
}

main:
    Person("Alice", 30)

    print(person.name)
    print(person.age)
"#
            ),
            vec!["Alice", "30"]
        );
    }

    #[test]
    fn struct_fields_can_be_used_in_expressions() {
        assert_eq!(
            run_program(
                r#"struct Point {
    x: num
    y: num
}

main:
    Point(x:10,y:32)
    print(point.x + point.y)
"#
            ),
            vec!["42"]
        );
    }


    // =========================================================================
    // Struct type-checking failures
    // =========================================================================

    #[test]
    fn struct_field_type_mismatch_is_rejected() {
        type_check_should_fail(
            r#"struct Person {
    name: string
    age: num
}

main:
    person = Person {
        name: "Alice",
        age: "thirty"
    }
"#,
        );
    }

#[test]
fn struct_constructor_too_few_arguments_is_rejected() {
    type_check_should_fail(
        r#"struct Person:
    name: string
    age: num

main:
    person = Person("Alice")
"#,
    );
}

#[test]
fn struct_constructor_too_many_arguments_is_rejected() {
    type_check_should_fail(
        r#"struct Person:
    name: string
    age: num

main:
    person = Person("Alice", 30, 100)
"#,
    );
}

#[test]
fn struct_constructor_is_parenthesized_in_both_modes() {
    assert_eq!(
        run_program(
            r#"struct Point:
    x: num
    y: num

main:
    point = Point(20, 22)
    print(point.x + point.y)
"#
        ),
        vec!["42"]
    );

    assert_eq!(
        run_program(
            r#"struct Point {
    x: num
    y: num
}

main {
    point = Point(20, 22)
    print(point.x + point.y)
}
"#
        ),
        vec!["42"]
    );
}

#[test]
fn struct_constructor_rejects_braces() {
    parse_should_fail(
        r#"struct Point:
    x: num
    y: num

main:
    point = Point {
        x: 10,
        y: 32
    }
"#,
    );
}

    #[test]
fn indentation_mode_rejects_left_brace() {
    let source = r#"main:
    print(10) {
"#;

    let mut lexer = Lexer::new(source, BlockMode::Unknown);

    assert!(lexer.tokenize().is_err());
}

#[test]
fn indentation_mode_rejects_right_brace() {
    let source = r#"main:
    print(10)
}
"#;

    let mut lexer = Lexer::new(source, BlockMode::Unknown);

    assert!(lexer.tokenize().is_err());
}

#[test]
fn brace_mode_does_not_generate_indentation_tokens() {
    let source = r#"main{
    print(10)
}
"#;

    let mut lexer = Lexer::new(source, BlockMode::Unknown);
    let tokens = lexer.tokenize().expect("should lex");

    assert!(!tokens.iter().any(|t| matches!(
        t.node,
        Token::Indent | Token::Dedent
    )));
}


// =========================================================================
// Structs
// =========================================================================

#[test]
fn indentation_struct_can_be_declared_and_constructed() {
    assert_eq!(
        run_program(
            r#"struct Person:
    name: string
    age: num

main:
    person = Person("Alice", 30)

    print(person.name)
    print(person.age)
"#
        ),
        vec!["Alice", "30"]
    );
}

#[test]
fn brace_struct_can_be_declared_and_constructed() {
    assert_eq!(
        run_program(
            r#"struct Person {
    name: string
    age: num
}

main {
    person = Person("Alice", 30)

    print(person.name)
    print(person.age)
}
"#
        ),
        vec!["Alice", "30"]
    );
}

#[test]
fn struct_constructor_uses_parentheses_in_indentation_mode() {
    assert_eq!(
        run_program(
            r#"struct Point:
    x: num
    y: num

main:
    point = Point(10, 32)

    print(point.x + point.y)
"#
        ),
        vec!["42"]
    );
}

#[test]
fn struct_constructor_uses_parentheses_in_brace_mode() {
    assert_eq!(
        run_program(
            r#"struct Point {
    x: num
    y: num
}

main {
    point = Point(10, 32)

    print(point.x + point.y)
}
"#
        ),
        vec!["42"]
    );
}

#[test]
fn multiple_struct_instances_are_independent() {
    assert_eq!(
        run_program(
            r#"struct Point:
    x: num
    y: num

main:
    first = Point(10, 20)
    second = Point(30, 40)

    print(first.x)
    print(second.x)
"#
        ),
        vec!["10", "30"]
    );
}

#[test]
fn struct_constructor_can_use_expressions() {
    assert_eq!(
        run_program(
            r#"struct Point:
    x: num
    y: num

main:
    a = 20
    b = 22

    point = Point(a, b)

    print(point.x + point.y)
"#
        ),
        vec!["42"]
    );
}

#[test]
fn struct_fields_can_be_read() {
    assert_eq!(
        run_program(
            r#"struct Point:
    x: num
    y: num

main:
    point = Point(10, 32)

    print(point.x)
    print(point.y)
"#
        ),
        vec!["10", "32"]
    );
}

// =========================================================================
// Struct syntax failures
// =========================================================================

#[test]
fn indentation_main_rejects_brace_struct() {
    parse_should_fail(
        r#"struct Point {
    x: num
    y: num
}

main:
    point = Point(10, 32)
"#,
    );
}

#[test]
fn brace_main_rejects_indentation_struct() {
    parse_should_fail(
        r#"main {
    point = Point(10, 32)
}

struct Point:
    x: num
    y: num
"#,
    );
}

#[test]
fn indentation_struct_matches_indentation_main() {
    assert_eq!(
        run_program(
            r#"struct Point:
    x: num
    y: num

main:
    point = Point(10, 32)

    print(point.x)
    print(point.y)
"#
        ),
        vec!["10", "32"]
    );
}

#[test]
fn brace_struct_matches_brace_main() {
    assert_eq!(
        run_program(
            r#"struct Point {
    x: num
    y: num
}

main {
    point = Point(10, 32)

    print(point.x)
    print(point.y)
}
"#
        ),
        vec!["10", "32"]
    );
}

#[test]
fn indentation_struct_requires_colon() {
    parse_should_fail(
        r#"struct Point
    x: num
    y: num

main:
    point = Point(10, 32)
"#,
    );
}

#[test]
fn brace_struct_requires_left_brace() {
    parse_should_fail(
        r#"struct Point:
    x: num
    y: num

main {
    point = Point(10, 32)
}
"#,
    );
}

// =========================================================================
// Struct type-checking failures
// =========================================================================

#[test]
fn unknown_struct_type_is_rejected() {
    type_check_should_fail(
        r#"main:
    point = DoesNotExist(10, 20)
"#,
    );
}

#[test]
fn unknown_struct_field_is_rejected() {
    type_check_should_fail(
        r#"struct Point:
    x: num
    y: num

main:
    point = Point(10, 20)
    print(point.z)
"#,
    );
}

#[test]
fn struct_constructor_argument_type_mismatch_is_rejected() {
    type_check_should_fail(
        r#"struct Person:
    name: string
    age: num

main:
    person = Person("Alice", "thirty")
"#,
    );
}

}