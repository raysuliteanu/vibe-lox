pub mod ast;
pub mod codegen;
pub mod error;
pub mod interpreter;
pub mod parser;
pub mod repl;
pub mod scanner;
pub mod stdlib;
pub mod vm;

// Re-export error types for convenience
pub use error::{CompileError, RuntimeError};
