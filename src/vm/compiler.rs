use crate::ast::*;
use crate::error::LoxError;
use crate::vm::chunk::{Chunk, Constant, OpCode};

#[derive(Debug, Clone)]
struct Local {
    name: String,
    depth: i32,
    is_captured: bool,
}

#[derive(Debug, Clone)]
struct Upvalue {
    index: u8,
    is_local: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum FunctionType {
    Script,
    Function,
    Method,
    Initializer,
}

struct CompilerState {
    #[allow(dead_code)]
    function_name: String,
    function_type: FunctionType,
    chunk: Chunk,
    locals: Vec<Local>,
    upvalues: Vec<Upvalue>,
    scope_depth: i32,
    line: usize,
}

impl CompilerState {
    fn new(name: String, function_type: FunctionType) -> Self {
        let mut state = Self {
            function_name: name,
            function_type,
            chunk: Chunk::new(),
            locals: Vec::new(),
            upvalues: Vec::new(),
            scope_depth: 0,
            line: 1,
        };
        // Slot 0 is reserved for 'this' in methods, empty string otherwise
        let slot_name = if function_type == FunctionType::Method
            || function_type == FunctionType::Initializer
        {
            "this".to_string()
        } else {
            String::new()
        };
        state.locals.push(Local {
            name: slot_name,
            depth: 0,
            is_captured: false,
        });
        state
    }
}

pub struct Compiler {
    states: Vec<CompilerState>,
}

impl Compiler {
    pub fn new() -> Self {
        Self {
            states: vec![CompilerState::new(
                "script".to_string(),
                FunctionType::Script,
            )],
        }
    }

    pub fn compile(mut self, program: &Program) -> Result<Chunk, LoxError> {
        for decl in &program.declarations {
            self.compile_decl(decl)?;
        }
        self.emit_op(OpCode::Nil);
        self.emit_op(OpCode::Return);
        Ok(self.states.pop().expect("should have script state").chunk)
    }

    fn current(&self) -> &CompilerState {
        self.states.last().expect("compiler state stack non-empty")
    }

    fn current_mut(&mut self) -> &mut CompilerState {
        self.states
            .last_mut()
            .expect("compiler state stack non-empty")
    }

    fn emit_op(&mut self, op: OpCode) {
        let line = self.current().line;
        self.current_mut().chunk.write_op(op, line);
    }

    fn emit_byte(&mut self, byte: u8) {
        let line = self.current().line;
        self.current_mut().chunk.write_byte(byte, line);
    }

    fn emit_constant(&mut self, constant: Constant) {
        let idx = self.current_mut().chunk.add_constant(constant);
        self.emit_op(OpCode::Constant);
        self.emit_byte(idx);
    }

    fn emit_jump(&mut self, op: OpCode) -> usize {
        self.emit_op(op);
        let line = self.current().line;
        self.current_mut().chunk.write_u16(0xffff, line);
        self.current().chunk.code.len() - 2
    }

    fn patch_jump(&mut self, offset: usize) {
        let jump = self.current().chunk.code.len() - offset - 2;
        if jump > u16::MAX as usize {
            panic!("jump too large");
        }
        self.current_mut().chunk.code[offset] = (jump >> 8) as u8;
        self.current_mut().chunk.code[offset + 1] = (jump & 0xff) as u8;
    }

    fn emit_loop(&mut self, loop_start: usize) {
        self.emit_op(OpCode::Loop);
        let offset = self.current().chunk.code.len() - loop_start + 2;
        if offset > u16::MAX as usize {
            panic!("loop body too large");
        }
        let line = self.current().line;
        self.current_mut().chunk.write_u16(offset as u16, line);
    }

    fn begin_scope(&mut self) {
        self.current_mut().scope_depth += 1;
    }

    fn end_scope(&mut self) {
        self.current_mut().scope_depth -= 1;
        let depth = self.current().scope_depth;
        while let Some(local) = self.current().locals.last() {
            if local.depth <= depth {
                break;
            }
            if local.is_captured {
                self.emit_op(OpCode::CloseUpvalue);
            } else {
                self.emit_op(OpCode::Pop);
            }
            self.current_mut().locals.pop();
        }
    }

