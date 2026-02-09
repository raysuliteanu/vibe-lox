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
    let json = serde_json::to_vec(&compiled).expect("serialize should succeed");
    let loaded: chunk::Chunk = serde_json::from_slice(&json).expect("deserialize should succeed");
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
