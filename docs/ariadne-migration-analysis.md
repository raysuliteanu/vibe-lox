# miette -> ariadne Migration Analysis

## Fundamental Architectural Difference

The key thing to understand is that **miette and ariadne solve the problem at
different layers**:

- **miette** is a *diagnostic protocol* + derive macro + renderer. You define
  error types declaratively with `#[derive(Diagnostic)]`, annotate fields with
  `#[label]`, `#[source_code]`, `#[diagnostic(code(...))]`, and the derive
  macro + report handler does the rest. Errors carry their own display metadata.

- **ariadne** is *only a renderer*. It provides a builder API
  (`Report::build(...).with_label(...).finish().eprint(...)`) that you call
  imperatively at rendering time. Your error types don't derive anything from
  ariadne -- you construct reports manually from your error data.

This means ariadne is not a drop-in replacement; it requires a different
integration pattern.

## Current miette Surface Area

The miette usage is well-contained. The affected locations are:

| Location | What it does |
|---|---|
| `Cargo.toml:10` | `miette = { version = "7.6.0", features = ["fancy"] }` dependency |
| `src/error.rs:4` | `use miette::{Diagnostic, SourceSpan}` |
| `src/error.rs:11` | `#[derive(Error, Debug, Diagnostic)]` on `CompileError` |
| `src/error.rs:14,24,34` | `#[diagnostic(code(lox::...))]` attributes |
| `src/error.rs:17,27,37` | `#[label("here")]` on span fields |
| `src/error.rs:19,29,39` | `#[source_code]` on `NamedSource<String>` fields |
| `src/error.rs:45-67` | Constructors using `SourceSpan::new()` and `NamedSource::new()` |
| `src/error.rs:69-90` | `with_source_code()` method using `NamedSource::new()` |
| `src/error.rs:243-246` | Test: `&dyn Diagnostic` cast, `.code()` assertion |
| `src/scanner/token.rs:112-116` | `From<Span> for miette::SourceSpan` impl |
| `src/main.rs:135` | `miette::Report::new(error_with_src)` for rendering |
| `src/repl.rs:46,57,68` | `miette::Report::new(error_with_src)` for rendering |

`RuntimeError` does **not** use miette at all, so it is unaffected.

## What the Migration Would Require

### 1. Dependency swap in `Cargo.toml`

Remove `miette` (and its transitive deps: `miette-derive`, `owo-colors`,
`supports-color`, etc.), add `ariadne`:

```toml
# Remove:
miette = { version = "7.6.0", features = ["fancy"] }
# Add:
ariadne = "0.6"
```

### 2. Simplify `CompileError` in `src/error.rs`

Strip all miette-specific derives and annotations. The enum becomes a plain
`thiserror` error with its own span data (no `NamedSource`, no `SourceSpan`):

```rust
use crate::scanner::token::Span;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CompileError {
    #[error("scan error: {message}")]
    Scan {
        message: String,
        span: Span,         // your own type, not miette's
        code: &'static str, // e.g. "lox::scan"
    },
    // ... same for Parse, Resolve
}
```

Key changes:

- Remove `#[derive(Diagnostic)]`
- Remove `#[diagnostic(code(...))]`, `#[label("here")]`, `#[source_code]`
  attributes
- Remove `miette::SourceSpan` field -- use the project's own `Span` type
  directly
- Remove `miette::NamedSource<String>` field entirely -- ariadne supplies
  source at render time, not on the error
- Remove the `with_source_code()` method entirely
- Remove the `#![allow(unused_assignments)]` at the top (that was for
  miette-derive codegen)
- Store the diagnostic code as a plain string if you still want it

The constructors simplify to:

```rust
pub fn scan(message: impl Into<String>, offset: usize, len: usize) -> Self {
    Self::Scan {
        message: message.into(),
        span: Span::new(offset, len),
        code: "lox::scan",
    }
}
```

### 3. Remove `From<Span> for miette::SourceSpan` in `src/scanner/token.rs:112-116`

