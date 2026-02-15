// thiserror + miette derive macros generate unused assignments in expanded code
#![allow(unused_assignments)]

use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

use crate::scanner::token::Span;

// ============= Compile-time errors (with miette diagnostics) =============

#[derive(Error, Debug, Diagnostic)]
pub enum CompileError {
    #[error("scan error: {message}")]
    #[diagnostic(code(lox::scan))]
    Scan {
        message: String,
        #[label("here")]
        span: SourceSpan,
        #[source_code]
        src: miette::NamedSource<String>,
    },

    #[error("parse error: {message}")]
    #[diagnostic(code(lox::parse))]
    Parse {
        message: String,
        #[label("here")]
        span: SourceSpan,
        #[source_code]
        src: miette::NamedSource<String>,
    },

    #[error("resolution error: {message}")]
    #[diagnostic(code(lox::resolve))]
    Resolve {
        message: String,
        #[label("here")]
        span: SourceSpan,
        #[source_code]
        src: miette::NamedSource<String>,
    },
}

impl CompileError {
    pub fn scan(message: impl Into<String>, offset: usize, len: usize) -> Self {
        Self::Scan {
            message: message.into(),
            span: SourceSpan::new(offset.into(), len),
            src: miette::NamedSource::new("input", String::new()),
        }
    }

    pub fn parse(message: impl Into<String>, offset: usize, len: usize) -> Self {
        Self::Parse {
            message: message.into(),
            span: SourceSpan::new(offset.into(), len),
            src: miette::NamedSource::new("input", String::new()),
        }
    }

    pub fn resolve(message: impl Into<String>, offset: usize, len: usize) -> Self {
        Self::Resolve {
            message: message.into(),
            span: SourceSpan::new(offset.into(), len),
            src: miette::NamedSource::new("input", String::new()),
        }
    }

    /// Attach source code for fancy miette diagnostics
    pub fn with_source_code(self, name: impl Into<String>, source: impl Into<String>) -> Self {
        let name_str = name.into();
        let source_str = source.into();
        match self {
            Self::Scan { message, span, .. } => Self::Scan {
                message,
                span,
                src: miette::NamedSource::new(name_str, source_str),
            },
            Self::Parse { message, span, .. } => Self::Parse {
                message,
                span,
                src: miette::NamedSource::new(name_str, source_str),
            },
            Self::Resolve { message, span, .. } => Self::Resolve {
                message,
                span,
                src: miette::NamedSource::new(name_str, source_str),
            },
        }
    }
}

// ============= Runtime errors (simple, no miette) =============

/// A single frame in the Lox call stack, captured at the point of a runtime error.
#[derive(Debug, Clone)]
pub struct StackFrame {
    pub function_name: String,
    pub line: usize,
}

#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("Error: {message}")]
    Error {
        message: String,
        span: Option<Span>,
        backtrace: Vec<StackFrame>,
    },

    #[error("return")]
    Return {
        value: crate::interpreter::value::Value,
    },
}

impl RuntimeError {
    /// Create a simple runtime error without source location
    pub fn new(message: impl Into<String>) -> Self {
        Self::Error {
            message: message.into(),
            span: None,
            backtrace: Vec::new(),
        }
    }

    /// Create a runtime error with source span (for interpreter mode)
    pub fn with_span(message: impl Into<String>, span: Span) -> Self {
        Self::Error {
            message: message.into(),
            span: Some(span),
            backtrace: Vec::new(),
        }
    }

    /// Attach a call-stack backtrace to this error.
    pub fn with_backtrace(self, frames: Vec<StackFrame>) -> Self {
        match self {
            Self::Error { message, span, .. } => Self::Error {
                message,
                span,
                backtrace: frames,
            },
            other => other,
        }
    }

    /// Get the backtrace frames (empty if none attached).
    pub fn backtrace_frames(&self) -> &[StackFrame] {
        match self {
            Self::Error { backtrace, .. } => backtrace,
            Self::Return { .. } => &[],
        }
    }

    /// Format error with line number (requires source code)
    /// Only call this for Error variant, not Return
    pub fn display_with_line(&self, source: &str) -> String {
        match self {
            Self::Error {
                message,
                span: Some(span),
                ..
            } => {
                let line = offset_to_line(source, span.offset);
                format!("Error: line {}: {}", line, message)
            }
            Self::Error {
                message,
                span: None,
                ..
            } => {
                format!("Error: {}", message)
            }
            Self::Return { .. } => {
                // Should never display Return as an error
                "Error: unexpected return".to_string()
            }
        }
    }

    /// Check if this is a return value (for control flow)
    pub fn is_return(&self) -> bool {
        matches!(self, Self::Return { .. })
    }

    /// Extract return value if this is a Return variant
    pub fn into_return_value(self) -> Option<crate::interpreter::value::Value> {
        match self {
            Self::Return { value } => Some(value),
            _ => None,
        }
    }

    /// Get reference to return value if this is a Return variant
    pub fn as_return_value(&self) -> Option<&crate::interpreter::value::Value> {
        match self {
            Self::Return { value } => Some(value),
            _ => None,
        }
    }
}

