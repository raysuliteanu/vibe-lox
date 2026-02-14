pub mod capture;
pub mod compiler;
pub mod native;
pub mod runtime;
pub mod types;

use std::collections::HashMap;

use anyhow::Result;
use inkwell::context::Context;
use inkwell::module::Module;

use crate::ast::{ExprId, Program};
use crate::interpreter::resolver::Resolver;

/// Compile a Lox AST to an LLVM Module for further processing.
///
/// Runs the resolver and capture analysis, then generates LLVM IR.
pub fn compile_to_module<'ctx>(
    context: &'ctx Context,
    program: &Program,
    source: &str,
) -> Result<Module<'ctx>> {
    let locals = resolve(program)?;
    let captures = capture::analyze_captures(program);
    let codegen = compiler::CodeGen::new(context, "lox", locals, captures, source);
    codegen.emit(program)
}

/// Compile a Lox AST to LLVM IR and return the IR as a string.
///
/// Runs the resolver and capture analysis, then generates LLVM IR.
pub fn compile(program: &Program, source: &str) -> Result<String> {
    let context = Context::create();
    let module = compile_to_module(&context, program, source)?;
    Ok(module.print_to_string().to_string())
}

fn resolve(program: &Program) -> Result<HashMap<ExprId, usize>> {
    let resolver = Resolver::new();
    resolver
        .resolve(program)
        .map_err(|errors| anyhow::anyhow!("resolution errors: {:?}", errors))
}
