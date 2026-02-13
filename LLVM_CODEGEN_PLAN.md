# LLVM Codegen Implementation Plan

## Overview

Add LLVM IR compilation support to vibe-lox, allowing Lox programs to be
compiled to `.ll` files executable via `lli`. The implementation proceeds in
phases, each building on the last. Every phase is designed so that later phases
(especially closures and classes) extend rather than rewrite earlier work.

**Target:** LLVM 21.1.8, inkwell 0.8.0 with feature `llvm21-1`

**Output:** LLVM IR text files (`.ll`), runnable with `lli`

**Value representation:** Tagged union struct `{ tag: i8, payload: i64 }` — the
single most important design decision, since it must support all Lox value types
from the start.

---

## Architectural Decisions (Read First)

### 1. Tagged Union Value Type

Every Lox value at the LLVM level is a `LoxValue` struct:

```llvm
%LoxValue = type { i8, i64 }
; tag 0 = nil
; tag 1 = bool   (payload: 0 or 1)
; tag 2 = number (payload: bitcast f64 → i64)
; tag 3 = string (payload: pointer to heap-allocated string)
; tag 4 = function/closure (payload: pointer to closure struct)
; tag 5 = class   (payload: pointer to class descriptor)
; tag 6 = instance (payload: pointer to instance struct)
```

This is defined once in Phase 1 and every subsequent phase adds new tag
values — no restructuring needed.

### 2. Runtime Library (`lox_runtime`)

A small set of C-callable helper functions emitted as LLVM IR declarations
(extern). These handle operations that are tedious or impossible to inline:

- `lox_print(value)` — print a LoxValue
- `lox_string_concat(a, b) -> string` — heap-allocate concatenated string
- `lox_string_equal(a, b) -> bool` — compare string contents
- `lox_value_truthy(value) -> bool` — Lox truthiness rules
- `lox_alloc_closure(fn_ptr, env_ptr) -> closure_ptr` — allocate closure struct
- `lox_alloc_instance(class_ptr) -> instance_ptr` — allocate instance
- `lox_get_field(instance, name) -> value` — field access
- `lox_set_field(instance, name, value)` — field mutation
- `lox_bind_method(instance, method) -> bound_method` — create bound method

We implement these as a small C file (`runtime/lox_runtime.c`) compiled to
bitcode. For `lli` execution, we compile to a shared library and load via
`lli -load`. Only the functions needed in the current phase are implemented;
the rest are stubs.

**Why a C runtime?** Hash maps for fields/globals, string operations, and
memory allocation are painful to emit as raw LLVM IR. A thin C runtime keeps
the codegen focused on compilation logic.

### 3. Globals via Runtime Hash Map

Global variables are stored in a runtime hash map (in the C runtime), accessed
via `lox_global_get(name)` and `lox_global_set(name, value)`. This matches
Lox semantics where globals can be referenced before definition.

### 4. Functions and Closures — Uniform Representation

From Phase 1, all functions (even top-level ones) are compiled as closures with
an environment pointer parameter. In early phases the environment pointer is
simply null. When closures are added in Phase 4, captured variables are placed
into a heap-allocated environment struct and the pointer becomes non-null.

This means function signatures never change when closures are added:

```llvm
; Every Lox function has this signature:
define %LoxValue @lox_fn_NAME(%LoxValue* %env, %LoxValue %arg0, ...)
```

The `%env` parameter is ignored in phases before closures, but its presence
means no function signatures need to change later.

### 5. Module Structure

```
src/codegen/
├── mod.rs          # Public API: compile(program) -> Result<String>
├── compiler.rs     # Main CodeGen struct, AST walking
├── runtime.rs      # Runtime function declarations and LoxValue type setup
└── types.rs        # LLVM type definitions (LoxValue, closure, class, etc.)

runtime/
├── lox_runtime.c   # C runtime implementation
├── Makefile         # Build runtime to shared library
└── lox_runtime.h   # Header for reference
```

---

## Phase 1: Infrastructure and Arithmetic

**Goal:** Compile programs with number literals, arithmetic, print statements,
and global variable declarations to working LLVM IR.

