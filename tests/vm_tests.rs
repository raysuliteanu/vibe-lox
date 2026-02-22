use std::path::PathBuf;

use rstest::rstest;
use vibe_lox::error::RuntimeError;
use vibe_lox::vm::chunk;
use vibe_lox::vm::compile_to_chunk;
use vibe_lox::vm::vm::Vm;

fn run_vm_fixture(source: &str) -> Vec<String> {
    let compiled = compile_to_chunk(source).expect("compile should succeed");
    let mut vm = Vm::new();
    vm.interpret(compiled).expect("interpret should succeed");
    vm.output().to_vec()
}

fn run_vm_roundtrip(source: &str) -> Vec<String> {
    let compiled = compile_to_chunk(source).expect("compile should succeed");
    let bytes = rmp_serde::to_vec(&compiled).expect("serialize should succeed");
    let loaded: chunk::Chunk = rmp_serde::from_slice(&bytes).expect("deserialize should succeed");
    let mut vm = Vm::new();
    vm.interpret(loaded).expect("interpret should succeed");
    vm.output().to_vec()
}

fn run_vm_err(source: &str) -> RuntimeError {
    let compiled = compile_to_chunk(source).expect("compile should succeed");
    let mut vm = Vm::new();
    vm.interpret(compiled).unwrap_err()
}

#[rstest]
#[case("arithmetic.lox")]
#[case("scoping.lox")]
#[case("classes.lox")]
#[case("counter.lox")]
#[case("fib.lox")]
#[case("hello.lox")]
fn vm_fixture(#[case] fixture: &str) {
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let source = std::fs::read_to_string(fixture_dir.join(fixture))
        .unwrap_or_else(|_| panic!("read fixture {fixture}"));
    let expected = std::fs::read_to_string(fixture_dir.join(fixture.replace(".lox", ".expected")))
        .unwrap_or_else(|_| panic!("read expected for {fixture}"));
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(run_vm_fixture(&source), expected_lines);
}

#[rstest]
#[case("fib.lox")]
#[case("classes.lox")]
fn vm_bytecode_roundtrip(#[case] fixture: &str) {
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let source = std::fs::read_to_string(fixture_dir.join(fixture))
        .unwrap_or_else(|_| panic!("read fixture {fixture}"));
    let expected = std::fs::read_to_string(fixture_dir.join(fixture.replace(".lox", ".expected")))
        .unwrap_or_else(|_| panic!("read expected for {fixture}"));
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(run_vm_roundtrip(&source), expected_lines);
}

#[test]
fn vm_backtrace_nested_calls() {
    let source = include_str!("../fixtures/backtrace_nested.lox");
    let err = run_vm_err(source);
    let frames = err.backtrace_frames();
    assert!(
        frames.len() >= 3,
        "expected at least 3 backtrace frames, got {}",
        frames.len()
    );
    // Innermost frame first (reversed from call order)
    assert_eq!(frames[0].function_name, "inner");
    assert_eq!(frames[1].function_name, "middle");
    assert_eq!(frames[2].function_name, "outer");
}

#[test]
fn vm_backtrace_includes_line_in_error_message() {
    let source = "var x = -\"bad\";\n";
    let err = run_vm_err(source);
    let msg = err.to_string();
    assert!(
        msg.contains("line 1"),
        "VM error should include line number, got: {msg}"
    );
}

#[test]
fn vm_backtrace_top_level_has_script_frame() {
    let source = "var x = -\"bad\";\n";
    let err = run_vm_err(source);
    let frames = err.backtrace_frames();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].function_name, "<script>");
}

#[test]
fn vm_bytecode_roundtrip_with_magic_header() {
    let compiled = compile_to_chunk("print 1 + 2;").expect("compile should succeed");
    let payload = rmp_serde::to_vec(&compiled).expect("serialize should succeed");

    let mut bytes = Vec::with_capacity(4 + payload.len());
    bytes.extend_from_slice(b"blox");
    bytes.extend_from_slice(&payload);

    assert_eq!(&bytes[..4], b"blox", "file should start with magic header");

    let loaded: chunk::Chunk =
        rmp_serde::from_slice(&bytes[4..]).expect("deserialize should succeed");
    let mut vm = Vm::new();
    vm.interpret(loaded).expect("interpret should succeed");
    assert_eq!(vm.output(), &["3"]);
}

// ========== VM toNumber() via inline execution ==========

