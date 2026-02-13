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
    let runtime_lib = project_root.join("runtime/liblox_runtime.so");

    // Ensure tmp/ and runtime exist
    std::fs::create_dir_all(project_root.join("tmp")).expect("create tmp dir");
    assert!(
        runtime_lib.exists(),
        "runtime library not found at {}: run `make -C runtime` first",
        runtime_lib.display()
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
