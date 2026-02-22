# Lox Language Extension: Standard Input Reading

**Status:** Accepted
**Date:** 2026-02-21

---

## Table of Contents

1. [Motivation](#motivation)
2. [Survey of Other Languages](#survey-of-other-languages)
3. [Design Space and Alternatives Considered](#design-space-and-alternatives-considered)
4. [Chosen Design](#chosen-design)
5. [Grammar Extension](#grammar-extension)
6. [Semantics](#semantics)
7. [Example Programs](#example-programs)
8. [Edge Cases and Error Handling](#edge-cases-and-error-handling)
9. [Implementation Notes (Rust)](#implementation-notes-rust)

---

## Motivation

Lox as defined in *Crafting Interpreters* has output (`print` statement) but no
input. This makes it impossible to write interactive programs or tools that
process data from the outside world without embedding hardcoded values in the
source. The language is therefore limited to:

- Demonstrations with fixed data
- Benchmarks (the `clock()` built-in allows timing)
- Programs that compute from nothing

Adding stdin reading enables a much richer class of programs: calculators,
text processors, interactive quizzes, simple shells, data pipelines. Crucially,
it also makes Lox programs useful as filters in Unix pipelines, which is a
natural fit for a scripting language that already supports shebangs.

The design challenge is to add this capability in a way that is:

1. **Consistent** with Lox's existing style (simple, uncluttered syntax)
2. **Unsurprising** to users familiar with other scripting languages
3. **Minimal** — no feature bloat, no new syntax complexity
4. **Testable** across all three backends (tree-walk, bytecode VM, LLVM)

---

## Survey of Other Languages

Understanding how other languages approach stdin reading informs the design.

### Python: `input(prompt=None)`

```python
name = input("Enter your name: ")
age_str = input("Enter your age: ")
age = int(age_str)  # explicit conversion
```

- Returns a string, stripping the trailing newline
- Optional prompt argument is printed before reading
- `EOFError` raised at EOF
- No built-in "read a number" — conversion is always explicit
- One function handles all cases: simple and discoverable

### Ruby: `gets` and `$stdin.readline`

```ruby
line = gets          # reads from $stdin or ARGV files
line&.chomp          # strip trailing newline (nil-safe)
n = line.to_i        # explicit conversion
```

- `gets` returns `nil` at EOF
- Returns string including newline unless `chomp`ed
- Type conversion is always explicit via `to_i`, `to_f`

### Lua: `io.read(format)`

```lua
local line = io.read()        -- reads a line, strips newline
local n    = io.read("n")     -- reads a number
```

- Format-driven: the argument selects what type to read
- Returns `nil` at EOF
- Part of a richer `io` library (files, buffering, etc.)

### Key Observations

| Language   | Mechanism      | EOF Signal    | Type conversion   |
|------------|----------------|---------------|-------------------|
| Python     | `input()` fn   | Exception     | Explicit (`int()`) |
| Ruby       | `gets` fn      | Returns `nil` | Explicit (`to_i`)  |
| Lua        | `io.read(fmt)` | Returns `nil` | Via format arg    |

The most common pattern is a **function** that returns a **string**, with
**`nil`** on EOF, and **explicit** type conversion as a separate step.

---

## Design Space and Alternatives Considered

### Option A: `read` statement (symmetric with `print`)

```lox
read var name;           // reads a line into variable `name`
```

**Drawbacks:**
- Poor composability: `print readLine();` is natural but `read var x; print x;` is not
- EOF handling unclear from syntax
- Introduces a new keyword and parser production unnecessarily

### Option B: `readLine()` + `readNumber()` as separate functions

```lox
var name = readLine();
var n    = readNumber();
```

**Drawbacks:**
- Couples I/O and parsing in `readNumber()`
- Two functions where one + a converter is cleaner
- Can't re-parse a line already read with `readLine()`

### Option C: Single `readLine()` + `toNumber()` conversion (chosen)

```lox
var line = readLine();
var n    = toNumber(line);   // returns nil if not parseable
```

Separates I/O from parsing. Clean, composable, consistent.

### Option D: A native `io` class/object

```lox
var line = io.readLine();
```

**Drawbacks:**
- No module system in Lox; `io` would be a magic global inconsistent with `clock()`
- Overkill for the core use case

---

## Chosen Design

The proposal adopts **Option C**: two native functions with no grammar changes.

1. **`readLine()`** — reads one line from stdin, returns a string without the
   trailing newline, or `nil` at EOF.
2. **`toNumber(value)`** — converts a Lox value to a number, returning a
   `number` or `nil` if the conversion is not possible.

**Rationale:**

- **Separation of concerns**: I/O and type conversion are distinct operations.
  `readLine()` is a pure I/O primitive; `toNumber()` is a pure conversion.
- **`toNumber()` is useful beyond I/O**: it can convert string literals,
  user-computed strings, or validate input without reading anything.
- **Fixed arity** throughout, consistent with all other Lox functions.
- **`nil` as EOF sentinel** is the natural Lox choice: `nil` already represents
  "no value" throughout the language.
- **No new grammar**: both functions are `NativeFunction` variants registered
  in the global environment at interpreter startup, exactly like `clock()`.

---

## Grammar Extension

The grammar requires **no syntactic changes**. `readLine` and `toNumber` are
native functions defined in the global environment, just like `clock`. They are
called as ordinary function expressions:

```bnf
; No grammar changes required.
; readLine and toNumber are identifiers bound to native functions
; in the global environment at interpreter startup.
```

Both functions integrate seamlessly: the only change to language infrastructure
is adding two new `NativeFunction` variants to the interpreter, VM, and LLVM
runtime.

---

## Semantics

### `readLine()`

**Signature:** `readLine() -> string | nil`

**Behavior:**

1. Read bytes from standard input up to and including the next `\n` character,
   or until EOF.
2. Strip the trailing `\n` (and any preceding `\r` on Windows-style line
   endings).
3. Return the resulting string.
4. If stdin is at EOF before any bytes are read, return `nil`.
5. If some bytes were read before EOF (i.e., the last line of a file has no
   trailing newline), return those bytes as a string.

**Return type:** `string` or `nil`

**Side effects:** Advances the stdin cursor past the consumed line.

**Notes:**
- The trailing newline is *always* stripped.
- The function reads exactly one line per call. Multiple calls read successive lines.
- Leading and trailing whitespace (other than the terminating newline) are preserved.

---

### `toNumber(value)`

**Signature:** `toNumber(value) -> number | nil`

**Behavior:**

1. If `value` is already a `number`, return it unchanged.
2. If `value` is a `string`, trim leading and trailing ASCII whitespace, then
   attempt to parse the trimmed string as a Lox `NUMBER` literal:
   - Accepted format: `DIGIT+ ("." DIGIT+)?`
   - Scientific notation (e.g., `1e10`) is **not** accepted, consistent with
     Lox's own literal syntax.
   - Negative numbers (e.g., `"-1"`) are **not** accepted; the Lox `NUMBER`
     literal rule does not include a sign.
3. If `value` is `nil`, `bool`, a function, class, or instance, return `nil`.

**Return type:** `number` or `nil`

**Side effects:** None.

**Numeric edge cases:**

| Input               | Result | Reason                                    |
|---------------------|--------|-------------------------------------------|
| `42`                | `42`   | Already a number, passed through          |
| `"42"`              | `42`   | Valid integer string                      |
| `"3.14"`            | `3.14` | Valid decimal string                      |
| `"  7  "`           | `7`    | Whitespace trimmed                        |
| `""`                | `nil`  | Empty string                              |
| `"  "`              | `nil`  | Whitespace only                           |
| `"1e5"`             | `nil`  | Scientific notation not in Lox grammar    |
| `"3.14.15"`         | `nil`  | Multiple decimal points                   |
| `"-1"`              | `nil`  | Unary minus not part of NUMBER literal    |
| `nil`               | `nil`  | Non-string, non-number input              |
| `true`              | `nil`  | Non-string, non-number input              |

---

### Interaction with REPL

In the REPL, `readLine()` reads from the same stdin stream that the REPL uses.
Calling `readLine()` in the REPL consumes the *next* line the user types. This
is inherent to any interactive session that shares stdin with the interpreter
and is expected behavior.

---

### Interaction with Pipeline and EOF

When a Lox program is used as part of a Unix pipeline:

```bash
echo -e "Alice\n30" | ./greeter.lox
```

`readLine()` reads from the pipe. When the pipe is exhausted, it returns `nil`,
which the program can use as a loop termination signal — the standard Unix
filter idiom.

---

## Example Programs

### Hello, User (basic interactive input)

```lox
print "What is your name? ";
var name = readLine();
if (name == nil) {
    print "No input provided.";
} else {
    print "Hello, " + name + "!";
}
```

---

### Number doubler

```lox
print "Enter a number: ";
var n = toNumber(readLine());
if (n == nil) {
    print "That was not a number.";
} else {
    print n * 2;
}
```

---

### Sum of numbers from stdin (pipeline filter)

Reads numbers until EOF, skips non-numeric lines.

```lox
var total = 0;
var count = 0;

var line = readLine();
while (line != nil) {
    var n = toNumber(line);
    if (n != nil) {
        total = total + n;
        count = count + 1;
    }
    line = readLine();
}

if (count == 0) {
    print "No numbers read.";
} else {
    print "Sum: " + total;
    print "Count: " + count;
}
```

---

### Line-by-line echo (cat equivalent)

```lox
var line = readLine();
while (line != nil) {
    print line;
    line = readLine();
}
```

---

### Word count (wc -l equivalent)

```lox
var count = 0;
var line = readLine();
while (line != nil) {
    count = count + 1;
    line = readLine();
}
print count;
```

---

### Retry loop until valid input

```lox
var n = nil;
while (n == nil) {
    print "Enter a positive number: ";
    n = toNumber(readLine());
    if (n != nil and n <= 0) {
        print "Must be positive.";
        n = nil;
    }
}
print "You entered: " + n;
```

---

### FizzBuzz with user-provided limit

```lox
print "FizzBuzz up to: ";
var limit = toNumber(readLine());

if (limit == nil) {
    print "Invalid input.";
} else {
    var i = 1;
    while (i <= limit) {
        var fb = "";
        if ((i/3)*3 == i) { fb = fb + "Fizz"; }
        if ((i/5)*5 == i) { fb = fb + "Buzz"; }
        if (fb == "") { print i; }
        else { print fb; }
        i = i + 1;
    }
}
```

---

## Edge Cases and Error Handling

### EOF immediately

`readLine()` returns `nil`. Code that does not check for `nil` will attempt to
use `nil` in string or numeric contexts, producing a Lox runtime error:

```lox
// Unsafe — will error if stdin is empty:
print "Hello, " + readLine() + "!";
// Runtime error: operands must be two strings

// Safe:
var name = readLine();
if (name != nil) {
    print "Hello, " + name + "!";
}
```

### Empty line

`readLine()` on an empty line (just `\n`) returns the empty string `""`, not
`nil`. Empty string and `nil` are distinct.

### Non-numeric input to `toNumber()`

Returns `nil`. The caller decides how to proceed: retry, use a default, or abort.

### Extremely long lines

Implementations handle arbitrarily long lines (limited only by available
memory). Lox strings are arbitrary-length; there is no built-in line-length
limit.

### Windows line endings (`\r\n`)

`readLine()` strips both `\r` and `\n` to handle Windows-style line endings:

```
"hello\r\n"  →  readLine()  →  "hello"
```

### Stdin from a file vs. a terminal

When stdin is a TTY, `readLine()` blocks waiting for the user to press Enter.
When stdin is a file or pipe, it reads without blocking. Both work correctly.

---

## Implementation Notes (Rust)

### Shared `src/stdlib.rs` Module

To avoid duplicating the line-reading and number-parsing logic across the
three backends, a shared `src/stdlib.rs` module provides:

```rust
/// Read one line from a BufRead source, strip the trailing newline.
/// Returns None on EOF or I/O error.
pub fn read_line_from<R: std::io::BufRead>(reader: &mut R) -> Option<String>;

/// Parse a string as a Lox NUMBER literal (no sign, no scientific notation).
/// Trims leading/trailing whitespace before parsing.
/// Returns None if the string is not a valid Lox number.
pub fn parse_lox_number(s: &str) -> Option<f64>;
```

Both the tree-walk interpreter and VM call these functions, passing
`&mut std::io::stdin().lock()` in production. Integration tests use
`std::process::Command` with piped stdin rather than injecting a reader,
keeping the interpreter and VM structs simple.

### Tree-Walk Interpreter

Two new variants added to `NativeFunction` in `src/interpreter/callable.rs`:

```rust
pub enum NativeFunction {
    Clock,
    ReadLine,  // arity 0, returns string | nil
    ToNumber,  // arity 1, returns number | nil
}
```

Registered in `Interpreter::new()`:
```rust
for native in [NativeFunction::Clock, NativeFunction::ReadLine, NativeFunction::ToNumber] {
    globals.borrow_mut().define(
        native.name().to_string(),
        Value::Function(Callable::Native(native)),
    );
}
```

### Bytecode VM

Two new variants added to `NativeFn` in `src/vm/vm.rs`:

```rust
enum NativeFn {
    Clock,
    ReadLine,
    ToNumber,
}
```

Registered in `Vm::new()`:
```rust
globals.insert("readLine".into(), VmValue::NativeFunction(NativeFn::ReadLine));
globals.insert("toNumber".into(), VmValue::NativeFunction(NativeFn::ToNumber));
```

### LLVM Codegen Backend

Two new C runtime functions in `runtime/lox_runtime.c`:

```c
// Reads one line from stdin, strips newline.
// Returns TAG_STRING LoxValue, or TAG_NIL on EOF.
LoxValue lox_read_line(void);

// Converts a LoxValue to a number.
// Pass-through for TAG_NUMBER; parses TAG_STRING as Lox NUMBER literal.
// Returns TAG_NIL for non-parseable or non-string/number input.
LoxValue lox_to_number(LoxValue value);
```

Both are declared in `runtime/lox_runtime.h` and exposed via wrapper LLVM
functions registered as globals in `src/codegen/compiler.rs`, following the
same pattern as `register_native_clock()`.

### Testing Strategy

`readLine()` requires piped stdin, so testing uses integration-style subprocess
tests via `std::process::Command`:

```rust
fn run_with_stdin(source: &str, stdin_data: &[u8]) -> Vec<String> {
    // write source to a temp file, run vibe-lox with piped stdin
}
```

`toNumber()` does not touch stdin and is easily unit-tested inline.

### Error Handling

`readLine()` and `toNumber()` do **not** raise Lox runtime errors on EOF or
parse failure — they return `nil`. I/O errors and EOF are normal conditions in
stream processing, not exceptional ones. An OS-level I/O error is treated as
EOF (returns `nil`).
