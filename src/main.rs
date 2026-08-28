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
    // The parser currently exposes a legacy `parse()` API.  Keep malformed
    // source from terminating the process while the parser is being migrated
    // to structured Result-based diagnostics.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut parser = Parser::new(tokens);
        parser.parse()
    }));

    result.map_err(|payload| {
        if let Some(message) = payload.downcast_ref::<String>() {
            format!("error: parser: {}", message)
        } else if let Some(message) = payload.downcast_ref::<&str>() {
            format!("error: parser: {}", message)
        } else {
            "error: parser failed while parsing the input".into()
        }
    })
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
        let program = std::panic::catch_unwind(
            std::panic::AssertUnwindSafe(|| parser.parse()),
        )
        .unwrap_or_else(|_| {
            panic!("test source should parse successfully");
        });

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

        let program = std::panic::catch_unwind(
            std::panic::AssertUnwindSafe(|| parser.parse()),
        )
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

}