### Tasks

1. **Add inkwell dependency**
   ```
   cargo add inkwell --features llvm21-1
   ```

2. **Create `src/codegen/types.rs`**
   - Define `LoxValue` struct type in LLVM (`{ i8, i64 }`)
   - Helper functions to create/extract values:
     - `build_nil() -> LoxValue`
     - `build_number(f64) -> LoxValue`
     - `build_bool(bool) -> LoxValue`
     - `extract_number(LoxValue) -> f64`
     - `extract_bool(LoxValue) -> bool`
     - `value_tag(LoxValue) -> i8`
   - Tag constants: `TAG_NIL`, `TAG_BOOL`, `TAG_NUMBER`, `TAG_STRING`, etc.

3. **Create `src/codegen/runtime.rs`**
   - Declare external runtime functions:
     - `lox_print(%LoxValue) -> void`
     - `lox_global_get(i8*, i64) -> %LoxValue` (name ptr + len)
     - `lox_global_set(i8*, i64, %LoxValue) -> void`
     - `lox_value_truthy(%LoxValue) -> i1`
   - Helper to declare all runtime functions in the LLVM module

4. **Create `src/codegen/compiler.rs`**
   - `CodeGen` struct wrapping inkwell `Context`, `Module`, `Builder`
   - Compilation entry point: create `main()` function, walk top-level decls
   - `compile_expr` for: `Literal` (Number, Bool, Nil), `Binary` (arithmetic
     and comparison), `Unary` (negate, not), `Grouping`
   - `compile_stmt` for: `Print`, `Expression`
   - `compile_decl` for: `Var` (global define), `Statement`
   - Binary operations: extract numbers, perform LLVM arithmetic, wrap result
   - Print: call `lox_print` runtime function
   - Global variables: call `lox_global_set`/`lox_global_get`
   - String literals: create global constant strings, wrap as LoxValue with
     TAG_STRING

5. **Create `src/codegen/mod.rs`**
   - Public `compile(program: &Program) -> Result<String>`
   - Returns LLVM IR as text (via `module.print_to_string()`)

6. **Create `runtime/lox_runtime.c`**
   - Tagged union struct matching LLVM layout
   - `lox_print`: print based on tag (number, bool, nil, string)
   - `lox_global_get` / `lox_global_set`: simple hash map (use a fixed-size
     array or linked list; does not need to be fast)
   - Build script or Makefile to compile to `liblox_runtime.so`

7. **Wire up CLI** in `main.rs`
   - Replace `bail!("--compile-llvm not yet implemented")` with actual
     compilation
   - Parse → AST → `codegen::compile()` → write `.ll` file

8. **Update `src/lib.rs`** to add `pub mod codegen;`

### Testing

**Unit tests** (`src/codegen/compiler.rs`):
- `test_number_literal`: compile `print 42;` → IR contains `TAG_NUMBER` and
  `double 4.2e1` constant
- `test_arithmetic`: compile `print 1 + 2;` → IR contains `fadd`
- `test_comparison`: compile `print 1 < 2;` → IR contains `fcmp`
- `test_unary_negate`: compile `print -5;` → IR contains `fneg`
- `test_global_var`: compile `var x = 10; print x;` → IR contains
  `lox_global_set` and `lox_global_get`
- `test_nil_literal`: compile `print nil;` → IR contains TAG_NIL
- `test_bool_literal`: compile `print true;` → IR contains TAG_BOOL
- `test_string_literal`: compile `print "hello";` → IR contains global string
  constant

**Integration tests** (`tests/llvm_tests.rs`):
- Run `.ll` output through `lli` (with runtime library), compare stdout to
  `.expected` files
- Start with `fixtures/arithmetic.lox`

### Deliverables

- Working `--compile-llvm` that produces `.ll` file for arithmetic programs
- `lli` can execute the output with the runtime library
- All existing tests still pass

---

## Phase 2: Control Flow and Logical Operators

**Goal:** `if`/`else`, `while` loops, `for` loops (desugared to `while` by
parser), `and`/`or` with short-circuit evaluation.

### Tasks

