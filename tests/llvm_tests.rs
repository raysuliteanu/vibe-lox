use std::path::PathBuf;
use std::process::Command;

use rstest::rstest;

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

#[rstest]
#[case("arithmetic.lox")]
#[case("hello.lox")]
#[case("control_flow.lox")]
#[case("scoping.lox")]
#[case("fib.lox")]
#[case("counter.lox")]
#[case("strings.lox")]
#[case("classes.lox")]
fn llvm_fixture(#[case] fixture: &str) {
    let output = run_llvm_fixture(fixture);
    let expected_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(fixture.replace(".lox", ".expected"));
    let expected = std::fs::read_to_string(&expected_path)
        .unwrap_or_else(|_| panic!("read expected file {}", expected_path.display()));
    assert_eq!(output, expected);
}

#[rstest]
#[case("error_type.lox")]
#[case("error_add_types.lox")]
#[case("error_not_callable.lox")]
#[case("error_wrong_arity.lox")]
#[case("error_property_non_instance.lox")]
fn llvm_error_fixture(#[case] fixture: &str) {
    let stderr = run_llvm_error_fixture(fixture);
    let expected_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(fixture.replace(".lox", ".expected_error"));
    let expected = std::fs::read_to_string(&expected_path)
        .unwrap_or_else(|_| panic!("read expected error file {}", expected_path.display()));
    assert_eq!(stderr, expected);
}
