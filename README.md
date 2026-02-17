# Vibe Coding a Lox Compiler

I have done a few implementations of a Lox interpreter, originally for the
CodeCrafters challenge and then iterating as I learned Rust more. But I only
ever got as far as closure support, not getting to adding classes and the rest
of the grammar.

Then I decided to see how Claude would do. This project is (so far) 100% created
by Claude using Opus 4.5/4.6 (as of creation of this README). Mostly I used the
Claude CLI but I also used Opencode as my tool as well, still with Opus via
Opencode Zen.

## TODOs

* enhance REPL to have "slash" commands using '\' since '/' is a reserved character; some possible commands are
    * `help` - show available commands
    * `quit` - exit the REPL
    * `clear` - clear the screen
    * `version` - show the current version of the compiler
* skip `#!` shebang line in the scanner so `.lox` files can be made directly executable (e.g. `#!/usr/bin/env -S cargo run --`)
* could we generate JVM bytecode?
* explore performance optimizations
* document the codebase comprehensively
* create some example Lox programs in examples/ to run with `cargo run --example`
