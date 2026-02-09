pub mod callable;
pub mod environment;
pub mod resolver;
pub mod value;

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;

use crate::ast::*;
use crate::error::LoxError;
use crate::interpreter::callable::{Callable, LoxFunction, NativeFunction};
use crate::interpreter::environment::Environment;
use crate::interpreter::value::{LoxClass, LoxInstance, Value};

pub struct Interpreter {
    globals: Rc<RefCell<Environment>>,
    environment: Rc<RefCell<Environment>>,
    locals: HashMap<ExprId, usize>,
    output: Vec<String>,
    /// Writer for print output (allows testing without stdout)
    writer: Box<dyn Write>,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        let globals = Rc::new(RefCell::new(Environment::new()));
        globals.borrow_mut().define(
            "clock".to_string(),
            Value::Function(Callable::Native(NativeFunction::Clock)),
        );

        Self {
            globals: Rc::clone(&globals),
            environment: globals,
            locals: HashMap::new(),
            output: Vec::new(),
            writer: Box::new(std::io::stdout()),
        }
    }

    /// Create an interpreter that captures output (for testing).
    #[cfg(test)]
    fn new_capturing() -> Self {
        let globals = Rc::new(RefCell::new(Environment::new()));
        globals.borrow_mut().define(
            "clock".to_string(),
            Value::Function(Callable::Native(NativeFunction::Clock)),
        );

        Self {
            globals: Rc::clone(&globals),
            environment: globals,
            locals: HashMap::new(),
            output: Vec::new(),
            writer: Box::new(Vec::<u8>::new()),
        }
    }

    pub fn interpret(
        &mut self,
        program: &Program,
        locals: HashMap<ExprId, usize>,
    ) -> Result<(), LoxError> {
        self.locals = locals;
        for decl in &program.declarations {
            self.execute_decl(decl)?;
        }
        Ok(())
    }

    pub fn output(&self) -> &[String] {
        &self.output
    }

    /// Provide mutable access to the environment (for REPL).
    pub fn environment(&self) -> &Rc<RefCell<Environment>> {
        &self.environment
    }

    /// Merge additional locals (for REPL line-by-line resolution).
    pub fn merge_locals(&mut self, locals: HashMap<ExprId, usize>) {
        self.locals.extend(locals);
    }

    /// Execute additional declarations without resetting the environment (for REPL).
    pub fn interpret_additional(&mut self, program: &Program) -> Result<(), LoxError> {
        for decl in &program.declarations {
            self.execute_decl(decl)?;
        }
        Ok(())
    }

    fn execute_decl(&mut self, decl: &Decl) -> Result<(), LoxError> {
        match decl {
            Decl::Var(v) => {
                let value = match &v.initializer {
                    Some(init) => self.evaluate_expr(init)?,
                    None => Value::Nil,
                };
                self.environment.borrow_mut().define(v.name.clone(), value);
                Ok(())
            }
            Decl::Fun(f) => {
                let function = LoxFunction {
                    declaration: f.function.clone(),
                    closure: Rc::clone(&self.environment),
                    is_initializer: false,
                };
                self.environment.borrow_mut().define(
                    f.function.name.clone(),
                    Value::Function(Callable::User(function)),
                );
                Ok(())
            }
            Decl::Class(c) => self.execute_class(c),
            Decl::Statement(s) => self.execute_stmt(s),
        }
    }

    fn execute_class(&mut self, class: &ClassDecl) -> Result<(), LoxError> {
        let superclass = if let Some(ref name) = class.superclass {
            let val = self.environment.borrow().get(name).ok_or_else(|| {
                LoxError::runtime(
                    format!("undefined variable '{name}'"),
                    class.span.offset,
                    class.span.len,
                )
            })?;
            match val {
                Value::Class(sc) => Some(sc),
                _ => {
                    return Err(LoxError::runtime(
                        "superclass must be a class",
                        class.span.offset,
                        class.span.len,
                    ));
                }
            }
        } else {
            None
        };

        self.environment
            .borrow_mut()
            .define(class.name.clone(), Value::Nil);

        let enclosing = if let Some(ref sc) = superclass {
            let env = Rc::new(RefCell::new(Environment::with_enclosing(Rc::clone(
                &self.environment,
            ))));
            env.borrow_mut()
                .define("super".to_string(), Value::Class(Rc::clone(sc)));
            let old = Rc::clone(&self.environment);
            self.environment = env;
            Some(old)
        } else {
            None
        };

        let mut methods = HashMap::new();
        for method in &class.methods {
            let function = Callable::User(LoxFunction {
                declaration: method.clone(),
                closure: Rc::clone(&self.environment),
                is_initializer: method.name == "init",
            });
            methods.insert(method.name.clone(), function);
        }

        if let Some(enc) = enclosing {
            self.environment = enc;
        }

        let lox_class = Rc::new(LoxClass {
            name: class.name.clone(),
            superclass,
            methods,
        });

        self.environment
            .borrow_mut()
            .assign(&class.name, Value::Class(lox_class));

        Ok(())
    }

    fn execute_stmt(&mut self, stmt: &Stmt) -> Result<(), LoxError> {
        match stmt {
            Stmt::Expression(e) => {
                self.evaluate_expr(&e.expression)?;
                Ok(())
            }
            Stmt::Print(p) => {
                let value = self.evaluate_expr(&p.expression)?;
                let text = format!("{value}");
                writeln!(self.writer, "{text}").expect("write should succeed");
                self.output.push(text);
                Ok(())
            }
            Stmt::Return(r) => {
                let value = match &r.value {
                    Some(val) => self.evaluate_expr(val)?,
                    None => Value::Nil,
                };
                Err(LoxError::Return {
                    value: Box::new(value),
                })
            }
            Stmt::Block(b) => {
                let env = Rc::new(RefCell::new(Environment::with_enclosing(Rc::clone(
                    &self.environment,
                ))));
                self.execute_block(&b.declarations, env)
            }
            Stmt::If(i) => {
                let condition = self.evaluate_expr(&i.condition)?;
                if condition.is_truthy() {
                    self.execute_stmt(&i.then_branch)
                } else if let Some(ref else_branch) = i.else_branch {
                    self.execute_stmt(else_branch)
                } else {
                    Ok(())
                }
            }
            Stmt::While(w) => {
                while self.evaluate_expr(&w.condition)?.is_truthy() {
                    self.execute_stmt(&w.body)?;
                }
                Ok(())
            }
        }
    }

    fn execute_block(
        &mut self,
        declarations: &[Decl],
        env: Rc<RefCell<Environment>>,
    ) -> Result<(), LoxError> {
        let previous = Rc::clone(&self.environment);
        self.environment = env;
        let result = declarations.iter().try_for_each(|d| self.execute_decl(d));
        self.environment = previous;
        result
    }

    fn evaluate_expr(&mut self, expr: &Expr) -> Result<Value, LoxError> {
        match expr {
            Expr::Literal(l) => Ok(match &l.value {
                LiteralValue::Number(n) => Value::Number(*n),
                LiteralValue::String(s) => Value::Str(s.clone()),
                LiteralValue::Bool(b) => Value::Bool(*b),
                LiteralValue::Nil => Value::Nil,
            }),
            Expr::Grouping(g) => self.evaluate_expr(&g.expression),
            Expr::Unary(u) => {
                let operand = self.evaluate_expr(&u.operand)?;
                match u.operator {
                    UnaryOp::Negate => match operand {
                        Value::Number(n) => Ok(Value::Number(-n)),
                        _ => Err(LoxError::runtime(
                            "operand must be a number",
                            u.span.offset,
                            u.span.len,
                        )),
                    },
                    UnaryOp::Not => Ok(Value::Bool(!operand.is_truthy())),
                }
            }
            Expr::Binary(b) => self.evaluate_binary(b),
            Expr::Variable(v) => self.look_up_variable(&v.name, v.id, v.span),
            Expr::Assign(a) => {
                let value = self.evaluate_expr(&a.value)?;
                if let Some(&distance) = self.locals.get(&a.id) {
                    self.environment
                        .borrow_mut()
                        .assign_at(distance, &a.name, value.clone());
                } else {
                    let ok = self.globals.borrow_mut().assign(&a.name, value.clone());
                    if !ok {
                        return Err(LoxError::runtime(
                            format!("undefined variable '{}'", a.name),
                            a.span.offset,
                            a.span.len,
                        ));
                    }
                }
                Ok(value)
            }
            Expr::Logical(l) => {
                let left = self.evaluate_expr(&l.left)?;
                match l.operator {
                    LogicalOp::Or => {
                        if left.is_truthy() {
                            return Ok(left);
                        }
                    }
                    LogicalOp::And => {
                        if !left.is_truthy() {
                            return Ok(left);
                        }
                    }
                }
                self.evaluate_expr(&l.right)
            }
            Expr::Call(c) => self.evaluate_call(c),
            Expr::Get(g) => {
                let object = self.evaluate_expr(&g.object)?;
                match object {
                    Value::Instance(inst) => {
                        let val = inst.borrow().get(&g.name, Rc::clone(&inst));
                        val.ok_or_else(|| {
                            LoxError::runtime(
                                format!("undefined property '{}'", g.name),
                                g.span.offset,
                                g.span.len,
                            )
                        })
                    }
                    _ => Err(LoxError::runtime(
                        "only instances have properties",
                        g.span.offset,
                        g.span.len,
                    )),
                }
            }
            Expr::Set(s) => {
                let object = self.evaluate_expr(&s.object)?;
                match object {
                    Value::Instance(inst) => {
                        let value = self.evaluate_expr(&s.value)?;
                        inst.borrow_mut().set(s.name.clone(), value.clone());
                        Ok(value)
                    }
                    _ => Err(LoxError::runtime(
                        "only instances have fields",
                        s.span.offset,
                        s.span.len,
                    )),
                }
            }
            Expr::This(t) => self.look_up_variable("this", t.id, t.span),
            Expr::Super(s) => {
                let distance = *self
                    .locals
                    .get(&s.id)
                    .expect("resolver should have resolved 'super'");
                let superclass = self
                    .environment
                    .borrow()
                    .get_at(distance, "super")
                    .expect("resolver guarantees 'super' exists");
                let object = self
                    .environment
                    .borrow()
                    .get_at(distance - 1, "this")
                    .expect("resolver guarantees 'this' exists");

                if let (Value::Class(sc), Value::Instance(inst)) = (superclass, object) {
                    let method = sc.find_method(&s.method).ok_or_else(|| {
                        LoxError::runtime(
                            format!("undefined property '{}'", s.method),
                            s.span.offset,
                            s.span.len,
                        )
                    })?;
                    Ok(Value::Function(method.bind(inst)))
                } else {
                    Err(LoxError::runtime(
                        "super lookup failed",
                        s.span.offset,
                        s.span.len,
                    ))
                }
            }
        }
    }

    fn evaluate_binary(&mut self, b: &BinaryExpr) -> Result<Value, LoxError> {
        let left = self.evaluate_expr(&b.left)?;
        let right = self.evaluate_expr(&b.right)?;

        match b.operator {
            BinaryOp::Add => match (&left, &right) {
                (Value::Number(a), Value::Number(b_val)) => Ok(Value::Number(a + b_val)),
                (Value::Str(a), Value::Str(b_val)) => Ok(Value::Str(format!("{a}{b_val}"))),
                _ => Err(LoxError::runtime(
                    "operands must be two numbers or two strings",
                    b.span.offset,
                    b.span.len,
                )),
            },
            BinaryOp::Subtract => number_binop(&left, &right, |a, c| a - c, b),
            BinaryOp::Multiply => number_binop(&left, &right, |a, c| a * c, b),
            BinaryOp::Divide => number_binop(&left, &right, |a, c| a / c, b),
            BinaryOp::Less => number_cmp(&left, &right, |a, c| a < c, b),
            BinaryOp::LessEqual => number_cmp(&left, &right, |a, c| a <= c, b),
            BinaryOp::Greater => number_cmp(&left, &right, |a, c| a > c, b),
            BinaryOp::GreaterEqual => number_cmp(&left, &right, |a, c| a >= c, b),
            BinaryOp::Equal => Ok(Value::Bool(left.is_equal(&right))),
            BinaryOp::NotEqual => Ok(Value::Bool(!left.is_equal(&right))),
        }
    }

    fn evaluate_call(&mut self, c: &CallExpr) -> Result<Value, LoxError> {
        let callee = self.evaluate_expr(&c.callee)?;

        let mut args = Vec::new();
        for arg in &c.arguments {
            args.push(self.evaluate_expr(arg)?);
        }

        match callee {
            Value::Function(func) => {
                if args.len() != func.arity() {
                    return Err(LoxError::runtime(
                        format!("expected {} arguments but got {}", func.arity(), args.len()),
                        c.span.offset,
                        c.span.len,
                    ));
                }
                self.call_function(&func, args)
            }
            Value::Class(class) => {
                let instance = Rc::new(RefCell::new(LoxInstance::new(Rc::clone(&class))));
                if let Some(init) = class.find_method("init") {
                    if args.len() != init.arity() {
                        return Err(LoxError::runtime(
                            format!("expected {} arguments but got {}", init.arity(), args.len()),
                            c.span.offset,
                            c.span.len,
                        ));
                    }
                    let bound = init.bind(Rc::clone(&instance));
                    self.call_function(&bound, args)?;
                } else if !args.is_empty() {
                    return Err(LoxError::runtime(
                        format!("expected 0 arguments but got {}", args.len()),
                        c.span.offset,
                        c.span.len,
                    ));
                }
                Ok(Value::Instance(instance))
            }
            _ => Err(LoxError::runtime(
                "can only call functions and classes",
                c.span.offset,
                c.span.len,
            )),
        }
    }

    fn call_function(&mut self, func: &Callable, args: Vec<Value>) -> Result<Value, LoxError> {
        match func {
            Callable::Native(native) => Ok(native.call(&args)),
            Callable::User(user_fn) => {
                let env = Rc::new(RefCell::new(Environment::with_enclosing(Rc::clone(
                    &user_fn.closure,
                ))));
                for (param, arg) in user_fn.declaration.params.iter().zip(args) {
                    env.borrow_mut().define(param.clone(), arg);
                }

                let result = self.execute_block(&user_fn.declaration.body, env);

                match result {
                    Ok(()) => {
                        if user_fn.is_initializer {
                            // Return 'this' from init
                            Ok(user_fn
                                .closure
                                .borrow()
                                .get_at(0, "this")
                                .expect("init closure has 'this'"))
                        } else {
                            Ok(Value::Nil)
                        }
                    }
                    Err(LoxError::Return { value }) => {
                        if user_fn.is_initializer {
                            Ok(user_fn
                                .closure
                                .borrow()
                                .get_at(0, "this")
                                .expect("init closure has 'this'"))
                        } else {
                            // Downcast the Box<dyn Any> back to Value
                            let val = value
                                .downcast::<Value>()
                                .expect("Return should contain Value");
                            Ok(*val)
                        }
                    }
                    Err(e) => Err(e),
                }
            }
        }
    }

    fn look_up_variable(
        &self,
        name: &str,
        id: ExprId,
        span: crate::scanner::token::Span,
    ) -> Result<Value, LoxError> {
        if let Some(&distance) = self.locals.get(&id) {
            Ok(self
                .environment
                .borrow()
                .get_at(distance, name)
                .expect("resolver guarantees variable exists"))
        } else {
            self.globals.borrow().get(name).ok_or_else(|| {
                LoxError::runtime(
                    format!("undefined variable '{name}'"),
                    span.offset,
                    span.len,
                )
            })
        }
    }
}

