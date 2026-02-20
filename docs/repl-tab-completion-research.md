# REPL Tab Completion Research

## Context

The REPL in `src/repl.rs` currently reads lines with
`stdin.lock().read_line(&mut line)` using only `std::io`. It recognises four
backslash commands, each of which already has a one-character short form:

| Command    | Short form |
|------------|------------|
| `\help`    | `\h`       |
| `\quit`    | `\q`       |
| `\clear`   | `\c`       |
| `\version` | `\v`       |

The goal is to understand what would be needed to support `\q<Tab>` expanding to
`\quit` (and similarly for the other commands).

---

## 1. Rust Crates for Readline-style Tab Completion

### 1.1 rustyline (v17.0.2)

**What it is.** A pure-Rust readline implementation modelled closely on Antirez's
Linenoise. It supports Unix and Windows. The library is well-established; it has
been the default choice for Rust REPLs since 2016.

**Completion API.** Completion is expressed through a small trait hierarchy:

```rust
// A single candidate returned by the completer.
pub trait Candidate {
    fn display(&self) -> &str;
    fn replacement(&self) -> &str;
}

// The built-in concrete candidate type.
pub struct Pair {
    pub display: String,
    pub replacement: String,
}

// The completer itself.
pub trait Completer {
    type Candidate: Candidate;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        ctx: &Context<'_>,
    ) -> Result<(usize, Vec<Self::Candidate>)>;

    // Optional: apply the elected candidate to the line buffer.
    fn update(
        &self,
        line: &mut LineBuffer,
        start: usize,
        elected: &str,
        cl: &mut Changeset,
    );
}
```

`complete()` returns the byte offset at which the completion word starts and a
vector of candidates. The default `update()` splices the elected string into the
line buffer for you.

**Helper trait.** rustyline aggregates all editor plug-ins through a `Helper`
supertrait:

```rust
pub trait Helper: Completer + Hinter + Highlighter + Validator {}
```

A blanket `impl Helper for ()` exists, so you can pass `()` for everything you
do not need. In practice you define a struct and implement only `Completer`
yourself; for the other three traits you delegate to `()`.

**Tab-completion modes** (selected via `Config::completion_type`):

- `CompletionType::Circular` (default) — pressing Tab cycles through candidates
  inline, replacing the typed text with each successive match.
- `CompletionType::List` — expands to the longest common prefix; a second Tab
  lists all candidates below the line (Bash/readline behaviour).
- `CompletionType::Fuzzy` — interactive fzf-style selection; Unix-only, requires
  the `with-fuzzy` feature flag.

For a single-match completion (e.g. `\q` → `\quit`), all three modes behave
identically: the text is expanded immediately on the first Tab press.

**Maturity and maintenance.** rustyline is at v17 (2025), actively maintained,
and widely used (fish-shell, evcxr, nu-protocol, many others). Docs.rs notes
that v17.0.2 failed to build on that platform; v17.0.1 is the last known-good
build, but this is a docs.rs infrastructure issue, not a crate defect.

**Dependencies.** 20+ transitive dependencies, including `libc`, `nix` (Unix),
`windows-sys` (Windows), `unicode-width`, `memchr`. Optional features add
`regex`, `rusqlite`, and clipboard support. Typical incremental compile time is
around 39 s on a cold cache; subsequent rebuilds of user code are fast.

**Binary size impact.** Adds roughly 300–600 KB to a stripped release binary,
depending on which optional features are enabled. Disabling the default features
and enabling only what is needed keeps this at the lower end.

---

### 1.2 reedline (v0.45.0)

**What it is.** The line editor embedded in NuShell. It is more feature-rich than
rustyline and designed for highly customisable, modern CLIs. It uses `crossterm`
for terminal I/O.

**Completion API.**

```rust
pub trait Completer: Send {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<Suggestion>;

    // Provided methods with sensible defaults:
    fn complete_with_base_ranges(...) -> (Vec<Suggestion>, Vec<Range<usize>>);
    fn partial_complete(...) -> Vec<Suggestion>;
    fn total_completions(...) -> usize;
}
```

`Suggestion` carries a `value`, an optional `description`, a `span`
(replacement range), and an `append_whitespace` flag.

A `DefaultCompleter` backed by an internal trie is available:

