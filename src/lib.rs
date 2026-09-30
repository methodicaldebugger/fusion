pub mod ast;
pub mod codegen;
pub mod environment;
pub mod errors;
pub mod foreign;
pub mod hir;
pub mod hir_lower;
pub mod interpreter;
pub mod ir;
pub mod lexer;
pub mod llvm_backend;
pub mod name_resolution;
pub mod parser;
pub mod span;
pub mod type_checker;
pub mod types;
pub mod value;

use lexer::{BlockMode, Lexer};
use llvm_backend::LLVMBackend;
use name_resolution::Resolver;
use parser::Parser;
use type_checker::TypeChecker;

pub fn compile_source(source: &str) -> Result<ast::Program, String> {
    let mut lexer = Lexer::new(source, BlockMode::Unknown);
    let tokens = lexer.tokenize().map_err(|e| format!("lexer: {}", e))?;
    let mut parser = Parser::new(tokens);
    let program = parser.parse().map_err(|e| format!("parser: {}", e))?;
    let mut resolver = Resolver::new();
    resolver
        .resolve(&program)
        .map_err(|e| format!("resolver: {}", e))?;
    let mut checker = TypeChecker::new();
    checker
        .check(&program)
        .map_err(|e| format!("type checker: {}", e))?;
    Ok(program)
}