fn run_vm_source(source: &str) -> Vec<String> {
    let compiled = compile_to_chunk(source).expect("compile should succeed");
    let mut vm = Vm::new();
    vm.interpret(compiled).expect("interpret should succeed");
    vm.output().to_vec()
}

#[rstest]
#[case(r#"print toNumber("42");"#, "42")]
#[case(r#"print toNumber("3.14");"#, "3.14")]
#[case(r#"print toNumber("  7  ");"#, "7")]
#[case(r#"print toNumber("0.5");"#, "0.5")]
#[case(r#"print toNumber("007");"#, "7")]
fn vm_to_number_valid_strings(#[case] source: &str, #[case] expected: &str) {
    assert_eq!(run_vm_source(source), vec![expected]);
}

#[test]
fn vm_to_number_passthrough_number() {
    assert_eq!(run_vm_source("print toNumber(100);"), vec!["100"]);
    assert_eq!(run_vm_source("print toNumber(2.5);"), vec!["2.5"]);
}

#[rstest]
#[case(r#"print toNumber("abc");"#)]
#[case(r#"print toNumber("");"#)]
#[case(r#"print toNumber("-1");"#)]
#[case(r#"print toNumber("1e5");"#)]
#[case(r#"print toNumber("3.14.15");"#)]
fn vm_to_number_invalid_string(#[case] source: &str) {
    assert_eq!(run_vm_source(source), vec!["nil"]);
}

#[test]
fn vm_to_number_non_string_types() {
    assert_eq!(run_vm_source("print toNumber(nil);"), vec!["nil"]);
    assert_eq!(run_vm_source("print toNumber(true);"), vec!["nil"]);
    assert_eq!(run_vm_source("print toNumber(false);"), vec!["nil"]);
}

// ========== to_number.lox fixture via VM ==========

#[test]
fn vm_to_number_fixture() {
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let source =
        std::fs::read_to_string(fixture_dir.join("to_number.lox")).expect("read to_number.lox");
    let expected = std::fs::read_to_string(fixture_dir.join("to_number.expected"))
        .expect("read to_number.expected");
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(run_vm_source(&source), expected_lines);
}

// ========== readLine() subprocess tests ==========

/// Spawn the binary with a fixture file and optional stdin, returning stdout.
fn run_vm_subprocess(fixture: &str, stdin_data: &[u8]) -> String {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(fixture);
    // Compile the .lox to a .blox in a temp dir so we exercise the VM backend.
    let source =
        std::fs::read_to_string(&fixture_path).unwrap_or_else(|_| panic!("read fixture {fixture}"));
    let compiled =
        compile_to_chunk(&source).unwrap_or_else(|_| panic!("compile fixture {fixture}"));
    let blox_bytes = {
        let payload = rmp_serde::to_vec(&compiled).expect("serialize");
        let mut b = Vec::with_capacity(4 + payload.len());
        b.extend_from_slice(b"blox");
        b.extend_from_slice(&payload);
        b
    };
    let blox_path = std::env::temp_dir().join(fixture.replace(".lox", ".blox"));
    std::fs::write(&blox_path, &blox_bytes).expect("write blox temp file");

    let mut child = Command::new(env!("CARGO_BIN_EXE_vibe-lox"))
        .arg("-q")
        .arg(&blox_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn vibe-lox");
    child
        .stdin
        .take()
        .expect("stdin handle")
        .write_all(stdin_data)
        .expect("write stdin");
    let out = child.wait_with_output().expect("wait for child");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn vm_read_line_eof_returns_nil() {
    // Empty stdin → readLine() returns nil → prints "EOF"
    let output = run_vm_subprocess("read_line_eof.lox", b"");
    assert_eq!(output.trim(), "EOF");
}

#[test]
fn vm_read_line_echo() {
    let output = run_vm_subprocess("read_line_echo.lox", b"hello\nworld\n");
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines, vec!["hello", "world"]);
}

#[test]
fn vm_read_line_to_number_valid() {
    // "21\n" → toNumber("21") → 21 → 21 * 2 = 42
    let output = run_vm_subprocess("read_line_to_number.lox", b"21\n");
    assert_eq!(output.trim(), "42");
}

#[test]
fn vm_read_line_to_number_invalid() {
    let output = run_vm_subprocess("read_line_to_number.lox", b"hello\n");
    assert_eq!(output.trim(), "not a number");
}
