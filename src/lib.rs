
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
pub mod name_resolution;
pub mod parser;
pub mod span;
pub mod type_checker;
pub mod types;
pub mod value;
pub mod llvm_backend;

use lexer::{BlockMode, Lexer};
use parser::Parser;
use type_checker::TypeChecker;
use name_resolution::Resolver;
use llvm_backend::LLVMBackend;

pub fn compile_source(source: &str) -> Result<ast::Program, String> {
    let mut lexer = Lexer::new(source, BlockMode::Unknown);
    let tokens = lexer.tokenize().map_err(|e| format!("lexer: {}", e))?;
    let mut parser = Parser::new(tokens);
    let program = parser.parse().map_err(|e| format!("parser: {}", e))?;
    let mut resolver = Resolver::new();
    resolver.resolve(&program).map_err(|e| format!("resolver: {}", e))?;
    let mut checker = TypeChecker::new();
    checker.check(&program).map_err(|e| format!("type checker: {}", e))?;
    Ok(program)
}

pub fn emit_llvm(source: &str) -> Result<String, String> {
    let program = compile_source(source)?;
    LLVMBackend::new().emit_program(&program).map_err(|e| e.to_string())
}


#[cfg(test)]
mod tests {
    use crate::interpreter::Interpreter;
    use crate::lexer::Token;
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

        assert!(
            !tokens
                .iter()
                .any(|token| matches!(token.node, Token::Indent | Token::Dedent))
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
}