pub fn emit_llvm(source: &str) -> Result<String, String> {
    let program = compile_source(source)?;
    LLVMBackend::new()
        .emit_program(&program)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::Interpreter;
    use crate::lexer::Token;

    // -------------------------------------------------------------------------
    // Test helpers
    // -------------------------------------------------------------------------

    fn compile_test_program(source: &str) -> ast::Program {
        let mut lexer = Lexer::new(source, BlockMode::Unknown);

        let tokens = lexer
            .tokenize()
            .expect("test source should lex successfully");

        let mut parser = Parser::new(tokens);

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

        let mut parser = Parser::new(tokens);

        assert!(
            parser.parse().is_err(),
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
    // Variables and expressions
    // =========================================================================

    #[test]
    fn variables_can_be_declared_and_used() {
        assert_eq!(
            run_program(
                r#"main:
    x = 10
    y = 32
    print(x + y)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn multiple_inferred_variables_are_independent() {
        assert_eq!(
            run_program(
                r#"main:
    x = 10, y = 32
    print(x + y)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn typed_variables_can_be_declared() {
        assert_eq!(
            run_program(
                r#"main:
    num x = 10
    string message = "hello"
    print(x)
    print(message)
"#
            ),
            vec!["10", "hello"]
        );
    }

    #[test]
    fn typed_multiple_variables_can_be_declared() {
        assert_eq!(
            run_program(
                r#"main:
    num x, y = 32
    print(y)
"#
            ),
            vec!["32"]
        );
    }

    #[test]
    fn arithmetic_precedence_is_respected() {
        assert_eq!(
            run_program(
                r#"main:
    result = 2 + 3 * 4
    print(result)
"#
            ),
            vec!["14"]
        );
    }

    #[test]
    fn parentheses_override_arithmetic_precedence() {
        assert_eq!(
            run_program(
                r#"main:
    result = (2 + 3) * 4
    print(result)
"#
            ),
            vec!["20"]
        );
    }

    #[test]
    fn variables_can_be_reassigned() {
        assert_eq!(
            run_program(
                r#"main:
    x = 10
    x = 40
    print(x)
"#
            ),
            vec!["40"]
        );
    }

    #[test]
    fn boolean_values_can_be_printed() {
        assert_eq!(
            run_program(
                r#"main:
    print(true)
    print(false)
"#
            ),
            vec!["true", "false"]
        );
    }

    #[test]
    fn strings_can_be_printed() {
        assert_eq!(
            run_program(
                r#"main:
    message = "hello"
    print(message)
"#
            ),
            vec!["hello"]
        );
    }

    #[test]
    fn floats_can_be_used() {
        assert_eq!(
            run_program(
                r#"main:
    value = 3.14
    print(value)
"#
            ),
            vec!["3.14"]
        );
    }

    // =========================================================================
    // Multiple declaration syntax
    // =========================================================================

    #[test]
    fn multiple_variables_work_in_brace_mode() {
        assert_eq!(
            run_program(
                r#"main {
    x = 10, y = 32
    print(x + y)
}
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn typed_and_inferred_variables_can_be_mixed() {
        assert_eq!(
            run_program(
                r#"main:
    x = 10
    num y = 32
    print(x + y)
"#
            ),
            vec!["42"]
        );
    }

    // =========================================================================
    // Control flow
    // =========================================================================

    #[test]
    fn if_executes_true_branch() {
        assert_eq!(
            run_program(
                r#"main:
    x = 10

    if x > 5:
        print(42)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn if_does_not_execute_false_branch() {
        assert_eq!(
            run_program(
                r#"main:
    x = 2

    if x > 5:
        print(10)
"#
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn while_loop_executes_repeatedly() {
        assert_eq!(
            run_program(
                r#"main:
    x = 0

    while x < 3:
        print(x)
        x = x + 1
"#
            ),
            vec!["0", "1", "2"]
        );
    }

    #[test]
    fn if_rejects_non_boolean_condition() {
        type_check_should_fail(
            r#"main:
    x = 10

    if x:
        print(1)
"#,
        );
    }

    #[test]
    fn while_rejects_non_boolean_condition() {
        type_check_should_fail(
            r#"main:
    x = 10

    while x:
        print(1)
"#,
        );
    }

    #[test]
    fn unknown_variable_is_rejected() {
        type_check_should_fail(
            r#"main:
    print(missing)
"#,
        );
    }

    // =========================================================================
    // Match
    // =========================================================================

    #[test]
    fn match_selects_matching_arm() {
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
    fn match_selects_wildcard_arm() {
        assert_eq!(
            run_program(
                r#"main:
    x = 99

    match x:
        1 => print(10)
        2 => print(20)
        _ => print(30)
"#
            ),
            vec!["30"]
        );
    }

    #[test]
    fn match_accepts_no_space_after_arrow() {
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
    fn match_arm_rejects_colon_after_arrow() {
        parse_should_fail(
            r#"main:
    x = 2

    match x:
        1 =>:
            print(10)
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
    fn function_accepts_one_parameter() {
        assert_eq!(
            run_program(
                r#"main:
    double(21)

fn double(x):
    print(x + x)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn function_accepts_multiple_parameters() {
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
    fn function_can_return_a_value() {
        assert_eq!(
            run_program(
                r#"main:
    result = add(20, 22)
    print(result)

fn add(a, b):
    return a + b
"#
            ),
            vec!["42"]
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
    fn typed_function_parameters_work() {
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
    fn typed_function_return_works() {
        assert_eq!(
            run_program(
                r#"main:
    result = add(20, 22)
    print(result)

fn add(a: num, b: num) -> num:
    return a + b
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn function_without_return_type_can_use_bare_return() {
        compile_test_program(
            r#"main:
    foo()

fn foo():
    return
"#,
        );
    }

    #[test]
    fn function_without_return_type_can_return_a_value() {
        assert_eq!(
            run_program(
                r#"main:
    print(foo())

fn foo():
    return 42
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn typed_function_cannot_use_bare_return() {
        type_check_should_fail(
            r#"main:
    foo()

fn foo() -> num:
    return
"#,
        );
    }

    #[test]
    fn typed_function_rejects_wrong_return_type() {
        type_check_should_fail(
            r#"main:
    result = get_number()
    print(result)

fn get_number() -> num:
    return "not a number"
"#,
        );
    }

    #[test]
    fn unknown_function_is_rejected() {
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

    // =========================================================================
    // Generic functions
    // =========================================================================

    #[test]
    fn generic_identity_accepts_num() {
        assert_eq!(
            run_program(
                r#"fn identity<T>(value: T) -> T:
    return value

main:
    result = identity<num>(42)
    print(result)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn generic_identity_accepts_string() {
        assert_eq!(
            run_program(
                r#"fn identity<T>(value: T) -> T:
    return value

main:
    result = identity<string>("hello")
    print(result)
"#
            ),
            vec!["hello"]
        );
    }

    #[test]
    fn generic_identity_accepts_bool() {
        assert_eq!(
            run_program(
                r#"fn identity<T>(value: T) -> T:
    return value

main:
    result = identity<bool>(true)
    print(result)
"#
            ),
            vec!["true"]
        );
    }

    #[test]
    fn generic_identity_accepts_float() {
        assert_eq!(
            run_program(
                r#"fn identity<T>(value: T) -> T:
    return value

main:
    result = identity<float>(3.14)
    print(result)
"#
            ),
            vec!["3.14"]
        );
    }

    #[test]
    fn generic_function_supports_multiple_type_parameters() {
        assert_eq!(
            run_program(
                r#"fn first<A, B>(a: A, b: B) -> A:
    return a

main:
    result = first<num, string>(42, "hello")
    print(result)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn generic_function_rejects_wrong_explicit_type() {
        type_check_should_fail(
            r#"fn identity<T>(value: T) -> T:
    return value

main:
    result = identity<num>("hello")
    print(result)
"#,
        );
    }

    #[test]
    fn generic_functions_work_in_brace_mode() {
        assert_eq!(
            run_program(
                r#"fn identity<T>(value: T) -> T {
    return value
}

main {
    result = identity<num>(42)
    print(result)
}
"#
            ),
            vec!["42"]
        );
    }

    // =========================================================================
    // Structs
    // =========================================================================

    #[test]
    fn struct_can_be_declared_and_constructed() {
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
    fn struct_fields_can_be_used_in_expressions() {
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
    fn struct_constructor_accepts_expressions() {
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
    fn structs_work_in_brace_mode() {
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
    fn struct_constructor_rejects_too_few_arguments() {
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
    fn struct_constructor_rejects_too_many_arguments() {
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
    fn struct_constructor_rejects_wrong_field_type() {
        type_check_should_fail(
            r#"struct Person:
    name: string
    age: num

main:
    person = Person("Alice", "thirty")
"#,
        );
    }

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
    fn struct_constructor_rejects_brace_literal_syntax() {
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

    // =========================================================================
    // Block syntax / lexer behavior
    // =========================================================================

    #[test]
    fn indentation_mode_accepts_indentation_struct() {
        let source = r#"
struct Person:
    name: string
    age: num

main:
    print("ok")
"#;

        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        assert!(lexer.tokenize().is_ok());
    }

    #[test]
    fn brace_mode_accepts_brace_struct() {
        let source = r#"
struct Person {
    name: string
    age: num
}

main {
    print("ok")
}
"#;

        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        assert!(lexer.tokenize().is_ok());
    }

    #[test]
    fn indentation_mode_rejects_brace_struct() {
        let source = r#"
struct Person {
    name: string
    age: num
}

main:
    print("ok")
"#;

        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        assert!(lexer.tokenize().is_err());
    }

    #[test]
    fn brace_mode_rejects_indentation_struct() {
        let source = r#"
struct Person:
    name: string
    age: num

main {
    print("ok")
}
"#;

        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        assert!(lexer.tokenize().is_err());
    }

    #[test]
    fn brace_mode_does_not_generate_indentation_tokens() {
        let source = r#"main {
    print(10)
}
"#;

        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("should lex");

        assert!(!tokens
            .iter()
            .any(|token| matches!(token.node, Token::Indent | Token::Dedent)));
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
    fn indentation_and_brace_styles_cannot_be_mixed() {
        parse_should_fail(
            r#"main {
    x = 10
}

fn add(a, b):
    return a + b
"#,
        );
    }
    #[test]
    fn less_than_comparison_works() {
        assert_eq!(
            run_program(
                r#"main:
    print(2 < 3)
    print(3 < 2)
"#
            ),
            vec!["true", "false"]
        );
    }

    #[test]
    fn greater_than_comparison_works() {
        assert_eq!(
            run_program(
                r#"main:
    print(3 > 2)
    print(2 > 3)
"#
            ),
            vec!["true", "false"]
        );
    }

    #[test]
    fn equality_comparison_works() {
        assert_eq!(
            run_program(
                r#"main:
    print(10 == 10)
    print(10 == 20)
"#
            ),
            vec!["true", "false"]
        );
    }

    #[test]
    fn inequality_comparison_works() {
        assert_eq!(
            run_program(
                r#"main:
    print(10 != 20)
    print(10 != 10)
"#
            ),
            vec!["true", "false"]
        );
    }
    #[test]
    fn while_loop_executes_repeatedly_2() {
        assert_eq!(
            run_program(
                r#"main:
    x = 0

    while x < 3:
        print(x)
        x = x + 1
"#
            ),
            vec!["0", "1", "2"]
        );
    }
    #[test]
    fn comparison_after_identifier_is_not_parsed_as_generic_arguments() {
        assert_eq!(
            run_program(
                r#"main:
    x = 2
    print(x < 3)
    print(x > 1)
"#
            ),
            vec!["true", "true"]
        );
    }
    #[test]
    #[should_panic]
    fn uninitialized_variable_cannot_be_read() {
        run_program(
            r#"main:
    num x
    print(x)
"#,
        );
    }
    #[test]
    fn explicit_variable_type_must_match_value() {
        type_check_should_fail(
            r#"main:
    num x = "hello"
"#,
        );
    }
    #[test]
    fn function_argument_type_mismatch_is_rejected_2() {
        type_check_should_fail(
            r#"fn add(num a, num b) -> num:
    return a + b

main:
    print(add("hello", 10))
"#,
        );
    }
    #[test]
    fn function_return_type_mismatch_is_rejected() {
        type_check_should_fail(
            r#"fn get_number() -> num:
    return "hello"

main:
    print(get_number())
"#,
        );
    }
    #[test]
    fn function_requiring_return_value_cannot_return_without_value() {
        type_check_should_fail(
            r#"fn get_number() -> num:
    return

main:
    print(get_number())
"#,
        );
    }

    #[test]
    fn function_requires_all_arguments() {
        type_check_should_fail(
            r#"fn add(num a, num b) -> num:
    return a + b

main:
    print(add(10))
"#,
        );
    }
    #[test]
    fn function_rejects_extra_arguments() {
        type_check_should_fail(
            r#"fn add(num a, num b) -> num:
    return a + b

main:
    print(add(10, 20, 30))
"#,
        );
    }
    #[test]
    fn if_rejects_non_boolean_condition_2() {
        type_check_should_fail(
            r#"main:
    if 10:
        print("yes")
"#,
        );
    }
    #[test]
    fn arithmetic_rejects_incompatible_types() {
        type_check_should_fail(
            r#"main:
    x = "hello" + 10
"#,
        );
    }
    #[test]
    fn boolean_cannot_be_used_in_arithmetic() {
        type_check_should_fail(
            r#"main:
    x = true + false
"#,
        );
    }
    #[test]
    fn ordering_comparison_rejects_incompatible_types() {
        type_check_should_fail(
            r#"main:
    x = 10 < "hello"
"#,
        );
    }
    #[test]
    fn array_rejects_incompatible_element_types() {
        type_check_should_fail(
            r#"main:
    values = [10, "hello", true]
"#,
        );
    }
    #[test]
    fn array_index_requires_num() {
        type_check_should_fail(
            r#"main:
    values = [10, 20, 30]
    x = values["hello"]
"#,
        );
    }
    #[test]
    fn non_array_cannot_be_indexed() {
        type_check_should_fail(
            r#"main:
    x = 10
    y = x[0]
"#,
        );
    }
    #[test]
    fn reassignment_must_preserve_variable_type() {
        type_check_should_fail(
            r#"main:
    num x = 10
    x = "hello"
"#,
        );
    }
    #[test]
    fn match_patterns_must_match_scrutinee_type() {
        type_check_should_fail(
            r#"main:
    x = 10

    match x:
        true:
            print("true")
        10:
            print("ten")
"#,
        );
    }
    #[test]
    fn duplicate_function_definition_is_rejected() {
        type_check_should_fail(
            r#"fn test() -> num:
    return 10

fn test() -> num:
    return 20

main:
    print(test())
"#,
        );
    }
    #[test]
    fn duplicate_variable_declaration_is_rejected() {
        type_check_should_fail(
            r#"main:
    num x = 10
    num x = 20
"#,
        );
    }

    #[test]
    fn break_outside_loop_is_rejected() {
        parse_should_fail(
            r#"main:
    break
"#,
        );
    }
    #[test]
    fn continue_outside_loop_is_rejected() {
        parse_should_fail(
            r#"main:
    continue
"#,
        );
    }
    #[test]
    fn program_requires_main() {
        parse_should_fail(
            r#"fn test() -> num:
    return 10
"#,
        );
    }
    #[test]
    fn non_function_value_cannot_be_called() {
        type_check_should_fail(
            r#"main:
    x = 10
    y = x()
"#,
        );
    }
    #[test]
    fn struct_field_assignment_requires_correct_type() {
        type_check_should_fail(
            r#"struct Person:
    name: string
    age: num

main:
    person = Person(name: "Alice", age: "twenty")
"#,
        );
    }
    #[test]
    fn unknown_struct_field_is_rejected_2() {
        type_check_should_fail(
            r#"struct Person:
    name: string
    age: num

main:
    person = Person(name: "Alice", height: 180)
"#,
        );
    }
    #[test]
    fn unknown_enum_variant_is_rejected() {
        type_check_should_fail(
            r#"enum Status:
    Active
    Inactive

main:
    status = Status.Unknown
"#,
        );
    }

    #[test]
    fn void_function_cannot_return_value() {
        type_check_should_fail(
            r#"fn do_work() -> void:
    return 10

main:
    do_work()
"#,
        );
    }
    #[test]
    fn nested_function_definition_is_rejected() {
        parse_should_fail(
            r#"main:
    fn nested() -> num:
        return 10
"#,
        );
    }
    #[test]
    fn generic_function_cannot_return_wrong_type() {
        type_check_should_fail(
            r#"fn identity<T>(T value) -> T:
    return 10

main:
    x = identity<string>("hello")
"#,
        );
    }
    #[test]
    fn generic_argument_must_match_explicit_type_parameter() {
        type_check_should_fail(
            r#"fn identity<T>(T value) -> T:
    return value

main:
    x = identity<string>(42)
"#,
        );
    }

    #[test]
    fn function_return_value_can_be_used_in_expression() {
        assert_eq!(
            run_program(
                r#"main:
    result = double(21) + 1
    print(result)

fn double(x):
    return x * 2
"#
            ),
            vec!["43"]
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
    fn input_builtin_is_a_string_expression() {
        let program = compile_test_program(
            r#"main:
    value = input()
    print(value)
"#,
        );
        assert!(matches!(program.statements[0], ast::Statement::Main { .. }));
    }

    #[test]
    fn explicit_as_conversion_works() {
        assert_eq!(
            run_program(
                r#"main:
    x = 42 as float
    print(x)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn from_conversion_uses_user_defined_converter() {
        assert_eq!(
            run_program(
                r#"fn from_num(value) -> num:
    return value + 10

main:
    x = num::from(32)
    print(x)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn try_conversion_returns_option() {
        assert_eq!(
            run_program(
                r#"main:
    x = num::try("42")
    print(x)
    y = num::try("not-a-number")
    print(y)
"#
            ),
            vec!["Some(42)", "None"]
        );
    }

    #[test]
    fn defer_closes_file_and_file_io_works() {
        let path = std::env::temp_dir().join(format!("fusion_test_{}.txt", std::process::id()));
        let path_string = path.to_string_lossy().replace('\\', "\\\\");
        let source = format!(
            r#"main:
    file = open("{}")
    defer file.close()
    file.write("hello fusion")
"#,
            path_string
        );
        run_program(&source);
        let contents =
            std::fs::read_to_string(&path).expect("Fusion should create and write the file");
        assert_eq!(contents, "hello fusion");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn invalid_conversion_is_rejected() {
        type_check_should_fail(
            r#"main:
    x = true as float
"#,
        );
    }

    #[test]
    fn invalid_file_method_arguments_are_rejected() {
        type_check_should_fail(
            r#"main:
    file = open("test.txt")
    file.write(42)
"#,
        );
    }

    #[test]
    fn invalid_conversion_arity_is_rejected() {
        parse_should_fail(
            r#"main:
    x = num::from(1, 2)
"#,
        );
    }

    // =========================================================================
    // Generics, traits, enums, modules, collections, iterators, and strings
    // =========================================================================

    #[test]
    fn generic_function_can_be_called_with_explicit_type_argument() {
        assert_eq!(
            run_program(
                r#"fn identity<T>(T value) -> T:
    return value

main:
    x = identity<num>(42)
    print(x)
"#
            ),
            vec!["42"]
        );
    }

    #[test]
    fn trait_impl_methods_can_be_called_through_the_receiver() {
        assert_eq!(
            run_program(
                r#"struct Person:
    name: string

trait Greeter:
    fn greet(person: Person) -> string

impl Greeter for Person:
    fn greet(person: Person) -> string:
        return "hello " + person.name

main:
    p = Person(name: "Fusion")
    print(p.greet())
"#
            ),
            vec!["hello Fusion"]
        );
    }

    #[test]
    fn enum_pattern_matching_works() {
        assert_eq!(
            run_program(
                r#"enum State:
    Ready
    Failed(string)

main:
    state = State::Failed("broken")
    match state:
        State::Ready => print("ready")
        State::Failed(message) => print(message)
"#
            ),
            vec!["broken"]
        );
    }

    #[test]
    fn import_module_syntax_is_accepted() {
        let program = compile_test_program(
            r#"import std.io

main:
    print("module syntax")
"#,
        );
        assert!(program
            .statements
            .iter()
            .any(|s| matches!(s, ast::Statement::Import { path, .. } if path == "std.io")));
    }

    #[test]
    fn growable_array_operations_work() {
        assert_eq!(
            run_program(
                r#"main:
    numbers = [1, 2, 3]
    numbers.add(4)
    numbers.insert_at_index(1, 9)
    numbers.modify(0, 7)
    print(numbers.access(0))
    print(numbers.get_length())
    print(numbers.remove_at_index(1))
    print(numbers.remove_last())
    numbers.clear()
    print(numbers.get_length())
"#
            ),
            vec!["7", "5", "9", "4", "0"]
        );
    }

    #[test]
    fn array_iteration_and_iterator_operations_work() {
        assert_eq!(
            run_program(
                r#"fn double(x: num) -> num:
    return x * 2

fn even(x: num) -> bool:
    return x == 2

fn add(acc: num, x: num) -> num:
    return acc + x

main:
    numbers = [1, 2, 3]
    doubled = numbers.iterate().map(double).collect()
    print(doubled)
    filtered = numbers.iterate().filter(even).collect()
    print(filtered)
    found = numbers.iterate().find(even)
    print(found)
    print(numbers.iterate().any(even))
    print(numbers.iterate().all(even))
    total = numbers.iterate().fold(0, add)
    print(total)
"#
            ),
            vec!["[2, 4, 6]", "[2]", "Some(2)", "true", "false", "6"]
        );
    }

    #[test]
    fn hash_map_operations_work() {
        assert_eq!(
            run_program(
                r#"main:
    m = hashmap()
    m.insert("one", 1)
    m.insert("two", 2)
    print(m.contains_key("one"))
    print(m.get("two"))
    print(m.keys())
    print(m.values())
    print(m.remove("one"))
    m.clear()
    print(m.get_length())
"#
            ),
            vec!["true", "Some(2)", "[one, two]", "[1, 2]", "Some(1)", "0"]
        );
    }

    #[test]
    fn string_operations_work() {
        assert_eq!(
            run_program(
                r#"main:
    text = "  Hello Fusion  "
    print(text.trim())
    print(text.to_uppercase())
    print(text.to_lowercase())
    print(text.substring(2, 7))
    print(text.find("Fusion"))
    print(text.replace("Fusion", "World"))
    print(text.split(" "))
    print(text.starts_with("  He"))
    print(text.ends_with("  "))
    print(text.contains("Fusion"))
    print(text.length())
"#
            ),
            vec![
                "Hello Fusion",
                "  HELLO FUSION  ",
                "  hello fusion  ",
                "Hello",
                "Some(8)",
                "  Hello World  ",
                "[, , Hello, Fusion, , ]",
                "true",
                "true",
                "true",
                "16"
            ]
        );
    }

    #[test]
    fn invalid_generic_arity_is_rejected() {
        type_check_should_fail(
            r#"fn identity<T>(T value) -> T:
    return value

main:
    x = identity(42)
"#,
        );
    }

    #[test]
    fn invalid_array_operation_types_are_rejected() {
        type_check_should_fail(
            r#"main:
    numbers = [1, 2, 3]
    numbers.access("wrong")
"#,
        );
    }

    #[test]
    fn invalid_hash_map_key_type_is_rejected() {
        type_check_should_fail(
            r#"main:
    HashMap<string, num> m = hashmap()
    m.insert(1, "number key")
"#,
        );
    }

    #[test]
    fn invalid_string_operation_types_are_rejected() {
        type_check_should_fail(
            r#"main:
    text = "hello"
    text.concatenate(42)
"#,
        );
    }

    #[test]
    fn incomplete_trait_impl_is_rejected() {
        type_check_should_fail(
            r#"struct Person:
    name: string

trait Greeter:
    fn greet(person: Person) -> string

impl Greeter for Person:
    fn other(person: Person) -> string:
        return "wrong"

main:
    print("x")
"#,
        );
    }
}

#[cfg(test)]
mod stdlib_feature_tests {
    use super::*;
    use crate::interpreter::Interpreter;
    use crate::lexer::{BlockMode, Lexer};
    use crate::parser::Parser;
    use std::fs;

    fn run(source: &str) -> Vec<String> {
        let mut lexer = Lexer::new(source, BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("lex");
        let mut parser = Parser::new(tokens);
        let program = parser.parse().expect("parse");
        let mut checker = TypeChecker::new();
        checker.check(&program).expect("type-check");
        let mut interpreter = Interpreter::new();
        interpreter.execute(&program);
        interpreter.output().to_vec()
    }

    fn invalid(source: &str) {
        assert!(
            compile_source(source).is_err(),
            "program should be rejected"
        );
    }

    #[test]
    fn async_await_executes_task() {
        assert_eq!(
            run(r#"async fn answer() -> num:
    return 42

main:
    value = await answer()
    print(value)
"#),
            vec!["42"]
        );
    }

    #[test]
    fn filesystem_round_trip() {
        let path =
            std::env::temp_dir().join(format!("fusion_stdlib_{}_test.txt", std::process::id()));
        let path = path.to_string_lossy().replace('\\', "\\\\");
        let source = format!(
            r#"main:
    write_file("{}", "hello fusion")
    print(file_exists("{}"))
    print(read_file("{}"))
    remove_file("{}")
"#,
            path, path, path, path
        );
        assert_eq!(run(&source), vec!["true", "hello fusion"]);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn time_random_encoding_format_serialization_and_logging_work() {
        let out = run(r#"main:
    print(date_utc(0))
    print(hex_encode("Fusion"))
    print(hex_decode("467573696f6e"))
    print(base64_encode("Fusion"))
    print(base64_decode("RnVzaW9u"))
    print(format("value={0}", [42]))
    print(serialize([1, 2, 3]))
    print(deserialize("[1,2,3]"))
    print(random_int(7, 7))
    log_info("ready")
"#);
        assert_eq!(out[0], "1970-01-01 00:00:00 UTC");
        assert_eq!(out[1], "467573696f6e");
        assert_eq!(out[2], "Fusion");
        assert_eq!(out[3], "RnVzaW9u");
        assert_eq!(out[4], "Fusion");
        assert_eq!(out[5], "value=42");
        assert_eq!(out[6], "[1, 2, 3]");
        assert_eq!(out[7], "[1, 2, 3]");
        assert_eq!(out[8], "7");
        assert_eq!(out[9], "[INFO] ready");
    }

    #[test]
    fn process_args_system_and_network_apis_type_check() {
        compile_source(
            r#"main:
    a = args()
    s = system()
    code = process_run("true", [])
"#,
        )
        .expect("process APIs should type-check");
        compile_source(
            r#"main:
    stream = tcp_connect("127.0.0.1", 1)
    stream.close()
"#,
        )
        .expect("network API should type-check");
    }

    #[test]
    fn invalid_async_await_type_is_rejected() {
        invalid(
            r#"main:
    value = await 42
"#,
        );
    }

    #[test]
    fn invalid_filesystem_arguments_are_rejected() {
        invalid(
            r#"main:
    value = read_file(42)
"#,
        );
        invalid(
            r#"main:
    write_file("x", 42)
"#,
        );
    }

    #[test]
    fn invalid_process_and_network_arguments_are_rejected() {
        invalid(
            r#"main:
    process_run(42, [])
"#,
        );
        invalid(
            r#"main:
    tcp_connect("localhost", "8080")
"#,
        );
    }

    #[test]
    fn invalid_encoding_and_format_arguments_are_rejected() {
        invalid(
            r#"main:
    hex_encode(42)
"#,
        );
        invalid(
            r#"main:
    format(42, [1])
"#,
        );
    }
    // =========================================================================
    // Async/await edge cases
    // =========================================================================

    #[test]
    fn async_function_accepts_parameters_and_await_returns_the_declared_type() {
        assert_eq!(
            run(
                r#"async fn add(a: num, b: num) -> num:
    return a + b

main:
    result = await add(20, 22)
    print(result)
"#,
            ),
            vec!["42"]
        );
    }

    #[test]
    fn nested_async_calls_propagate_task_results() {
        assert_eq!(
            run(
                r#"async fn inner() -> num:
    return 20

async fn outer() -> num:
    value = await inner()
    return value + 22

main:
    print(await outer())
"#,
            ),
            vec!["42"]
        );
    }

    #[test]
    fn async_functions_can_return_different_result_types() {
        assert_eq!(
            run(
                r#"async fn number() -> num:
    return 42

async fn message() -> string:
    return "fusion"

main:
    print(await number())
    print(await message())
"#,
            ),
            vec!["42", "fusion"]
        );
    }

    #[test]
    fn multiple_awaits_in_one_function_are_evaluated_in_order() {
        assert_eq!(
            run(
                r#"async fn value(x: num) -> num:
    return x

main:
    first = await value(10)
    second = await value(32)
    print(first + second)
"#,
            ),
            vec!["42"]
        );
    }

    #[test]
    fn await_works_inside_conditionals_and_loops() {
        assert_eq!(
            run(
                r#"async fn value(x: num) -> num:
    return x

main:
    total = 0
    i = 0

    while i < 3:
        if i == 1:
            total = total + await value(20)
        else:
            total = total + await value(1)
        i = i + 1

    print(total)
"#,
            ),
            vec!["22"]
        );
    }

    #[test]
    fn await_on_non_task_is_rejected_as_a_task_type_error() {
        let error = compile_source(
            r#"main:
    value = await 42
"#,
        )
        .expect_err("awaiting a non-task must fail");

        assert!(
            error.contains("Task<T>") && error.contains("num"),
            "expected Task<T>/num type error, got: {}",
            error
        );
    }

    // =========================================================================
    // Function semantics
    // =========================================================================

    #[test]
    fn recursive_functions_can_call_themselves() {
        assert_eq!(
            run(
                r#"fn factorial(n: num) -> num:
    if n <= 1:
        return 1
    else:
        return n * factorial(n - 1)

main:
    print(factorial(5))
"#,
            ),
            vec!["120"]
        );
    }

    #[test]
    fn generic_functions_preserve_the_instantiated_runtime_value() {
        assert_eq!(
            run(
                r#"fn identity<T>(value: T) -> T:
    return value

main:
    print(identity<num>(42))
    print(identity<string>("fusion"))
    print(identity<bool>(true))
"#,
            ),
            vec!["42", "fusion", "true"]
        );
    }

    #[test]
    fn explicit_return_paths_are_checked_and_execute_correctly() {
        assert_eq!(
            run(
                r#"fn choose(flag: bool) -> num:
    if flag:
        return 10
    else:
        return 32

main:
    print(choose(true))
    print(choose(false))
"#,
            ),
            vec!["10", "32"]
        );
    }

    #[test]
    fn missing_return_path_is_rejected() {
        let error = compile_source(
            r#"fn choose(flag: bool) -> num:
    if flag:
        return 42

main:
    print(choose(true))
"#,
        )
        .expect_err("a value-returning function needs a return on every path");

        assert!(
            error.contains("must return a value") || error.contains("may reach the end"),
            "expected a return-path error, got: {}",
            error
        );
    }

    #[test]
    fn nested_scopes_can_shadow_outer_variables_without_changing_outer_scope() {
        assert_eq!(
            run(
                r#"main:
    value = 10

    if true:
        num value = 32
        print(value)

    print(value)
"#,
            ),
            vec!["32", "10"]
        );
    }

    #[test]
    fn function_arguments_are_evaluated_left_to_right() {
        assert_eq!(
            run(
                r#"fn mark(value: num) -> num:
    print(value)
    return value

fn add(a: num, b: num) -> num:
    return a + b

main:
    print(add(mark(10), mark(32)))
"#,
            ),
            vec!["10", "32", "42"]
        );
    }

    // =========================================================================
    // Type-system/runtime agreement
    // =========================================================================

    #[test]
    fn primitive_runtime_values_match_their_static_types() {
        assert_eq!(
            run(
                r#"main:
    num n = 42
    float f = 3.5
    bool b = true
    string s = "fusion"
    print(n)
    print(f)
    print(b)
    print(s)
"#,
            ),
            vec!["42", "3.5", "true", "fusion"]
        );
    }

    #[test]
    fn collection_runtime_values_match_their_static_types() {
        assert_eq!(
            run(
                r#"main:
    num[] numbers = [1, 2, 3]
    HashMap<string, num> scores = hashmap()
    scores.insert("answer", 42)

    print(numbers)
    print(scores.get("answer"))
"#,
            ),
            vec!["[1, 2, 3]", "Some(42)"]
        );
    }

    #[test]
    fn option_iterator_struct_enum_and_task_runtime_values_are_represented_correctly() {
        assert_eq!(
            run(
                r#"struct Person:
    name: string

enum State:
    Ready
    Failed(string)

async fn answer() -> num:
    return 42

main:
    option = num::try("42")
    numbers = [1, 2, 3]
    person = Person(name: "Fusion")
    state = State::Failed("broken")
    task = answer()

    print(option)
    print(numbers.iterate())
    print(person)
    print(state)
    print(task)
    print(await task)
"#,
            ),
            vec![
                "Some(42)",
                "Iterator[1, 2, 3]",
                "Person { name: Fusion }",
                "State::Failed(broken)",
                "Task(42)",
                "42",
            ]
        );
    }

    #[test]
    fn invalid_programs_fail_at_the_type_boundary_for_wrong_value_types() {
        let cases = [
            (
                "wrong numeric argument",
                r#"fn takes_num(value: num) -> num:
    return value

main:
    print(takes_num("not a number"))
"#,
                "num",
            ),
            (
                "wrong array element",
                r#"main:
    num[] values = [1, "two"]
"#,
                "num",
            ),
            (
                "wrong boolean condition",
                r#"main:
    if 1:
        print("bad")
"#,
                "bool",
            ),
        ];

        for (name, source, expected) in cases {
            let error = compile_source(source)
                .expect_err("program should be rejected");
            assert!(
                error.contains(expected),
                "{} produced an unrelated error: {}",
                name,
                error
            );
        }
    }

}
