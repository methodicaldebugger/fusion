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
use type_checker::*;
use interpreter::*;

fn main() {
    println!("Fusion Compiler 0.1");
let source = r#"
main:
    num x = 10
      print(x)
"#;
    let mut lexer = Lexer::new(source, BlockMode::Unknown);

let tokens = match lexer.tokenize() {
    Ok(tokens) => tokens,
    Err(error) => {
        eprintln!("Lexer error: {}", error);
        return;
    }
};

let mut parser = Parser::new(tokens);
let program = parser.parse();
}