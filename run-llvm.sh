#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "Usage: $0 <file.lox>"
    echo "Compiles a Lox file to LLVM IR and runs it via lli."
    exit 1
}

if [[ $# -ne 1 ]]; then
    usage
fi

input="$1"

if [[ ! -f "$input" ]]; then
    echo "Error: file not found: $input" >&2
    exit 1
fi

if [[ "$input" != *.lox ]]; then
    echo "Error: expected a .lox file, got: $input" >&2
    exit 1
fi

ll_file="${input%.lox}.ll"
script_dir="$(cd "$(dirname "$0")" && pwd)"
runtime_so="$script_dir/runtime/liblox_runtime.so"

# Ensure the runtime .so is built
if [[ ! -f "$runtime_so" ]]; then
    echo "Runtime not found, running cargo build..." >&2
    cargo build --manifest-path "$script_dir/Cargo.toml"
fi

# Compile to LLVM IR
cargo run --manifest-path "$script_dir/Cargo.toml" -- --compile-llvm "$input"

# Run via lli
lli -load "$runtime_so" "$ll_file"
