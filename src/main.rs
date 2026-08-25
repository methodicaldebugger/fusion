//contents of main.rs


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

use lexer::*;
use parser::*;
use type_checker::TypeChecker;
use interpreter::Interpreter;

fn main() {
    println!("Fusion Compiler 0.1");

    let source = r#"
main:
    x = 10
    print(x)

    if x > 5:
        y = 20
        print(y)

        if y > 10:
            z = 30
            print(z)

    x = 40
    print(x)
"#;

    // -------------------------
    // Lexer
    // -------------------------
    let mut lexer = Lexer::new(source, BlockMode::Unknown);

    let tokens = match lexer.tokenize() {
        Ok(tokens) => tokens,
        Err(error) => {
            eprintln!("Lexer error: {}", error);
            return;
        }
    };

    // -------------------------
    // Parser
    // -------------------------
    let mut parser = Parser::new(tokens);
    let program = parser.parse();

    println!("{:#?}", program);

    // -------------------------
    // Type checker
    // -------------------------
    let mut checker = TypeChecker::new();

    match checker.check(&program) {
        Ok(()) => {
            println!("Type checking passed.");
        }
        Err(error) => {
            eprintln!("Type error: {:?}", error);
            return;
        }
    }

    // -------------------------
    // Interpreter
    // -------------------------
    let mut interpreter = Interpreter::new();
    interpreter.execute(&program);
}
