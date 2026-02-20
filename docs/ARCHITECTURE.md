# vibe-lox Architecture

## Overview

**vibe-lox** is a Lox language interpreter and compiler implemented in Rust.
It can directly interpret Lox source code, compile and execute bytecode, or
compile to LLVM IR for execution via `lli`.

1. **Tree-walk Interpreter** - Direct AST interpretation of Lox source code (no
   compilation)
2. **Stack-based VM** - execute bytecode Lox program, compiled with
   --compile-bytecode
3. **Bytecode Compiler** (`--compile-bytecode`) - Compile to custom Lox bytecode
   for use with the VM
4. **LLVM IR Compiler** (`--compile-llvm`) - Compile to LLVM IR, run via `lli`
   with the C runtime library
5. **Native Compiler** (`--compile`) - Compile to a native ELF executable via
   LLVM object emission and system linker

The architecture follows a classic compiler pipeline with clear separation between phases:

```plain
Source Code → Tokenization → Parsing → AST → [Resolution] → Execution
                                              ↓
                                         Interpreter
                                              ↓
                                       Bytecode Compiler → VM
                                              ↓
                                       LLVM IR Codegen → lli
                                              ↓
                                       Native Compiler → ELF executable
```

---

## Core Architecture Principles

### 1. **Phase Separation**

Each compiler phase is isolated in its own module with clear interfaces. This allows:

- Independent testing of each phase
- Debug outputs at any phase boundary (`--dump-tokens`, `--dump-ast`)
- Multiple backend implementations sharing the same frontend

### 2. **Error Handling Strategy**

**Two-tier error handling:**

- **Domain Errors** (`LoxError`): Rich, user-facing errors using `thiserror` + `miette`
    - Carry source spans for precise error reporting
    - Implement `miette::Diagnostic` for fancy terminal output with source snippets
    - Variants: `ScanError`, `ParseError`, `ResolveError`, `RuntimeError`

- **Propagation Wrapper** (`anyhow::Result`): Used throughout for error propagation
    - All functions return `anyhow::Result<T>`
    - Use `.context("while doing X")` before every `?` operator
    - Provides error context chain for debugging

**Example:**

```rust
pub fn scan(source: &str) -> Result<Vec<Token>, Vec<LoxError>> {
    // Returns domain-specific errors
}

fn run_source(source: &str) -> Result<()> {
    let tokens = scanner::scan(source)
        .map_err(|e| report_lox_errors(&e))
        .context("scanning source")?;
    // Uses anyhow for propagation
}
```

### 3. **Shared Frontend, Multiple Backends**

The tokenizer and parser are shared across all execution backends:

- Tree-walk interpreter operates directly on the AST
- Bytecode VM compiles the AST to bytecode
- LLVM IR codegen compiles the AST to LLVM IR text files

This ensures all backends handle the same language semantics.

---

## Phase 1: Tokenization (Lexical Analysis)

**Location:** `src/scanner/`

### Purpose

Transform source code string into a stream of tokens with precise source location tracking.

### Key Modules

#### `src/scanner/mod.rs`

- **Public API:** `scan(source: &str) -> Result<Vec<Token>, Vec<LoxError>>`
- Entry point for tokenization
- Returns all tokens or all errors (error recovery enabled)

#### `src/scanner/token.rs`

- **`TokenKind` enum:** 50+ token variants
    - Literals: `Number(f64)`, `String(String)`, `True`, `False`, `Nil`
    - Operators: `Plus`, `Minus`, `Star`, `Slash`, `Bang`, `Equal`, etc.
    - Keywords: `And`, `Class`, `Fun`, `For`, `If`, `While`, `Return`, etc.
    - Delimiters: `LeftParen`, `RightParen`, `LeftBrace`, etc.
    - Special: `Eof`

- **`Token` struct:**

  ```rust
  pub struct Token {
      pub kind: TokenKind,
      pub lexeme: String,  // Owns the text for simplicity
      pub span: Span,
  }
  ```

- **`Span` struct:**

  ```rust
  pub struct Span {
      pub offset: usize,  // Byte offset in source
      pub len: usize,     // Length in bytes
  }
  ```

#### `src/scanner/lexer.rs`

- **Implementation:** Uses `winnow` parser combinator library
- **Key Functions:**
    - `scan_all()` - Main entry point, skips optional shebang then collects all tokens
    - `shebang()` - Consume `#!...` through end of first line (enables direct Unix execution)
    - `scan_token()` - Parse single token with error recovery
    - `whitespace_and_comments()` - Skip whitespace and `//` comments
    - `string_literal()` - Parse strings with escape sequences (`\n`, `\t`, `\\`, `\"`)
    - `number_literal()` - Parse integers and decimals
    - `identifier_or_keyword()` - Parse identifiers, match keywords
    - `two_char_token()` - Parse `==`, `!=`, `<=`, `>=`
    - `single_char_token()` - Parse single-char operators

### Design Decisions

1. **Tokens own lexemes:** Each `Token` owns its `String` lexeme rather than using string slices
    - Simplifies lifetime management in later phases
    - Allows tokens to outlive source string

2. **winnow for parsing:** Parser combinator library instead of hand-written scanner
    - Composable parsers
    - Built-in span tracking with `Located<&str>`
    - Clean error handling

3. **Error recovery:** Scanner collects all errors before returning
    - Allows reporting multiple syntax errors at once
    - Better user experience than stopping at first error

4. **Shebang skipping in the scanner, not the caller:** `#!...` is consumed by
   `opt(shebang)` at the top of `scan_all()` before `LocatingSlice` is read further
    - Span offsets remain correct relative to the original source string
    - Pre-stripping in the caller would shift all offsets, breaking error diagnostics

### Data Flow

```plain
Source String
    ↓
winnow parsers with Located<&str>
    ↓
Vec<Token> with accurate Spans
```

---

## Phase 2: Parsing (Syntax Analysis)

**Location:** `src/parser/` and `src/ast/`

### Purpose