/// Format the backtrace portion for display. Returns empty string if no frames.
pub fn format_backtrace(frames: &[StackFrame]) -> String {
    if frames.is_empty() {
        return String::new();
    }
    let mut out = String::from("stack backtrace:\n");
    for (i, frame) in frames.iter().enumerate() {
        out.push_str(&format!(
            "  {}: {}()\t\t[line {}]\n",
            i, frame.function_name, frame.line
        ));
    }
    out
}

/// Returns true if the user has opted into backtraces via LOX_BACKTRACE env var.
pub fn backtrace_enabled() -> bool {
    matches!(
        std::env::var("LOX_BACKTRACE").as_deref(),
        Ok("1") | Ok("full")
    )
}

/// Calculate line number from byte offset in source
fn offset_to_line(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())]
        .chars()
        .filter(|&c| c == '\n')
        .count()
        + 1
}

// ============= Tests =============

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_error_implements_diagnostic() {
        let err = CompileError::scan("test", 0, 1);
        let diag: &dyn Diagnostic = &err;
        assert!(diag.code().is_some());
    }

    #[test]
    fn compile_error_with_source() {
        let err =
            CompileError::parse("expected ';'", 5, 1).with_source_code("test.lox", "var x = 1\n");
        assert!(matches!(err, CompileError::Parse { .. }));
    }

    #[test]
    fn compile_error_all_variants() {
        let _scan = CompileError::scan("test", 0, 1);
        let _parse = CompileError::parse("test", 0, 1);
        let _resolve = CompileError::resolve("test", 0, 1);
    }

    #[test]
    fn runtime_error_simple() {
        let err = RuntimeError::new("undefined variable 'x'");
        assert!(matches!(err, RuntimeError::Error { .. }));
        assert!(!err.is_return());
    }

    #[test]
    fn runtime_error_with_span() {
        let span = Span { offset: 10, len: 5 };
        let err = RuntimeError::with_span("type error", span);
        assert!(matches!(err, RuntimeError::Error { span: Some(_), .. }));
    }

    #[test]
    fn runtime_error_return() {
        use crate::interpreter::value::Value;
        let err = RuntimeError::Return {
            value: Value::Number(42.0),
        };
        assert!(err.is_return());
        let value = err.into_return_value();
        assert!(matches!(value, Some(Value::Number(n)) if n == 42.0));
    }

    #[test]
    fn offset_to_line_basic() {
        let source = "line 1\nline 2\nline 3";
        assert_eq!(offset_to_line(source, 0), 1); // Start of line 1
        assert_eq!(offset_to_line(source, 7), 2); // Start of line 2
        assert_eq!(offset_to_line(source, 14), 3); // Start of line 3
    }

    #[test]
    fn offset_to_line_middle() {
        let source = "var x = 1;\nvar y = x + z;\n";
        assert_eq!(offset_to_line(source, 5), 1); // Middle of line 1
        assert_eq!(offset_to_line(source, 21), 2); // 'z' on line 2
    }

    #[test]
    fn runtime_error_display_with_line() {
        let source = "var x = 1;\nvar y = x + z;\n";
        let span = Span { offset: 21, len: 1 }; // 'z' is on line 2
        let err = RuntimeError::with_span("undefined variable 'z'", span);

        let display = err.display_with_line(source);
        assert_eq!(display, "Error: line 2: undefined variable 'z'");
    }

    #[test]
    fn runtime_error_display_no_span() {
        let err = RuntimeError::new("operands must be numbers");
        let display = err.display_with_line("dummy source");
        assert_eq!(display, "Error: operands must be numbers");
    }

    #[test]
    fn offset_to_line_at_newline() {
        let source = "line1\nline2\n";
        assert_eq!(offset_to_line(source, 5), 1); // At the '\n'
        assert_eq!(offset_to_line(source, 6), 2); // After the '\n'
    }

    #[test]
    fn offset_to_line_past_end() {
        let source = "short";
        assert_eq!(offset_to_line(source, 100), 1); // Past end, still line 1
    }

    #[test]
    fn runtime_error_with_backtrace() {
        let err = RuntimeError::new("operand must be a number").with_backtrace(vec![
            StackFrame {
                function_name: "inner".to_string(),
                line: 6,
            },
            StackFrame {
                function_name: "outer".to_string(),
                line: 10,
            },
            StackFrame {
                function_name: "<script>".to_string(),
                line: 13,
            },
        ]);
        let frames = err.backtrace_frames();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].function_name, "inner");
        assert_eq!(frames[0].line, 6);
        assert_eq!(frames[2].function_name, "<script>");
    }

    #[test]
    fn runtime_error_empty_backtrace() {
        let err = RuntimeError::new("some error");
        assert!(err.backtrace_frames().is_empty());
    }

    #[test]
    fn format_backtrace_renders_correctly() {
        let frames = vec![
            StackFrame {
                function_name: "inner".to_string(),
                line: 6,
            },
            StackFrame {
                function_name: "outer".to_string(),
                line: 10,
            },
            StackFrame {
                function_name: "<script>".to_string(),
                line: 13,
            },
        ];
        let output = format_backtrace(&frames);
        assert!(output.starts_with("stack backtrace:\n"));
        assert!(output.contains("0: inner()"));
        assert!(output.contains("[line 6]"));
        assert!(output.contains("1: outer()"));
        assert!(output.contains("2: <script>()"));
    }

    #[test]
    fn format_backtrace_empty_returns_empty_string() {
        assert_eq!(format_backtrace(&[]), "");
    }
}