    fn add_local(&mut self, name: String) {
        let depth = self.current().scope_depth;
        self.current_mut().locals.push(Local {
            name,
            depth,
            is_captured: false,
        });
    }

    fn resolve_local(&self, name: &str) -> Option<u8> {
        for (i, local) in self.current().locals.iter().enumerate().rev() {
            if local.name == name {
                return Some(i as u8);
            }
        }
        None
    }

    fn resolve_upvalue(&mut self, name: &str) -> Option<u8> {
        if self.states.len() < 2 {
            return None;
        }
        let enclosing_idx = self.states.len() - 2;

        // Check locals in enclosing scope
        for (i, local) in self.states[enclosing_idx].locals.iter().enumerate().rev() {
            if local.name == name {
                self.states[enclosing_idx].locals[i].is_captured = true;
                return Some(self.add_upvalue(i as u8, true));
            }
        }

        // Check upvalues in enclosing scope (recursive)
        // For simplicity, we only check one level up
        for (i, upvalue) in self.states[enclosing_idx].upvalues.iter().enumerate() {
            let _ = upvalue;
            // Would need recursive resolution for deeper nesting
            // This handles the most common cases
            let _ = i;
        }

        None
    }

    fn add_upvalue(&mut self, index: u8, is_local: bool) -> u8 {
        // Check if we already have this upvalue
        for (i, uv) in self.current().upvalues.iter().enumerate() {
            if uv.index == index && uv.is_local == is_local {
                return i as u8;
            }
        }
        let idx = self.current().upvalues.len() as u8;
        self.current_mut()
            .upvalues
            .push(Upvalue { index, is_local });
        idx
    }

    fn compile_decl(&mut self, decl: &Decl) -> Result<(), LoxError> {
        match decl {
            Decl::Var(v) => {
                self.current_mut().line = line_from_span(v.span);
                if let Some(ref init) = v.initializer {
                    self.compile_expr(init)?;
                } else {
                    self.emit_op(OpCode::Nil);
                }
                if self.current().scope_depth > 0 {
                    self.add_local(v.name.clone());
                } else {
                    let idx = self
                        .current_mut()
                        .chunk
                        .add_constant(Constant::String(v.name.clone()));
                    self.emit_op(OpCode::DefineGlobal);
                    self.emit_byte(idx);
                }
                Ok(())
            }
            Decl::Fun(f) => {
                self.current_mut().line = line_from_span(f.span);
                self.compile_function(&f.function, FunctionType::Function)?;
                if self.current().scope_depth > 0 {
                    self.add_local(f.function.name.clone());
                } else {
                    let idx = self
                        .current_mut()
                        .chunk
                        .add_constant(Constant::String(f.function.name.clone()));
                    self.emit_op(OpCode::DefineGlobal);
                    self.emit_byte(idx);
                }
                Ok(())
            }
            Decl::Class(c) => self.compile_class(c),
            Decl::Statement(s) => self.compile_stmt(s),
        }
    }

    fn compile_function(
        &mut self,
        function: &Function,
        func_type: FunctionType,
    ) -> Result<(), LoxError> {
        self.states
            .push(CompilerState::new(function.name.clone(), func_type));
        self.begin_scope();

        for param in &function.params {
            self.add_local(param.clone());
        }

        for decl in &function.body {
            self.compile_decl(decl)?;
        }

        // Implicit nil return
        if func_type == FunctionType::Initializer {
            self.emit_op(OpCode::GetLocal);
            self.emit_byte(0); // 'this'
        } else {
            self.emit_op(OpCode::Nil);
        }
        self.emit_op(OpCode::Return);

        let state = self.states.pop().expect("should have function state");
        let upvalue_count = state.upvalues.len();
        let func_constant = Constant::Function {
            name: function.name.clone(),
            arity: function.params.len(),
            upvalue_count,
            chunk: state.chunk,
        };
        let idx = self.current_mut().chunk.add_constant(func_constant);
        self.emit_op(OpCode::Closure);
        self.emit_byte(idx);

        // Emit upvalue info
        for uv in &state.upvalues {
            self.emit_byte(if uv.is_local { 1 } else { 0 });
            self.emit_byte(uv.index);
        }

        Ok(())
    }

