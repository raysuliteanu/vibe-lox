use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

#[derive(Error, Debug, Diagnostic)]
pub enum LoxError {
    #[error("scan error: {message}")]
    #[diagnostic(code(lox::scan))]
    ScanError {
        message: String,
        #[label("here")]
        span: SourceSpan,
    },

    #[error("parse error: {message}")]
    #[diagnostic(code(lox::parse))]
    ParseError {
        message: String,
        #[label("here")]
        span: SourceSpan,
    },

    #[error("resolution error: {message}")]
    #[diagnostic(code(lox::resolve))]
    ResolveError {
        message: String,
        #[label("here")]
        span: SourceSpan,
    },

    #[error("runtime error: {message}")]
    #[diagnostic(code(lox::runtime))]
    RuntimeError {
        message: String,
        #[label("here")]
        span: SourceSpan,
    },

    #[error("return value")]
    Return { value: Box<dyn std::any::Any> },
}

impl LoxError {
    pub fn scan(message: impl Into<String>, offset: usize, len: usize) -> Self {
        Self::ScanError {
            message: message.into(),
            span: SourceSpan::new(offset.into(), len),
        }
    }

    pub fn parse(message: impl Into<String>, offset: usize, len: usize) -> Self {
        Self::ParseError {
            message: message.into(),
            span: SourceSpan::new(offset.into(), len),
        }
    }

    pub fn resolve(message: impl Into<String>, offset: usize, len: usize) -> Self {
        Self::ResolveError {
            message: message.into(),
            span: SourceSpan::new(offset.into(), len),
        }
    }

    pub fn runtime(message: impl Into<String>, offset: usize, len: usize) -> Self {
        Self::RuntimeError {
            message: message.into(),
            span: SourceSpan::new(offset.into(), len),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_error_implements_error_and_diagnostic() {
        let err = LoxError::scan("unexpected character", 0, 1);
        // Verify it implements std::error::Error via Display + Debug
        let _msg = format!("{err}");
        let _dbg = format!("{err:?}");
        // Verify it implements miette::Diagnostic
        let diag: &dyn Diagnostic = &err;
        assert!(diag.code().is_some());
    }

    #[test]
    fn parse_error_implements_error_and_diagnostic() {
        let err = LoxError::parse("expected ';'", 5, 1);
        let diag: &dyn Diagnostic = &err;
        assert!(diag.code().is_some());
        assert!(diag.labels().is_some());
    }

    #[test]
    fn runtime_error_implements_error_and_diagnostic() {
        let err = LoxError::runtime("undefined variable 'x'", 10, 1);
        let diag: &dyn Diagnostic = &err;
        assert!(diag.code().is_some());
    }
}
