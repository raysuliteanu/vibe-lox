pub mod chunk;
pub mod compiler;
#[allow(clippy::module_inception)]
pub mod vm;

use crate::error::LoxError;
use crate::parser::Parser;
use crate::scanner;
use crate::vm::compiler::Compiler;
use crate::vm::vm::Vm;

/// Interpret source code via the bytecode VM.
pub fn interpret_vm(source: &str) -> Result<(), LoxError> {
    let tokens = scanner::scan(source)
        .map_err(|errors| errors.into_iter().next().expect("at least one error"))?;
    let program = Parser::new(tokens)
        .parse()
        .map_err(|errors| errors.into_iter().next().expect("at least one error"))?;
    let chunk = Compiler::new().compile(&program)?;
    let mut vm = Vm::new();
    vm.interpret(chunk)
}

/// Compile source code to bytecode and return the chunk.
pub fn compile_to_chunk(source: &str) -> Result<chunk::Chunk, LoxError> {
    let tokens = scanner::scan(source)
        .map_err(|errors| errors.into_iter().next().expect("at least one error"))?;
    let program = Parser::new(tokens)
        .parse()
        .map_err(|errors| errors.into_iter().next().expect("at least one error"))?;
    Compiler::new().compile(&program)
}
