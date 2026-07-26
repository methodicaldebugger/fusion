mod lexer;
mod parser;
mod ast;
mod interpreter;

use lexer::Lexer;
use parser::Parser;
use interpreter::Interpreter;

fn main() {
    println!("Fusion Compiler 0.1");

let source = r#"

fn greet(name):
    return "Hello " + name


message = greet("Alice")

print(message)

"#;

    let mut lexer = Lexer::new(source);

    match lexer.tokenize() {
        Ok(tokens) => {
            println!("TOKENS:");

            for token in &tokens {
                println!("{:?}", token);
            }

            let mut parser = Parser::new(tokens);

            let program = parser.parse();

            println!("{:#?}", program);

            // Execute the AST
            let mut interpreter = Interpreter::new();
            interpreter.execute(&program);
        }

        Err(error) => {
            println!("Lexer error: {}", error);
        }
    }
}