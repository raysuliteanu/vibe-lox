pub mod lexer;
pub mod token;

use crate::error::CompileError;
use token::Token;

/// Scan source code into a list of tokens.
pub fn scan(source: &str) -> Result<Vec<Token>, Vec<CompileError>> {
    lexer::scan_all(source)
}
