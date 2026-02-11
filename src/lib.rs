#![allow(unused_assignments)]

pub mod ast;
pub mod error;
pub mod interpreter;
pub mod parser;
pub mod repl;
pub mod scanner;
pub mod vm;

// Re-export error types for convenience
pub use error::{CompileError, RuntimeError};
