use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=runtime/lox_runtime.c");
    println!("cargo:rerun-if-changed=runtime/lox_runtime.h");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is always set");
    let runtime_dir = Path::new(&manifest_dir).join("runtime");
    let source = runtime_dir.join("lox_runtime.c");
    let output = runtime_dir.join("liblox_runtime.so");

    let cc = env::var("CC").unwrap_or_else(|_| "gcc".to_string());

    let status = Command::new(&cc)
        .args(["-Wall", "-Wextra", "-O2", "-fPIC", "-shared", "-o"])
        .arg(&output)
        .arg(&source)
        .arg("-lm")
        .status()
        .unwrap_or_else(|e| panic!("failed to run C compiler `{cc}`: {e}"));

    if !status.success() {
        panic!("C compiler failed to build {}", output.display());
    }
}
