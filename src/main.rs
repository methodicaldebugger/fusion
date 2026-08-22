//contents of main.rs
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
struct Person {
    name: string,
    age: num,
}

Person person = Person {
    name: 123,
    age: 30,
}


"#;
    let mut lexer = Lexer::new(source);
    let tokens = lexer.tokenize()
        .expect("Lexer failed");
    println!("TOKENS:");
    for token in &tokens {
        println!("{:?}", token);
    }
    let mut parser = Parser::new(tokens);
    let program = parser.parse();
    println!("AST:");
    println!("{:#?}", program);
    let mut checker = TypeChecker::new();
match checker.check(&program) {
    Ok(_) => println!("Type checking succeeded"),
    Err(e) => {
        println!("Type error: {:?}", e);
        return;
    }
}
let mut interpreter = Interpreter::new();
interpreter.execute(&program);
}