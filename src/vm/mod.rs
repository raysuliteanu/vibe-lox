pub mod chunk;
pub mod compiler;
#[allow(clippy::module_inception)]
pub mod vm;

use crate::error::{CompileError, RuntimeError};
use crate::parser::Parser;
use crate::scanner;
use crate::vm::compiler::Compiler;
use crate::vm::vm::Vm;

/// Interpret source code via the bytecode VM.
/// Returns RuntimeError for execution errors.
/// Compile errors are converted to RuntimeError for simplicity.
pub fn interpret_vm(source: &str) -> Result<(), RuntimeError> {
    let tokens = scanner::scan(source).map_err(|errors| {
        RuntimeError::new(
            errors
                .into_iter()
                .next()
                .expect("at least one error")
                .to_string(),
        )
    })?;
    let program = Parser::new(tokens).parse().map_err(|errors| {
        RuntimeError::new(
            errors
                .into_iter()
                .next()
                .expect("at least one error")
                .to_string(),
        )
    })?;
    let chunk = Compiler::new()
        .compile(&program)
        .map_err(|e| RuntimeError::new(e.to_string()))?;
    let mut vm = Vm::new();
    vm.interpret(chunk)
}

/// Compile source code to bytecode and return the chunk.
pub fn compile_to_chunk(source: &str) -> Result<chunk::Chunk, CompileError> {
    let tokens = scanner::scan(source)
        .map_err(|errors| errors.into_iter().next().expect("at least one error"))?;
    let program = Parser::new(tokens)
        .parse()
        .map_err(|errors| errors.into_iter().next().expect("at least one error"))?;
    Compiler::new().compile(&program)
}
