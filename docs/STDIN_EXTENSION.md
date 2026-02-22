# Lox Language Extension Proposal: Standard Input Reading

**Status:** Draft
**Author:** vibe-lox project
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
10. [Rejected Alternatives](#rejected-alternatives)

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
line = $stdin.gets   # reads explicitly from stdin
line&.chomp          # strip trailing newline (nil-safe)
```

- `gets` returns `nil` at EOF
- Returns string including newline unless `chomp`ed
- `readline` raises `EOFError` instead of returning `nil`
- The `ARGF` / file-or-stdin duality is Ruby-specific complexity

### Lua: `io.read(format)`

```lua
local line = io.read()        -- reads a line, strips newline
local n    = io.read("n")     -- reads a number
local all  = io.read("a")     -- reads all of stdin
local line = io.read("l")     -- reads a line (same as default)
io.read("L")                  -- reads a line, keeps newline
```

- Format-driven: the argument selects what type to read
- Returns `nil` at EOF
- `io.read("n")` returns a number directly, or `nil` if the line is not numeric
- Part of a richer `io` library (files, buffering, etc.)

### JavaScript (Node.js): readline module

```js
const rl = require('readline');
const iface = rl.createInterface({ input: process.stdin });
iface.on('line', line => { /* process */ });
```

- Event-driven, async — not suitable for a simple synchronous language
- Demonstrates that "just add readline" is not always simple

### Tcl: `gets` and `read`

```tcl
gets stdin line    ; # reads into variable, returns char count or -1 on EOF
read stdin         ; # reads all remaining input
```

- `gets` stores the result in a named variable rather than returning it
- EOF is signaled by return value `-1`

### Shell (bash): `read`

```bash
read -p "Enter name: " name
```

- `read` is a shell *statement* (not a function): reads directly into a named variable
- `-p` flag specifies a prompt
- `$?` is nonzero at EOF
- Symmetric with the general "commands produce side effects" style of the shell

### Key Observations

| Language   | Mechanism      | EOF Signal    | Type conversion   | Prompt built-in |
|------------|----------------|---------------|-------------------|-----------------|
| Python     | `input()` fn   | Exception     | Explicit (`int()`)| Yes             |
| Ruby       | `gets` fn      | Returns `nil` | Explicit          | No              |
| Lua        | `io.read(fmt)` | Returns `nil` | Via format arg    | No              |
| Bash       | `read` stmt    | Return code   | All strings       | Via flag        |
| Tcl        | `gets` stmt    | Return -1     | Explicit          | No              |

The most common pattern is a **function** that returns a **string**, with
**`nil`/`null`** on EOF. Python's model is the most popular for scripting
because it handles the prompt naturally and keeps the return type simple.
Lua's format-dispatch model is elegant but adds a string-argument mini-language
that feels foreign in Lox.

---

## Design Space and Alternatives Considered

### Option A: `read` statement (symmetric with `print`)

```lox
read var name;           // reads a line into variable `name`
```

Symmetric grammar: `print` outputs, `read` inputs. Appealing at first because
it mirrors the existing `printStmt` syntax.

**Drawbacks:**

- Unlike `print`, reading requires assigning to a variable — there is no
  "read and discard" use case, so a statement form forces an awkward
  variable-binding syntax.
- EOF handling is unclear: what does the statement *do* if there is no more
  input? Silently assign `nil`? Signal a runtime error? Neither choice is
  obvious from the syntax.
- The read value cannot be used directly in an expression without first going
  through a variable:
  ```lox
  read var raw;
  if (raw == "quit") { ... }   // two lines for what could be one
  ```
- Composability is lost: `print readLine();` is naturally readable but
  `read var x; print x;` is not.
- It introduces a new keyword (`read`) and a new statement production, which
  complicates the parser even though the actual capability could be a function.

### Option B: Native functions — `readLine()` and `readNumber()`

```lox
var name = readLine();
var n    = readNumber();
```

Returns a string or number respectively. `nil` on EOF. `readNumber()` returns
`nil` if the line is not a valid number.

**Strengths:**

- Zero new syntax — fits exactly into the existing function-call expression
- Values can be used anywhere an expression is valid
- Composable: `print readLine();` just works
- EOF represented as `nil`, consistent with Lox's existing uninitialized/absent
  sentinel
- Separate `readNumber()` avoids the need to parse strings inside user code

**Drawbacks:**

- Two functions instead of one; mild discovery problem
- `readNumber()` couples I/O and parsing, which some find inelegant
- No built-in prompt mechanism (must `print` then call `readLine()`)

### Option C: Single `readLine()` with a `toNumber()` conversion function

```lox
var line = readLine();
var n    = toNumber(line);   // returns nil if not parseable
```

Keeps I/O and parsing separate. Cleaner than Option B but requires users to
always do two steps for numeric input.

### Option D: A native `io` class/object

```lox
var line = io.readLine();
var n    = io.readNumber();
io.writeLine("hello");
```

A richer API surface with a namespace. More amenable to extension (file I/O,
stderr) without polluting the global namespace.

**Drawbacks:**

- Adds a global object or class, which is not how `clock()` is provided
- Lox has no module system, so `io` would be a magic global (inconsistent)
- Overkill for the core use case: reading a line from stdin
- Complicates all three backends (interpreter, VM, LLVM codegen)

### Option E: Combined `readLine(prompt?)` with optional prompt argument

```lox
var name = readLine("Enter your name: ");
var age  = readLine();    // no prompt
```

Python's `input()` model. Clean, composable, self-prompting.

**Strengths:**

- Mirrors the most widely-known scripting language model
- Prompt is optional, so simple cases stay simple
- Still returns a string, keeping type rules uniform

**Drawbacks:**

- Variadic/optional arguments are not a Lox feature (arity is fixed)
- Would require a special case in the interpreter for optional parameters
- Alternatively, `readLine("")` with explicit empty string works but is wordy

---

## Chosen Design

The proposal adopts **Option B with an optional companion conversion function**,
slightly refined:

1. **`readLine()`** — reads one line from stdin, returns a string without the
   trailing newline, or `nil` at EOF.
2. **`readNumber()`** — reads one line from stdin and parses it as a Lox number
   (IEEE 754 double), returning the number or `nil` if the line is not a valid
   number or if EOF was reached.

Rationale for choosing two dedicated functions over a single `readLine()`:

- **Lox is a teaching language.** The dual-function design is easier to explain
  in documentation and when teaching: "use `readLine()` to read text, use
  `readNumber()` to read a number."
- **Avoids a string-parsing mini-language** (Lua-style format args).
- **Avoids hidden type coercion.** `readNumber()` makes the parsing step
  visible and explicit, even though it is a built-in.
- **Fixed arity is consistent with all other Lox functions** and requires no
  special-casing in arity-checking code.
- **Prompting is handled by `print`.** Since `print` does not append a newline
  in the standard output buffering sense (it does write `\n`, but the prompt
  pattern uses a separate `print` first), the user simply writes:
  ```lox
  print "Enter your name: ";
  var name = readLine();
  ```
  This is slightly more verbose than Python's `input("prompt")` but perfectly
  readable and avoids variadic arguments.

The `nil` sentinel for EOF is the natural Lox choice: `nil` already represents
"no value" throughout the language. Callers that need to handle EOF explicitly
check with `== nil`; callers that do not care about EOF (e.g., programs that
read exactly N lines) do not need to.

---

## Grammar Extension

The grammar requires **no syntactic changes**. `readLine` and `readNumber` are
native functions defined in the global environment, just like `clock`. They are
called as ordinary function expressions:

```bnf
; No grammar changes required.
; readLine and readNumber are identifiers bound to native functions
; in the global environment at interpreter startup.

; For documentation purposes, their call sites look like:
readLineCall  → "readLine" "(" ")" ;
readNumberCall → "readNumber" "(" ")" ;
```

Both functions have arity 0 — they take no arguments. An attempt to call them
with arguments produces the standard Lox arity error:

```
Error: expected 0 arguments but got 1
```

The only change to language infrastructure is adding two new `NativeFunction`
variants to the interpreter, VM, and optionally the LLVM runtime.

---

## Semantics

### `readLine()`

**Signature:** `readLine() -> string | nil`

**Behavior:**

1. Read bytes from standard input up to and including the next `\n` character,
   or until EOF.
2. Strip the trailing `\n` (and any preceding `\r` on Windows-style line
   endings, i.e., strip `\r\n` as a unit).
3. Return the resulting string.
4. If stdin is at EOF before any bytes are read, return `nil`.
5. If some bytes were read before EOF (i.e., the last line of a file has no
   trailing newline), return those bytes as a string — the behavior is the same
   as a normal line except the newline stripping step is a no-op.

**Return type:** `string` or `nil`

**Side effects:** Advances the stdin cursor past the consumed line.

**Notes:**

- The trailing newline is *always* stripped. This matches Python's `input()`,
  Ruby's `gets.chomp`, and Lua's `io.read()`. Lox programs should not need to
  deal with raw newline characters in their string data.
- The function reads exactly one line per call. Multiple calls read successive
  lines.
- Leading and trailing whitespace (other than the terminating newline) are
  preserved. Users who want stripped input call `trim()` if/when Lox gains
  string methods, or process the string manually.

**Pseudocode:**

```
readLine():
  line = stdin.read_line()
  if line is EOF_IMMEDIATELY:
    return nil
  strip trailing newline (and \r)
  return string(line)
```

---

### `readNumber()`

**Signature:** `readNumber() -> number | nil`

**Behavior:**

1. Read one line from stdin using the same algorithm as `readLine()`.
2. If stdin was at EOF (i.e., `readLine()` would return `nil`), return `nil`.
3. Trim leading and trailing ASCII whitespace from the line.
4. Attempt to parse the trimmed string as a Lox `NUMBER` literal:
   - Lox numbers are IEEE 754 doubles.
   - The accepted format is the Lox `NUMBER` lexical rule: `DIGIT+ ("." DIGIT+)?`
   - Scientific notation (e.g., `1e10`) is **not** accepted, for consistency
     with Lox's own literal syntax.
5. If parsing succeeds, return the number.
6. If parsing fails (e.g., the line is `"hello"`, `""`, `"3.14.15"`, `"1e5"`),
   return `nil`.

**Return type:** `number` or `nil`

**Side effects:** Advances the stdin cursor past the consumed line, regardless
of whether parsing succeeds. The line is consumed even when `nil` is returned.

**Rationale for consuming the line on parse failure:** This matches Lua's
`io.read("n")` behavior and avoids an awkward "re-read" situation where the
caller loops trying to get a valid number from an already-partially-consumed
buffer.

**Pseudocode:**

```
readNumber():
  line = stdin.read_line()
  if line is EOF_IMMEDIATELY:
    return nil
  strip trailing newline (and \r)
  trimmed = trim_whitespace(line)
  parsed = parse_lox_number(trimmed)
  if parsed is valid:
    return number(parsed)
  else:
    return nil
```

---

### Interaction with REPL

In the REPL, `readLine()` and `readNumber()` read from the same stdin stream
that the REPL itself uses. This creates an unusual situation: calling
`readLine()` in the REPL consumes the *next* line the user types, rather than
echoing an independent prompt. This is inherent to any interactive session that
shares stdin with the interpreter and is documented as expected behavior.

Implementations may choose to note this in REPL startup text or issue a warning
when `readLine()` or `readNumber()` are called interactively.

---

### Interaction with Pipeline and EOF

When a Lox program is used as part of a Unix pipeline:

```bash
echo -e "Alice\n30" | ./greeter.lox
```

or

```bash
cat data.txt | ./process.lox
```

`readLine()` and `readNumber()` read from the pipe. When the pipe is exhausted,
both functions return `nil`, which the program can use as a loop termination
signal. This is the standard Unix filter idiom.

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

Sample session:
```
What is your name? Alice
Hello, Alice!
```

---

### Number doubler

```lox
print "Enter a number: ";
var n = readNumber();
if (n == nil) {
    print "That was not a number.";
} else {
    print n * 2;
}
```

Sample session:
```
Enter a number: 21
42
```

```
Enter a number: banana
That was not a number.
```

---

### Sum of numbers from stdin (pipeline filter)

Reads numbers until EOF, prints their sum. Skips non-numeric lines.

```lox
var total = 0;
var count = 0;

var line = readLine();
while (line != nil) {
    var n = readNumber();  // BUG: this would consume the NEXT line
    // ...
}
```

Wait — in the above, `readLine()` consumed the line, and then `readNumber()`
would try to read the *next* line. The correct approach is to use `readNumber()`
directly as the loop driver when reading numbers:

```lox
var total = 0;
var count = 0;

var n = readNumber();
while (n != nil) {
    total = total + n;
    count = count + 1;
    n = readNumber();
}

if (count == 0) {
    print "No numbers read.";
} else {
    print "Sum: " + total;
    print "Count: " + count;
}
```

Usage:
```bash
printf "1\n2\n3\n4\n5\n" | cargo run -- sum.lox
Sum: 15
Count: 5
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

### Simple interactive calculator (REPL-style)

```lox
fun prompt(msg) {
    print msg;
    return readNumber();
}

var a = prompt("First number: ");
var op_line = nil;
print "Operator (+, -, *, /): ";
var op = readLine();
var b = prompt("Second number: ");

if (a == nil or b == nil) {
    print "Error: expected numbers.";
} else if (op == "+") {
    print a + b;
} else if (op == "-") {
    print a - b;
} else if (op == "*") {
    print a * b;
} else if (op == "/") {
    if (b == 0) {
        print "Error: division by zero.";
    } else {
        print a / b;
    }
} else {
    print "Unknown operator.";
}
```

---

### Word count (wc -l equivalent)

Counts lines from stdin:

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
    n = readNumber();
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
var limit = readNumber();

if (limit == nil) {
    print "Invalid input.";
} else {
    var i = 1;
    while (i <= limit) {
        if (i / 3 == (i / 3 - (i / 3) % 1) and (i / 3) % 1 == 0
            and i / 5 == (i / 5 - (i / 5) % 1) and (i / 5) % 1 == 0) {
            // Lox lacks modulo — work around with division
        }
        // Simpler: use a helper
        i = i + 1;
    }
}
```

Since Lox lacks a modulo operator, a cleaner version using a helper function:

```lox
fun divisible(n, d) {
    // integer division check: n / d has no fractional part
    var q = n / d;
    return q == q - (q - (q * 1));
    // Actually: check if (n / d) * d == n using floating equality
}

// Practical FizzBuzz with readNumber:
print "FizzBuzz limit: ";
var limit = readNumber();
if (limit == nil) { print "Need a number."; }
else {
    var i = 1;
    while (i <= limit) {
        // Lox number equality: (i/3)*3 == i
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

### Reading structured data (CSV-like)

Reads pairs of name,score lines:

```lox
print "Enter name,score pairs (one per line, empty line to stop):";

var name = readLine();
while (name != nil and name != "") {
    print "Score for " + name + ": ";
    var score = readNumber();
    if (score == nil) {
        print "  (invalid score, skipping)";
    } else {
        print "  " + name + " -> " + score;
    }
    name = readLine();
}

print "Done.";
```

---

## Edge Cases and Error Handling

### EOF immediately

Both functions return `nil`. Code that does not check for `nil` will
attempt to use `nil` in string or numeric contexts, producing a Lox
runtime error. This is intentional — `nil` is the signal that input has
been exhausted, and programs that do not handle it are likely buggy.

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

`readLine()` on an empty line (just `\n`) returns the empty string `""`,
not `nil`. Empty string and `nil` are distinct. This matches Python and
Ruby behavior.

```lox
var line = readLine();
// line == ""    if user pressed Enter with no input
// line == nil   if stdin was already at EOF
```

### Non-numeric input to `readNumber()`

Returns `nil`. The line is consumed. The caller must decide how to proceed —
retry, use a default, or abort.

### Extremely long lines

Implementations should handle arbitrarily long lines (limited only by available
memory). Lox strings are arbitrary-length. There is no built-in line-length
limit.

### Binary / non-UTF-8 input

Lox strings are sequences of characters. Implementations that use UTF-8
internally (all three backends in vibe-lox) should handle valid UTF-8 input.
Behavior on invalid UTF-8 bytes is implementation-defined. The reference
recommendation is to either:
- Replace invalid byte sequences with the Unicode replacement character U+FFFD, or
- Return `nil` and halt further reads

The vibe-lox implementation should document which choice it makes. The
recommended default for a teaching interpreter is the replacement-character
approach (most forgiving).

### Mixing `readLine()` and `readNumber()`

Each call to either function reads exactly one line. They share the same stdin
buffer. Thus:

```lox
var s = readLine();    // reads line 1
var n = readNumber();  // reads line 2 and parses it
var t = readLine();    // reads line 3
```

There is no "put back" (unget) mechanism. Callers that read a line with
`readLine()` and then want to parse it as a number must use a string-to-number
conversion (`toNumber()` — see the Future Extensions section).

### Windows line endings (`\r\n`)

Both functions strip the trailing newline. When the implementation strips `\n`,
it should also strip a preceding `\r` to handle Windows-style line endings
correctly. This makes Lox programs portable across platforms.

```
"hello\r\n"  →  readLine()  →  "hello"
"42\r\n"     →  readNumber()  →  42
```

### Stdin from a file vs. a terminal

When stdin is a TTY, `readLine()` will block waiting for the user to press Enter.
When stdin is a file or pipe, it reads without blocking. Lox programs cannot
distinguish these cases — both work correctly and the behavior is the intended
Unix convention.

### Numeric edge cases for `readNumber()`

| Input string  | Result       | Reason                                    |
|---------------|--------------|-------------------------------------------|
| `"42"`        | `42`         | Valid integer                             |
| `"3.14"`      | `3.14`       | Valid decimal                             |
| `"  7  "`     | `7`          | Whitespace trimmed                        |
| `""`          | `nil`        | Empty string, not a number                |
| `"  "`        | `nil`        | Whitespace only, not a number             |
| `"1e5"`       | `nil`        | Scientific notation not in Lox grammar    |
| `"3.14.15"`   | `nil`        | Multiple decimal points                   |
| `"-1"`        | `nil`        | Unary minus not part of the literal form  |
| `"inf"`       | `nil`        | Not a valid Lox literal                   |
| `"nan"`       | `nil`        | Not a valid Lox literal                   |
| `"0.5"`       | `0.5`        | Valid decimal                             |
| `"007"`       | `7`          | Leading zeros parse (Lox has no octal)    |

Note on negative numbers: `readNumber()` does not accept `-1` because the Lox
`NUMBER` grammar rule does not include a sign — negation is a unary operator
applied at the expression level. Programs that need to accept negative numbers
should read a string with `readLine()` and parse it, or use a future
`toNumber()` conversion function that handles signs. This is consistent with
how Lox handles negative literals: `print -1;` is parsed as `print` applied to
the unary negation of `1`.

---

## Implementation Notes (Rust)

### Tree-Walk Interpreter

Add two new variants to the `NativeFunction` enum in
`src/interpreter/callable.rs`:

```rust
#[derive(Debug, Clone, Copy)]
pub enum NativeFunction {
    Clock,
    ReadLine,
    ReadNumber,
}

impl NativeFunction {
    pub fn name(&self) -> &str {
        match self {
            Self::Clock      => "clock",
            Self::ReadLine   => "readLine",
            Self::ReadNumber => "readNumber",
        }
    }

    pub fn arity(&self) -> usize {
        // All three take zero arguments.
        0
    }

    pub fn call(&self, _args: &[Value]) -> Value {
        match self {
            Self::Clock => { /* existing */ }
            Self::ReadLine   => read_line_native(),
            Self::ReadNumber => read_number_native(),
        }
    }
}
```

The native implementations live in the same file or a sibling
`src/interpreter/io.rs` module:

```rust
fn read_line_native() -> Value {
    let mut buf = String::new();
    match std::io::stdin().read_line(&mut buf) {
        Ok(0) => Value::Nil,           // EOF
        Ok(_) => {
            // Strip \r\n or \n
            if buf.ends_with('\n') {
                buf.pop();
                if buf.ends_with('\r') {
                    buf.pop();
                }
            }
            Value::Str(buf)
        }
        Err(_) => Value::Nil,          // I/O error treated as EOF
    }
}

fn read_number_native() -> Value {
    match read_line_native() {
        Value::Nil      => Value::Nil,
        Value::Str(s)   => {
            let trimmed = s.trim();
            // Parse using Lox NUMBER grammar: DIGIT+ ("." DIGIT+)?
            parse_lox_number(trimmed)
                .map(Value::Number)
                .unwrap_or(Value::Nil)
        }
        _ => unreachable!("read_line_native only returns Nil or Str"),
    }
}

fn parse_lox_number(s: &str) -> Option<f64> {
    // Accepts only: DIGIT+ | DIGIT+ "." DIGIT+
    // Rejects: scientific notation, signs, empty, non-digits.
    if s.is_empty() {
        return None;
    }
    let bytes = s.as_bytes();
    let mut i = 0;
    // Consume leading digits
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return None; // no leading digit
    }
    // Optional "." followed by digits
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let decimal_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if i == decimal_start {
            return None; // "3." with no digits after decimal
        }
    }
    // Must have consumed all input
    if i != bytes.len() {
        return None;
    }
    s.parse::<f64>().ok()
}
```

Register the new natives in `Interpreter::new()`:

```rust
pub fn new() -> Self {
    let globals = Rc::new(RefCell::new(Environment::new()));
    for native in [NativeFunction::Clock,
                   NativeFunction::ReadLine,
                   NativeFunction::ReadNumber] {
        globals.borrow_mut().define(
            native.name().to_string(),
            Value::Function(Callable::Native(native)),
        );
    }
    // ...
}
```

#### Testing Concern: Stdin in Unit Tests

Native functions that read from stdin are difficult to unit test without
redirecting stdin. Two strategies:

1. **Integration tests using subprocess:** Pipe crafted input to a child
   process running the interpreter and assert on stdout. This is the most
   robust approach.

2. **Dependency injection:** Add a `stdin` field to `Interpreter` (a
   `Box<dyn BufRead>`) defaulting to `std::io::stdin()`, swapped for a
   `Cursor<&[u8]>` in tests. This requires passing the reader through to
   `NativeFunction::call`, which changes the signature.

The simplest approach that preserves the existing architecture is (1): add
fixture `.lox` files that exercise `readLine()` / `readNumber()` and test them
via integration tests that pipe input with `std::process::Command`.

```rust
#[test]
fn test_read_line() {
    use std::process::{Command, Stdio};
    use std::io::Write;

    let mut child = Command::new("cargo")
        .args(["run", "--quiet", "--", "fixtures/read_line.lox"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("cargo run should start");

    child.stdin.take()
        .expect("stdin should be piped")
        .write_all(b"Alice\n")
        .expect("write should succeed");

    let output = child.wait_with_output()
        .expect("child should exit");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Hello, Alice!\n"
    );
}
```

### Bytecode VM

The VM's `NativeFn` type and global registration need parallel changes. In
`src/vm/vm.rs`:

```rust
#[derive(Debug, Clone, Copy)]
enum NativeFn {
    Clock,
    ReadLine,
    ReadNumber,
}

// In Vm::new() or Vm::define_globals():
vm.globals.insert("readLine".into(),   VmValue::NativeFunction(NativeFn::ReadLine));
vm.globals.insert("readNumber".into(), VmValue::NativeFunction(NativeFn::ReadNumber));

// In call_native():
NativeFn::ReadLine   => vm_read_line(),
NativeFn::ReadNumber => vm_read_number(),
```

The `vm_read_line()` and `vm_read_number()` implementations are identical in
logic to the tree-walk versions. Consider extracting to a shared
`src/stdlib.rs` module to avoid duplication.

### LLVM Codegen Backend

The LLVM backend requires:

1. **C runtime functions** in `runtime/lox_runtime.c`:

```c
// Reads one line from stdin, strips newline.
// Returns heap-allocated string, or NULL on EOF.
// Caller owns the returned string.
char *lox_read_line(void);

// Reads one line, parses as Lox number.
// Returns a LoxValue with TAG_NUMBER, or TAG_NIL on parse failure or EOF.
LoxValue lox_read_number(void);
```

2. **LLVM IR declarations** in `src/codegen/runtime.rs`:

```rust
// Declaration of external C functions
let read_line_fn = module.add_function(
    "lox_read_line",
    context.ptr_type(AddressSpace::default()).fn_type(&[], false),
    None,
);
let read_number_fn = module.add_function(
    "lox_read_number",
    lox_value_type.fn_type(&[], false),
    None,
);
```

3. **Codegen call sites** in `src/codegen/compiler.rs` for when
   `readLine` or `readNumber` identifiers are resolved to native functions:

```rust
// When compiling a Call expression where the callee is readLine:
let result_ptr = builder
    .build_call(read_line_fn, &[], "read_line_result")
    .context("building readLine call")?
    .try_as_basic_value()
    .unwrap_basic();
// Convert char* to LoxValue string (TAG_STRING, payload = ptr)
```

The LLVM backend is more involved because strings require heap allocation
and the tagged-union representation. The C runtime can handle this:
`lox_read_line` returns a `char*` that the codegen wraps into a `LoxValue`,
or more simply, `lox_read_line_value` returns a `LoxValue` directly
(TAG_STRING on success, TAG_NIL on EOF), consistent with how other string
operations are handled.

### Shared Implementation Module

To avoid triplicating the line-reading and number-parsing logic, create a
shared module `src/stdlib/mod.rs` (or `src/stdlib.rs`) containing:

```rust
/// Read one line from a BufRead, strip newline, return None on EOF.
pub fn read_line_from<R: std::io::BufRead>(reader: &mut R) -> Option<String> {
    let mut buf = String::new();
    match reader.read_line(&mut buf) {
        Ok(0) | Err(_) => None,
        Ok(_) => {
            if buf.ends_with('\n') {
                buf.pop();
                if buf.ends_with('\r') { buf.pop(); }
            }
            Some(buf)
        }
    }
}

/// Parse a string as a Lox NUMBER literal (no sign, no scientific notation).
pub fn parse_lox_number(s: &str) -> Option<f64> {
    // ... (as described above)
}
```

Both the tree-walk interpreter and VM call these functions, passing
`&mut std::io::stdin().lock()` in production and `&mut Cursor::new(b"...")` in
tests.

### Error Handling

`readLine()` and `readNumber()` do **not** raise Lox runtime errors on EOF or
parse failure — they return `nil`. This is a deliberate design decision: I/O
errors and EOF are normal conditions in stream processing, not exceptional ones.

The only case where a runtime error is appropriate is if the stdin file
descriptor is closed or broken in a way that the OS reports as an error (not
EOF). In that case, the implementation should treat it as EOF (`nil`) to keep
error handling simple for Lox programs. The OS-level I/O error is lost, which
is acceptable for a teaching language.

---

## Future Extensions

These are out of scope for this proposal but are natural follow-ons:

### `toNumber(string)`

Convert a string to a number, returning `nil` on failure. This separates
parsing from I/O and enables patterns like:

```lox
var line = readLine();
var n    = toNumber(line);
```

This function would use the same `parse_lox_number` logic as `readNumber()`.

### `toString(value)`

Convert any Lox value to its string representation (the same format as
`print` uses). Enables string formatting without `print`:

```lox
var msg = "Value is: " + toString(42);
```

### `readLine(prompt)`

A one-argument variant that prints a prompt before reading, matching Python's
`input()`. Requires either variadic argument support or a separate overload.
Deferred because Lox has fixed-arity functions and adding special cases for
optional arguments is a larger change.

### File I/O

An `open(path, mode)` function returning a file handle class. Out of scope for
this proposal, which focuses on the minimal stdin capability. File I/O would
benefit from Lox gaining a proper standard library namespace.

### `readAll()`

Read all of stdin into a single string. Useful for programs that process
entire documents:

```lox
var content = readAll();
```

Straightforward to implement once `readLine()` exists.

---

## Summary

This proposal adds two native functions to Lox:

| Function        | Returns           | On EOF | On bad input  |
|-----------------|-------------------|--------|---------------|
| `readLine()`    | `string`          | `nil`  | N/A           |
| `readNumber()`  | `number`          | `nil`  | `nil`         |

No grammar changes are required. Both functions have arity 0, integrate cleanly
with the existing native function infrastructure, and follow the `nil`-as-absent
convention already established by Lox. The design is intentionally minimal —
it adds exactly the capability needed for interactive and pipeline programs
without introducing new keywords, special forms, or a module system.

The implementation across all three backends (tree-walk interpreter, bytecode
VM, LLVM codegen) is straightforward, with the recommended approach of
extracting shared parsing logic into a `src/stdlib` module to avoid
duplication.
