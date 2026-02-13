use std::collections::{HashMap, HashSet};

use crate::ast::*;

/// Information about which variables are captured by closures.
///
/// Produced by analyzing the AST before codegen. Used to determine:
/// - Which local variables need heap-allocated cells instead of stack allocas
/// - Which captured variables each function needs in its environment
pub struct CaptureInfo {
    /// Variable names that are captured by at least one inner function,
    /// keyed by the declaring function (empty string = top-level).
    /// These variables must use cells instead of allocas.
    pub captured_vars: HashSet<CapturedVar>,

    /// For each function (by name), the list of captured variable names
    /// it references from enclosing scopes, in order.
    pub function_captures: HashMap<String, Vec<String>>,
}

/// Identifies a captured variable by its name and the function it's declared in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CapturedVar {
    pub var_name: String,
    /// The function in which this variable is declared.
    /// Empty string means top-level (main).
    pub declaring_function: String,
}

struct CaptureAnalyzer {
    /// Stack of function scopes. Each entry is (function_name, set of locally declared var names).
    function_scopes: Vec<(String, HashSet<String>)>,
    captured_vars: HashSet<CapturedVar>,
    function_captures: HashMap<String, Vec<String>>,
}

impl CaptureAnalyzer {
    fn new() -> Self {
        Self {
            function_scopes: vec![("".to_string(), HashSet::new())],
            captured_vars: HashSet::new(),
            function_captures: HashMap::new(),
        }
    }

    fn analyze(mut self, program: &Program) -> CaptureInfo {
        for decl in &program.declarations {
            self.visit_decl(decl);
        }
        CaptureInfo {
            captured_vars: self.captured_vars,
            function_captures: self.function_captures,
        }
    }

    fn current_function(&self) -> &str {
        &self
            .function_scopes
            .last()
            .expect("non-empty scope stack")
            .0
    }

    fn declare_var(&mut self, name: &str) {
        self.function_scopes
            .last_mut()
            .expect("non-empty scope stack")
            .1
            .insert(name.to_string());
    }

    /// Check if a variable reference crosses a function boundary.
    /// If so, mark it as captured. Variables in the top-level scope are globals
    /// and don't need capture (they're accessed via lox_global_get/set).
    fn reference_var(&mut self, name: &str) {
        let current_fn = self.current_function().to_string();

        // Walk function scopes from innermost to outermost
        for scope in self.function_scopes.iter().rev() {
            if scope.1.contains(name) {
                // Variables in the top-level scope are globals, not captured
                if scope.0.is_empty() {
                    return;
                }
                if scope.0 != current_fn {
                    // Variable declared in a different (outer) function — it's captured
                    self.captured_vars.insert(CapturedVar {
                        var_name: name.to_string(),
                        declaring_function: scope.0.clone(),
                    });
                    // Record that the current function captures this variable
                    let captures = self
                        .function_captures
                        .entry(current_fn.clone())
                        .or_default();
                    if !captures.contains(&name.to_string()) {
                        captures.push(name.to_string());
                    }

                    // Also mark capture for any intermediate functions between
                    // the declaring function and the current function
                    let declaring_idx = self
                        .function_scopes
                        .iter()
                        .position(|s| s.0 == scope.0)
                        .expect("declaring function in scope stack");
                    let current_idx = self.function_scopes.len() - 1;
                    for mid_scope in &self.function_scopes[declaring_idx + 1..current_idx] {
                        let mid_captures = self
                            .function_captures
                            .entry(mid_scope.0.clone())
                            .or_default();
                        if !mid_captures.contains(&name.to_string()) {
                            mid_captures.push(name.to_string());
                        }
                    }
                }
                return;
            }
        }
        // Not found in any function scope — must be a global, no capture needed
    }

    fn visit_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Var(v) => {
                if let Some(ref init) = v.initializer {
                    self.visit_expr(init);
                }
                self.declare_var(&v.name);
            }
            Decl::Fun(f) => {
                self.declare_var(&f.function.name);
                self.visit_function(&f.function);
            }
            Decl::Statement(s) => self.visit_stmt(s),
            Decl::Class(_) => {} // Phase 6
        }
    }

    fn visit_function(&mut self, function: &Function) {
        self.function_scopes
            .push((function.name.clone(), HashSet::new()));
        for param in &function.params {
            self.declare_var(param);
        }
        for decl in &function.body {
            self.visit_decl(decl);
        }
        self.function_scopes.pop();
    }

    fn visit_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expression(e) => self.visit_expr(&e.expression),
            Stmt::Print(p) => self.visit_expr(&p.expression),
            Stmt::Return(r) => {
                if let Some(ref val) = r.value {
                    self.visit_expr(val);
                }
            }
            Stmt::Block(b) => {
                for decl in &b.declarations {
                    self.visit_decl(decl);
                }
            }
            Stmt::If(i) => {
                self.visit_expr(&i.condition);
                self.visit_stmt(&i.then_branch);
                if let Some(ref else_branch) = i.else_branch {
                    self.visit_stmt(else_branch);
                }
            }
            Stmt::While(w) => {
                self.visit_expr(&w.condition);
                self.visit_stmt(&w.body);
            }
        }
    }

    fn visit_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Variable(v) => self.reference_var(&v.name),
            Expr::Assign(a) => {
                self.visit_expr(&a.value);
                self.reference_var(&a.name);
            }
            Expr::Binary(b) => {
                self.visit_expr(&b.left);
                self.visit_expr(&b.right);
            }
            Expr::Unary(u) => self.visit_expr(&u.operand),
            Expr::Logical(l) => {
                self.visit_expr(&l.left);
                self.visit_expr(&l.right);
            }
            Expr::Call(c) => {
                self.visit_expr(&c.callee);
                for arg in &c.arguments {
                    self.visit_expr(arg);
                }
            }
            Expr::Grouping(g) => self.visit_expr(&g.expression),
            Expr::Get(g) => self.visit_expr(&g.object),
            Expr::Set(s) => {
                self.visit_expr(&s.value);
                self.visit_expr(&s.object);
            }
            Expr::Literal(_) | Expr::This(_) | Expr::Super(_) => {}
        }
    }
}

/// Analyze a program to find which variables are captured by closures.
pub fn analyze_captures(program: &Program) -> CaptureInfo {
    CaptureAnalyzer::new().analyze(program)
}
