mod ast;
mod check;
mod cli;
mod codegen;
mod compiler;
mod diagnostics;
mod fmt;
mod lexer;
mod matchcheck;
mod modules;
mod parser;
mod project;
mod token;
mod toolchain;
mod types;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = cli::run(args);
    std::process::exit(code);
}
