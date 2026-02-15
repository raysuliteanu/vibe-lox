use std::path::PathBuf;
use std::process::Command;

use rstest::rstest;

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

#[rstest]
#[case("hello.lox")]
#[case("arithmetic.lox")]
#[case("control_flow.lox")]
#[case("scoping.lox")]
#[case("fib.lox")]
#[case("counter.lox")]
#[case("strings.lox")]
#[case("classes.lox")]
fn native_fixture(#[case] fixture: &str) {
    let output = run_native_fixture(fixture);
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
fn native_error_fixture(#[case] fixture: &str) {
    let stderr = run_native_error_fixture(fixture);
    let expected_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(fixture.replace(".lox", ".expected_error"));
    let expected = std::fs::read_to_string(&expected_path)
        .unwrap_or_else(|_| panic!("read expected error file {}", expected_path.display()));
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
