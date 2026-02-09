pub mod lexer;
pub mod token;

use crate::error::LoxError;
use token::Token;

/// Scan source code into a list of tokens.
pub fn scan(source: &str) -> Result<Vec<Token>, Vec<LoxError>> {
    lexer::scan_all(source)
}