fn number_binop(
    left: &Value,
    right: &Value,
    op: fn(f64, f64) -> f64,
    b: &BinaryExpr,
) -> Result<Value, LoxError> {
    match (left, right) {
        (Value::Number(a), Value::Number(c)) => Ok(Value::Number(op(*a, *c))),
        _ => Err(LoxError::runtime(
            "operands must be numbers",
            b.span.offset,
            b.span.len,
        )),
    }
}

fn number_cmp(
    left: &Value,
    right: &Value,
    op: fn(f64, f64) -> bool,
    b: &BinaryExpr,
) -> Result<Value, LoxError> {
    match (left, right) {
        (Value::Number(a), Value::Number(c)) => Ok(Value::Bool(op(*a, *c))),
        _ => Err(LoxError::runtime(
            "operands must be numbers",
            b.span.offset,
            b.span.len,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interpreter::resolver::Resolver;
    use crate::parser::Parser;
    use crate::scanner;
    use rstest::rstest;

    fn run(source: &str) -> Vec<String> {
        let tokens = scanner::scan(source).expect("scan should succeed");
        let program = Parser::new(tokens).parse().expect("parse should succeed");
        let locals = Resolver::new()
            .resolve(&program)
            .expect("resolve should succeed");
        let mut interp = Interpreter::new_capturing();
        interp
            .interpret(&program, locals)
            .expect("interpret should succeed");
        interp.output.clone()
    }

    fn run_err(source: &str) -> LoxError {
        let tokens = scanner::scan(source).expect("scan should succeed");
        let program = Parser::new(tokens).parse().expect("parse should succeed");
        let locals = Resolver::new()
            .resolve(&program)
            .expect("resolve should succeed");
        let mut interp = Interpreter::new_capturing();
        interp.interpret(&program, locals).unwrap_err()
    }

    #[rstest]
    #[case("print 1 + 2;", "3")]
    #[case("print 10 - 3;", "7")]
    #[case("print 2 * 3;", "6")]
    #[case("print 10 / 4;", "2.5")]
    #[case("print -5;", "-5")]
    fn arithmetic(#[case] source: &str, #[case] expected: &str) {
        assert_eq!(run(source), vec![expected]);
    }

    #[rstest]
    #[case("print \"hello\" + \" world\";", "hello world")]
    fn string_concatenation(#[case] source: &str, #[case] expected: &str) {
        assert_eq!(run(source), vec![expected]);
    }

    #[test]
    fn truthiness() {
        assert_eq!(run("print !nil;"), vec!["true"]);
        assert_eq!(run("print !false;"), vec!["true"]);
        assert_eq!(run("print !0;"), vec!["false"]);
        assert_eq!(run("print !\"hello\";"), vec!["false"]);
    }

    #[test]
    fn equality() {
        assert_eq!(run("print 1 == 1;"), vec!["true"]);
        assert_eq!(run("print 1 == 2;"), vec!["false"]);
        assert_eq!(run("print nil == nil;"), vec!["true"]);
        assert_eq!(run("print 1 != 2;"), vec!["true"]);
    }

    #[test]
    fn variables() {
        assert_eq!(run("var x = 10; print x;"), vec!["10"]);
        assert_eq!(run("var x; print x;"), vec!["nil"]);
        assert_eq!(run("var x = 1; x = 2; print x;"), vec!["2"]);
    }

    #[test]
    fn blocks_and_scoping() {
        let output = run("var x = 1; { var x = 2; print x; } print x;");
        assert_eq!(output, vec!["2", "1"]);
    }

    #[test]
    fn if_else() {
        assert_eq!(run("if (true) print 1; else print 2;"), vec!["1"]);
        assert_eq!(run("if (false) print 1; else print 2;"), vec!["2"]);
    }

    #[test]
    fn while_loop() {
        let output = run("var i = 0; while (i < 3) { print i; i = i + 1; }");
        assert_eq!(output, vec!["0", "1", "2"]);
    }

    #[test]
    fn for_loop() {
        let output = run("for (var i = 0; i < 3; i = i + 1) print i;");
        assert_eq!(output, vec!["0", "1", "2"]);
    }

    #[test]
    fn functions() {
        let output = run("fun add(a, b) { return a + b; } print add(1, 2);");
        assert_eq!(output, vec!["3"]);
    }

    #[test]
    fn closures() {
        let output = run("fun makeCounter() {
                var i = 0;
                fun count() {
                    i = i + 1;
                    return i;
                }
                return count;
            }
            var counter = makeCounter();
            print counter();
            print counter();");
        assert_eq!(output, vec!["1", "2"]);
    }

    #[test]
    fn classes() {
        let output = run("class Foo {
                bar() { return 42; }
            }
            var foo = Foo();
            print foo.bar();");
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn class_fields() {
        let output = run("class Foo {}
            var foo = Foo();
            foo.x = 10;
            print foo.x;");
        assert_eq!(output, vec!["10"]);
    }

    #[test]
    fn class_this() {
        let output = run("class Foo {
                init(x) { this.x = x; }
                getX() { return this.x; }
            }
            var foo = Foo(42);
            print foo.getX();");
        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn inheritance() {
        let output = run("class Animal {
                speak() { return \"...\"; }
            }
            class Dog < Animal {
                speak() { return \"Woof!\"; }
            }
            var dog = Dog();
            print dog.speak();");
        assert_eq!(output, vec!["Woof!"]);
    }

    #[test]
    fn super_call() {
        let output = run("class A {
                greet() { return \"A\"; }
            }
            class B < A {
                greet() { return super.greet() + \"B\"; }
            }
            var b = B();
            print b.greet();");
        assert_eq!(output, vec!["AB"]);
    }

    #[test]
    fn logical_operators() {
        assert_eq!(run("print true or false;"), vec!["true"]);
        assert_eq!(run("print false and true;"), vec!["false"]);
        assert_eq!(run("print nil or \"yes\";"), vec!["yes"]);
    }

    #[test]
    fn undefined_variable_error() {
        let err = run_err("print x;");
        assert!(err.to_string().contains("undefined variable"));
    }

    #[test]
    fn wrong_arity_error() {
        let err = run_err("fun f(a) {} f(1, 2);");
        assert!(err.to_string().contains("expected 1 arguments"));
    }

    #[test]
    fn type_error_addition() {
        let err = run_err("print 1 + \"a\";");
        assert!(err.to_string().contains("operands must be"));
    }

    #[test]
    fn fibonacci() {
        let output = run("fun fib(n) {
                if (n <= 1) return n;
                return fib(n - 1) + fib(n - 2);
            }
            for (var i = 0; i < 10; i = i + 1) {
                print fib(i);
            }");
        assert_eq!(
            output,
            vec!["0", "1", "1", "2", "3", "5", "8", "13", "21", "34"]
        );
    }
}
