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

#[test]
fn vm_fixture_arithmetic() {
    let source = include_str!("../fixtures/arithmetic.lox");
    let expected = include_str!("../fixtures/arithmetic.expected");
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(run_vm_fixture(source), expected_lines);
}

#[test]
fn vm_fixture_scoping() {
    let source = include_str!("../fixtures/scoping.lox");
    let expected = include_str!("../fixtures/scoping.expected");
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(run_vm_fixture(source), expected_lines);
}

#[test]
fn vm_fixture_classes() {
    let source = include_str!("../fixtures/classes.lox");
    let expected = include_str!("../fixtures/classes.expected");
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(run_vm_fixture(source), expected_lines);
}

#[test]
fn vm_fixture_counter() {
    let source = include_str!("../fixtures/counter.lox");
    let expected = include_str!("../fixtures/counter.expected");
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(run_vm_fixture(source), expected_lines);
}

#[test]
fn vm_fixture_fibonacci() {
    let source = include_str!("../fixtures/fib.lox");
    let expected = include_str!("../fixtures/fib.expected");
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(run_vm_fixture(source), expected_lines);
}

#[test]
fn vm_fixture_hello() {
    let source = include_str!("../fixtures/hello.lox");
    let expected = include_str!("../fixtures/hello.expected");
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(run_vm_fixture(source), expected_lines);
}

#[test]
fn vm_bytecode_roundtrip_fibonacci() {
    let source = include_str!("../fixtures/fib.lox");
    let expected = include_str!("../fixtures/fib.expected");
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(run_vm_roundtrip(source), expected_lines);
}

#[test]
fn vm_bytecode_roundtrip_classes() {
    let source = include_str!("../fixtures/classes.lox");
    let expected = include_str!("../fixtures/classes.expected");
    let expected_lines: Vec<&str> = expected.lines().collect();
    assert_eq!(run_vm_roundtrip(source), expected_lines);
}

fn run_vm_err(source: &str) -> RuntimeError {
    let compiled = compile_to_chunk(source).expect("compile should succeed");
    let mut vm = Vm::new();
    vm.interpret(compiled).unwrap_err()
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
