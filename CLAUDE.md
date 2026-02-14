# vibe-lox Project Instructions

## Project Overview

A Lox language interpreter and compiler in Rust, implementing the grammar
defined in `Grammar.md`. See `PLAN.md` for the phased implementation plan.

## Build & Test Commands

```bash
cargo build                    # Build the project
cargo test                     # Run all tests (unit + integration)
cargo clippy -- -D warnings    # Lint (must be clean before commits)
cargo fmt --check              # Check formatting
cargo fmt                      # Fix formatting
cargo run -- <file.lox>        # Interpret a Lox file (tree-walk, default)
cargo run -- <file.blox>       # Autodetect bytecode and run via VM
cargo run -- --compile-llvm <file.lox> # Compile to LLVM IR (.ll file)
cargo run -- --compile-llvm -o out.ll <file.lox>  # Compile to custom output path
lli -load runtime/liblox_runtime.so <file.ll>  # Run compiled LLVM IR
./run-llvm.sh <file.lox>              # Compile and run via lli (convenience)
cargo run -- --compile <file.lox>          # Compile to native executable
cargo run -- --compile -o out <file.lox>   # Compile with custom output path
cargo run -- --dump-tokens <f> # Show tokens and stop
cargo run -- --dump-ast <f>    # Show AST (S-expressions) and stop
cargo run -- --compile-bytecode <file.lox>  # Compile and save bytecode to .blox
cargo run -- --disassemble <f> # Disassemble (source or .blox) and print
cargo run                      # Enter REPL (no file argument)
LOX_BACKTRACE=1 cargo run -- <file.lox>  # Show stack backtrace on runtime errors
```

## Architecture

Pipeline: Source -> Scanner (winnow) -> Tokens -> Parser -> AST -> Interpreter/VM/Codegen

- `src/scanner/` -- Tokenizer using `winnow` crate
- `src/parser/` -- Recursive descent parser
- `src/ast/` -- AST node definitions and printers (S-expr, JSON)
- `src/interpreter/` -- Tree-walk interpreter (default backend)
- `src/vm/` -- Bytecode VM (alternative backend)
- `src/codegen/` -- LLVM IR generation via `inkwell`
- `src/error.rs` -- Error types (`thiserror` + `miette` diagnostics)
- `runtime/` -- C runtime library for LLVM-compiled programs (built automatically by `build.rs`)

## Key Crate Dependencies

- `winnow` for tokenization (not hand-written scanner)
- `miette` (fancy) for user-facing error diagnostics with source context
- `thiserror` for error type definitions
- `anyhow` for `Result` type and `.context()` error propagation
- `clap` (derive) for CLI argument parsing
- `serde` / `serde_json` for JSON AST output
- `rmp-serde` for binary bytecode serialization (MessagePack)
- `inkwell` (llvm21-1) for LLVM IR generation via `src/codegen/`

## Conventions

- Use `anyhow::Result` throughout; use `.context("while ...")` before every `?`
- Use `thiserror` for domain error enums that implement `miette::Diagnostic`
- Use `expect()` over `unwrap()` with concise reason why it can't fail
- Test fixtures go in `fixtures/` as `.lox` files with `.expected` sidecar files
- Integration tests in `tests/`, examples in `examples/`
- Run `cargo clippy` and `cargo fmt --check` after every change
