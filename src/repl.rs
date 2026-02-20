use std::io::{self, Write};

use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{CompletionType, Config, Context, Editor, Helper};

use crate::interpreter::Interpreter;
use crate::interpreter::resolver::Resolver;
use crate::parser::Parser;
use crate::scanner;

// Long-form commands offered for tab completion. Short forms (\h, \q, etc.)
// match as prefixes and expand to these automatically.
const COMMANDS: &[(&str, &str)] = &[
    ("\\help", "show this help message"),
    ("\\quit", "exit the REPL"),
    ("\\clear", "clear the terminal screen"),
    ("\\version", "show the interpreter version"),
];

struct ReplHelper;

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let prefix = &line[..pos];
        // Only complete backslash commands at the start of an otherwise empty line.
        if !prefix.starts_with('\\') || prefix.contains(char::is_whitespace) {
            return Ok((pos, vec![]));
        }
        Ok((0, complete_commands(prefix)))
    }
}

impl Hinter for ReplHelper {
    type Hint = String;
}
impl Highlighter for ReplHelper {}
impl Validator for ReplHelper {}
impl Helper for ReplHelper {}

/// Run the interactive REPL. Environment persists across lines.
pub fn run_repl() {
    let config = Config::builder()
        .completion_type(CompletionType::List)
        .build();

    let mut rl: Editor<ReplHelper, rustyline::history::DefaultHistory> =
        Editor::with_config(config).expect("rustyline init cannot fail with valid config");
    rl.set_helper(Some(ReplHelper));

    let mut interpreter = Interpreter::new();

    loop {
        let line = match rl.readline("> ") {
            Ok(l) => l,
            Err(rustyline::error::ReadlineError::Interrupted) => break, // Ctrl-C
            Err(rustyline::error::ReadlineError::Eof) => break,         // Ctrl-D
            Err(e) => {
                eprintln!("read error: {e}");
                break;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('\\') {
            let mut parts = trimmed.split_whitespace();
            let cmd = parts.next().unwrap_or("");
            let args: Vec<&str> = parts.collect();
            if handle_command(cmd, &args) {
                break;
            }
            continue;
        }

        // Only Lox expressions go into history, keeping it focused on code.
        let _ = rl.add_history_entry(trimmed);

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
                for error in errors {
                    let error_with_src = error.with_source_code("<repl>", &source);
                    eprintln!("{:?}", miette::Report::new(error_with_src));
                }
                continue;
            }
        };

        let program = match Parser::new(tokens).parse() {
            Ok(p) => p,
            Err(errors) => {
                for error in errors {
                    let error_with_src = error.with_source_code("<repl>", &source);
                    eprintln!("{:?}", miette::Report::new(error_with_src));
                }
                continue;
            }
        };

        let locals = match Resolver::new().resolve(&program) {
            Ok(l) => l,
            Err(errors) => {
                for error in errors {
                    let error_with_src = error.with_source_code("<repl>", &source);
                    eprintln!("{:?}", miette::Report::new(error_with_src));
                }
                continue;
            }
        };

        interpreter.merge_locals(locals);
        interpreter.set_source(&source);
        if let Err(e) = interpreter.interpret_additional(&program)
            && !e.is_return()
        {
            eprintln!("{}", e.display_with_line(&source));
            if crate::error::backtrace_enabled() {
                let bt = crate::error::format_backtrace(e.backtrace_frames());
                if !bt.is_empty() {
                    eprint!("{bt}");
                }
            }
        }
    }
}

/// Dispatch a backslash command. Returns `true` if the REPL should exit.
fn handle_command(cmd: &str, args: &[&str]) -> bool {
    if !args.is_empty() {
        eprintln!("warning: '{cmd}' does not accept arguments");
    }
    match cmd {
        "\\h" | "\\help" => {
            println!("REPL commands:");
            println!("  \\h, \\help     Show this help message");
            println!("  \\q, \\quit     Exit the REPL");
            println!("  \\c, \\clear    Clear the terminal screen");
            println!("  \\v, \\version  Show the interpreter version");
            false
        }
        "\\q" | "\\quit" => true,
        "\\c" | "\\clear" => {
            print!("\x1b[2J\x1b[H");
            io::stdout().flush().expect("flush stdout");
            false
        }
        "\\v" | "\\version" => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            false
        }
        other => {
            eprintln!("Unknown command '{other}'. Type \\help for available commands.");
            false
        }
    }
}

/// Return the commands from `COMMANDS` whose name starts with `prefix`.
fn complete_commands(prefix: &str) -> Vec<Pair> {
    COMMANDS
        .iter()
        .filter(|(cmd, _)| cmd.starts_with(prefix))
        .map(|(cmd, desc)| Pair {
            replacement: cmd.to_string(),
            display: format!("{cmd}  {desc}"),
        })
        .collect()
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

    #[test]
    fn handle_command_quit_returns_true() {
        assert!(handle_command("\\quit", &[]));
        assert!(handle_command("\\q", &[]));
    }

    #[test]
    fn handle_command_non_quit_returns_false() {
        assert!(!handle_command("\\help", &[]));
        assert!(!handle_command("\\h", &[]));
        assert!(!handle_command("\\clear", &[]));
        assert!(!handle_command("\\c", &[]));
        assert!(!handle_command("\\version", &[]));
        assert!(!handle_command("\\v", &[]));
        assert!(!handle_command("\\unknown", &[]));
    }

    #[test]
    fn handle_command_quit_with_args_still_exits() {
        // Extra args trigger a warning but quit should still return true.
        assert!(handle_command("\\quit", &["extra"]));
        assert!(handle_command("\\q", &["extra"]));
    }

    #[test]
    fn complete_commands_all_on_backslash_only() {
        assert_eq!(complete_commands("\\").len(), 4);
    }

    #[test]
    fn complete_commands_single_match() {
        let matches = complete_commands("\\q");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].replacement, "\\quit");
    }

    #[test]
    fn complete_commands_short_forms_expand_to_long() {
        assert_eq!(complete_commands("\\h")[0].replacement, "\\help");
        assert_eq!(complete_commands("\\c")[0].replacement, "\\clear");
        assert_eq!(complete_commands("\\v")[0].replacement, "\\version");
    }

    #[test]
    fn complete_commands_empty_for_unknown_prefix() {
        assert!(complete_commands("\\xyz").is_empty());
    }
}
