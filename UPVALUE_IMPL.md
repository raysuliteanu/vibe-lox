An "upvalue" is a variable that is used by a nested function (a closure) but is defined in an enclosing (parent) function. Upvalues are the mechanism that allows closures to access and modify variables from their surrounding scope, even after the parent function has finished executing.

In `vibe-lox`, upvalues are essential for the bytecode VM's implementation of closures. Here's a breakdown of how they are used:

### 1. What is an Upvalue?

Imagine this Lox code:

```lox
fun makeCounter() {
  var i = 0;
  fun count() {
    i = i + 1;
    return i;
  }
  return count;
}

var counter = makeCounter();
print counter(); // 1
print counter(); // 2
```

Here, the `count` function accesses the variable `i` from its parent function, `makeCounter`. When `makeCounter` returns, its local variable `i` should be destroyed. However, the returned `count` function still needs to be able to access and modify `i`.

The variable `i` is an **upvalue** for the `count` function. It's "up" in the lexical scope.

### 2. How Upvalues are Implemented in `vibe-lox`

The implementation spans both the compiler (`src/vm/compiler.rs`) and the virtual machine (`src/vm/vm.rs`).

#### **During Compilation (`src/vm/compiler.rs`)**

1.  **Detection:** When the compiler is compiling a function and encounters a variable, it first tries to resolve it as a local variable within the current function. If it can't find it, it tries to resolve it as an **upvalue** by looking in the enclosing function's locals (`resolve_upvalue` function).

2.  **Capturing:** If a variable from an enclosing scope is found, the compiler marks that variable as "captured" (`is_captured = true`). This is a flag on the `Local` struct.

3.  **Upvalue Struct:** The compiler creates an `Upvalue` struct for the inner function:
    ```rust
    struct Upvalue {
        index: u8,
        is_local: bool,
    }
    ```
    *   `index`: The stack slot of the captured variable in the parent function.
    *   `is_local`: `true` if it's a direct local of the parent, `false` if it's another upvalue being passed down.

4.  **`OpCode::Closure`:** When the compiler finishes compiling a function that captures variables, it emits an `OpCode::Closure`. Following this opcode, it writes a series of bytes that describe each upvalue to be captured.

5.  **`OpCode::CloseUpvalue`:** When a captured local variable goes out of scope (e.g., at the end of a block), the compiler emits an `OpCode::CloseUpvalue`. This is a signal to the VM to "close" the upvalue.

#### **During Execution (`src/vm/vm.rs`)**

1.  **`VmUpvalue` Enum:** The VM has a runtime representation of an upvalue:
    ```rust
    enum VmUpvalue {
        Open(usize),    // Stack index (still on stack)
        Closed(VmValue),// Closed-over value (moved to heap)
    }
    ```
    *   `Open(usize)`: The upvalue is still "open," meaning the local variable it refers to is still active on the VM's stack. The `usize` is the absolute index to that value on the stack.
    *   `Closed(VmValue)`: The upvalue is "closed." The local variable has gone out of scope, so its value has been copied from the stack and is now stored directly within the `VmUpvalue` enum itself (effectively on the heap, as it's part of the closure's data).

2.  **`OpCode::Closure` Execution:** When the VM executes `OpCode::Closure`, it creates a `VmClosure` object. It reads the upvalue information emitted by the compiler and, for each upvalue, it either creates a new `VmUpvalue::Open` pointing to the variable on the stack (`capture_upvalue` function) or reuses an existing one if another closure has already captured the same variable.

3.  **`OpCode::GetUpvalue` / `OpCode::SetUpvalue`:** When the closure needs to read or write to its captured variable, these opcodes are used. The VM looks up the `VmUpvalue` in the current closure.
    *   If it's `Open`, it accesses the value on the stack at the stored index.
    *   If it's `Closed`, it accesses the `VmValue` stored directly inside the upvalue.

4.  **`OpCode::CloseUpvalue` Execution:** When this opcode is executed, the VM finds the corresponding `Open` upvalue and transforms it into a `Closed` upvalue. It does this by copying the value from the stack into a new `VmUpvalue::Closed` variant. This is the crucial step that allows the variable to outlive its stack frame.

### Summary

In essence, `Upvalue` is a clever mechanism to bridge the gap between stack-based local variables and heap-allocated closures.

*   While a variable is still in scope, upvalues are just pointers to its location on the stack (`Open`). This is efficient.
*   When the variable is about to go out of scope, its value is "closed over" by being moved from the stack into the upvalue object itself (`Closed`), ensuring it lives on for as long as the closure needs it.

This implementation is a classic and efficient way to support closures in a bytecode virtual machine, and it's a key feature that makes `vibe-lox` a powerful language.
