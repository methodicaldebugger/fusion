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

    #[test]
    fn indentation_main_lexes() {
        let mut lexer = Lexer::new("main:\n    x = 1\n", BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("valid indentation source");
        assert!(tokens.iter().any(|token| token.node == Token::Indent));
        assert!(tokens.iter().any(|token| token.node == Token::Dedent));
    }

    #[test]
    fn brace_main_lexes() {
        let mut lexer = Lexer::new("main{\n    x = 1\n}\n", BlockMode::Unknown);
        let tokens = lexer.tokenize().expect("valid brace source");
        assert!(tokens.iter().any(|token| token.node == Token::LeftBrace));
        assert!(tokens.iter().any(|token| token.node == Token::RightBrace));
    }

    #[test]
    fn invalid_main_spacing_is_a_lexer_error() {
        let mut lexer = Lexer::new("main {\n}\n", BlockMode::Unknown);
        assert!(lexer.tokenize().is_err());
    }

    #[test]
    fn const_assignment_is_rejected_by_type_checker() {
        let program = ast::Program {
            statements: vec![
                ast::Statement::ConstDeclaration {
                    name: "x".into(),
                    name_span: span::Span::point(0),
                    declared_type: None,
                    value: ast::Expression::Number(1),
                    span: span::Span::new(0, 1),
                },
                ast::Statement::Assignment {
                    target: ast::Expression::Identifier("x".into()),
                    value: ast::Expression::Number(2),
                },
            ],
        };
        let mut checker = TypeChecker::new();
        assert!(matches!(checker.check(&program), Err(errors::FusionError::CannotAssignToConst { .. })));
    }
}