Transform token stream into a typed Abstract Syntax Tree (AST) that represents program structure.

### Key Modules

#### `src/ast/mod.rs`

Defines all AST node types. All nodes:

- Derive `Debug`, `Clone`, `Serialize` for testing and JSON output
- Carry source `Span` for error reporting
- Use owned data (no lifetimes)

**Core AST Types:**

```rust
pub struct Program {
    pub declarations: Vec<Decl>,
}

pub enum Decl {
    Class(ClassDecl),
    Fun(FunDecl),
    Var(VarDecl),
    Statement(Stmt),
}

pub enum Stmt {
    Expression(ExprStmt),
    Print(PrintStmt),
    Return(ReturnStmt),
    Block(BlockStmt),
    If(IfStmt),
    While(WhileStmt),
}

pub enum Expr {
    Binary(BinaryExpr),      // a + b
    Unary(UnaryExpr),        // -a, !a
    Literal(LiteralExpr),    // 42, "hello", true, nil
    Grouping(GroupingExpr),  // (expr)
    Variable(VariableExpr),  // x
    Assign(AssignExpr),      // x = value
    Logical(LogicalExpr),    // a and b, a or b
    Call(CallExpr),          // f(a, b)
    Get(GetExpr),            // obj.property
    Set(SetExpr),            // obj.property = value
    This(ThisExpr),          // this
    Super(SuperExpr),        // super.method
}

pub enum LiteralValue {
    Number(f64),
    String(String),
    Bool(bool),
    Nil,
}
```

**Important Details:**

- **`ExprId`:** Each expression has a unique ID for resolver's locals map

  ```rust
  static NEXT_EXPR_ID: AtomicUsize = AtomicUsize::new(0);

  pub type ExprId = usize;

  impl Expr {
      pub fn id(&self) -> ExprId { /* ... */ }
  }
  ```

- **Spans everywhere:** Every AST node carries its source location

#### `src/ast/printer.rs`

- **`to_sexp(program) -> String`**: S-expression format for debugging
    - Example: `(binary (literal 1) + (literal 2))`

- **`to_json(program) -> String`**: JSON format via `serde_json`
    - Machine-readable, includes all node details and spans

#### `src/parser/mod.rs`

**Implementation:** Recursive descent parser following Lox grammar (see `Grammar.md`)

**Key Structure:**

```rust
struct Parser {
    tokens: Vec<Token>,
    current: usize,
    errors: Vec<LoxError>,
    next_expr_id: usize,
}

impl Parser {
    pub fn parse(mut self) -> Result<Program, Vec<LoxError>>

    // Grammar production methods (top-down)
    fn declaration(&mut self) -> Option<Decl>
    fn statement(&mut self) -> Option<Stmt>

    // Expression precedence chain (lowest to highest)
    fn assignment(&mut self) -> Option<Expr>
    fn or(&mut self) -> Option<Expr>
    fn and(&mut self) -> Option<Expr>
    fn equality(&mut self) -> Option<Expr>
    fn comparison(&mut self) -> Option<Expr>
    fn term(&mut self) -> Option<Expr>
    fn factor(&mut self) -> Option<Expr>
    fn unary(&mut self) -> Option<Expr>
    fn call(&mut self) -> Option<Expr>
    fn primary(&mut self) -> Option<Expr>
}
```

### Design Decisions

1. **Recursive descent:** Hand-written parser following grammar structure
    - Each grammar rule becomes a method
    - Natural expression precedence via method call chain
    - Easy to understand and debug

2. **Error recovery:** Panic-mode synchronization
    - On error, skip tokens until next statement boundary
    - Collect all errors, continue parsing
    - Returns all errors at end

3. **Desugar `for` to `while`:**

   ```lox
   for (var i = 0; i < 10; i = i + 1) body;

   // Becomes:
   { var i = 0; while (i < 10) { body; i = i + 1; } }
   ```

4. **Max 255 parameters/arguments:** Enforced during parsing
    - Matches bytecode limit (single byte for count)
    - Error reported with source span

### Data Flow

```plain
Vec<Token>
    ↓
Recursive descent parsing
    ↓
AST (Program with Decls/Stmts/Exprs)
```

---

## Phase 3A: Resolution (Static Analysis)

**Location:** `src/interpreter/resolver.rs`

### Purpose

Resolve variable references before interpretation to:

1. Detect semantic errors at compile time
2. Calculate environment depth for each variable access
3. Validate `return`, `this`, and `super` usage

### Key Data Structures

```rust
pub struct Resolver {
    scopes: Vec<HashMap<String, bool>>,  // Stack of scopes
    locals: HashMap<ExprId, usize>,       // Expr ID → depth
    current_function: FunctionType,       // Track context
    current_class: ClassType,             // Track context
    errors: Vec<LoxError>,                // Collected errors
}

enum FunctionType {
    None,
    Function,
    Method,
    Initializer,
}

enum ClassType {
    None,
    Class,
    Subclass,
}
```

### Algorithm

**Two-pass variable resolution:**

1. **Declare:** Add variable to current scope, mark as uninitialized

   ```rust
   fn declare(&mut self, name: &str) {
       if let Some(scope) = self.scopes.last_mut() {
           scope.insert(name.to_string(), false);  // false = not ready
       }
   }
   ```

2. **Define:** Mark variable as initialized

   ```rust
   fn define(&mut self, name: &str) {
       if let Some(scope) = self.scopes.last_mut() {
           scope.insert(name.to_string(), true);  // true = ready
       }
   }
   ```

3. **Resolve:** Calculate depth for variable access

   ```rust
   fn resolve_local(&mut self, id: ExprId, name: &str) {
       for (i, scope) in self.scopes.iter().rev().enumerate() {
           if scope.contains_key(name) {
               self.locals.insert(id, i);  // Store depth
               return;
           }
       }
       // Not found: assume global (not in locals map)
   }
   ```

### Semantic Checks

