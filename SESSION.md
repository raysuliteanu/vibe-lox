# Session Notes

## Phase 7 remaining polish items
- Runtime error messages could include dynamic arity info (e.g. "expected 2 arguments but got 1") — currently uses a generic "wrong number of arguments" because the arity is a runtime value
- Division by zero: IEEE 754 handles it (produces Inf/-Inf/NaN) so no explicit check is needed, but could add one for Lox semantics if desired
- Stack overflow detection: no depth counter is implemented; deep recursion will segfault
- `lox_runtime_error` format uses "Error: line N: message" — the interpreter uses "Error: line N: message" too, so they match
- Cross-backend comparison test (run every fixture through both backends and assert identical output) not yet implemented
- ARCHITECTURE.md not yet updated with codegen section