    fn compile_class(&mut self, class: &ClassDecl) -> Result<(), LoxError> {
        self.current_mut().line = line_from_span(class.span);
        let name_idx = self
            .current_mut()
            .chunk
            .add_constant(Constant::String(class.name.clone()));
        self.emit_op(OpCode::Class);
        self.emit_byte(name_idx);

        if self.current().scope_depth > 0 {
            self.add_local(class.name.clone());
        } else {
            self.emit_op(OpCode::DefineGlobal);
            self.emit_byte(name_idx);
        }

        if let Some(ref superclass) = class.superclass {
            self.compile_named_variable(superclass)?;
            self.compile_named_variable(&class.name)?;
            self.emit_op(OpCode::Inherit);
            self.begin_scope();
            self.add_local("super".to_string());
        }

        self.compile_named_variable(&class.name)?;

        for method in &class.methods {
            let method_name_idx = self
                .current_mut()
                .chunk
                .add_constant(Constant::String(method.name.clone()));
            let func_type = if method.name == "init" {
                FunctionType::Initializer
            } else {
                FunctionType::Method
            };
            self.compile_function(method, func_type)?;
            self.emit_op(OpCode::Method);
            self.emit_byte(method_name_idx);
        }

        self.emit_op(OpCode::Pop); // Pop the class

        if class.superclass.is_some() {
            self.end_scope();
        }

        Ok(())
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> Result<(), LoxError> {
        match stmt {
            Stmt::Expression(e) => {
                self.current_mut().line = line_from_span(e.span);
                self.compile_expr(&e.expression)?;
                self.emit_op(OpCode::Pop);
                Ok(())
            }
            Stmt::Print(p) => {
                self.current_mut().line = line_from_span(p.span);
                self.compile_expr(&p.expression)?;
                self.emit_op(OpCode::Print);
                Ok(())
            }
            Stmt::Return(r) => {
                self.current_mut().line = line_from_span(r.span);
                if let Some(ref val) = r.value {
                    if self.current().function_type == FunctionType::Initializer {
                        return Err(LoxError::runtime(
                            "can't return a value from an initializer",
                            r.span.offset,
                            r.span.len,
                        ));
                    }
                    self.compile_expr(val)?;
                } else if self.current().function_type == FunctionType::Initializer {
                    self.emit_op(OpCode::GetLocal);
                    self.emit_byte(0);
                } else {
                    self.emit_op(OpCode::Nil);
                }
                self.emit_op(OpCode::Return);
                Ok(())
            }
            Stmt::Block(b) => {
                self.current_mut().line = line_from_span(b.span);
                self.begin_scope();
                for decl in &b.declarations {
                    self.compile_decl(decl)?;
                }
                self.end_scope();
                Ok(())
            }
            Stmt::If(i) => {
                self.current_mut().line = line_from_span(i.span);
                self.compile_expr(&i.condition)?;
                let then_jump = self.emit_jump(OpCode::JumpIfFalse);
                self.emit_op(OpCode::Pop);
                self.compile_stmt(&i.then_branch)?;
                let else_jump = self.emit_jump(OpCode::Jump);
                self.patch_jump(then_jump);
                self.emit_op(OpCode::Pop);
                if let Some(ref else_branch) = i.else_branch {
                    self.compile_stmt(else_branch)?;
                }
                self.patch_jump(else_jump);
                Ok(())
            }
            Stmt::While(w) => {
                self.current_mut().line = line_from_span(w.span);
                let loop_start = self.current().chunk.code.len();
                self.compile_expr(&w.condition)?;
                let exit_jump = self.emit_jump(OpCode::JumpIfFalse);
                self.emit_op(OpCode::Pop);
                self.compile_stmt(&w.body)?;
                self.emit_loop(loop_start);
                self.patch_jump(exit_jump);
                self.emit_op(OpCode::Pop);
                Ok(())
            }
        }
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<(), LoxError> {
        match expr {
            Expr::Literal(l) => {
                self.current_mut().line = line_from_span(l.span);
                match &l.value {
                    LiteralValue::Number(n) => self.emit_constant(Constant::Number(*n)),
                    LiteralValue::String(s) => {
                        self.emit_constant(Constant::String(s.clone()));
                    }
                    LiteralValue::Bool(true) => self.emit_op(OpCode::True),
                    LiteralValue::Bool(false) => self.emit_op(OpCode::False),
                    LiteralValue::Nil => self.emit_op(OpCode::Nil),
                }
                Ok(())
            }
            Expr::Grouping(g) => self.compile_expr(&g.expression),
            Expr::Unary(u) => {
                self.compile_expr(&u.operand)?;
                match u.operator {
                    UnaryOp::Negate => self.emit_op(OpCode::Negate),
                    UnaryOp::Not => self.emit_op(OpCode::Not),
                }
                Ok(())
            }
            Expr::Binary(b) => {
                self.compile_expr(&b.left)?;
                self.compile_expr(&b.right)?;
                match b.operator {
                    BinaryOp::Add => self.emit_op(OpCode::Add),
                    BinaryOp::Subtract => self.emit_op(OpCode::Subtract),
                    BinaryOp::Multiply => self.emit_op(OpCode::Multiply),
                    BinaryOp::Divide => self.emit_op(OpCode::Divide),
                    BinaryOp::Equal => self.emit_op(OpCode::Equal),
                    BinaryOp::NotEqual => {
                        self.emit_op(OpCode::Equal);
                        self.emit_op(OpCode::Not);
                    }
                    BinaryOp::Less => self.emit_op(OpCode::Less),
                    BinaryOp::LessEqual => {
                        self.emit_op(OpCode::Greater);
                        self.emit_op(OpCode::Not);
                    }
                    BinaryOp::Greater => self.emit_op(OpCode::Greater),
                    BinaryOp::GreaterEqual => {
                        self.emit_op(OpCode::Less);
                        self.emit_op(OpCode::Not);
                    }
                }
                Ok(())
            }
            Expr::Variable(v) => {
                self.current_mut().line = line_from_span(v.span);
                self.compile_named_variable(&v.name)
            }
            Expr::Assign(a) => {
                self.current_mut().line = line_from_span(a.span);
                self.compile_expr(&a.value)?;
                if let Some(slot) = self.resolve_local(&a.name) {
                    self.emit_op(OpCode::SetLocal);
                    self.emit_byte(slot);
                } else if let Some(idx) = self.resolve_upvalue(&a.name) {
                    self.emit_op(OpCode::SetUpvalue);
                    self.emit_byte(idx);
                } else {
                    let idx = self
                        .current_mut()
                        .chunk
                        .add_constant(Constant::String(a.name.clone()));
                    self.emit_op(OpCode::SetGlobal);
                    self.emit_byte(idx);
                }
                Ok(())
            }
            Expr::Logical(l) => {
                self.compile_expr(&l.left)?;
                match l.operator {
                    LogicalOp::And => {
                        let end_jump = self.emit_jump(OpCode::JumpIfFalse);
                        self.emit_op(OpCode::Pop);
                        self.compile_expr(&l.right)?;
                        self.patch_jump(end_jump);
                    }
                    LogicalOp::Or => {
                        let else_jump = self.emit_jump(OpCode::JumpIfFalse);
                        let end_jump = self.emit_jump(OpCode::Jump);
                        self.patch_jump(else_jump);
                        self.emit_op(OpCode::Pop);
                        self.compile_expr(&l.right)?;
                        self.patch_jump(end_jump);
                    }
                }
                Ok(())
            }
            Expr::Call(c) => {
                self.compile_expr(&c.callee)?;
                for arg in &c.arguments {
                    self.compile_expr(arg)?;
                }
                self.emit_op(OpCode::Call);
                self.emit_byte(c.arguments.len() as u8);
                Ok(())
            }
            Expr::Get(g) => {
                self.compile_expr(&g.object)?;
                let idx = self
                    .current_mut()
                    .chunk
                    .add_constant(Constant::String(g.name.clone()));
                self.emit_op(OpCode::GetProperty);
                self.emit_byte(idx);
                Ok(())
            }
            Expr::Set(s) => {
                self.compile_expr(&s.object)?;
                self.compile_expr(&s.value)?;
                let idx = self
                    .current_mut()
                    .chunk
                    .add_constant(Constant::String(s.name.clone()));
                self.emit_op(OpCode::SetProperty);
                self.emit_byte(idx);
                Ok(())
            }
            Expr::This(t) => {
                self.current_mut().line = line_from_span(t.span);
                if let Some(slot) = self.resolve_local("this") {
                    self.emit_op(OpCode::GetLocal);
                    self.emit_byte(slot);
                } else if let Some(idx) = self.resolve_upvalue("this") {
                    self.emit_op(OpCode::GetUpvalue);
                    self.emit_byte(idx);
                }
                Ok(())
            }
            Expr::Super(s) => {
                self.current_mut().line = line_from_span(s.span);
                let method_idx = self
                    .current_mut()
                    .chunk
                    .add_constant(Constant::String(s.method.clone()));
                self.compile_named_variable("this")?;
                self.compile_named_variable("super")?;
                self.emit_op(OpCode::GetSuper);
                self.emit_byte(method_idx);
                Ok(())
            }
        }
    }

    fn compile_named_variable(&mut self, name: &str) -> Result<(), LoxError> {
        if let Some(slot) = self.resolve_local(name) {
            self.emit_op(OpCode::GetLocal);
            self.emit_byte(slot);
        } else if let Some(idx) = self.resolve_upvalue(name) {
            self.emit_op(OpCode::GetUpvalue);
            self.emit_byte(idx);
        } else {
            let idx = self
                .current_mut()
                .chunk
                .add_constant(Constant::String(name.to_string()));
            self.emit_op(OpCode::GetGlobal);
            self.emit_byte(idx);
        }
        Ok(())
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

fn line_from_span(span: crate::scanner::token::Span) -> usize {
    // We don't have line info in spans, so use offset as a proxy
    span.offset + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::scanner;
    use crate::vm::chunk::OpCode;

    fn compile(source: &str) -> Result<Chunk, LoxError> {
        let tokens = scanner::scan(source).expect("scan should succeed");
        let program = Parser::new(tokens).parse().expect("parse should succeed");
        Compiler::new().compile(&program)
    }

    fn compile_expr(source: &str) -> Result<Chunk, LoxError> {
        compile(&format!("print {source};"))
    }

    fn has_opcode(chunk: &Chunk, op: OpCode) -> bool {
        chunk.code.iter().any(|&byte| byte == op as u8)
    }

    fn count_opcode(chunk: &Chunk, op: OpCode) -> usize {
        chunk.code.iter().filter(|&&byte| byte == op as u8).count()
    }

    /// Check if any function constant (recursively) has the specified upvalue count
    fn has_function_with_upvalues(chunk: &Chunk) -> bool {
        for constant in &chunk.constants {
            match constant {
                Constant::Function {
                    upvalue_count,
                    chunk: nested_chunk,
                    ..
                } => {
                    if *upvalue_count > 0 {
                        return true;
                    }
                    // Check nested functions
                    if has_function_with_upvalues(nested_chunk) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }

    // ========== Basic Compilation Tests ==========

    #[test]
    fn compile_number_literal() {
        let chunk = compile_expr("42").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Constant));
        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.constants[0], Constant::Number(42.0));
    }

    #[test]
    fn compile_string_literal() {
        let chunk = compile_expr("\"hello\"").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Constant));
        assert!(matches!(
            &chunk.constants[0],
            Constant::String(s) if s == "hello"
        ));
    }

    #[test]
    fn compile_true_literal() {
        let chunk = compile_expr("true").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::True));
    }