1. **Variable redeclaration:** Error if same name declared twice in same scope
2. **Reading in initializer:** Error for `var x = x;`
3. **Return outside function:** Error for top-level `return`
4. **Return value from initializer:** Error for `init() { return 42; }`
5. **`this` outside class:** Error for `fun f() { return this; }`
6. **`super` without superclass:** Error for `class Foo { m() { super.m(); } }`
7. **Self-inheritance:** Error for `class Foo < Foo {}`

### Output

`HashMap<ExprId, usize>` - Maps each variable/assignment expression to its scope depth

- Depth 0 = current scope
- Depth 1 = enclosing scope
- etc.
- Not in map = global variable

### Data Flow

```plain
AST
    ↓
Resolver (two-pass traversal)
    ↓
HashMap<ExprId, usize> (locals map)
    ↓
Passed to Interpreter
```

---

## Phase 3B: Tree-Walk Interpretation

**Location:** `src/interpreter/`

### Purpose

Execute Lox programs by walking the AST and directly evaluating nodes.

### Key Modules

#### `src/interpreter/mod.rs`

Main interpreter implementation.

```rust
pub struct Interpreter {
    globals: Rc<RefCell<Environment>>,
    environment: Rc<RefCell<Environment>>,
    locals: HashMap<ExprId, usize>,  // From resolver
    output: Vec<String>,              // For testing
    writer: Box<dyn Write>,           // stdout or capture
    call_stack: Vec<StackFrame>,      // For backtrace on runtime errors
    source: String,                   // Source code for line number calculation
}

impl Interpreter {
    pub fn interpret(
        &mut self,
        program: &Program,
        locals: HashMap<ExprId, usize>
    ) -> Result<(), LoxError>

    fn execute_decl(&mut self, decl: &Decl) -> Result<(), LoxError>
    fn execute_stmt(&mut self, stmt: &Stmt) -> Result<(), LoxError>
    fn evaluate_expr(&mut self, expr: &Expr) -> Result<Value, LoxError>
}
```

#### `src/interpreter/value.rs`

Runtime value representation.

```rust
pub enum Value {
    Number(f64),
    Str(String),
    Bool(bool),
    Nil,
    Function(Rc<Callable>),
    Class(Rc<RefCell<LoxClass>>),
    Instance(Rc<RefCell<LoxInstance>>),
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        !matches!(self, Value::Nil | Value::Bool(false))
    }

    pub fn is_equal(&self, other: &Value) -> bool {
        // Lox equality semantics
    }
}
```

**Display formatting:**

- Numbers: Integers display without `.0` (e.g., `42` not `42.0`)
- Strings: Plain text without quotes
- Booleans: `true` / `false`
- Nil: `nil`
- Functions: `<fn name>`
- Classes: Class name
- Instances: `ClassName instance`

#### `src/interpreter/environment.rs`

Variable storage with lexical scoping.

```rust
pub struct Environment {
    values: HashMap<String, Value>,
    enclosing: Option<Rc<RefCell<Environment>>>,
}

impl Environment {
    pub fn new() -> Self
    pub fn with_enclosing(enclosing: Rc<RefCell<Environment>>) -> Self

    pub fn define(&mut self, name: String, value: Value)
    pub fn get(&self, name: &str) -> Option<Value>
    pub fn assign(&mut self, name: &str, value: Value) -> bool

    // Direct access at specific depth (uses resolver data)
    pub fn get_at(&self, distance: usize, name: &str) -> Option<Value>
    pub fn assign_at(&mut self, distance: usize, name: &str, value: Value) -> bool
}
```

**Scoping:**

- Linked chain of environments: `child → parent → grandparent → ... → globals`
- Use `Rc<RefCell<>>` for shared mutable access
- Resolver provides exact depth, eliminating scope chain walk

#### `src/interpreter/callable.rs`

Function representation and calling.

```rust
pub enum Callable {
    Native(NativeFunction),
    User(LoxFunction),
}

pub struct LoxFunction {
    pub declaration: Function,  // AST node
    pub closure: Rc<RefCell<Environment>>,  // Captured environment
    pub is_initializer: bool,   // `init` method special case
}

impl Callable {
    pub fn arity(&self) -> usize
    pub fn call(
        &self,
        interpreter: &mut Interpreter,
        arguments: Vec<Value>
    ) -> Result<Value, LoxError>

    // For methods: bind `this` to instance
    pub fn bind(&self, instance: Value) -> Callable
}

pub enum NativeFunction {
    Clock,  // Returns Unix timestamp
}
```

**Closures:**

- Functions capture their definition environment
- Store `Rc<RefCell<Environment>>` pointer
- Nested functions close over outer function's locals

#### `src/interpreter/value.rs` (Classes)

Object-oriented features.

```rust
pub struct LoxClass {
    pub name: String,
    pub superclass: Option<Rc<RefCell<LoxClass>>>,
    pub methods: HashMap<String, Rc<Callable>>,
}

pub struct LoxInstance {
    pub class: Rc<RefCell<LoxClass>>,
    pub fields: HashMap<String, Value>,
}

impl LoxClass {
    pub fn find_method(&self, name: &str) -> Option<Rc<Callable>> {
        // Check own methods, then superclass chain
    }
}

impl LoxInstance {
    pub fn get(&self, name: &str, instance: &Rc<RefCell<LoxInstance>>)
               -> Result<Value, LoxError> {
        // Check fields first, then methods (bind `this`)
    }

    pub fn set(&mut self, name: String, value: Value) {
        // Always succeeds - fields created on assignment
    }
}
```

**`this` binding:**

- Methods create new environment with `this` at slot 0
- Resolver marks `this` as depth 0 in method scopes

**`super` binding:**

- Subclass methods create environment with `super` pointing to superclass
- `super.method()` looks up in superclass, binds `this` to current instance

### Execution Semantics

#### Return Handling

Uses Rust's `Result` for control flow:

```rust
// In LoxError enum:
Return(Value)  // Not really an error, used for unwinding

// In function call:
match self .call_function(...) {
Err(LoxError::Return(value)) => Ok(value),
other => other,
}
```

