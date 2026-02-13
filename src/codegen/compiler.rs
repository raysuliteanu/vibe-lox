use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::values::{BasicValueEnum, FunctionValue, StructValue};

use crate::ast::{
    AssignExpr, BinaryExpr, BinaryOp, BlockStmt, Decl, Expr, ExprStmt, IfStmt, LiteralExpr,
    LiteralValue, LogicalExpr, LogicalOp, PrintStmt, Program, Stmt, UnaryExpr, UnaryOp, VarDecl,
    VariableExpr, WhileStmt,
};

use super::runtime::RuntimeDecls;
use super::types::LoxValueType;

/// LLVM IR code generator for Lox programs.
///
/// Walks the AST and emits LLVM IR using inkwell. In Phase 1 this handles
/// literals, arithmetic, comparisons, unary ops, print, and global variables.
pub struct CodeGen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    lox_value: LoxValueType<'ctx>,
    runtime: RuntimeDecls<'ctx>,
    /// The current function being compiled into.
    current_fn: Option<FunctionValue<'ctx>>,
}

impl<'ctx> CodeGen<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        let lox_value = LoxValueType::new(context);
        let runtime = RuntimeDecls::declare(&module, &lox_value);
        Self {
            context,
            module,
            builder,
            lox_value,
            runtime,
            current_fn: None,
        }
    }

    /// Compile a Lox program to LLVM IR and return the IR as a string.
    pub fn compile(mut self, program: &Program) -> anyhow::Result<String> {
        self.emit_main(program)?;
        Ok(self.module.print_to_string().to_string())
    }

    fn emit_main(&mut self, program: &Program) -> anyhow::Result<()> {
        let i32_type = self.context.i32_type();
        let main_fn_type = i32_type.fn_type(&[], false);
        let main_fn = self.module.add_function("main", main_fn_type, None);
        let entry = self.context.append_basic_block(main_fn, "entry");
        self.builder.position_at_end(entry);
        self.current_fn = Some(main_fn);

        for decl in &program.declarations {
            self.compile_decl(decl)?;
        }

        // return 0
        self.builder
            .build_return(Some(&i32_type.const_int(0, false)))
            .expect("build return from main");
        Ok(())
    }

    fn compile_decl(&mut self, decl: &Decl) -> anyhow::Result<()> {
        match decl {
            Decl::Var(var_decl) => self.compile_var_decl(var_decl),
            Decl::Statement(stmt) => self.compile_stmt(stmt),
            Decl::Fun(_) => {
                anyhow::bail!("function declarations not yet supported in LLVM codegen")
            }
            Decl::Class(_) => {
                anyhow::bail!("class declarations not yet supported in LLVM codegen")
            }
        }
    }

    fn compile_var_decl(&mut self, var_decl: &VarDecl) -> anyhow::Result<()> {
        let value = match &var_decl.initializer {
            Some(expr) => self.compile_expr(expr)?,
            None => self.lox_value.build_nil(&self.builder),
        };
        self.emit_global_set(&var_decl.name, value);
        Ok(())
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> anyhow::Result<()> {
        match stmt {
            Stmt::Print(print_stmt) => self.compile_print_stmt(print_stmt),
            Stmt::Expression(expr_stmt) => self.compile_expr_stmt(expr_stmt),
            Stmt::Block(block) => self.compile_block(block),
            Stmt::If(if_stmt) => self.compile_if(if_stmt),
            Stmt::While(while_stmt) => self.compile_while(while_stmt),
            Stmt::Return(_) => {
                anyhow::bail!("return statements not yet supported in LLVM codegen")
            }
        }
    }

    fn compile_print_stmt(&mut self, stmt: &PrintStmt) -> anyhow::Result<()> {
        let value = self.compile_expr(&stmt.expression)?;
        self.builder
            .build_call(self.runtime.lox_print, &[value.into()], "")
            .expect("call lox_print");
        Ok(())
    }

    fn compile_expr_stmt(&mut self, stmt: &ExprStmt) -> anyhow::Result<()> {
        self.compile_expr(&stmt.expression)?;
        Ok(())
    }

    fn compile_block(&mut self, block: &BlockStmt) -> anyhow::Result<()> {
        for decl in &block.declarations {
            self.compile_decl(decl)?;
        }
        Ok(())
    }

    fn compile_if(&mut self, if_stmt: &IfStmt) -> anyhow::Result<()> {
        let current_fn = self.current_fn.expect("must be inside a function");

        // Evaluate condition and convert to i1 via lox_value_truthy
        let condition = self.compile_expr(&if_stmt.condition)?;
        let cond_bool = self.emit_truthy(condition);

        let then_bb = self.context.append_basic_block(current_fn, "then");
        let merge_bb = self.context.append_basic_block(current_fn, "merge");

        if let Some(else_branch) = &if_stmt.else_branch {
            let else_bb = self.context.append_basic_block(current_fn, "else");
            self.builder
                .build_conditional_branch(cond_bool, then_bb, else_bb)
                .expect("conditional branch");

            // Then branch
            self.builder.position_at_end(then_bb);
            self.compile_stmt(&if_stmt.then_branch)?;
            self.builder
                .build_unconditional_branch(merge_bb)
                .expect("branch to merge from then");

            // Else branch
            self.builder.position_at_end(else_bb);
            self.compile_stmt(else_branch)?;
            self.builder
                .build_unconditional_branch(merge_bb)
                .expect("branch to merge from else");
        } else {
            self.builder
                .build_conditional_branch(cond_bool, then_bb, merge_bb)
                .expect("conditional branch");

            // Then branch
            self.builder.position_at_end(then_bb);
            self.compile_stmt(&if_stmt.then_branch)?;
            self.builder
                .build_unconditional_branch(merge_bb)
                .expect("branch to merge from then");
        }

        // Continue at merge point
        self.builder.position_at_end(merge_bb);
        Ok(())
    }

    fn compile_while(&mut self, while_stmt: &WhileStmt) -> anyhow::Result<()> {
        let current_fn = self.current_fn.expect("must be inside a function");

        let cond_bb = self.context.append_basic_block(current_fn, "while_cond");
        let body_bb = self.context.append_basic_block(current_fn, "while_body");
        let exit_bb = self.context.append_basic_block(current_fn, "while_exit");

        // Jump to condition check
        self.builder
            .build_unconditional_branch(cond_bb)
            .expect("branch to while condition");

        // Condition block
        self.builder.position_at_end(cond_bb);
        let condition = self.compile_expr(&while_stmt.condition)?;
        let cond_bool = self.emit_truthy(condition);
        self.builder
            .build_conditional_branch(cond_bool, body_bb, exit_bb)
            .expect("while conditional branch");

        // Body block
        self.builder.position_at_end(body_bb);
        self.compile_stmt(&while_stmt.body)?;
        self.builder
            .build_unconditional_branch(cond_bb)
            .expect("loop back to condition");

        // Exit
        self.builder.position_at_end(exit_bb);
        Ok(())
    }

    fn compile_expr(&mut self, expr: &Expr) -> anyhow::Result<StructValue<'ctx>> {
        match expr {
            Expr::Literal(lit) => self.compile_literal(lit),
            Expr::Binary(bin) => self.compile_binary(bin),
            Expr::Unary(un) => self.compile_unary(un),
            Expr::Grouping(g) => self.compile_expr(&g.expression),
            Expr::Variable(var) => self.compile_variable(var),
            Expr::Assign(assign) => self.compile_assign(assign),
            Expr::Logical(logical) => self.compile_logical(logical),
            Expr::Call(_) => {
                anyhow::bail!("call expressions not yet supported in LLVM codegen")
            }
            Expr::Get(_) => {
                anyhow::bail!("get expressions not yet supported in LLVM codegen")
            }
            Expr::Set(_) => {
                anyhow::bail!("set expressions not yet supported in LLVM codegen")
            }
            Expr::This(_) => {
                anyhow::bail!("'this' not yet supported in LLVM codegen")
            }
            Expr::Super(_) => {
                anyhow::bail!("'super' not yet supported in LLVM codegen")
            }
        }
    }

    fn compile_literal(&mut self, lit: &LiteralExpr) -> anyhow::Result<StructValue<'ctx>> {
        match &lit.value {
            LiteralValue::Number(n) => Ok(self.lox_value.build_number(&self.builder, *n)),
            LiteralValue::Bool(b) => Ok(self.lox_value.build_bool(&self.builder, *b)),
            LiteralValue::Nil => Ok(self.lox_value.build_nil(&self.builder)),
            LiteralValue::String(s) => self.compile_string_literal(s),
        }
    }

    fn compile_string_literal(&mut self, s: &str) -> anyhow::Result<StructValue<'ctx>> {
        let global = self
            .builder
            .build_global_string_ptr(s, "str")
            .expect("build global string");
        let ptr_as_int = self
            .builder
            .build_ptr_to_int(
                global.as_pointer_value(),
                self.context.i64_type(),
                "str_ptr",
            )
            .expect("ptr to int for string");
        Ok(self.lox_value.build_string(&self.builder, ptr_as_int))
    }

    fn compile_binary(&mut self, bin: &BinaryExpr) -> anyhow::Result<StructValue<'ctx>> {
        let left = self.compile_expr(&bin.left)?;
        let right = self.compile_expr(&bin.right)?;

        match bin.operator {
            BinaryOp::Add => self.compile_add(left, right),
            BinaryOp::Subtract => self.compile_numeric_binop(left, right, "sub"),
            BinaryOp::Multiply => self.compile_numeric_binop(left, right, "mul"),
            BinaryOp::Divide => self.compile_numeric_binop(left, right, "div"),
            BinaryOp::Less => self.compile_comparison(left, right, "lt"),
            BinaryOp::LessEqual => self.compile_comparison(left, right, "le"),
            BinaryOp::Greater => self.compile_comparison(left, right, "gt"),
            BinaryOp::GreaterEqual => self.compile_comparison(left, right, "ge"),
            BinaryOp::Equal => self.compile_equality(left, right, false),
            BinaryOp::NotEqual => self.compile_equality(left, right, true),
        }
    }

    fn compile_add(
        &mut self,
        left: StructValue<'ctx>,
        right: StructValue<'ctx>,
    ) -> anyhow::Result<StructValue<'ctx>> {
        // Phase 1: numeric addition only. String concatenation added later.
        let lhs = self.lox_value.extract_number(&self.builder, left);
        let rhs = self.lox_value.extract_number(&self.builder, right);
        let result = self
            .builder
            .build_float_add(lhs, rhs, "add")
            .expect("float add");
        let payload = self
            .builder
            .build_bit_cast(result, self.context.i64_type(), "add_i64")
            .expect("bitcast add result")
            .into_int_value();
        Ok(self.lox_value.build_tagged_number(&self.builder, payload))
    }

    fn compile_numeric_binop(
        &mut self,
        left: StructValue<'ctx>,
        right: StructValue<'ctx>,
        op_name: &str,
    ) -> anyhow::Result<StructValue<'ctx>> {
        let lhs = self.lox_value.extract_number(&self.builder, left);
        let rhs = self.lox_value.extract_number(&self.builder, right);

        let result = match op_name {
            "sub" => self
                .builder
                .build_float_sub(lhs, rhs, "sub")
                .expect("float sub"),
            "mul" => self
                .builder
                .build_float_mul(lhs, rhs, "mul")
                .expect("float mul"),
            "div" => self
                .builder
                .build_float_div(lhs, rhs, "div")
                .expect("float div"),
            _ => unreachable!("unknown numeric binop: {op_name}"),
        };

        let payload = self
            .builder
            .build_bit_cast(result, self.context.i64_type(), &format!("{op_name}_i64"))
            .expect("bitcast binop result")
            .into_int_value();
        Ok(self.lox_value.build_tagged_number(&self.builder, payload))
    }

    fn compile_comparison(
        &mut self,
        left: StructValue<'ctx>,
        right: StructValue<'ctx>,
        cmp_name: &str,
    ) -> anyhow::Result<StructValue<'ctx>> {
        let lhs = self.lox_value.extract_number(&self.builder, left);
        let rhs = self.lox_value.extract_number(&self.builder, right);

        use inkwell::FloatPredicate;
        let predicate = match cmp_name {
            "lt" => FloatPredicate::OLT,
            "le" => FloatPredicate::OLE,
            "gt" => FloatPredicate::OGT,
            "ge" => FloatPredicate::OGE,
            _ => unreachable!("unknown comparison: {cmp_name}"),
        };

        let cmp = self
            .builder
            .build_float_compare(predicate, lhs, rhs, cmp_name)
            .expect("float compare");
        Ok(self.lox_value.build_bool_from_i1(&self.builder, cmp))
    }

    fn compile_equality(
        &mut self,
        left: StructValue<'ctx>,
        right: StructValue<'ctx>,
        negate: bool,
    ) -> anyhow::Result<StructValue<'ctx>> {
        let left_tag = self.lox_value.extract_tag(&self.builder, left);
        let right_tag = self.lox_value.extract_tag(&self.builder, right);
        let tags_equal = self
            .builder
            .build_int_compare(inkwell::IntPredicate::EQ, left_tag, right_tag, "tags_eq")
            .expect("compare tags");

        let left_payload = self.lox_value.extract_payload(&self.builder, left);
        let right_payload = self.lox_value.extract_payload(&self.builder, right);
        let payloads_equal = self
            .builder
            .build_int_compare(
                inkwell::IntPredicate::EQ,
                left_payload,
                right_payload,
                "payloads_eq",
            )
            .expect("compare payloads");

        let equal = self
            .builder
            .build_and(tags_equal, payloads_equal, "equal")
            .expect("and tags and payloads");

        let result = if negate {
            self.builder
                .build_not(equal, "not_equal")
                .expect("not equal")
        } else {
            equal
        };

        Ok(self.lox_value.build_bool_from_i1(&self.builder, result))
    }

    fn compile_unary(&mut self, un: &UnaryExpr) -> anyhow::Result<StructValue<'ctx>> {
        let operand = self.compile_expr(&un.operand)?;
        match un.operator {
            UnaryOp::Negate => {
                let num = self.lox_value.extract_number(&self.builder, operand);
                let negated = self.builder.build_float_neg(num, "neg").expect("float neg");
                let payload = self
                    .builder
                    .build_bit_cast(negated, self.context.i64_type(), "neg_i64")
                    .expect("bitcast neg result")
                    .into_int_value();
                Ok(self.lox_value.build_tagged_number(&self.builder, payload))
            }
            UnaryOp::Not => {
                let truthy = self
                    .builder
                    .build_call(self.runtime.lox_value_truthy, &[operand.into()], "truthy")
                    .expect("call lox_value_truthy")
                    .try_as_basic_value()
                    .unwrap_basic()
                    .into_int_value();
                let negated = self
                    .builder
                    .build_not(truthy, "not_truthy")
                    .expect("not truthy");
                Ok(self.lox_value.build_bool_from_i1(&self.builder, negated))
            }
        }
    }

    fn compile_logical(&mut self, logical: &LogicalExpr) -> anyhow::Result<StructValue<'ctx>> {
        let current_fn = self.current_fn.expect("must be inside a function");

        let left = self.compile_expr(&logical.left)?;
        let left_truthy = self.emit_truthy(left);

        let rhs_bb = self.context.append_basic_block(current_fn, "log_rhs");
        let merge_bb = self.context.append_basic_block(current_fn, "log_merge");

        // Record which block the left value was computed in (for the phi node)
        let left_bb = self.builder.get_insert_block().expect("have insert block");

        match logical.operator {
            LogicalOp::And => {
                // Short-circuit: if left is falsy, skip right and use left
                self.builder
                    .build_conditional_branch(left_truthy, rhs_bb, merge_bb)
                    .expect("and short-circuit branch");
            }
            LogicalOp::Or => {
                // Short-circuit: if left is truthy, skip right and use left
                self.builder
                    .build_conditional_branch(left_truthy, merge_bb, rhs_bb)
                    .expect("or short-circuit branch");
            }
        }

        // Evaluate right operand
        self.builder.position_at_end(rhs_bb);
        let right = self.compile_expr(&logical.right)?;
        let rhs_exit_bb = self.builder.get_insert_block().expect("have insert block");
        self.builder
            .build_unconditional_branch(merge_bb)
            .expect("branch to merge from rhs");

        // Merge: use phi to select left or right value
        self.builder.position_at_end(merge_bb);
        let phi = self
            .builder
            .build_phi(self.lox_value.llvm_type(), "log_result")
            .expect("build phi for logical");
        phi.add_incoming(&[(&left, left_bb), (&right, rhs_exit_bb)]);
        Ok(phi.as_basic_value().into_struct_value())
    }

    fn compile_variable(&mut self, var: &VariableExpr) -> anyhow::Result<StructValue<'ctx>> {
        Ok(self.emit_global_get(&var.name))
    }

    fn compile_assign(&mut self, assign: &AssignExpr) -> anyhow::Result<StructValue<'ctx>> {
        let value = self.compile_expr(&assign.value)?;
        self.emit_global_set(&assign.name, value);
        Ok(value)
    }

    // --- Helpers ---

    /// Call `lox_value_truthy` to convert a LoxValue to an LLVM i1.
    fn emit_truthy(&mut self, value: StructValue<'ctx>) -> inkwell::values::IntValue<'ctx> {
        self.builder
            .build_call(self.runtime.lox_value_truthy, &[value.into()], "truthy")
            .expect("call lox_value_truthy")
            .try_as_basic_value()
            .unwrap_basic()
            .into_int_value()
    }

    // --- Global variable access ---

    fn emit_global_get(&mut self, name: &str) -> StructValue<'ctx> {
        let (name_ptr, name_len) = self.build_string_constant(name);
        self.builder
            .build_call(
                self.runtime.lox_global_get,
                &[name_ptr.into(), name_len.into()],
                "global_get",
            )
            .expect("call lox_global_get")
            .try_as_basic_value()
            .unwrap_basic()
            .into_struct_value()
    }

    fn emit_global_set(&mut self, name: &str, value: StructValue<'ctx>) {
        let (name_ptr, name_len) = self.build_string_constant(name);
        self.builder
            .build_call(
                self.runtime.lox_global_set,
                &[name_ptr.into(), name_len.into(), value.into()],
                "",
            )
            .expect("call lox_global_set");
    }

    /// Create a global string constant and return (pointer, length).
    fn build_string_constant(&mut self, s: &str) -> (BasicValueEnum<'ctx>, BasicValueEnum<'ctx>) {
        let global = self
            .builder
            .build_global_string_ptr(s, "name")
            .expect("build global string");
        let len = self.context.i64_type().const_int(s.len() as u64, false);
        (global.as_pointer_value().into(), len.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::scanner;

    fn compile_to_ir(source: &str) -> String {
        let tokens = scanner::scan(source).expect("scan succeeds");
        let program = Parser::new(tokens).parse().expect("parse succeeds");
        let context = Context::create();
        let codegen = CodeGen::new(&context, "test");
        codegen.compile(&program).expect("compile succeeds")
    }

    #[test]
    fn number_literal() {
        let ir = compile_to_ir("print 42;");
        assert!(ir.contains("call void @lox_print"));
    }

    #[test]
    fn nil_literal() {
        let ir = compile_to_ir("print nil;");
        assert!(ir.contains("call void @lox_print"));
    }

    #[test]
    fn bool_literal() {
        let ir = compile_to_ir("print true;");
        assert!(ir.contains("call void @lox_print"));
    }

    // Use global variables to prevent LLVM constant folding, so we can
    // verify that the correct LLVM instructions are emitted.

    #[test]
    fn arithmetic_add() {
        let ir = compile_to_ir("var a = 1; var b = 2; print a + b;");
        assert!(ir.contains("fadd"), "should contain float add");
    }

    #[test]
    fn arithmetic_sub() {
        let ir = compile_to_ir("var a = 5; var b = 3; print a - b;");
        assert!(ir.contains("fsub"), "should contain float sub");
    }

    #[test]
    fn arithmetic_mul() {
        let ir = compile_to_ir("var a = 2; var b = 3; print a * b;");
        assert!(ir.contains("fmul"), "should contain float mul");
    }

    #[test]
    fn arithmetic_div() {
        let ir = compile_to_ir("var a = 10; var b = 2; print a / b;");
        assert!(ir.contains("fdiv"), "should contain float div");
    }

    #[test]
    fn comparison_less() {
        let ir = compile_to_ir("var a = 1; var b = 2; print a < b;");
        assert!(ir.contains("fcmp olt"), "should contain ordered less-than");
    }

    #[test]
    fn comparison_greater() {
        let ir = compile_to_ir("var a = 2; var b = 1; print a > b;");
        assert!(
            ir.contains("fcmp ogt"),
            "should contain ordered greater-than"
        );
    }

    #[test]
    fn equality() {
        let ir = compile_to_ir("var a = 1; var b = 1; print a == b;");
        assert!(ir.contains("icmp eq"), "should contain integer compare");
    }

    #[test]
    fn unary_negate() {
        let ir = compile_to_ir("var a = 5; print -a;");
        assert!(ir.contains("fneg"), "should contain float negate");
    }

    #[test]
    fn unary_not() {
        let ir = compile_to_ir("var a = true; print !a;");
        assert!(
            ir.contains("lox_value_truthy"),
            "should call truthiness check"
        );
    }

    #[test]
    fn global_var_define_and_read() {
        let ir = compile_to_ir("var x = 10; print x;");
        assert!(ir.contains("lox_global_set"));
        assert!(ir.contains("lox_global_get"));
    }

    #[test]
    fn global_var_assign() {
        let ir = compile_to_ir("var x = 1; x = 2; print x;");
        let set_count = ir.matches("lox_global_set").count();
        assert!(
            set_count >= 2,
            "expected >= 2 global_set calls, got {set_count}"
        );
    }

    #[test]
    fn string_literal() {
        let ir = compile_to_ir(r#"print "hello";"#);
        assert!(ir.contains("hello"), "should contain string constant");
    }

    #[test]
    fn main_returns_zero() {
        let ir = compile_to_ir("print 1;");
        assert!(ir.contains("ret i32 0"), "main should return 0");
    }

    #[test]
    fn constant_folded_arithmetic() {
        // Pure constant expressions get folded by LLVM, so just verify
        // the IR compiles and calls lox_print with a result.
        let ir = compile_to_ir("print (1 + 2) * 3 - 4 / 2;");
        assert!(ir.contains("call void @lox_print"));
    }

    // --- Phase 2: Control flow ---

    #[test]
    fn if_then() {
        let ir = compile_to_ir("if (true) print 1;");
        assert!(ir.contains("br i1"), "should contain conditional branch");
        assert!(ir.contains("then"), "should have then block");
        assert!(ir.contains("merge"), "should have merge block");
    }

    #[test]
    fn if_else() {
        let ir = compile_to_ir("if (true) print 1; else print 2;");
        assert!(ir.contains("then"), "should have then block");
        assert!(ir.contains("else"), "should have else block");
        assert!(ir.contains("merge"), "should have merge block");
    }

    #[test]
    fn while_loop() {
        let ir = compile_to_ir("var i = 0; while (i < 3) i = i + 1;");
        assert!(ir.contains("while_cond"), "should have condition block");
        assert!(ir.contains("while_body"), "should have body block");
        assert!(ir.contains("while_exit"), "should have exit block");
    }

    #[test]
    fn for_loop() {
        // Parser desugars for to while, so the IR should look the same
        let ir = compile_to_ir("for (var i = 0; i < 3; i = i + 1) print i;");
        assert!(ir.contains("while_cond"), "for desugars to while");
    }

    #[test]
    fn block_statement() {
        let ir = compile_to_ir("{ print 1; print 2; }");
        // Block just sequences declarations; verify two print calls
        let print_count = ir.matches("call void @lox_print").count();
        assert_eq!(print_count, 2, "should have two print calls");
    }

    #[test]
    fn logical_and() {
        let ir = compile_to_ir("var a = true; var b = false; print a and b;");
        assert!(ir.contains("log_rhs"), "should have rhs block for and");
        assert!(ir.contains("log_merge"), "should have merge block for and");
    }

    #[test]
    fn logical_or() {
        let ir = compile_to_ir("var a = false; var b = true; print a or b;");
        assert!(ir.contains("log_rhs"), "should have rhs block for or");
        assert!(ir.contains("log_merge"), "should have merge block for or");
    }

    #[test]
    fn nested_if() {
        let ir = compile_to_ir(
            "var x = 10; if (x > 5) { if (x > 20) print 1; else print 2; } else print 3;",
        );
        // Should have multiple then/else/merge blocks
        let then_count = ir.matches("then").count();
        assert!(then_count >= 2, "should have nested then blocks");
    }

    #[test]
    fn while_with_logical_condition() {
        let ir = compile_to_ir("var a = 0; var b = 1; while (a < 5 and b > 0) { a = a + 1; }");
        assert!(ir.contains("while_cond"));
        assert!(ir.contains("log_rhs"), "and in while condition");
    }
}