1. **`compile_stmt` additions:**
   - `If`: compile condition, `build_conditional_branch`, then/else blocks,
     merge block with phi or continuation
   - `While`: loop header block, condition check, body block, back-edge,
     exit block
   - `Block`: new scope — for now just compile inner declarations sequentially
     (local variables come in Phase 3)

2. **`compile_expr` additions:**
   - `Logical(And)`: short-circuit — if left is falsy, skip right
   - `Logical(Or)`: short-circuit — if left is truthy, skip right
   - Use `lox_value_truthy` runtime call for truthiness checks

3. **Equality and comparison for mixed types:**
   - `==` / `!=`: compare tags first, then payloads
   - Type error for `<`, `>`, `<=`, `>=` on non-numbers (runtime check)

### Testing

**Unit tests:**
- `test_if_true`/`test_if_false`: verify correct branch taken
- `test_if_else`: verify else branch
- `test_while_loop`: verify loop with counter
- `test_and_short_circuit`: `false and side_effect()` should not evaluate right
- `test_or_short_circuit`: `true or side_effect()` should not evaluate right
- `test_equality_nil`: `nil == nil` is true
- `test_equality_mixed`: `1 == "1"` is false

**Integration tests:**
- `fixtures/scoping.lox` (basic scoping via globals)
- New fixture: `fixtures/control_flow.lox` with expected output

### Deliverables

- Control flow works in LLVM output
- Short-circuit logical operators
- `while` and `for` loops

---

## Phase 3: Local Variables and Scoping

**Goal:** Local variables in blocks, proper lexical scoping.

### Tasks

1. **Variable tracking in `CodeGen`:**
   - Maintain a scope stack: `Vec<HashMap<String, PointerValue>>` — each entry
     maps variable names to their `alloca` pointers
   - `begin_scope()` / `end_scope()` push/pop the scope stack
   - Variable lookup walks scopes from innermost to outermost, falls back to
     global get

2. **Local variable compilation:**
   - `Var` declaration in a local scope: `alloca` in entry block, `store`
     initializer value
   - `Variable` expression: `load` from the alloca
   - `Assign`: `store` to the alloca
   - Blocks push/pop scopes

3. **`alloca` in entry block pattern:**
   - All `alloca`s go in the function's entry block (LLVM best practice for
     `mem2reg` optimization)
   - Helper: `create_entry_block_alloca(name) -> PointerValue`

### Testing

**Unit tests:**
- `test_local_var`: `{ var x = 1; print x; }` → prints 1
- `test_scope_shadowing`: inner `var x` shadows outer
- `test_scope_exit`: variable not accessible after block ends (tested by
  verifying correct global lookup fallback)
- `test_nested_blocks`: multiple nesting levels

**Integration tests:**
- `fixtures/scoping.lox` now runs correctly via LLVM path

### Deliverables

- Local variables with block scoping
- Proper shadowing behavior

---

## Phase 4: Functions and Closures

**Goal:** Named functions, function calls, closures that capture variables.

This is the phase that justifies the uniform `%env` pointer design from Phase 1.

### Tasks

1. **Function compilation:**
   - Each `FunDecl` compiles to a separate LLVM function with signature
     `(%LoxValue*, %LoxValue, %LoxValue, ...) -> %LoxValue`
   - First param is always `%env` (environment pointer)
   - Parameters after `%env` are the Lox parameters
   - Body compiled into the function
   - Implicit `return nil` at end if no explicit return
   - Function value wrapped as `TAG_FUNCTION` LoxValue (pointer to closure
     struct)

2. **Closure struct:**
   ```llvm
   %Closure = type { %LoxValue(...)*, %LoxValue* }
   ; [0] = function pointer
   ; [1] = environment pointer (array of captured LoxValues)
   ```
   - Non-capturing functions: environment pointer is null
   - Capturing functions: environment is heap-allocated array of captured values