```rust
let mut completions = DefaultCompleter::default();
completions.insert(vec!["\\help", "\\quit", "\\clear", "\\version"]
    .iter()
    .map(|s| s.to_string())
    .collect());
```

Tab completion is wired up with a menu (e.g. `ColumnarMenu`) and a keybinding:

```rust
let mut line_editor = Reedline::create()
    .with_completer(Box::new(completions))
    .with_menu(ReedlineMenu::EngineCompleter(Box::new(
        ColumnarMenu::default().with_name("completion_menu"),
    )))
    .with_edit_mode(Box::new(Emacs::new(keybindings)));
```

The keybinding must explicitly map `Tab` to `ReedlineEvent::Menu("completion_menu")`.

**Maturity and maintenance.** Pre-1.0 (v0.45.0) but actively maintained by the
NuShell team. The API has changed substantially between minor versions; expect
breaking changes before a 1.0 release. Well-tested in production inside NuShell.

**Dependencies.** Approximately 15 direct dependencies including `crossterm`,
`serde`, `unicode-segmentation`, `unicode-width`, and optionally `arboard`
(clipboard) and `rusqlite` (history). The transitive closure is larger than
rustyline's.

**API complexity.** Noticeably higher. Three concepts — completer, menu, and
keybinding — must be coordinated. Getting the keybinding wiring correct requires
reading documentation carefully. For a simple REPL this is over-engineered.

**Binary size impact.** Larger than rustyline due to `crossterm` and additional
abstractions; expect 500 KB – 1 MB added to a stripped release binary.

---

### 1.3 liner (v0.4.4)

A minimalist readline alternative using `termion` for terminal I/O. It provides
a `Completer` trait with a `BasicCompleter` that matches against a fixed word
list. However:

- Only 35% of the API is documented.
- Latest release is v0.4.4, which appears to have had no activity for several
  years.
- Uses `termion`, which does not support Windows.
- The `redox_liner` fork exists for Redox OS but is equally inactive on Linux.

**Verdict.** Not recommended — insufficient maintenance, poor documentation, and
Unix-only.

---

### 1.4 DIY via crossterm (v0.29.0)

`crossterm` provides raw terminal mode, byte-level key-event reading, and cursor
manipulation. A minimal tab-completion loop would:

1. Enable raw mode (`terminal::enable_raw_mode()`).
2. Read `Event::Key` events one at a time.
3. On `KeyCode::Tab`, scan the input buffer for a `\…` prefix and expand it.
4. On printable characters, append to the buffer and redraw the line.
5. On `Enter`, disable raw mode and return the assembled string.

A realistic implementation covering backspace, Ctrl-C, Ctrl-D, left/right arrow
keys, and tab completion would be approximately 250–400 lines of Rust. That is
not enormous, but it is a bespoke line editor with all the maintenance burden
that implies. Edge cases (multi-byte Unicode, pasted text, terminal resize,
Windows console API differences) are non-trivial.

**Verdict.** Viable but not advisable when a polished library exists.

---

### Comparison Summary

| Criterion              | rustyline     | reedline       | liner         | DIY crossterm  |
|------------------------|---------------|----------------|---------------|----------------|
| Version (2025)         | 17.0.2        | 0.45.0         | 0.4.4         | N/A            |
| Stability              | Stable (v17+) | Pre-1.0        | Abandoned     | N/A            |
| Active maintenance     | Yes           | Yes (NuShell)  | No            | Self           |
| Windows support        | Yes           | Yes            | No            | Yes            |
| Completion API         | Simple trait  | Trait + menu   | Trait         | Manual         |
| API complexity         | Low-medium    | Medium-high    | Low           | High           |
| Dep count (approx)     | ~20           | ~20+           | ~3            | ~8             |
| Binary size delta      | ~300-600 KB   | ~500 KB-1 MB   | ~150 KB       | ~200 KB        |
| Tab behaviour options  | Circular/List | Menu-based     | Circular      | Custom         |
| History support        | Built-in      | Built-in       | Built-in      | Manual         |
| Unicode               | Full           | Full           | Partial       | Full           |

---

## 2. Integration Sketch for rustyline

rustyline is the recommended crate (see §4). Here is how `src/repl.rs` would
change.

### 2.1 `Cargo.toml`

