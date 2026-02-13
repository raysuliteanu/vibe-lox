use std::path::PathBuf;
use std::process::Command;

use vibe_lox::interpreter::Interpreter;
use vibe_lox::interpreter::resolver::Resolver;
use vibe_lox::parser::Parser;
use vibe_lox::scanner;

/// Run a Lox source through the tree-walk interpreter, returning output lines.
fn run_interpreter(source: &str) -> Vec<String> {
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

/// Compile a .lox fixture to LLVM IR, run via lli, and return stdout lines.
fn run_llvm(fixture_path: &str) -> Vec<String> {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let lox_file = project_root.join(fixture_path);
    let ll_file = lox_file.with_extension("ll");
    let runtime_lib = project_root.join("runtime/liblox_runtime.so");

    assert!(
        runtime_lib.exists(),
        "runtime library not found at {}: run `make -C runtime` first",
        runtime_lib.display()
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
            "-load",
            runtime_lib.to_str().unwrap(),
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

    let _ = std::fs::remove_file(&ll_file);

    let stdout = String::from_utf8(run_output.stdout).expect("lli output is valid UTF-8");
    stdout.lines().map(String::from).collect()
}

/// Compare tree-walk interpreter and LLVM codegen output for a fixture.
fn assert_backends_match(fixture_name: &str) {
    let source = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(fixture_name),
    )
    .unwrap_or_else(|_| panic!("read fixture {fixture_name}"));

    let interp_output = run_interpreter(&source);
    let llvm_output = run_llvm(&format!("fixtures/{fixture_name}"));

    assert_eq!(
        interp_output, llvm_output,
        "output mismatch for {fixture_name}:\n  interpreter: {interp_output:?}\n  llvm:        {llvm_output:?}"
    );
}

#[test]
fn cross_backend_arithmetic() {
    assert_backends_match("arithmetic.lox");
}

#[test]
fn cross_backend_hello() {
    assert_backends_match("hello.lox");
}

#[test]
fn cross_backend_control_flow() {
    assert_backends_match("control_flow.lox");
}

#[test]
fn cross_backend_scoping() {
    assert_backends_match("scoping.lox");
}

#[test]
fn cross_backend_fib() {
    assert_backends_match("fib.lox");
}

#[test]
fn cross_backend_counter() {
    assert_backends_match("counter.lox");
}

#[test]
fn cross_backend_strings() {
    assert_backends_match("strings.lox");
}

#[test]
fn cross_backend_classes() {
    assert_backends_match("classes.lox");
}
