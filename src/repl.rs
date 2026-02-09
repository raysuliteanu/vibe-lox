use std::io::{self, BufRead, Write};

use crate::interpreter::Interpreter;
use crate::interpreter::resolver::Resolver;
use crate::parser::Parser;
use crate::scanner;

/// Run the interactive REPL. Environment persists across lines.
pub fn run_repl() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut interpreter = Interpreter::new();

    loop {
        print!("> ");
        stdout.flush().expect("flush stdout");

        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // Ctrl-D / EOF
            Ok(_) => {}
            Err(e) => {
                eprintln!("read error: {e}");
                break;
            }
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Auto-wrap bare expressions: if the line doesn't end with ';' or '}',
        // wrap it as `print <expr>;` so the user sees the result.
        let source = if is_bare_expression(trimmed) {
            format!("print {trimmed};")
        } else {
            trimmed.to_string()
        };

        let tokens = match scanner::scan(&source) {
            Ok(t) => t,
            Err(errors) => {
                for e in &errors {
                    eprintln!("{e}");
                }
                continue;
            }
        };

        let program = match Parser::new(tokens).parse() {
            Ok(p) => p,
            Err(errors) => {
                for e in &errors {
                    eprintln!("{e}");
                }
                continue;
            }
        };

        let locals = match Resolver::new().resolve(&program) {
            Ok(l) => l,
            Err(errors) => {
                for e in &errors {
                    eprintln!("{e}");
                }
                continue;
            }
        };

        interpreter.merge_locals(locals);
        if let Err(e) = interpreter.interpret_additional(&program) {
            eprintln!("{e}");
        }
    }
}

/// Heuristic: treat the line as a bare expression if it doesn't end with
/// ';' or '}' and doesn't start with a keyword that begins a declaration
/// or statement.
fn is_bare_expression(line: &str) -> bool {
    if line.ends_with(';') || line.ends_with('}') {
        return false;
    }
    let first_word = line.split_whitespace().next().unwrap_or("");
    !matches!(
        first_word,
        "var" | "fun" | "class" | "if" | "while" | "for" | "print" | "return" | "{"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_expression_detection() {
        assert!(is_bare_expression("1 + 2"));
        assert!(is_bare_expression("x"));
        assert!(!is_bare_expression("var x = 1;"));
        assert!(!is_bare_expression("print 1;"));
        assert!(!is_bare_expression("{ var x = 1; }"));
        assert!(!is_bare_expression("if (true) print 1;"));
        assert!(!is_bare_expression("fun foo() {}"));
    }
}
