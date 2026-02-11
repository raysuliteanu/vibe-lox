# Vibe Coding a Lox Compiler

I have done a few implementations of a Lox interpreter, originally for the
CodeCrafters challenge and then iterating as I learned Rust more. But I only
ever got as far as closure support, not getting to adding classes and the rest
of the grammar.

Then I decided to see how Claude would do. This project is (so far) 100% created
by Claude using Opus 4.5/4.6 (as of creation of this README). Mostly I used the
Claude CLI but I also used Opencode as my tool as well, still with Opus via
Opencode Zen.

One thing I want to do with this is go beyond the bytecode VM described in the
`Crafting Interpreters` book and have Claude try and create LLVM IR so I can
actually compile this to native code.
