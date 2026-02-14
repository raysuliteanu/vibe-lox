# Stack Backtrace for Runtime Errors

## Context

Runtime errors currently show only a single error message and line number (interpreter) or just a message (VM). There's no way to see the call stack that led to the error. This adds Rust-style optional stack backtraces controlled by an environment variable `LOX_BACKTRACE=1`.

Example output with `LOX_BACKTRACE=1`:
```
Error: line 6: operand must be a number
stack backtrace:
  0: inner()        [line 6]
  1: outer()        [line 10]
  2: <script>       [line 13]
```

## Approach

### 1. Add a stack frame type to `RuntimeError` (`src/error.rs`)

Add a `StackFrame` struct and extend `RuntimeError::Error` with a `Vec<StackFrame>`:

```rust
#[derive(Debug, Clone)]
pub struct StackFrame {
    pub function_name: String,
    pub line: usize,
}
```

Add a method `RuntimeError::with_backtrace(self, frames: Vec<StackFrame>) -> Self` that attaches frames to an existing error.

Update `display_with_line()` to optionally render the backtrace when `LOX_BACKTRACE=1` is set.

### 2. Tree-walk interpreter: maintain a call stack (`src/interpreter/mod.rs`)

Add a `call_stack: Vec<StackFrame>` field to `Interpreter`. Push a frame in `call_function()` before executing the body, pop it after. On error, snapshot the call stack and attach it to the `RuntimeError` via `with_backtrace()`.

The interpreter already has access to:
- Function name via `Callable::name()` (`callable.rs:17`)
- Call-site span via `CallExpr.span` (`mod.rs:391`)
- Source code is available at the reporting layer

We need to push/pop frames around `execute_block` in `call_function()` (line 446) and attach the backtrace when an error propagates out (line 472).

### 3. VM: build backtrace from existing `frames` + add line numbers to errors (`src/vm/vm.rs`)

The VM already has an explicit `frames: Vec<CallFrame>` (line 115). Each frame has:
- `closure.function.name` (function name)
- `closure.function.chunk.lines` (line number table, indexed by bytecode offset)
- `ip` (current instruction pointer)

Currently the VM creates errors with `RuntimeError::new(msg)` (no span/line). Add a helper `runtime_error(&self, msg)` that:
1. Looks up the current line from `chunk.lines[ip]`
2. Snapshots `self.frames` into `Vec<StackFrame>`
3. Returns a `RuntimeError` with both line info and backtrace

Replace all `RuntimeError::new(...)` calls in the VM with `self.runtime_error(...)`.
This fixes the missing line numbers in VM errors as a natural part of the backtrace work.

### 4. Error reporting (`src/main.rs`)

Update `report_runtime_error()` (line 115) to check `std::env::var("LOX_BACKTRACE")` and, if set to `"1"` or `"full"`, render the backtrace after the error message.

### 5. REPL (`src/repl.rs`)

Same treatment — check the env var and render backtrace if present.

## Files to modify

1. **`src/error.rs`** — Add `StackFrame` struct, add `backtrace: Vec<StackFrame>` field to `RuntimeError::Error`, add `with_backtrace()` method, update `display_with_line()` to render frames
2. **`src/interpreter/mod.rs`** — Add `call_stack: Vec<StackFrame>` to `Interpreter`, push/pop in `call_function()`, attach backtrace on error propagation
3. **`src/vm/vm.rs`** — Add helper to snapshot frames into `Vec<StackFrame>`, attach backtrace when returning `Err(RuntimeError)`
4. **`src/main.rs`** — Update `report_runtime_error()` to conditionally print backtrace
5. **`src/repl.rs`** — Same backtrace rendering

## Tests

- Unit test: `RuntimeError` with backtrace renders correctly
- Unit test: backtrace is empty when no calls are active
- Integration test: run a fixture that triggers a runtime error inside nested calls, assert the backtrace contains the expected function names and ordering
- Test that without `LOX_BACKTRACE`, the output is unchanged (no regression)

## Verification

1. `cargo clippy -- -D warnings` and `cargo fmt --check`
2. `cargo test` — all existing + new tests pass
3. Manual: `LOX_BACKTRACE=1 cargo run -- tmp/counter2.lox` (with a runtime error fixture) shows the backtrace
