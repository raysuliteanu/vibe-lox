pub mod capture;
pub mod compiler;
pub mod runtime;
pub mod types;

use std::collections::HashMap;

use anyhow::Result;
use inkwell::context::Context;

use crate::ast::{ExprId, Program};
use crate::interpreter::resolver::Resolver;

/// Compile a Lox AST to LLVM IR and return the IR as a string.
///
/// Runs the resolver and capture analysis, then generates LLVM IR.
pub fn compile(program: &Program, source: &str) -> Result<String> {
    let locals = resolve(program)?;
    let captures = capture::analyze_captures(program);
    let context = Context::create();
    let codegen = compiler::CodeGen::new(&context, "lox", locals, captures, source);
    codegen.compile(program)
}

fn resolve(program: &Program) -> Result<HashMap<ExprId, usize>> {
    let resolver = Resolver::new();
    resolver
        .resolve(program)
        .map_err(|errors| anyhow::anyhow!("resolution errors: {:?}", errors))
}
