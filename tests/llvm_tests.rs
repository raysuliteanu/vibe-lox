use std::path::PathBuf;
use std::process::Command;

/// Compile a .lox fixture to LLVM IR, run via lli, and return stdout.
fn run_llvm_fixture(fixture_name: &str) -> String {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let lox_file = project_root.join("fixtures").join(fixture_name);
    // --compile-llvm writes .ll next to the input file
    let ll_file = project_root
        .join("fixtures")
        .join(fixture_name.replace(".lox", ".ll"));
    let runtime_obj = project_root.join("runtime/lox_runtime.o");

    // Ensure tmp/ and runtime exist
    std::fs::create_dir_all(project_root.join("tmp")).expect("create tmp dir");
    assert!(
        runtime_obj.exists(),
        "runtime object not found at {}: run `cargo build` first",
        runtime_obj.display()
    );

    // Compile .lox → .ll
    let compile_output = Command::new(env!("CARGO_BIN_EXE_vibe-lox"))
        .args(["--compile-llvm", lox_file.to_str().unwrap()])
        .output()
        .expect("run vibe-lox --compile-llvm");
    assert!(
        compile_output.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&compile_output.stderr)
    );

    // Run .ll via lli
    let run_output = Command::new("lli")
        .args([
            "--extra-object",
            runtime_obj.to_str().unwrap(),
            ll_file.to_str().unwrap(),
        ])
        .output()
        .expect("run lli");
    assert!(
        run_output.status.success(),
        "lli failed (exit {}): {}",
        run_output.status,
        String::from_utf8_lossy(&run_output.stderr)
    );

    // Clean up generated .ll file
    let _ = std::fs::remove_file(&ll_file);

    String::from_utf8(run_output.stdout).expect("lli output is valid UTF-8")
}

#[test]
fn llvm_fixture_arithmetic() {
    let output = run_llvm_fixture("arithmetic.lox");
    let expected = include_str!("../fixtures/arithmetic.expected");
    assert_eq!(output, expected);
}

#[test]
fn llvm_fixture_hello() {
    let output = run_llvm_fixture("hello.lox");
    let expected = include_str!("../fixtures/hello.expected");
    assert_eq!(output, expected);
}

#[test]
fn llvm_fixture_control_flow() {
    let output = run_llvm_fixture("control_flow.lox");
    let expected = include_str!("../fixtures/control_flow.expected");
    assert_eq!(output, expected);
}

#[test]
fn llvm_fixture_scoping() {
    let output = run_llvm_fixture("scoping.lox");
    let expected = include_str!("../fixtures/scoping.expected");
    assert_eq!(output, expected);
}

#[test]
fn llvm_fixture_fib() {
    let output = run_llvm_fixture("fib.lox");
    let expected = include_str!("../fixtures/fib.expected");
    assert_eq!(output, expected);
}

#[test]
fn llvm_fixture_counter() {
    let output = run_llvm_fixture("counter.lox");
    let expected = include_str!("../fixtures/counter.expected");
    assert_eq!(output, expected);
}

#[test]
fn llvm_fixture_strings() {
    let output = run_llvm_fixture("strings.lox");
    let expected = include_str!("../fixtures/strings.expected");
    assert_eq!(output, expected);
}

#[test]
fn llvm_fixture_classes() {
    let output = run_llvm_fixture("classes.lox");
    let expected = include_str!("../fixtures/classes.expected");
    assert_eq!(output, expected);
}

/// Compile a .lox fixture to LLVM IR, run via lli, and return stderr.
/// Asserts that lli exits with a non-zero status (runtime error).
fn run_llvm_error_fixture(fixture_name: &str) -> String {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let lox_file = project_root.join("fixtures").join(fixture_name);
    let ll_file = project_root
        .join("fixtures")
        .join(fixture_name.replace(".lox", ".ll"));
    let runtime_obj = project_root.join("runtime/lox_runtime.o");

    std::fs::create_dir_all(project_root.join("tmp")).expect("create tmp dir");
    assert!(
        runtime_obj.exists(),
        "runtime object not found at {}: run `cargo build` first",
        runtime_obj.display()
    );

    let compile_output = Command::new(env!("CARGO_BIN_EXE_vibe-lox"))
        .args(["--compile-llvm", lox_file.to_str().unwrap()])
        .output()
        .expect("run vibe-lox --compile-llvm");
    assert!(
        compile_output.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&compile_output.stderr)
    );

    let run_output = Command::new("lli")
        .args([
            "--extra-object",
            runtime_obj.to_str().unwrap(),
            ll_file.to_str().unwrap(),
        ])
        .output()
        .expect("run lli");
    assert!(
        !run_output.status.success(),
        "expected lli to fail but it succeeded with stdout: {}",
        String::from_utf8_lossy(&run_output.stdout)
    );

    let _ = std::fs::remove_file(&ll_file);

    String::from_utf8(run_output.stderr).expect("lli stderr is valid UTF-8")
}

#[test]
fn llvm_error_type_negate() {
    let stderr = run_llvm_error_fixture("error_type.lox");
    let expected = include_str!("../fixtures/error_type.expected_error");
    assert_eq!(stderr, expected);
}

#[test]
fn llvm_error_add_types() {
    let stderr = run_llvm_error_fixture("error_add_types.lox");
    let expected = include_str!("../fixtures/error_add_types.expected_error");
    assert_eq!(stderr, expected);
}

#[test]
fn llvm_error_not_callable() {
    let stderr = run_llvm_error_fixture("error_not_callable.lox");
    let expected = include_str!("../fixtures/error_not_callable.expected_error");
    assert_eq!(stderr, expected);
}

#[test]
fn llvm_error_wrong_arity() {
    let stderr = run_llvm_error_fixture("error_wrong_arity.lox");
    let expected = include_str!("../fixtures/error_wrong_arity.expected_error");
    assert_eq!(stderr, expected);
}

#[test]
fn llvm_error_property_non_instance() {
    let stderr = run_llvm_error_fixture("error_property_non_instance.lox");
    let expected = include_str!("../fixtures/error_property_non_instance.expected_error");
    assert_eq!(stderr, expected);
}