3. **Closure capture:**
   - During compilation, track which variables are referenced from inner
     functions (similar to the bytecode compiler's upvalue resolution)
   - When a variable is captured: store it into a heap-allocated "cell"
     (`lox_alloc_cell`) so mutations are shared
   - Environment struct: array of pointers to cells
   - Free variables resolved by loading from the environment struct

4. **Function calls (`Call` expression):**
   - Extract function pointer from closure struct
   - Extract environment pointer from closure struct
   - Build LLVM `call` with env pointer + arguments
   - Arity checking at runtime (or: trust the resolver and skip)

5. **Return statement:**
   - Compile to LLVM `ret` instruction
   - Early returns in nested blocks: use the same alloca-and-branch pattern

6. **Native `clock()` function:**
   - Declare `lox_clock` in runtime
   - Register in global scope at startup

### Testing

**Unit tests:**
- `test_simple_function`: `fun f(x) { return x + 1; } print f(1);` → 2
- `test_closure_capture`: `fun make() { var x = 1; fun get() { return x; } return get; } print make()();` → 1
- `test_closure_mutation`: captured variable mutation visible through closure
- `test_nested_closures`: multi-level capture
- `test_recursion`: fibonacci via recursive calls
- `test_native_clock`: `clock()` returns a number

**Integration tests:**
- `fixtures/fibonacci.lox`
- `fixtures/counter.lox` (closures)

### Deliverables

- Functions and closures compile to LLVM IR
- Captured variables work correctly
- Recursive functions work

---

## Phase 5: String Operations

**Goal:** String concatenation, comparison, and printing.

### Tasks

1. **String representation:**
   - Strings are heap-allocated, pointer stored in LoxValue payload
   - String struct: `{ i64 len, i8* data }` or just null-terminated C strings

2. **Runtime additions:**
   - `lox_string_concat(a, b) -> string_ptr`: allocate new string
   - `lox_string_equal(a, b) -> bool`: compare contents
   - `lox_string_length(ptr) -> i64`
   - Update `lox_print` to handle strings

3. **Codegen:**
   - `Add` on strings calls `lox_string_concat`
   - `Equal`/`NotEqual` on strings calls `lox_string_equal`
   - Type checking: `+` works on two numbers or two strings, error otherwise

### Testing

**Unit tests:**
- `test_string_concat`: `"hello" + " " + "world"` → `"hello world"`
- `test_string_print`: `print "hello";` → `hello`
- `test_string_equality`: `"abc" == "abc"` → true
- `test_string_number_error`: `"a" + 1` → runtime error

**Integration tests:**
- `fixtures/hello.lox`
- New string-focused fixture

### Deliverables

- String operations work in LLVM-compiled programs

---

## Phase 6: Classes and Inheritance

**Goal:** Classes, instances, methods, `this`, `super`, inheritance.

This is the most complex phase. It builds on closures (Phase 4) for method
binding.

### Tasks

1. **Class descriptor struct:**
   ```llvm
   %ClassDescriptor = type {
     i8*,                    ; class name
     %ClassDescriptor*,      ; superclass (or null)
     %MethodTable*           ; pointer to method table
   }
   ```
   - Method table: array of `{ name: i8*, closure: %Closure* }` pairs
   - Stored as TAG_CLASS LoxValue

2. **Instance struct:**
   ```llvm
   %Instance = type {
     %ClassDescriptor*,      ; class pointer
     %FieldTable*            ; hash map of fields (in runtime)
   }
   ```
   - Stored as TAG_INSTANCE LoxValue

3. **Runtime additions:**
   - `lox_alloc_instance(class_ptr) -> instance_ptr`
   - `lox_get_field(instance, name, name_len) -> LoxValue`
   - `lox_set_field(instance, name, name_len, value)`
   - `lox_find_method(class, name, name_len) -> closure_ptr` (walks superclass
     chain)
   - `lox_bind_method(instance, closure) -> bound_method`

4. **Codegen additions:**
   - `ClassDecl`: create class descriptor, compile methods as closures, populate
     method table, define global
   - `Get` expression: field access → try fields first, then methods (with bind)
   - `Set` expression: field assignment
   - `This`: passed as first argument to methods (like `self` in Python)
   - `Super`: look up method in superclass, bind to current instance
   - `Inherit`: copy superclass methods into subclass method table
   - Constructor (`init`): allocate instance, call `init` method, return
     instance

5. **Method calls (`Invoke` optimization):**
   - `obj.method(args)` can be compiled as a single operation rather than
     get + call

### Testing

**Unit tests:**
- `test_class_instance`: create instance, set/get fields
- `test_method_call`: basic method invocation
- `test_this_binding`: `this` refers to the instance
- `test_inheritance`: subclass inherits superclass methods
- `test_super_call`: `super.method()` calls parent's implementation
- `test_init_method`: constructor returns instance
- `test_init_return_this`: `init` implicitly returns `this`

**Integration tests:**
- `fixtures/classes.lox`
- New inheritance fixture

### Deliverables

- Full OOP support in LLVM-compiled programs
- All fixtures produce identical output to tree-walk interpreter

---

## Phase 7: Error Handling and Polish

**Goal:** Runtime error messages with line numbers, edge cases, full fixture
compatibility.

### Tasks

1. **Runtime errors:**
   - Type errors (wrong operand types) → runtime abort with message and line
   - Undefined variable → runtime error
   - Wrong arity → runtime error
   - Stack overflow → detect via depth counter
   - Pass line number info to runtime functions for error reporting

2. **Line number tracking:**
   - Embed line number metadata in LLVM IR (debug info or explicit parameter)
   - Runtime functions receive line number for error messages

3. **Edge cases:**
   - Division by zero
   - Print formatting matches tree-walk interpreter exactly (integers without
     `.0`)
   - `nil` comparisons
   - Maximum argument count (255)

4. **Integration test parity:**
   - Every `.lox` / `.expected` fixture pair must produce identical output
     from both tree-walk and LLVM paths
   - Add `tests/llvm_tests.rs` mirroring `tests/interpreter_tests.rs`

5. **Documentation updates:**
   - Update `ARCHITECTURE.md` with codegen section
   - Update `CLAUDE.md` with new commands
   - Update `PLAN.md` to mark Phase 7 complete

### Testing

- All existing fixtures pass through LLVM path
- Error fixtures produce correct error messages
- Cross-backend comparison test: run every fixture through both backends,
  assert identical output

### Deliverables

- Production-quality LLVM compilation
- Full test parity with tree-walk interpreter

---

## Phase Dependency Graph

```
Phase 1: Infrastructure + Arithmetic
    ↓
Phase 2: Control Flow
    ↓
Phase 3: Local Variables
    ↓
Phase 4: Functions + Closures  ←── Phase 5: Strings (independent)
    ↓
Phase 6: Classes + Inheritance
    ↓
Phase 7: Polish + Error Handling
```

Phases 5 (Strings) and 4 (Functions) are somewhat independent and could be
done in either order, but functions are more architecturally important.

---

## Key Design Decisions That Prevent Rewrites

| Decision | Prevents | Phase introduced |
|----------|----------|-----------------|
| Tagged union `{i8, i64}` for all values | Restructuring when adding new types | 1 |
| All functions take `%env` as first param | Rewriting function signatures for closures | 1 |
| Globals in runtime hash map | Restructuring for forward references | 1 |
| Allocas in entry block | Rewriting for mem2reg optimization | 3 |
| Closure = `{fn_ptr, env_ptr}` struct | Rewriting for method binding | 4 |
| Runtime C library for complex operations | Rewriting inline IR for strings/fields | 1 |

---

## Running LLVM Output

```bash
# Compile the runtime (once)
cd runtime && make

# Compile a Lox program
cargo run -- --compile-llvm fixtures/arithmetic.lox

# Run with lli
lli -load runtime/liblox_runtime.so fixtures/arithmetic.ll

# Or compile to native (future):
# clang fixtures/arithmetic.ll runtime/liblox_runtime.c -o arithmetic
# ./arithmetic
```

---

## Commit Strategy

After each phase:
1. All tests pass (`cargo test`)
2. Linting clean (`cargo clippy -- -D warnings`)
3. Formatting clean (`cargo fmt --check`)
4. Write commit message to `./tmp/commit_msg.txt`
5. Pause for user to review and commit
6. Update `ARCHITECTURE.md` and `CLAUDE.md` if the phase changes public API
   or project structure
