#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "Usage: $0 <program>"
    echo "Runs a Lox program via lli."
    echo ""
    echo "Accepts a base name (without extension), a .ll file, or a .lox file."
    echo "If given a base name, looks for <program>.ll first, then <program>.lox."
    exit 1
}

if [[ $# -ne 1 ]]; then
    usage
fi

input="$1"
script_dir="$(cd "$(dirname "$0")" && pwd)"
runtime_obj="$script_dir/runtime/lox_runtime.o"

# Strip any .ll or .lox extension to get the base name
base="${input%.ll}"
base="${base%.lox}"

ll_file="${base}.ll"
lox_file="${base}.lox"

if [[ -f "$ll_file" ]]; then
    : # already compiled
elif [[ -f "$lox_file" ]]; then
    if [[ ! -f "$runtime_obj" ]]; then
        cargo -q build --manifest-path "$script_dir/Cargo.toml" >&2
    fi

    cargo -q run --manifest-path "$script_dir/Cargo.toml" -- -q --compile-llvm "$lox_file" >&2
else
    echo "Error: neither $ll_file nor $lox_file found." >&2
    exit 1
fi

# Run via lli
lli --extra-object "$runtime_obj" "$ll_file"