#### Initializer Special Case

```lox
class Foo {
    init(x) {
        this.x = x;
        // Implicit: return this;
    }
}
```

- Always returns `this` instance
- Error if explicit `return value;` (checked by resolver)

### Data Flow

```plain
AST + Locals Map
    ↓
Tree-walk interpretation
    ↓
Execute statements, evaluate expressions
    ↓
Side effects (print, mutations) + final Value
```

---

## Phase 4: Bytecode Compilation and VM

**Location:** `src/vm/`

### Purpose

Alternative execution backend: compile AST to bytecode, execute in stack-based virtual machine.

### Key Modules

#### `src/vm/chunk.rs`

Bytecode representation.

```rust
#[repr(u8)]
pub enum OpCode {
    // Constants
    Constant,
    Nil,
    True,
    False,

    // Stack operations
    Pop,
    GetLocal,
    SetLocal,
    GetGlobal,
    SetGlobal,
    DefineGlobal,
    GetUpvalue,
    SetUpvalue,
    GetProperty,
    SetProperty,
    GetSuper,

    // Operators
    Equal,
    Greater,
    Less,
    Add,
    Subtract,
    Multiply,
    Divide,
    Not,
    Negate,

    // Control flow
    Print,
    Jump,
    JumpIfFalse,
    Loop,
    Call,
    Invoke,
    SuperInvoke,

    // Functions and closures
    Closure,
    CloseUpvalue,
    Return,

    // Classes
    Class,
    Inherit,
    Method,
}

pub enum Constant {
    Number(f64),
    String(String),
    Function {
        name: String,
        arity: usize,
        upvalue_count: usize,
        chunk: Chunk,  // Nested chunk for function body
    },
}

pub struct Chunk {
    pub code: Vec<u8>,           // Bytecode instructions
    pub constants: Vec<Constant>, // Constant pool (max 256)
    pub lines: Vec<usize>,        // Line numbers for each instruction
}
```

**Key Methods:**

```rust
impl Chunk {
    pub fn write_op(&mut self, op: OpCode, line: usize)
    pub fn write_byte(&mut self, byte: u8, line: usize)
    pub fn write_u16(&mut self, value: u16, line: usize)  // For jumps
    pub fn add_constant(&mut self, constant: Constant) -> u8
    pub fn read_u16(&self, offset: usize) -> u16
}

pub fn disassemble(chunk: &Chunk, name: &str) -> String  // Human-readable output
```

**Serialization:**

- `Chunk` implements `Serialize` / `Deserialize` (serde)
- Uses binary MessagePack format via `rmp-serde`
- File format: 4-byte magic header (`b"blox"`) followed by MessagePack payload
- Save bytecode with `--compile-bytecode` (derives output path: `.lox` → `.blox`)
- CLI autodetects `.blox` files by checking the magic header and runs them via VM

#### `src/vm/compiler.rs`

AST to bytecode compiler.

```rust
pub struct Compiler {
    states: Vec<CompilerState>,  // Stack for nested functions
}

struct CompilerState {
    function_type: FunctionType,  // Script, Function, Method, Initializer
    chunk: Chunk,
    locals: Vec<Local>,           // Current function's locals
    upvalues: Vec<Upvalue>,       // Captured variables
    scope_depth: i32,
    line: usize,
}

struct Local {
    name: String,
    depth: i32,
    is_captured: bool,  // Used in a closure?
}

struct Upvalue {
    index: u8,
    is_local: bool,  // Captures local or upvalue from enclosing?
}
```

**Key Methods:**

```rust
impl Compiler {
    pub fn compile(self, program: &Program) -> Result<Chunk, LoxError>

    fn compile_decl(&mut self, decl: &Decl) -> Result<(), LoxError>
    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), LoxError>
    fn compile_expr(&mut self, expr: &Expr) -> Result<(), LoxError>

    // Control flow
    fn emit_jump(&mut self, op: OpCode) -> usize      // Returns offset to patch
    fn patch_jump(&mut self, offset: usize)           // Fill in jump distance
    fn emit_loop(&mut self, loop_start: usize)        // Jump backward

    // Scoping
    fn begin_scope(&mut self)
    fn end_scope(&mut self)  // Emits Pop or CloseUpvalue for each local

    // Variable resolution
    fn resolve_local(&self, name: &str) -> Option<u8>
    fn resolve_upvalue(&mut self, name: &str) -> Option<u8>
    fn add_upvalue(&mut self, index: u8, is_local: bool) -> u8
}
```

**Compilation Strategy:**

1. **Variables:**
    - Globals: Use `DefineGlobal`, `GetGlobal`, `SetGlobal` with constant pool index
    - Locals: Use `GetLocal`, `SetLocal` with stack slot index
    - Upvalues: Use `GetUpvalue`, `SetUpvalue` with upvalue index

2. **Control flow:**
    - `if`: Compile condition, emit `JumpIfFalse` with placeholder, compile then-branch,
      patch jump, compile else-branch
    - `while`: Mark loop start, compile condition, emit `JumpIfFalse` to end,
      compile body, emit `Loop` back to start
    - Logical `and`/`or`: Short-circuit with conditional jumps

3. **Functions:**
    - Push new `CompilerState` onto stack
    - Compile parameters as locals
    - Compile body
    - Pop state, emit `Closure` instruction with upvalue info
    - Nested functions access parent's upvalues

4. **Classes:**
    - Emit `Class` instruction
    - For each method: compile as function, emit `Method`
    - For inheritance: emit `Inherit`, create `super` scope

#### `src/vm/vm.rs`

Stack-based virtual machine.

