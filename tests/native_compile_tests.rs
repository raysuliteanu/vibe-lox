use std::path::PathBuf;
use std::process::Command;

/// Compile a .lox fixture to a native executable, run it, and return stdout.
fn run_native_fixture(fixture_name: &str) -> String {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let lox_file = project_root.join("fixtures").join(fixture_name);
    let tmp_dir = project_root.join("tmp");
    let exe_name = fixture_name.strip_suffix(".lox").unwrap_or(fixture_name);
    let exe_path = tmp_dir.join(exe_name);

    std::fs::create_dir_all(&tmp_dir).expect("create tmp dir");

    // Compile .lox → native executable
    let compile_output = Command::new(env!("CARGO_BIN_EXE_vibe-lox"))
        .args([
            "--compile",
            lox_file.to_str().unwrap(),
            "-o",
            exe_path.to_str().unwrap(),
        ])
        .output()
        .expect("run vibe-lox --compile");
    assert!(
        compile_output.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&compile_output.stderr)
    );

    // Run the executable
    let run_output = Command::new(&exe_path)
        .output()
        .expect("run native executable");
    assert!(
        run_output.status.success(),
        "executable failed (exit {}): {}",
        run_output.status,
        String::from_utf8_lossy(&run_output.stderr)
    );

    // Clean up
    let _ = std::fs::remove_file(&exe_path);

    String::from_utf8(run_output.stdout).expect("output is valid UTF-8")
}

/// Compile a .lox fixture to a native executable, run it, and return stderr.
/// Asserts that the executable exits with a non-zero status (runtime error).
fn run_native_error_fixture(fixture_name: &str) -> String {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let lox_file = project_root.join("fixtures").join(fixture_name);
    let tmp_dir = project_root.join("tmp");
    let exe_name = fixture_name.strip_suffix(".lox").unwrap_or(fixture_name);
    let exe_path = tmp_dir.join(exe_name);

    std::fs::create_dir_all(&tmp_dir).expect("create tmp dir");

    let compile_output = Command::new(env!("CARGO_BIN_EXE_vibe-lox"))
        .args([
            "--compile",
            lox_file.to_str().unwrap(),
            "-o",
            exe_path.to_str().unwrap(),
        ])
        .output()
        .expect("run vibe-lox --compile");
    assert!(
        compile_output.status.success(),
        "compile failed: {}",
        String::from_utf8_lossy(&compile_output.stderr)
    );

    let run_output = Command::new(&exe_path)
        .output()
        .expect("run native executable");
    assert!(
        !run_output.status.success(),
        "expected executable to fail but it succeeded with stdout: {}",
        String::from_utf8_lossy(&run_output.stdout)
    );

    let _ = std::fs::remove_file(&exe_path);

    String::from_utf8(run_output.stderr).expect("stderr is valid UTF-8")
}

#[test]
fn native_fixture_hello() {
    let output = run_native_fixture("hello.lox");
    let expected = include_str!("../fixtures/hello.expected");
    assert_eq!(output, expected);
}

#[test]
fn native_fixture_arithmetic() {
    let output = run_native_fixture("arithmetic.lox");
    let expected = include_str!("../fixtures/arithmetic.expected");
    assert_eq!(output, expected);
}

#[test]
fn native_fixture_control_flow() {
    let output = run_native_fixture("control_flow.lox");
    let expected = include_str!("../fixtures/control_flow.expected");
    assert_eq!(output, expected);
}

#[test]
fn native_fixture_scoping() {
    let output = run_native_fixture("scoping.lox");
    let expected = include_str!("../fixtures/scoping.expected");
    assert_eq!(output, expected);
}

#[test]
fn native_fixture_fib() {
    let output = run_native_fixture("fib.lox");
    let expected = include_str!("../fixtures/fib.expected");
    assert_eq!(output, expected);
}

#[test]
fn native_fixture_counter() {
    let output = run_native_fixture("counter.lox");
    let expected = include_str!("../fixtures/counter.expected");
    assert_eq!(output, expected);
}

#[test]
fn native_fixture_strings() {
    let output = run_native_fixture("strings.lox");
    let expected = include_str!("../fixtures/strings.expected");
    assert_eq!(output, expected);
}

#[test]
fn native_fixture_classes() {
    let output = run_native_fixture("classes.lox");
    let expected = include_str!("../fixtures/classes.expected");
    assert_eq!(output, expected);
}

#[test]
fn native_error_type_negate() {
    let stderr = run_native_error_fixture("error_type.lox");
    let expected = include_str!("../fixtures/error_type.expected_error");
    assert_eq!(stderr, expected);
}

#[test]
fn native_error_add_types() {
    let stderr = run_native_error_fixture("error_add_types.lox");
    let expected = include_str!("../fixtures/error_add_types.expected_error");
    assert_eq!(stderr, expected);
}

#[test]
fn native_error_not_callable() {
    let stderr = run_native_error_fixture("error_not_callable.lox");
    let expected = include_str!("../fixtures/error_not_callable.expected_error");
    assert_eq!(stderr, expected);
}

#[test]
fn native_error_wrong_arity() {
    let stderr = run_native_error_fixture("error_wrong_arity.lox");
    let expected = include_str!("../fixtures/error_wrong_arity.expected_error");
    assert_eq!(stderr, expected);
}

#[test]
fn native_error_property_non_instance() {
    let stderr = run_native_error_fixture("error_property_non_instance.lox");
    let expected = include_str!("../fixtures/error_property_non_instance.expected_error");
    assert_eq!(stderr, expected);
}

#[test]
fn native_compile_rejects_blox() {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let tmp_dir = project_root.join("tmp");
    std::fs::create_dir_all(&tmp_dir).expect("create tmp dir");

    // First create a .blox file
    let lox_file = project_root.join("fixtures/hello.lox");
    let blox_file = tmp_dir.join("test_reject.blox");
    let compile_bc = Command::new(env!("CARGO_BIN_EXE_vibe-lox"))
        .args([
            "--compile-bytecode",
            lox_file.to_str().unwrap(),
            "-o",
            blox_file.to_str().unwrap(),
        ])
        .output()
        .expect("run --compile-bytecode");
    assert!(compile_bc.status.success());

    // Now try to --compile the .blox file
    let exe_path = tmp_dir.join("test_reject");
    let output = Command::new(env!("CARGO_BIN_EXE_vibe-lox"))
        .args([
            "--compile",
            blox_file.to_str().unwrap(),
            "-o",
            exe_path.to_str().unwrap(),
        ])
        .output()
        .expect("run --compile on .blox");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("cannot compile .blox"),
        "expected .blox rejection error, got: {stderr}"
    );

    let _ = std::fs::remove_file(&blox_file);
    let _ = std::fs::remove_file(&exe_path);
}