```toml
rustyline = { version = "17", default-features = false }
```

Disabling default features drops the SQLite history backend and clipboard
support, neither of which this REPL needs.

### 2.2 Custom completer

```rust
use rustyline::completion::{Completer, Pair};
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Helper};

const COMMANDS: &[(&str, &str)] = &[
    ("\\help",    "\\help — show this help message"),
    ("\\quit",    "\\quit — exit the REPL"),
    ("\\clear",   "\\clear — clear the terminal screen"),
    ("\\version", "\\version — show the interpreter version"),
];

struct ReplHelper;

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        // Only complete when the cursor is at the end of a backslash prefix.
        let prefix = &line[..pos];
        if !prefix.starts_with('\\') || prefix.contains(char::is_whitespace) {
            return Ok((pos, vec![]));
        }
        let candidates = COMMANDS
            .iter()
            .filter(|(cmd, _)| cmd.starts_with(prefix))
            .map(|(cmd, desc)| Pair {
                replacement: cmd.to_string(),
                display:     format!("{cmd}  ({desc})"),
            })
            .collect();
        Ok((0, candidates))
    }
}

// The remaining traits are no-ops; the blanket impl handles `Helper`.
impl Hinter    for ReplHelper { type Hint = String; }
impl Highlighter for ReplHelper {}
impl Validator  for ReplHelper {}
impl Helper     for ReplHelper {}
```

### 2.3 Updated `run_repl` function

```rust
use rustyline::config::CompletionType;
use rustyline::{Config, Editor};

pub fn run_repl() {
    let config = Config::builder()
        .completion_type(CompletionType::List) // Bash-style: show list on ambiguity
        .build();

    let mut rl: Editor<ReplHelper, rustyline::history::DefaultHistory> =
        Editor::with_config(config).expect("rustyline init cannot fail with valid config");
    rl.set_helper(Some(ReplHelper));

    let mut interpreter = Interpreter::new();

    loop {
        match rl.readline("> ") {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // Persist non-empty lines in history (skip backslash commands
                // if you prefer a clean history).
                let _ = rl.add_history_entry(trimmed);

                if trimmed.starts_with('\\') {
                    let mut parts = trimmed.split_whitespace();
                    let cmd  = parts.next().unwrap_or("");
                    let args: Vec<&str> = parts.collect();
                    if handle_command(cmd, &args) {
                        break;
                    }
                    continue;
                }

                // … rest of expression evaluation unchanged …
            }
            Err(rustyline::error::ReadlineError::Interrupted) => break, // Ctrl-C
            Err(rustyline::error::ReadlineError::Eof)         => break, // Ctrl-D
            Err(e) => {
                eprintln!("read error: {e}");
                break;
            }
        }
    }
}
```

Key points:

- `rl.readline("> ")` replaces the `print!("> ")` + `read_line()` pair. It
  handles raw-mode entry and exit internally.
- `CompletionType::List` matches the Bash/readline convention: Tab expands to the
  longest common prefix; a second Tab lists all matches. For the current four
  commands this is indistinguishable from `Circular` (each command has a unique
  prefix after two characters, e.g. `\h` unambiguously completes to `\help`).
- The existing `handle_command` function requires no changes.
- `is_bare_expression` requires no changes.
- The `Completer::complete` implementation returns `(0, candidates)` — the
  replacement start is position 0 because the entire `\quit`-style token is
  replaced, not just the suffix.
- History is available for free. Whether to include backslash commands in
  history is a style choice; excluding them keeps the arrow-key history focused
  on Lox expressions.

### 2.4 Short-form completion

The same `complete()` function already handles short forms. `\q` matches only
`\quit` (prefix `\q` appears only in `\quit`), so Tab immediately expands it.
`\h` is similarly unambiguous. The `\c` prefix matches only `\clear` because
`\clear` and no other command begins with `\c`. All four short forms resolve
without ambiguity.

---

## 3. DIY Raw Terminal Approach

A from-scratch implementation using `crossterm` is feasible but carries
significant hidden cost.

**What must be implemented manually:**