```rust
pub struct Vm {
    stack: Vec<VmValue>,              // Operand stack
    frames: Vec<CallFrame>,           // Call stack
    globals: HashMap<String, VmValue>,
    open_upvalues: Vec<Rc<RefCell<VmUpvalue>>>,
    output: Vec<String>,              // For testing
    writer: Box<dyn Write>,
}

struct CallFrame {
    closure: Rc<VmClosure>,
    ip: usize,           // Instruction pointer
    slot_offset: usize,  // Base of this frame's stack slots
}

enum VmValue {
    Number(f64),
    Bool(bool),
    Nil,
    String(Rc<String>),
    Closure(Rc<VmClosure>),
    NativeFunction(NativeFn),
    Class(Rc<RefCell<VmClass>>),
    Instance(Rc<RefCell<VmInstance>>),
    BoundMethod(Rc<VmBoundMethod>),
}

struct VmClosure {
    function: Rc<VmFunction>,
    upvalues: Vec<Rc<RefCell<VmUpvalue>>>,
}

enum VmUpvalue {
    Open(usize),        // Stack index (still on stack)
    Closed(VmValue),    // Closed-over value (moved to heap)
}
```

**Why closures only (no separate `Function` variant):**

All user-defined functions are represented as `Closure` at runtime — a closure
with zero upvalues is simply a plain function. This follows the approach from
*Crafting Interpreters* (ch. 25) and Lua: rather than branching on "is this a
function or a closure?" at every call site, the VM uses a single `Closure`
representation unconditionally. The cost of an empty upvalue vector is
negligible compared to the branch elimination it buys.

Note that the LLVM codegen backend *can* optimize non-capturing functions to
bare function pointers as a separate codegen-level optimization, since it
operates at a different abstraction level than the bytecode VM.

**Execution Loop:**

```rust
impl Vm {
    pub fn interpret(&mut self, chunk: Chunk) -> Result<(), LoxError> {
        // Create top-level function, push onto call stack
        // Run main loop
    }

    fn run(&mut self) -> Result<(), LoxError> {
        loop {
            let op = self.read_byte();
            match op_from_u8(op) {
                Some(OpCode::Constant) => { /* ... */ }
                Some(OpCode::Add) => { /* pop 2, push sum */ }
                Some(OpCode::Call) => { /* call function */ }
                // ... 40+ opcodes
            }
        }
    }
}
```

**Key VM Operations:**

1. **Stack operations:**

   ```rust
   GetLocal(slot)  → stack.push(stack[frame.base + slot])
   SetLocal(slot)  → stack[frame.base + slot] = stack.top()
   Pop             → stack.pop()
   ```

2. **Function calls:**

   ```rust
   Call(arg_count) {
       let callee = stack[len - arg_count - 1];
       // Check arity
       frames.push(CallFrame { closure, ip: 0, slot_offset: len - arg_count - 1 });
   }
   ```

3. **Upvalue closing:**

   ```rust
   CloseUpvalue {
       // When local goes out of scope but is captured
       let value = stack[idx];
       upvalue.close(value);  // Open(idx) → Closed(value)
       stack.truncate(idx);
   }
   ```

4. **Method invocation optimization:**

   ```rust
   Invoke(name, arg_count) {
       // Combined: get property + call
       // Faster than separate GetProperty + Call
   }
   ```

### Bytecode Example

```lox
fun add(a, b) {
    return a + b;
}
print add(1, 2);
```

**Compiled bytecode:**

```plain
== script ==
0000  Closure      0 <fn add>
0002  DefineGlobal 1 "add"
0004  GetGlobal    1 "add"
0006  Constant     2 '1'
0008  Constant     3 '2'
0010  Call         2
0011  Print
0012  Nil
0013  Return

== add ==
0000  GetLocal     1  ; a
0002  GetLocal     2  ; b
0004  Add
0005  Return
```

### Data Flow

```plain
AST
    ↓
Compiler (single-pass)
    ↓
Bytecode Chunk
    ↓
VM (fetch-decode-execute loop)
    ↓
Side effects + result
```

---

## Phase 5: LLVM IR Compilation

**Location:** `src/codegen/` and `runtime/`

### Purpose

Compile Lox AST to LLVM IR text files (`.ll`) that can be executed via `lli`
or compiled to native code. Uses the `inkwell` crate (safe Rust bindings for
LLVM 21).

### Value Representation

All Lox values are represented as a tagged union struct `{ i8, i64 }`:

| Tag | Type     | Payload                              |
|-----|----------|--------------------------------------|
| 0   | nil      | unused (0)                           |
| 1   | bool     | 0 or 1                               |
| 2   | number   | f64 bitcast to i64                   |
| 3   | string   | pointer to null-terminated C string  |
| 4   | function | pointer to closure struct             |
| 5   | class    | pointer to class descriptor          |
| 6   | instance | pointer to instance struct           |

### Key Modules

#### `src/codegen/types.rs`

- `LoxValueType`: Helper struct for building and extracting LoxValue structs
- Tag constants: `TAG_NIL`, `TAG_BOOL`, `TAG_NUMBER`, `TAG_STRING`, etc.
- Builder methods: `build_nil()`, `build_number()`, `build_bool()`, etc.
- Extractor methods: `extract_tag()`, `extract_payload()`, `extract_number()`

#### `src/codegen/capture.rs`

- `CaptureInfo`: Pre-codegen AST pass identifying variables that cross function
  boundaries and need heap-allocated cells
- Handles classes with synthetic `__class_Name` scopes for `this`/`super`

#### `src/codegen/runtime.rs`

- `RuntimeDecls`: Declares all external C runtime functions in the LLVM module
- Functions: `lox_print`, `lox_global_get`, `lox_global_set`,
  `lox_value_truthy`, `lox_runtime_error`, `lox_alloc_closure`,
  `lox_alloc_cell`, `lox_cell_get`, `lox_cell_set`, `lox_string_concat`,
  `lox_string_equal`, `lox_alloc_class`, `lox_class_add_method`,
  `lox_alloc_instance`, `lox_instance_get_property`, `lox_instance_set_field`,
  `lox_class_find_method`, `lox_bind_method`, `lox_clock`

#### `src/codegen/compiler.rs`

