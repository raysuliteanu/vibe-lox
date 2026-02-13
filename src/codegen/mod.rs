pub mod compiler;
pub mod runtime;
pub mod types;

use anyhow::Result;
use inkwell::context::Context;

use crate::ast::Program;

/// Compile a Lox AST to LLVM IR and return the IR as a string.
pub fn compile(program: &Program) -> Result<String> {
    let context = Context::create();
    let codegen = compiler::CodeGen::new(&context, "lox");
    codegen.compile(program)
}
