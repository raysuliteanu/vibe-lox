use std::path::PathBuf;

use rstest::rstest;
use vibe_lox::error::RuntimeError;
use vibe_lox::interpreter::Interpreter;
use vibe_lox::interpreter::resolver::Resolver;
use vibe_lox::parser::Parser;
use vibe_lox::scanner;

fn run_fixture(source: &str) -> Vec<String> {
    let tokens = scanner::scan(source).expect("scan should succeed");
    let program = Parser::new(tokens).parse().expect("parse should succeed");
    let locals = Resolver::new()
        .resolve(&program)
        .expect("resolve should succeed");
    let mut interp = Interpreter::new();
    interp
        .interpret(&program, locals)
        .expect("interpret should succeed");
    interp.output().to_vec()
}

fn run_fixture_err(source: &str) -> RuntimeError {
    let tokens = scanner::scan(source).expect("scan should succeed");
    let program = Parser::new(tokens).parse().expect("parse should succeed");
    let locals = Resolver::new()
        .resolve(&program)
        .expect("resolve should succeed");
    let mut interp = Interpreter::new();
    interp.set_source(source);
    interp.interpret(&program, locals).unwrap_err()
}

#[rstest]
#[case("arithmetic.lox")]
#[case("scoping.lox")]
#[case("classes.lox")]
#[case("counter.lox")]
#[case("fib.lox")]
#[case("hello.lox")]
#[case("shebang.lox")]
#[case("to_number.lox")]
fn interpreter_fixture(#[case] fixture: &str) {
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let source = std::fs::read_to_string(fixture_dir.join(fixture))
        .unwrap_or_else(|_| panic!("read fixture {fixture}"));
    let expected = std::fs::read_to_string(fixture_dir.join(fixture.replace(".lox", ".expected")))
        .unwrap_or_else(|_| panic!("read expected for {fixture}"));
    let output = run_fixture(&source);
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(output, expected_lines);
}

#[test]
fn backtrace_nested_calls() {
    let source = include_str!("../fixtures/backtrace_nested.lox");
    let err = run_fixture_err(source);
    let frames = err.backtrace_frames();
    assert!(
        frames.len() >= 3,
        "expected at least 3 backtrace frames, got {}",
        frames.len()
    );
    // Innermost frame first: inner, then middle, then outer
    assert_eq!(frames[0].function_name, "inner");
    assert_eq!(frames[1].function_name, "middle");
    assert_eq!(frames[2].function_name, "outer");
}

#[test]
fn backtrace_empty_at_top_level() {
    let source = "var x = -\"nope\";";
    let err = run_fixture_err(source);
    assert!(
        err.backtrace_frames().is_empty(),
        "top-level errors should have no backtrace frames"
    );
}

#[test]
fn backtrace_single_call() {
    let source = r#"
fun bad() {
  var x = -"oops";
}
bad();
"#;
    let err = run_fixture_err(source);
    let frames = err.backtrace_frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].function_name, "bad");
}

// ---------------------------------------------------------------------------
// readLine() — subprocess-based tests (require a real stdin pipe)
// ---------------------------------------------------------------------------

fn run_lox_with_stdin(fixture: &str, stdin_data: &[u8]) -> String {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(fixture);
    let mut child = Command::new(env!("CARGO_BIN_EXE_vibe-lox"))
        .arg("-q")
        .arg(fixture_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn vibe-lox");
    child
        .stdin
        .take()
        .expect("stdin handle should be present")
        .write_all(stdin_data)
        .expect("write stdin data");
    let out = child.wait_with_output().expect("wait for child process");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn read_line_echo() {
    let output = run_lox_with_stdin("read_line_echo.lox", b"hello\nworld\n");
    assert_eq!(output, "hello\nworld\n");
}

#[test]
fn read_line_eof() {
    let output = run_lox_with_stdin("read_line_eof.lox", b"");
    assert_eq!(output, "EOF\n");
}

#[test]
fn read_line_to_number_valid() {
    let output = run_lox_with_stdin("read_line_to_number.lox", b"21\n");
    assert_eq!(output, "42\n");
}

#[test]
fn read_line_to_number_invalid() {
    let output = run_lox_with_stdin("read_line_to_number.lox", b"banana\n");
    assert_eq!(output, "not a number\n");
}