- `CodeGen`: Main code generator struct wrapping inkwell Context/Module/Builder
- `compile(program) -> Result<String>`: Entry point, returns LLVM IR text
- Full Lox language support: literals, arithmetic, comparisons, unary ops,
  print, global/local variables, control flow, functions, closures, classes,
  inheritance, `this`, `super`, runtime type checks with line numbers

#### `runtime/lox_runtime.c`

- C runtime library loaded via `lli --extra-object`
- Implements: printing, global variable hash map, truthiness, error reporting,
  closure allocation, heap cells for captured variables, string concatenation
  and equality, class/instance allocation, field get/set, method lookup
  (walks superclass chain), method binding, `clock()` native function
- Number formatting matches Lox semantics (integers without `.0`)

### Feature Coverage

The LLVM codegen supports the full Lox language: literals, arithmetic, string
operations, control flow (`if`/`else`, `while`, `for`, `and`/`or`), local and
global variables with lexical scoping, functions, closures with captured
variables, classes, inheritance, `this`, `super`, `init` constructors, and
runtime error reporting with line numbers.

### Running LLVM Output

```bash
# Build the runtime (once)
make -C runtime

# Compile to LLVM IR and run via lli
cargo run -- --compile-llvm file.lox
lli --extra-object runtime/lox_runtime.o file.ll

# Compile to native executable
cargo run -- --compile file.lox         # produces ./file
cargo run -- --compile -o out file.lox  # custom output path
```

### Native Compilation Pipeline

The `--compile` flag extends the LLVM codegen to produce a self-contained native
ELF executable. The pipeline:

1. **Module emission** — same as `--compile-llvm`, producing an in-memory LLVM
   `Module` via `compile_to_module()`
2. **Object emission** (`src/codegen/native.rs`) — initializes the host's native
   LLVM target, creates a `TargetMachine`, sets the module's triple and data
   layout, then calls `machine.write_to_file()` to emit a `.o` object file
3. **Linking** — invokes `gcc` (or `$CC`) to link the program object with
   `lox_runtime.o` (statically linked C runtime) and `-lm`

The `build.rs` script compiles `lox_runtime.o`, used by both `lli --extra-object`
and native linking. The object file path is exposed at compile time via
`env!("LOX_RUNTIME_OBJ")`.

Bytecode `.blox` files cannot be compiled to native executables because they
discard AST structure and resolution data needed for LLVM IR generation.

### Data Flow

```plain
AST
    ↓
CodeGen (AST walking, inkwell API)
    ↓
LLVM Module (in-memory)
    ├──→ print_to_string() → .ll file → lli (--compile-llvm)
    └──→ TargetMachine → .o file → gcc → ELF executable (--compile)
```

---

## Error Handling Architecture

### Two-Tier Error System

vibe-lox uses a clean separation between compile-time and runtime errors:

1. **CompileError** - For scanner, parser, resolver (with miette diagnostics and source context)
2. **RuntimeError** - For interpreter and VM runtime errors (simple display, optional line numbers)

This separation provides:

- Appropriate level of detail for each error type
- Rich diagnostics where source code is available
- Zero overhead for line number tracking (only calculated when displaying errors)
- No trait bound conflicts (RuntimeError doesn't need Send + Sync)

### CompileError (Compile-Time)

**Used for:** Scanner, Parser, Resolver

```rust
#[derive(Error, Debug, Diagnostic)]
pub enum CompileError {
    #[error("scan error: {message}")]
    #[diagnostic(code(lox::scan))]
    Scan {
        message: String,
        #[label("here")]
        span: SourceSpan,
        #[source_code]
        src: miette::NamedSource<String>,
    },

    #[error("parse error: {message}")]
    #[diagnostic(code(lox::parse))]
    Parse {
        message: String,
        #[label("here")]
        span: SourceSpan,
        #[source_code]
        src: miette::NamedSource<String>,
    },

    #[error("resolution error: {message}")]
    #[diagnostic(code(lox::resolve))]
    Resolve {
        message: String,
        #[label("here")]
        span: SourceSpan,
        #[source_code]
        src: miette::NamedSource<String>,
    },
}

// Helper constructors
impl CompileError {
    pub fn scan(message: impl Into<String>, offset: usize, len: usize) -> Self
    pub fn parse(message: impl Into<String>, offset: usize, len: usize) -> Self
    pub fn resolve(message: impl Into<String>, offset: usize, len: usize) -> Self
    pub fn with_source_code(self, name: impl Into<String>, source: impl Into<String>) -> Self
}
```

**Example output:**

```
lox::parse

  × parse error: expected ';' after expression, found 'print'
   ╭─[tmp/counter2.lox:8:5]
 7 │     j = i
 8 │     print i;
   ·     ──┬──
   ·       ╰── here
 9 │   }
   ╰────
```

### RuntimeError (Runtime Execution)

**Used for:** Interpreter and VM runtime errors

```rust
/// A single frame in the Lox call stack, captured at the point of a runtime error.
pub struct StackFrame {
    pub function_name: String,
    pub line: usize,
}

#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("Error: {message}")]
    Error {
        message: String,
        span: Option<Span>,           // Only available in interpreter mode
        backtrace: Vec<StackFrame>,   // Call stack snapshot at error site
    },

    #[error("return")]
    Return {
        value: Value,  // Control flow, not really an error
    },
}

impl RuntimeError {
    pub fn new(message: impl Into<String>) -> Self
    pub fn with_span(message: impl Into<String>, span: Span) -> Self
    pub fn with_backtrace(self, frames: Vec<StackFrame>) -> Self
    pub fn backtrace_frames(&self) -> &[StackFrame]
    pub fn display_with_line(&self, source: &str) -> String
    pub fn is_return(&self) -> bool
    pub fn into_return_value(self) -> Option<Value>
}

// Backtrace formatting helpers
pub fn format_backtrace(frames: &[StackFrame]) -> String
pub fn backtrace_enabled() -> bool  // checks LOX_BACKTRACE env var
```

**Example output (interpreter with source):**

```
Error: line 3: operands must be two numbers or two strings
```

**Example output (VM with line numbers and backtrace, `LOX_BACKTRACE=1`):**

```
Error: line 6: operand must be a number
stack backtrace:
  0: inner()        [line 6]
  1: middle()       [line 10]
  2: outer()        [line 13]
  3: <script>()     [line 15]
```

### Line Number Calculation

For interpreter mode, line numbers are calculated on-demand when displaying errors:

```rust
fn offset_to_line(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())]
        .chars()
        .filter(|&c| c == '\n')
        .count()
        + 1
}
```

**Design rationale:**

- Only called when displaying errors (not during execution)
- Simple linear scan - acceptable performance for error cases
- Allows keeping `Span` simple (just offset + len, no line/column)
- Both interpreter and VM provide line numbers (interpreter via spans, VM via chunk line tables)

### Stack Backtraces

Both the interpreter and VM support optional stack backtraces, controlled by the
`LOX_BACKTRACE` environment variable (set to `1` or `full`):

- **Interpreter:** Maintains a `call_stack: Vec<StackFrame>` field. Pushes a frame
  in `call_function()` before executing the body, pops after. On error, snapshots
  the call stack before popping and attaches it to the error via `with_backtrace()`.

- **VM:** The `runtime_error()` helper snapshots `self.frames` (which already tracks
  the call stack) into `Vec<StackFrame>`, reversing to innermost-first order. Also
  extracts line numbers from `chunk.lines[ip]` for each frame.

Both backends produce frames in innermost-first order (most recent call at index 0).

### When to Use Each Error Type

| Situation           | Error Type              | Has Source Context? | Shows Line Number?          |
|---------------------|-------------------------|---------------------|-----------------------------|
| Lexical error       | `CompileError::Scan`    | Yes (miette)        | Yes (miette)                |
| Syntax error        | `CompileError::Parse`   | Yes (miette)        | Yes (miette)                |
| Semantic error      | `CompileError::Resolve` | Yes (miette)        | Yes (miette)                |
| Interpreter runtime | `RuntimeError::Error`   | Optional span       | Yes (calculated)            |
| VM runtime          | `RuntimeError::Error`   | No span             | Yes (from chunk line table) |
| Function return     | `RuntimeError::Return`  | N/A                 | N/A (control flow)          |

### Error Display Format

All errors start with "Error:" for consistency:

**Compile errors (miette):**

```
lox::parse

  × parse error: expected ';' after expression
   ╭─[test.lox:3:5]
   ...
```

**Runtime errors (interpreter):**

```
Error: line 42: undefined variable 'x'
```

**Runtime errors (VM, now with line numbers):**

```
Error: line 3: operands must be numbers
```

### Error Recovery

1. **Scanner:** Continues after error, collects all lexical errors
2. **Parser:** Panic-mode recovery - synchronizes at statement boundaries
3. **Resolver:** Continues checking, collects all semantic errors
4. **Interpreter:** Stops at first error, unwinds with `Result`
5. **VM:** Stops at first error (runtime errors can't recover)

### Implementation Guidelines

**Creating compile errors:**

```rust
// Requires offset and length from span
CompileError::parse("expected ';'", token.span.offset, token.span.len)
.with_source_code(filename, source_code)
```

**Creating runtime errors:**

```rust
// Interpreter - with span:
RuntimeError::with_span("type error", expr.span)

// Interpreter or VM - without span:
RuntimeError::new("undefined variable 'x'")

// Return value (control flow):
RuntimeError::Return { value }
```

**Displaying errors:**

```rust
// Compile errors (main.rs):
for error in errors {
let error_with_src = error.with_source_code(filename, source);
eprintln ! ("{:?}", miette::Report::new(error_with_src));
}

// Runtime errors (main.rs):
if let Some(source) = source_code {
eprintln!("{}", error.display_with_line(source));  // Interpreter
} else {
eprintln ! ("{}", error);  // VM
}
```

---

## Module Organization

```
src/
├── main.rs              # CLI entry point, argument parsing
├── lib.rs               # Library crate root, public API
├── error.rs             # LoxError enum, error types
├── repl.rs              # Interactive REPL
│
├── scanner/             # Phase 1: Tokenization
│   ├── mod.rs          # Public scan() API
│   ├── token.rs        # Token, TokenKind, Span
│   └── lexer.rs        # winnow-based implementation
│
├── ast/                 # AST definitions
│   ├── mod.rs          # Program, Decl, Stmt, Expr
│   └── printer.rs      # to_sexp(), to_json()
│
├── parser/              # Phase 2: Parsing
│   └── mod.rs          # Recursive descent parser
│
├── interpreter/         # Phase 3B: Tree-walk interpretation
│   ├── mod.rs          # Interpreter struct, main logic
│   ├── value.rs        # Value enum, LoxClass, LoxInstance
│   ├── callable.rs     # Function calling, NativeFunction
│   ├── environment.rs  # Variable scoping
│   └── resolver.rs     # Phase 3A: Static resolution
│
├── vm/                  # Phase 4: Bytecode VM
│   ├── mod.rs          # Public API
│   ├── chunk.rs        # OpCode, Constant, Chunk
│   ├── compiler.rs     # AST → bytecode compiler
│   └── vm.rs           # Stack-based VM execution
│
└── codegen/             # Phase 5: LLVM IR and native compilation
    ├── mod.rs          # Public compile() and compile_to_module() API
    ├── compiler.rs     # CodeGen struct, AST → LLVM IR
    ├── native.rs       # Native ELF compilation (object emission + linking)
    ├── capture.rs      # Capture analysis (variables crossing function boundaries)
    ├── types.rs        # LoxValue type ({i8, i64} tagged union)
    └── runtime.rs      # External runtime function declarations

runtime/                 # C runtime for LLVM-compiled programs
├── lox_runtime.c       # print, globals, truthiness, closures, strings, classes
└── lox_runtime.h       # Header with LoxValue struct and tag constants
```

---

## Testing Strategy

### Unit Tests (~302 tests)

**By module:**

- `scanner/lexer.rs` (23 tests): Token types, spans, errors, shebang handling
- `parser/mod.rs` (22 tests): Grammar rules, precedence, recovery
- `ast/printer.rs` (2 tests): S-expr and JSON output
- `interpreter/environment.rs` (6 tests): Scope operations
- `interpreter/mod.rs` (26 tests): Language semantics
- `interpreter/resolver.rs` (36 tests): Semantic errors, resolution
- `vm/chunk.rs` (30 tests): Bytecode operations, serialization
- `vm/compiler.rs` (50 tests): Compilation correctness
- `vm/vm.rs` (70 tests): VM execution, all opcodes
- `codegen/compiler.rs` (52 tests): LLVM IR generation, type checks
- `error.rs` (14 tests): Error trait implementations
- `repl.rs` (4 tests): Bare expression detection, backslash command dispatch

**Test helpers:**

```rust
// Most tests use helper functions
fn compile(source: &str) -> Result<Chunk, LoxError>
fn run_vm(source: &str) -> Vec<String>
fn resolve(source: &str) -> Result<HashMap<ExprId, usize>, Vec<LoxError>>
```

### Integration Tests (34 tests)

**Fixture-based testing:**

```
tests/
├── interpreter_tests.rs       # Tree-walk interpreter (10 tests)
├── vm_tests.rs                # Bytecode VM (12 tests)
├── llvm_tests.rs              # LLVM IR codegen (13 tests)
└── native_compile_tests.rs    # Native ELF compilation (14 tests)

fixtures/
├── hello.lox               # Hello world
├── arithmetic.lox          # Math operations
├── scoping.lox             # Variable scoping
├── control_flow.lox        # If/else, while, for, logical operators
├── classes.lox             # OOP features
├── counter.lox             # Closures
├── fib.lox                 # Recursion
├── shebang.lox             # Shebang line handling
├── strings.lox             # String operations
├── error_*.lox             # Runtime error test cases
├── *.expected              # Expected stdout for success fixtures
└── *.expected_error        # Expected stderr for error fixtures
```

**Test execution:**

```rust
#[test]
fn fixture_fibonacci() {
    let source = include_str!("../fixtures/fibonacci.lox");
    let expected = include_str!("../fixtures/fibonacci.expected");

    let output = run_interpreter(source);
    assert_eq!(output, expected);
}
```

### Test Coverage

| Component      | Coverage |
|----------------|----------|
| Scanner        | ~95%     |
| Parser         | ~90%     |
| Resolver       | ~95%     |
| Interpreter    | ~85%     |
| VM Compiler    | ~90%     |
| VM Execution   | ~95%     |
| Bytecode Chunk | ~95%     |
| **Overall**    | **~85%** |

---

## Performance Characteristics

### Tree-Walk Interpreter

- **Startup:** Very fast (no compilation)
- **Execution:** Slower (AST traversal overhead)
- **Memory:** Higher (full AST in memory)
- **Best for:** Scripts, REPL, debugging

### Bytecode VM

- **Startup:** Slower (compilation required)
- **Execution:** Faster (tight dispatch loop)
- **Memory:** Lower (compact bytecode)
- **Best for:** Production, longer-running programs

**Benchmark (fibonacci(20)):**

- Tree-walk: ~150ms
- Bytecode VM: ~50ms
- **~3x speedup**

---

## Dependencies

### Core Dependencies

```toml
anyhow = "1.0"          # Error handling, context
clap = "4.5"            # CLI parsing (derive)
miette = "7.6"          # Fancy error diagnostics
thiserror = "2.0"       # Error type derivation
winnow = "0.7"          # Parser combinators
serde = "1.0"           # Serialization
serde_json = "1.0"      # JSON AST output
rmp-serde = "1.3"       # MessagePack bytecode serialization
inkwell = "0.8"         # LLVM 21 bindings (feature: llvm21-1)
```

### Dev Dependencies

```toml
rstest = "0.26"         # Parameterized testing
```

---

## Future Work

### Potential Improvements

1. **Constant pool optimization:** Deduplicate constants
2. **Jump optimization:** Use 8-bit jumps for short distances
3. **Invoke optimization:** Extend to more property access patterns
4. ~~**Bytecode format:** Binary format instead of JSON~~ (done — uses MessagePack with magic header)
5. ~~**LLVM IR compilation**~~ (done — full Lox language support via `inkwell`)
6. **REPL improvements:** Multi-line input, syntax highlighting, ~~backslash commands~~ (done — `\help`, `\quit`, `\clear`, `\version`)
7. **Debugger:** Step through bytecode, inspect stack
8. ~~**LLVM native compilation:** Compile `.ll` to native binary via `clang`~~ (done — `--compile` produces native ELF executables via inkwell `TargetMachine`)
9. **Garbage collection:** The C runtime currently leaks all heap allocations
10. **Stack overflow detection:** No depth counter for deep recursion

---

## Design Philosophy

### 1. **Simplicity First**

- Hand-written parser over parser generators
- Direct AST interpretation before bytecode
- Clear separation of concerns

### 2. **Excellent Error Messages**

- Precise source spans everywhere
- Rich terminal output with `miette`
- Multiple errors reported at once where possible

### 3. **Testability**

- Each phase independently testable
- Helper functions for easy test writing
- Both unit and integration test coverage

### 4. **Rust Idioms**

- `Result` for errors, not exceptions
- `.context()` on all error propagation
- `expect()` over `unwrap()` with rationale
- Smart pointers (`Rc`, `RefCell`) where needed

### 5. **Performance When Ready**

- Start with simplest correct implementation
- Add bytecode VM as optimization
- Plan for LLVM backend for maximum speed

---

## References

- **Lox Language Spec:** [Crafting Interpreters](https://craftinginterpreters.com/)
- **Grammar Definition:** `Grammar.md`
- **Implementation Plan:** `PLAN.md`
- **Testing Strategy:** `TESTING_IMPROVEMENTS.md`
- **Code Quality:** `TEST_AND_QUALITY_PLAN.md`
