use std::collections::HashMap;

use crate::ast::*;
use crate::error::CompileError;

#[derive(Debug, Clone, Copy, PartialEq)]
enum FunctionType {
    None,
    Function,
    Method,
    Initializer,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ClassType {
    None,
    Class,
    Subclass,
}

pub struct Resolver {
    scopes: Vec<HashMap<String, bool>>,
    locals: HashMap<ExprId, usize>,
    current_function: FunctionType,
    current_class: ClassType,
    errors: Vec<CompileError>,
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Resolver {
    pub fn new() -> Self {
        Self {
            scopes: Vec::new(),
            locals: HashMap::new(),
            current_function: FunctionType::None,
            current_class: ClassType::None,
            errors: Vec::new(),
        }
    }

    pub fn resolve(
        mut self,
        program: &Program,
    ) -> Result<HashMap<ExprId, usize>, Vec<CompileError>> {
        for decl in &program.declarations {
            self.resolve_decl(decl);
        }
        if self.errors.is_empty() {
            Ok(self.locals)
        } else {
            Err(self.errors)
        }
    }

    fn begin_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn end_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: &str, span: crate::scanner::token::Span) {
        if let Some(scope) = self.scopes.last_mut() {
            if scope.contains_key(name) {
                self.errors.push(CompileError::resolve(
                    format!("variable '{name}' already declared in this scope"),
                    span.offset,
                    span.len,
                ));
            }
            scope.insert(name.to_string(), false);
        }
    }

    fn define(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), true);
        }
    }

    fn resolve_local(&mut self, id: ExprId, name: &str) {
        for (i, scope) in self.scopes.iter().rev().enumerate() {
            if scope.contains_key(name) {
                self.locals.insert(id, i);
                return;
            }
        }
        // Not found in any scope: assume global
    }

    fn resolve_decl(&mut self, decl: &Decl) {
        match decl {
            Decl::Var(v) => {
                self.declare(&v.name, v.span);
                if let Some(ref init) = v.initializer {
                    self.resolve_expr(init);
                }
                self.define(&v.name);
            }
            Decl::Fun(f) => {
                self.declare(&f.function.name, f.span);
                self.define(&f.function.name);
                self.resolve_function(&f.function, FunctionType::Function);
            }
            Decl::Class(c) => {
                let enclosing_class = self.current_class;
                self.current_class = ClassType::Class;

                self.declare(&c.name, c.span);
                self.define(&c.name);

                if let Some(ref superclass) = c.superclass {
                    if *superclass == c.name {
                        self.errors.push(CompileError::resolve(
                            "a class can't inherit from itself",
                            c.span.offset,
                            c.span.len,
                        ));
                    }
                    self.current_class = ClassType::Subclass;
                    self.resolve_local(0, superclass); // ID doesn't matter for superclass lookup
                    self.begin_scope();
                    self.scopes
                        .last_mut()
                        .expect("just pushed scope")
                        .insert("super".to_string(), true);
                }

                self.begin_scope();
                self.scopes
                    .last_mut()
                    .expect("just pushed scope")
                    .insert("this".to_string(), true);

                for method in &c.methods {
                    let func_type = if method.name == "init" {
                        FunctionType::Initializer
                    } else {
                        FunctionType::Method
                    };
                    self.resolve_function(method, func_type);
                }

                self.end_scope();
                if c.superclass.is_some() {
                    self.end_scope();
                }
                self.current_class = enclosing_class;
            }
            Decl::Statement(s) => self.resolve_stmt(s),
        }
    }

    fn resolve_function(&mut self, function: &Function, func_type: FunctionType) {
        let enclosing = self.current_function;
        self.current_function = func_type;
        self.begin_scope();
        for param in &function.params {
            self.declare(param, function.span);
            self.define(param);
        }
        for decl in &function.body {
            self.resolve_decl(decl);
        }
        self.end_scope();
        self.current_function = enclosing;
    }

    fn resolve_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Expression(e) => self.resolve_expr(&e.expression),
            Stmt::Print(p) => self.resolve_expr(&p.expression),
            Stmt::Return(r) => {
                if self.current_function == FunctionType::None {
                    self.errors.push(CompileError::resolve(
                        "can't return from top-level code",
                        r.span.offset,
                        r.span.len,
                    ));
                }
                if let Some(ref val) = r.value {
                    if self.current_function == FunctionType::Initializer {
                        self.errors.push(CompileError::resolve(
                            "can't return a value from an initializer",
                            r.span.offset,
                            r.span.len,
                        ));
                    }
                    self.resolve_expr(val);
                }
            }
            Stmt::Block(b) => {
                self.begin_scope();
                for decl in &b.declarations {
                    self.resolve_decl(decl);
                }
                self.end_scope();
            }
            Stmt::If(i) => {
                self.resolve_expr(&i.condition);
                self.resolve_stmt(&i.then_branch);
                if let Some(ref else_branch) = i.else_branch {
                    self.resolve_stmt(else_branch);
                }
            }
            Stmt::While(w) => {
                self.resolve_expr(&w.condition);
                self.resolve_stmt(&w.body);
            }
        }
    }

    fn resolve_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Variable(v) => {
                if let Some(scope) = self.scopes.last()
                    && scope.get(&v.name) == Some(&false)
                {
                    self.errors.push(CompileError::resolve(
                        "can't read local variable in its own initializer",
                        v.span.offset,
                        v.span.len,
                    ));
                }
                self.resolve_local(v.id, &v.name);
            }
            Expr::Assign(a) => {
                self.resolve_expr(&a.value);
                self.resolve_local(a.id, &a.name);
            }
            Expr::Binary(b) => {
                self.resolve_expr(&b.left);
                self.resolve_expr(&b.right);
            }
            Expr::Unary(u) => {
                self.resolve_expr(&u.operand);
            }
            Expr::Logical(l) => {
                self.resolve_expr(&l.left);
                self.resolve_expr(&l.right);
            }
            Expr::Call(c) => {
                self.resolve_expr(&c.callee);
                for arg in &c.arguments {
                    self.resolve_expr(arg);
                }
            }
            Expr::Get(g) => {
                self.resolve_expr(&g.object);
            }
            Expr::Set(s) => {
                self.resolve_expr(&s.value);
                self.resolve_expr(&s.object);
            }
            Expr::Grouping(g) => {
                self.resolve_expr(&g.expression);
            }
            Expr::This(t) => {
                if self.current_class == ClassType::None {
                    self.errors.push(CompileError::resolve(
                        "can't use 'this' outside of a class",
                        t.span.offset,
                        t.span.len,
                    ));
                }
                self.resolve_local(t.id, "this");
            }
            Expr::Super(s) => {
                match self.current_class {
                    ClassType::None => {
                        self.errors.push(CompileError::resolve(
                            "can't use 'super' outside of a class",
                            s.span.offset,
                            s.span.len,
                        ));
                    }
                    ClassType::Class => {
                        self.errors.push(CompileError::resolve(
                            "can't use 'super' in a class with no superclass",
                            s.span.offset,
                            s.span.len,
                        ));
                    }
                    ClassType::Subclass => {}
                }
                self.resolve_local(s.id, "super");
            }
            Expr::Literal(_) => {}
        }
    }
}
