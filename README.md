# Vibe Coding a Lox Compiler

I have done a few implementations of a Lox interpreter, originally for the
CodeCrafters challenge and then iterating as I learned Rust more. But I only
ever got as far as closure support, not getting to adding classes and the rest
of the grammar.

Then I decided to see how Claude would do. This project is (so far) 100% created
by Claude using Opus 4.5/4.6 (as of creation of this README). Mostly I used the
Claude CLI but I also used Opencode as my tool as well, still with Opus via
Opencode Zen.

## Usage

### Running a Lox file

```bash
cargo run -- hello.lox            # Tree-walk interpreter (default)
cargo run -- program.blox         # Bytecode VM (autodetected from .blox magic header)
```

Files with a `#!/usr/bin/env -S cargo run --` shebang can be run directly:

```bash
chmod +x hello.lox
./hello.lox
```

### Compiling

```bash
cargo run -- --compile-bytecode hello.lox    # Produce hello.blox (portable bytecode)
cargo run -- --compile-llvm hello.lox        # Produce hello.ll (LLVM IR)
cargo run -- --compile hello.lox             # Produce ./hello (native ELF executable)
cargo run -- --compile -o out hello.lox      # Custom output path
```

### Diagnostics and debugging

```bash
cargo run -- --dump-tokens hello.lox         # Print token stream and stop
cargo run -- --dump-ast hello.lox            # Print AST (S-expressions) and stop
cargo run -- --disassemble hello.lox         # Disassemble bytecode and print
LOX_BACKTRACE=1 cargo run -- hello.lox       # Include call-stack backtrace on errors
```

### REPL

Start the REPL with no arguments:

```bash
cargo run
```

Bare expressions are auto-printed, so you can type `1 + 2` and see `3` without
wrapping it in `print`. The REPL preserves the environment across lines, so
variables and functions you define in one line are available in the next.

The REPL uses `rustyline` for line editing, providing:

- **Tab completion** for backslash commands — `\<Tab>` lists all commands;
  a partial prefix (e.g. `\q<Tab>`) expands unambiguously to the long form
- **Arrow-key history** and **Ctrl-R** reverse search across Lox expressions

The REPL supports the following backslash commands (backslash instead of slash
because `/` is the Lox division operator):

| Short | Long       | Description                   |
| ----- | ---------- | ----------------------------- |
| `\h`  | `\help`    | Show available REPL commands  |
| `\q`  | `\quit`    | Exit the REPL                 |
| `\c`  | `\clear`   | Clear the terminal screen     |
| `\v`  | `\version` | Print the interpreter version |

## TODOs

- ~~enhance REPL to have "slash" commands using '\' since '/' is a reserved character~~ (done — `\help`, `\quit`, `\clear`, `\version`)
- ~~skip `#!` shebang line in the scanner so `.lox` files can be made directly executable (e.g. `#!/usr/bin/env -S cargo run --`)~~
- could we generate JVM bytecode?
- explore performance optimizations
- document the codebase comprehensively
- create some example Lox programs in examples/ to run with `cargo run --example`
- enhance the Lox grammar to support input; currently a Lox program can only
  write to `stdout` via `print`; there is no `read` or some such capability in the
  Lox grammar; an extension would be interesting
- add string concatenation to the language
- add some other string operations like length and indexing into a string