This conversion impl only exists to bridge your `Span` to miette's
`SourceSpan`. Delete it.

### 4. Create a rendering function using ariadne's builder API

Replace the `miette::Report::new()` calls with ariadne report building. Write
a function like:

```rust
use ariadne::{Color, Label, Report, ReportKind, Source};

fn report_compile_error(error: &CompileError, filename: &str, source: &str) {
    let (message, span, code) = match error {
        CompileError::Scan { message, span, code } => (message, span, code),
        CompileError::Parse { message, span, code } => (message, span, code),
        CompileError::Resolve { message, span, code } => (message, span, code),
    };

    Report::build(ReportKind::Error, (filename, span.offset..span.offset + span.len))
        .with_code(code)
        .with_message(message)
        .with_label(
            Label::new((filename, span.offset..span.offset + span.len))
                .with_message("here")
                .with_color(Color::Red),
        )
        .finish()
        .eprint((filename, Source::from(source)))
        .expect("failed to write diagnostic");
}
```

Note that ariadne uses `Range<usize>` for spans (not offset+length), so you'd
convert `Span { offset, len }` to `offset..offset+len`.

### 5. Update all render sites

**`src/main.rs:127-138`** (`report_compile_errors`): Replace the loop body --
instead of `error.with_source_code(...) + miette::Report::new(...)`, call the
new ariadne rendering function.

**`src/repl.rs:44-47, 55-58, 65-69`**: Same replacement at three locations --
call the ariadne rendering function instead of
`error.with_source_code("<repl>", &source) + miette::Report::new(...)`.

### 6. Update the test in `src/error.rs:243-247`

The test `compile_error_implements_diagnostic` casts to `&dyn Diagnostic` and
checks `.code()`. Since `Diagnostic` is a miette trait, this test must be
rewritten or removed. If you still store a code string on the error, you can
test that field directly instead.

### 7. Update integration/parser error tests

Check `tests/parser_error_tests.rs` -- it tests errors via `.to_string()`,
which goes through `thiserror`'s `#[error(...)]` formatting, not miette
rendering. These tests should still pass unchanged since the
`#[error("parse error: {message}")]` display format stays the same.

## Trade-offs Summary

| Aspect | miette (current) | ariadne |
|---|---|---|
| **Integration style** | Declarative: derive macros on error types | Imperative: builder API at render sites |
| **Error type coupling** | Errors carry display metadata (`SourceSpan`, `NamedSource`, `#[label]`) | Errors are plain data; rendering is separate |
| **Code volume** | Concise: ~4 derive attributes per variant | More verbose: ~10 lines of builder calls per error rendering |
| **Dependency weight** | ~10 transitive deps (owo-colors, supports-color, textwrap, terminal_size, etc.) | ~1 dep (yansi for colors) |
| **Multi-file support** | Via `NamedSource` | First-class via `Cache` trait + file ID in spans |
| **Rendering flexibility** | Can swap report handlers (graphical, narratable, JSON) at runtime | One built-in graphical renderer, but very configurable (CharSet, Config, colors) |
| **Ecosystem** | Works with `anyhow`/`eyre` error chains; errors are `std::error::Error + Diagnostic` | Standalone; no trait integration with error chain crates |
| **Separation of concerns** | Error definition and display hints are mixed | Clean separation -- errors know nothing about rendering |

## Effort Estimate

This is a **small migration** given the contained surface area:

- **~5 files** need changes
- **~20 lines** of miette-specific code in `error.rs` to restructure
- **~4 render sites** (`main.rs` x1, `repl.rs` x3) to convert to builder API
- **1 trait impl** to delete (`From<Span> for SourceSpan`)
- **1 test** to rewrite
- Probably **1-2 hours** of focused work including testing

The main thing you'd gain is a cleaner separation between your error types and
their rendering. The main thing you'd lose is the declarative ergonomics of
`#[derive(Diagnostic)]` and the ability to cast errors to `&dyn Diagnostic` for
generic error handling. For this project's relatively simple diagnostic needs
(single label, single source file, three error codes), either crate works well.