- Entering/leaving raw mode around every readline call.
- Collecting `Event::Key` events in a loop.
- Maintaining a `Vec<char>` (or `String`) buffer and a cursor byte-offset.
- Handling `Backspace`, `Delete`, `Left`, `Right`, `Home`, `End`.
- Handling `Ctrl-C` (send interrupt), `Ctrl-D` (EOF on empty buffer),
  `Ctrl-U` (kill line), `Ctrl-W` (kill word) — standard REPL expectations.
- Redrawing the current line after each mutation (overwrite from cursor to end,
  then reposition cursor). Multi-byte Unicode characters complicate column math.
- Tab: scan buffer for a `\…` prefix, find matches, expand if unique or cycle/
  list if ambiguous.
- Handling pasted text (a sequence of characters arriving in rapid succession
  that must not be interpreted as Tab events).
- On Windows: the console API uses `ENABLE_VIRTUAL_TERMINAL_INPUT` and has
  different event semantics from Unix PTYs.

Estimated scope: 300–500 lines of careful Rust, plus ongoing maintenance as
terminal edge cases surface. The result would be a bespoke line editor that
reimplements ~30% of what rustyline already provides.

**When DIY is worth it:**

- Zero additional dependencies is a hard requirement.
- The REPL will never need history, multi-line input, or Emacs/Vi keybindings.
- The target platforms are strictly controlled (e.g. Linux-only, no Windows).

None of those conditions apply to vibe-lox, so DIY is not advisable here.

---

## 4. When Tab Completion Is Still Useful Given Short Forms

With only four commands, each having a distinct one-character short form, a
user who knows the commands will never need Tab completion. The value arises in
three scenarios:

1. **Discoverability for new users.** A user who types `\` and presses Tab
   immediately sees all available commands listed. This is the most compelling
   use case even today.

2. **Future commands without natural short forms.** As the REPL grows, not every
   command will have an obvious or memorable single-character abbreviation. For
   example, hypothetical commands like `\locals`, `\globals`, `\trace`, `\env`,
   or `\reset` would be awkward to condense. Tab completion scales to any number
   of commands without requiring the user to memorise a table.

3. **Avoiding typos in longer commands.** `\version` is long enough that some
   users will mistype it (`\versoin`). Tab completion eliminates the class of
   errors entirely for commands typed from a partial prefix.

4. **History integration is a corollary benefit.** Adopting rustyline for
   completion also gives arrow-key history, Ctrl-R reverse search, and
   persistent history files — features whose value is entirely independent of
   the number of backslash commands.

---

## 5. Recommendation

**Use rustyline.**

### Rationale

- **Simplest API for the task.** Implementing `Completer` for a fixed word list
  requires fewer than 25 lines of Rust. The `Helper` supertrait scaffolding is
  boilerplate but copy-paste trivial.
- **Mature and stable.** At v17, rustyline has been used in production Rust
  tooling for nearly a decade. Its API is stable; the upgrade path from v14 to
  v17 involved only minor breaking changes.
- **Cross-platform.** Works on Linux, macOS, and Windows without conditional
  compilation in user code.
- **History for free.** Arrow-key history and Ctrl-R search are built in at no
  additional API cost, and they are expected features in any interactive REPL.
- **Manageable dependency cost.** ~20 transitive crates and ~300–600 KB binary
  delta is acceptable for an interactive tool. If binary size becomes a concern,
  `default-features = false` trims the dependency surface significantly.

### Why not reedline?

reedline's additional power (menus, syntax highlighting pipeline, NuShell
integration) is not needed here. Its pre-1.0 status means the API may break
across minor versions, and the three-way coordination of completer + menu +
keybinding is unnecessary complexity for four static commands.

### Why not DIY crossterm?

A hand-rolled line editor would have to reproduce a large fraction of rustyline's
existing work, accumulate its own bugs, and be maintained indefinitely. The
one-time dependency cost of rustyline is a better trade-off.

### Suggested implementation order

1. `cargo add rustyline --no-default-features` — add the dependency.
2. Add the `ReplHelper` struct implementing `Completer` as sketched in §2.2.
3. Replace the `run_repl` read loop with the rustyline-based version from §2.3.
4. Verify that the four existing commands complete correctly, that Ctrl-C and
   Ctrl-D exit cleanly, and that arrow-key history works.
5. Run `cargo clippy -- -D warnings` and `cargo fmt --check`.
6. Update `CLAUDE.md` build commands if the REPL startup experience changes
   visibly.