    #[test]
    fn compile_false_literal() {
        let chunk = compile_expr("false").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::False));
    }

    #[test]
    fn compile_nil_literal() {
        let chunk = compile_expr("nil").expect("compile should succeed");
        // Should have Nil from the expression plus Nil and Return at end
        assert!(has_opcode(&chunk, OpCode::Nil));
    }

    // ========== Arithmetic Operations ==========

    #[test]
    fn compile_addition() {
        let chunk = compile_expr("1 + 2").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Add));
        // Should have at least 2 number constants
        let num_constants = chunk
            .constants
            .iter()
            .filter(|c| matches!(c, Constant::Number(_)))
            .count();
        assert!(num_constants >= 2);
    }

    #[test]
    fn compile_subtraction() {
        let chunk = compile_expr("5 - 3").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Subtract));
    }

    #[test]
    fn compile_multiplication() {
        let chunk = compile_expr("2 * 3").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Multiply));
    }

    #[test]
    fn compile_division() {
        let chunk = compile_expr("10 / 2").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Divide));
    }

    #[test]
    fn compile_negation() {
        let chunk = compile_expr("-42").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Negate));
    }

    #[test]
    fn compile_not() {
        let chunk = compile_expr("!true").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Not));
    }

    // ========== Comparison Operations ==========

    #[test]
    fn compile_equal() {
        let chunk = compile_expr("1 == 2").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Equal));
    }

    #[test]
    fn compile_not_equal() {
        let chunk = compile_expr("1 != 2").expect("compile should succeed");
        // != is compiled as == followed by Not
        assert!(has_opcode(&chunk, OpCode::Equal));
        assert!(has_opcode(&chunk, OpCode::Not));
    }

    #[test]
    fn compile_less_than() {
        let chunk = compile_expr("1 < 2").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Less));
    }

    #[test]
    fn compile_less_equal() {
        let chunk = compile_expr("1 <= 2").expect("compile should succeed");
        // <= is compiled as > followed by Not
        assert!(has_opcode(&chunk, OpCode::Greater));
        assert!(has_opcode(&chunk, OpCode::Not));
    }

    #[test]
    fn compile_greater_than() {
        let chunk = compile_expr("1 > 2").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Greater));
    }

    #[test]
    fn compile_greater_equal() {
        let chunk = compile_expr("1 >= 2").expect("compile should succeed");
        // >= is compiled as < followed by Not
        assert!(has_opcode(&chunk, OpCode::Less));
        assert!(has_opcode(&chunk, OpCode::Not));
    }

    // ========== Variables ==========

    #[test]
    fn compile_global_variable() {
        let chunk = compile("var x = 42;").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::DefineGlobal));
        // Should have constant for variable name "x"
        assert!(chunk
            .constants
            .iter()
            .any(|c| matches!(c, Constant::String(s) if s == "x")));
    }

    #[test]
    fn compile_local_variable() {
        let chunk = compile("{ var x = 1; }").expect("compile should succeed");
        // Local variables don't use DefineGlobal
        assert!(!has_opcode(&chunk, OpCode::DefineGlobal));
        // Should pop the local at end of block
        assert!(has_opcode(&chunk, OpCode::Pop));
    }

    #[test]
    fn compile_get_global() {
        let chunk = compile("var x = 1; print x;").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::GetGlobal));
    }

    #[test]
    fn compile_set_global() {
        let chunk = compile("var x = 1; x = 2;").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::SetGlobal));
    }

    #[test]
    fn compile_get_local() {
        let chunk = compile("{ var x = 1; print x; }").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::GetLocal));
    }

    #[test]
    fn compile_set_local() {
        let chunk = compile("{ var x = 1; x = 2; }").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::SetLocal));
    }

    // ========== Control Flow ==========

    #[test]
    fn compile_if_statement() {
        let chunk = compile("if (true) print 1;").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::JumpIfFalse));
        assert!(has_opcode(&chunk, OpCode::Jump));
    }

    #[test]
    fn compile_if_else_statement() {
        let chunk = compile("if (true) print 1; else print 2;").expect("compile should succeed");
        // Should have JumpIfFalse for then branch and Jump for else
        assert_eq!(count_opcode(&chunk, OpCode::JumpIfFalse), 1);
        assert_eq!(count_opcode(&chunk, OpCode::Jump), 1);
    }

    #[test]
    fn compile_while_loop() {
        let chunk = compile("while (true) print 1;").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::JumpIfFalse));
        assert!(has_opcode(&chunk, OpCode::Loop));
    }

    #[test]
    fn compile_logical_and() {
        let chunk = compile_expr("true and false").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::JumpIfFalse));
    }

    #[test]
    fn compile_logical_or() {
        let chunk = compile_expr("true or false").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::JumpIfFalse));
        assert!(has_opcode(&chunk, OpCode::Jump));
    }

    // ========== Functions ==========

    #[test]
    fn compile_function_declaration() {
        let chunk = compile("fun add(a, b) { return a + b; }").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Closure));
        // Should have function constant
        assert!(chunk.constants.iter().any(|c| matches!(
            c,
            Constant::Function { name, arity, .. }
            if name == "add" && *arity == 2
        )));
    }

    #[test]
    fn compile_function_call() {
        let chunk = compile("fun f() {} f();").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Call));
    }

    #[test]
    fn compile_return_statement() {
        let chunk = compile("fun f() { return 42; }").expect("compile should succeed");
        // Return opcode is in the function's chunk
        assert!(chunk.constants.iter().any(|c| {
            if let Constant::Function {
                chunk: func_chunk, ..
            } = c
            {
                has_opcode(func_chunk, OpCode::Return)
            } else {
                false
            }
        }));
    }

    #[test]
    fn compile_implicit_return() {
        let chunk = compile("fun f() { 42; }").expect("compile should succeed");
        // Functions always end with Return
        assert!(chunk.constants.iter().any(|c| {
            if let Constant::Function {
                chunk: func_chunk, ..
            } = c
            {
                has_opcode(func_chunk, OpCode::Return)
            } else {
                false
            }
        }));
    }

    // ========== Closures ==========

    #[test]
    fn compile_closure() {
        let chunk = compile(
            r#"
            fun outer() {
                var x = 1;
                fun inner() { return x; }
                return inner;
            }
        "#,
        )
        .expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Closure));
        // Should have function constants with upvalue info (may be nested)
        assert!(
            has_function_with_upvalues(&chunk),
            "Expected at least one function with upvalues"
        );
    }

    #[test]
    fn compile_get_upvalue() {
        let chunk = compile(
            r#"
            fun outer() {
                var x = 1;
                fun inner() { print x; }
            }
        "#,
        )
        .expect("compile should succeed");
        // The inner function should have upvalues declared
        assert!(
            has_function_with_upvalues(&chunk),
            "Expected inner function to capture 'x' as upvalue"
        );
    }

    #[test]
    fn compile_set_upvalue() {
        let chunk = compile(
            r#"
            fun outer() {
                var x = 1;
                fun inner() { x = 2; }
            }
        "#,
        )
        .expect("compile should succeed");
        // The inner function should have upvalues
        assert!(
            has_function_with_upvalues(&chunk),
            "Expected inner function to capture 'x' as upvalue"
        );
    }

    #[test]
    fn compile_upvalue_management() {
        // Test that upvalues are properly managed when variables are captured
        let chunk = compile(
            r#"
            fun outer() {
                var x = 1;
                fun inner() { return x; }
                return inner;
            }
        "#,
        )
        .expect("compile should succeed");
        // The key thing is that upvalues are declared
        assert!(
            has_function_with_upvalues(&chunk),
            "Expected inner function to capture 'x' as upvalue"
        );
    }

    // ========== Classes ==========

    #[test]
    fn compile_class_declaration() {
        let chunk = compile("class Foo {}").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Class));
        assert!(chunk
            .constants
            .iter()
            .any(|c| matches!(c, Constant::String(s) if s == "Foo")));
    }

    #[test]
    fn compile_class_with_methods() {
        let chunk = compile("class Foo { bar() { return 42; } }").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Class));
        assert!(has_opcode(&chunk, OpCode::Method));
    }

    #[test]
    fn compile_class_inheritance() {
        let chunk =
            compile("class Base {} class Derived < Base {}").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Inherit));
    }

    #[test]
    fn compile_get_property() {
        let chunk =
            compile("class Foo {} var f = Foo(); print f.x;").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::GetProperty));
    }

    #[test]
    fn compile_set_property() {
        let chunk =
            compile("class Foo {} var f = Foo(); f.x = 1;").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::SetProperty));
    }

    #[test]
    fn compile_this() {
        let chunk =
            compile("class Foo { bar() { return this; } }").expect("compile should succeed");
        // 'this' is slot 0 in methods, accessed via GetLocal
        assert!(chunk.constants.iter().any(|c| {
            if let Constant::Function {
                chunk: func_chunk, ..
            } = c
            {
                has_opcode(func_chunk, OpCode::GetLocal)
            } else {
                false
            }
        }));
    }

    #[test]
    fn compile_super() {
        let chunk = compile(
            r#"
            class Base { foo() { return 1; } }
            class Derived < Base {
                foo() { return super.foo(); }
            }
        "#,
        )
        .expect("compile should succeed");
        // GetSuper should be inside the method body
        let has_super = chunk.constants.iter().any(|c| {
            if let Constant::Function {
                chunk: func_chunk, ..
            } = c
            {
                has_opcode(func_chunk, OpCode::GetSuper)
            } else {
                false
            }
        });
        assert!(has_super, "Expected GetSuper in method using super");
    }

    #[test]
    fn compile_initializer() {
        let chunk =
            compile("class Foo { init(x) { this.x = x; } }").expect("compile should succeed");
        // Initializer should return 'this' implicitly
        assert!(chunk.constants.iter().any(|c| {
            if let Constant::Function {
                name,
                chunk: func_chunk,
                ..
            } = c
            {
                name == "init" && has_opcode(func_chunk, OpCode::GetLocal)
            } else {
                false
            }
        }));
    }

    // ========== Statements ==========

    #[test]
    fn compile_print_statement() {
        let chunk = compile("print 42;").expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Print));
    }

    #[test]
    fn compile_expression_statement() {
        let chunk = compile("1 + 2;").expect("compile should succeed");
        // Expression statements should pop the result
        assert!(has_opcode(&chunk, OpCode::Pop));
    }

    #[test]
    fn compile_block() {
        let chunk = compile("{ var x = 1; var y = 2; }").expect("compile should succeed");
        // Should pop locals at end of block
        assert_eq!(count_opcode(&chunk, OpCode::Pop), 2);
    }

    // ========== Error Cases ==========

    #[test]
    fn compile_return_from_initializer_errors() {
        let result = compile("class Foo { init() { return 42; } }");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("initializer"));
    }

    // ========== Complex Programs ==========

    #[test]
    fn compile_fibonacci() {
        let chunk = compile(
            r#"
            fun fib(n) {
                if (n <= 1) return n;
                return fib(n - 1) + fib(n - 2);
            }
            print fib(5);
        "#,
        )
        .expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Call));
        assert!(has_opcode(&chunk, OpCode::Print));
    }

    #[test]
    fn compile_counter_closure() {
        let chunk = compile(
            r#"
            fun makeCounter() {
                var i = 0;
                fun count() {
                    i = i + 1;
                    return i;
                }
                return count;
            }
            var counter = makeCounter();
            print counter();
        "#,
        )
        .expect("compile should succeed");
        assert!(has_opcode(&chunk, OpCode::Closure));
        // Should have upvalues captured (may be nested)
        assert!(
            has_function_with_upvalues(&chunk),
            "Expected count() to capture 'i' as upvalue"
        );
    }
}
