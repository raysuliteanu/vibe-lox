use std::collections::HashMap;

use inkwell::AddressSpace;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::values::{
    BasicMetadataValueEnum, BasicValueEnum, FunctionValue, PointerValue, StructValue,
};

use crate::ast::{
    AssignExpr, BinaryExpr, BinaryOp, BlockStmt, CallExpr, Decl, Expr, ExprId, ExprStmt, FunDecl,
    IfStmt, LiteralExpr, LiteralValue, LogicalExpr, LogicalOp, PrintStmt, Program, ReturnStmt,
    Stmt, UnaryExpr, UnaryOp, VarDecl, VariableExpr, WhileStmt,
};

use super::capture::{CaptureInfo, CapturedVar};
use super::runtime::RuntimeDecls;
use super::types::LoxValueType;

/// Tracks how a local variable is stored.
#[derive(Clone)]
enum VarStorage<'ctx> {
    /// Stack-allocated via alloca (not captured by any closure).
    Alloca(PointerValue<'ctx>),
    /// Heap-allocated cell (captured by at least one closure).
    /// The PointerValue points to the cell (LoxValue*).
    Cell(PointerValue<'ctx>),
}

/// LLVM IR code generator for Lox programs.
///
/// Walks the AST and emits LLVM IR using inkwell. Handles literals, arithmetic,
/// comparisons, unary ops, print, global and local variables, control flow,
/// logical operators, functions, closures, and return statements.
pub struct CodeGen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    lox_value: LoxValueType<'ctx>,
    runtime: RuntimeDecls<'ctx>,
    /// The current LLVM function being compiled into.
    current_fn: Option<FunctionValue<'ctx>>,
    /// Variable resolution results from the resolver: ExprId → scope depth.
    /// If an ExprId is present, the variable is local; otherwise it's global.
    locals: HashMap<ExprId, usize>,
    /// Stack of local variable scopes. Each scope maps variable names to
    /// their storage (alloca or cell pointer).
    scopes: Vec<HashMap<String, VarStorage<'ctx>>>,
    /// Capture analysis results.
    captures: CaptureInfo,
    /// Name of the Lox function currently being compiled (empty = top-level).
    current_lox_fn: String,
    /// For return statements: alloca for the return value and the exit block.
    return_target: Option<(PointerValue<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)>,
}

impl<'ctx> CodeGen<'ctx> {
    pub fn new(
        context: &'ctx Context,
        module_name: &str,
        locals: HashMap<ExprId, usize>,
        captures: CaptureInfo,
    ) -> Self {
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
            locals,
            scopes: Vec::new(),
            captures,
            current_lox_fn: String::new(),
            return_target: None,
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

        // Register native clock() function
        self.register_native_clock()?;

        for decl in &program.declarations {
            self.compile_decl(decl)?;
        }

        // return 0
        self.builder
            .build_return(Some(&i32_type.const_int(0, false)))
            .expect("build return from main");
        Ok(())
    }

    /// Register the native `clock()` function as a global.
    fn register_native_clock(&mut self) -> anyhow::Result<()> {
        // Create a wrapper LLVM function that ignores env and calls lox_clock
        let ptr_type = self.context.ptr_type(AddressSpace::default());
        let lv_type = self.lox_value.llvm_type();
        let clock_fn_type = lv_type.fn_type(&[ptr_type.into()], false);
        let clock_fn = self
            .module
            .add_function("lox_clock_wrapper", clock_fn_type, None);
        let entry = self.context.append_basic_block(clock_fn, "entry");

        // Save/restore builder position
        let saved_bb = self.builder.get_insert_block();
        self.builder.position_at_end(entry);

        let result = self
            .builder
            .build_call(self.runtime.lox_clock, &[], "clock_val")
            .expect("call lox_clock")
            .try_as_basic_value()
            .unwrap_basic();
        self.builder
            .build_return(Some(&result))
            .expect("return from clock wrapper");

        if let Some(bb) = saved_bb {
            self.builder.position_at_end(bb);
        }

        // Create a closure for clock and store as global
        let closure_val = self.build_closure(clock_fn, "clock", &[])?;
        self.emit_global_set("clock", closure_val);

        Ok(())
    }

    fn compile_decl(&mut self, decl: &Decl) -> anyhow::Result<()> {
        match decl {
            Decl::Var(var_decl) => self.compile_var_decl(var_decl),
            Decl::Statement(stmt) => self.compile_stmt(stmt),
            Decl::Fun(fun_decl) => self.compile_fun_decl(fun_decl),
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

        if self.scopes.is_empty() {
            // Top-level: store as global
            self.emit_global_set(&var_decl.name, value);
        } else {
            // Check if this variable is captured by an inner function
            let is_captured = self.captures.captured_vars.contains(&CapturedVar {
                var_name: var_decl.name.clone(),
                declaring_function: self.current_lox_fn.clone(),
            });

            let storage = if is_captured {
                // Captured: allocate a heap cell so closures can share it
                let cell = self
                    .builder
                    .build_call(self.runtime.lox_alloc_cell, &[value.into()], "cell")
                    .expect("call lox_alloc_cell")
                    .try_as_basic_value()
                    .unwrap_basic()
                    .into_pointer_value();
                VarStorage::Cell(cell)
            } else {
                // Not captured: use stack alloca
                let alloca = self.create_entry_block_alloca(&var_decl.name);
                self.builder
                    .build_store(alloca, value)
                    .expect("store local var initializer");
                VarStorage::Alloca(alloca)
            };

            self.scopes
                .last_mut()
                .expect("checked non-empty above")
                .insert(var_decl.name.clone(), storage);
        }
        Ok(())
    }

    fn compile_stmt(&mut self, stmt: &Stmt) -> anyhow::Result<()> {
        match stmt {
            Stmt::Print(print_stmt) => self.compile_print_stmt(print_stmt),
            Stmt::Expression(expr_stmt) => self.compile_expr_stmt(expr_stmt),
            Stmt::Block(block) => self.compile_block(block),
            Stmt::If(if_stmt) => self.compile_if(if_stmt),
            Stmt::While(while_stmt) => self.compile_while(while_stmt),
            Stmt::Return(ret) => self.compile_return(ret),
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
        self.begin_scope();
        for decl in &block.declarations {
            self.compile_decl(decl)?;
        }
        self.end_scope();
        Ok(())
    }

    fn compile_fun_decl(&mut self, fun_decl: &FunDecl) -> anyhow::Result<()> {
        let function = &fun_decl.function;
        let fn_name = &function.name;

        // Determine which variables this function captures from enclosing scopes
        let captured_names = self
            .captures
            .function_captures
            .get(fn_name)
            .cloned()
            .unwrap_or_default();

        // Build the LLVM function type: (ptr %env, LoxValue %arg0, ...) -> LoxValue
        let ptr_type = self.context.ptr_type(AddressSpace::default());
        let lv_type = self.lox_value.llvm_type();
        let mut param_types: Vec<inkwell::types::BasicMetadataTypeEnum> = vec![ptr_type.into()];
        for _ in &function.params {
            param_types.push(lv_type.into());
        }
        let fn_type = lv_type.fn_type(&param_types, false);
        let llvm_fn_name = format!("lox_fn_{fn_name}");
        let llvm_fn = self.module.add_function(&llvm_fn_name, fn_type, None);

        // Save the current compilation state
        let saved_fn = self.current_fn;
        let saved_lox_fn = self.current_lox_fn.clone();
        let saved_scopes = std::mem::take(&mut self.scopes);
        let saved_return_target = self.return_target.take();
        let saved_insert_block = self.builder.get_insert_block();

        // Set up for compiling the function body
        self.current_fn = Some(llvm_fn);
        self.current_lox_fn = fn_name.clone();

        let entry_bb = self.context.append_basic_block(llvm_fn, "entry");
        let exit_bb = self.context.append_basic_block(llvm_fn, "exit");
        self.builder.position_at_end(entry_bb);

        // Create return value alloca and set up return target
        let ret_alloca = self.create_entry_block_alloca("retval");
        // Initialize to nil (implicit return)
        self.builder
            .build_store(ret_alloca, self.lox_value.build_nil(&self.builder))
            .expect("store initial retval");
        self.return_target = Some((ret_alloca, exit_bb));

        // Create a scope for the function body
        self.begin_scope();

        // Bind the env parameter: load captured cells from the env array
        let env_param = llvm_fn
            .get_nth_param(0)
            .expect("env parameter exists")
            .into_pointer_value();

        for (i, cap_name) in captured_names.iter().enumerate() {
            // env is an array of LoxValue* (cell pointers)
            // Load the i-th cell pointer from the env array
            let cell_ptr_ptr = unsafe {
                self.builder
                    .build_gep(
                        ptr_type,
                        env_param,
                        &[self.context.i64_type().const_int(i as u64, false)],
                        &format!("env_{cap_name}_ptr"),
                    )
                    .expect("GEP into env array")
            };
            let cell_ptr = self
                .builder
                .build_load(ptr_type, cell_ptr_ptr, &format!("env_{cap_name}"))
                .expect("load cell ptr from env")
                .into_pointer_value();
            self.scopes
                .last_mut()
                .expect("have scope")
                .insert(cap_name.clone(), VarStorage::Cell(cell_ptr));
        }

        // Bind parameters as local variables
        for (i, param_name) in function.params.iter().enumerate() {
            let param_val = llvm_fn
                .get_nth_param((i + 1) as u32)
                .expect("parameter exists")
                .into_struct_value();

            // Check if this parameter is captured
            let is_captured = self.captures.captured_vars.contains(&CapturedVar {
                var_name: param_name.clone(),
                declaring_function: fn_name.clone(),
            });

            let storage = if is_captured {
                let cell = self
                    .builder
                    .build_call(self.runtime.lox_alloc_cell, &[param_val.into()], "cell")
                    .expect("call lox_alloc_cell")
                    .try_as_basic_value()
                    .unwrap_basic()
                    .into_pointer_value();
                VarStorage::Cell(cell)
            } else {
                let alloca = self.create_entry_block_alloca(param_name);
                self.builder
                    .build_store(alloca, param_val)
                    .expect("store param to alloca");
                VarStorage::Alloca(alloca)
            };

            self.scopes
                .last_mut()
                .expect("have scope")
                .insert(param_name.clone(), storage);
        }

        // Compile the function body
        for decl in &function.body {
            self.compile_decl(decl)?;
        }

        // Branch to exit block (if the body didn't already terminate)
        if self
            .builder
            .get_insert_block()
            .expect("have insert block")
            .get_terminator()
            .is_none()
        {
            self.builder
                .build_unconditional_branch(exit_bb)
                .expect("branch to exit");
        }

        // Exit block: load return value and return it
        self.builder.position_at_end(exit_bb);
        let ret_val = self
            .builder
            .build_load(self.lox_value.llvm_type(), ret_alloca, "retval")
            .expect("load return value");
        self.builder
            .build_return(Some(&ret_val))
            .expect("build return");

        self.end_scope();

        // Restore compilation state
        self.current_fn = saved_fn;
        self.current_lox_fn = saved_lox_fn;
        self.scopes = saved_scopes;
        self.return_target = saved_return_target;
        if let Some(bb) = saved_insert_block {
            self.builder.position_at_end(bb);
        }

        // Build the closure struct and store as a function value
        // Collect cell pointers for the environment
        let closure_val = self.build_closure(llvm_fn, fn_name, &captured_names)?;

        // Store function as a global (or local if in a scope)
        if self.scopes.is_empty() {
            self.emit_global_set(fn_name, closure_val);
        } else {
            // Check if function name is captured
            let is_captured = self.captures.captured_vars.contains(&CapturedVar {
                var_name: fn_name.clone(),
                declaring_function: self.current_lox_fn.clone(),
            });
            let storage = if is_captured {
                let cell = self
                    .builder
                    .build_call(
                        self.runtime.lox_alloc_cell,
                        &[closure_val.into()],
                        "fn_cell",
                    )
                    .expect("call lox_alloc_cell")
                    .try_as_basic_value()
                    .unwrap_basic()
                    .into_pointer_value();
                VarStorage::Cell(cell)
            } else {
                let alloca = self.create_entry_block_alloca(fn_name);
                self.builder
                    .build_store(alloca, closure_val)
                    .expect("store function to alloca");
                VarStorage::Alloca(alloca)
            };
            self.scopes
                .last_mut()
                .expect("have scope")
                .insert(fn_name.clone(), storage);
        }

        Ok(())
    }

    /// Build a closure LoxValue from an LLVM function, its name, and its captured variable names.
    fn build_closure(
        &mut self,
        llvm_fn: FunctionValue<'ctx>,
        name: &str,
        captured_names: &[String],
    ) -> anyhow::Result<StructValue<'ctx>> {
        let ptr_type = self.context.ptr_type(AddressSpace::default());
        let i32_type = self.context.i32_type();

        let fn_ptr = llvm_fn.as_global_value().as_pointer_value();
        let arity = i32_type.const_int(
            (llvm_fn.count_params() - 1) as u64, // subtract env param
            false,
        );
        let name_str = self
            .builder
            .build_global_string_ptr(name, "fn_name")
            .expect("build fn name string");

        let (env_ptr, env_count) = if captured_names.is_empty() {
            (ptr_type.const_null(), i32_type.const_zero())
        } else {
            // Build an array of cell pointers on the stack and pass to lox_alloc_closure
            let arr_alloca = self
                .builder
                .build_array_alloca(
                    ptr_type,
                    i32_type.const_int(captured_names.len() as u64, false),
                    "env_arr",
                )
                .expect("alloca for env array");

            for (i, cap_name) in captured_names.iter().enumerate() {
                // Find the cell pointer for this captured variable in current scopes
                let cell_ptr = self.find_cell_for_capture(cap_name);
                let slot = unsafe {
                    self.builder
                        .build_gep(
                            ptr_type,
                            arr_alloca,
                            &[self.context.i64_type().const_int(i as u64, false)],
                            &format!("env_slot_{i}"),
                        )
                        .expect("GEP into env array")
                };
                self.builder
                    .build_store(slot, cell_ptr)
                    .expect("store cell ptr into env array");
            }

            (
                arr_alloca,
                i32_type.const_int(captured_names.len() as u64, false),
            )
        };

        let closure_ptr = self
            .builder
            .build_call(
                self.runtime.lox_alloc_closure,
                &[
                    fn_ptr.into(),
                    arity.into(),
                    name_str.as_pointer_value().into(),
                    env_ptr.into(),
                    env_count.into(),
                ],
                "closure",
            )
            .expect("call lox_alloc_closure")
            .try_as_basic_value()
            .unwrap_basic()
            .into_pointer_value();

        // Wrap closure pointer as a TAG_FUNCTION LoxValue
        let closure_as_int = self
            .builder
            .build_ptr_to_int(closure_ptr, self.context.i64_type(), "closure_int")
            .expect("ptr to int for closure");
        Ok(self.lox_value.build_tagged_value_with_int(
            &self.builder,
            super::types::TAG_FUNCTION,
            closure_as_int,
        ))
    }

    /// Find the cell pointer for a captured variable by searching current scopes.
    fn find_cell_for_capture(&self, name: &str) -> PointerValue<'ctx> {
        for scope in self.scopes.iter().rev() {
            if let Some(storage) = scope.get(name) {
                return match storage {
                    VarStorage::Cell(cell) => *cell,
                    VarStorage::Alloca(_) => {
                        panic!("captured variable '{name}' should be in a cell, not an alloca")
                    }
                };
            }
        }
        panic!("captured variable '{name}' not found in any scope")
    }

    fn compile_call(&mut self, call: &CallExpr) -> anyhow::Result<StructValue<'ctx>> {
        let callee = self.compile_expr(&call.callee)?;

        // Extract closure pointer from the LoxValue
        let closure_ptr_int = self.lox_value.extract_payload(&self.builder, callee);
        let ptr_type = self.context.ptr_type(AddressSpace::default());
        let closure_ptr = self
            .builder
            .build_int_to_ptr(closure_ptr_int, ptr_type, "closure_ptr")
            .expect("int to closure ptr");

        // Load fn_ptr from closure struct (field 0)
        let fn_ptr_ptr = self
            .builder
            .build_struct_gep(self.closure_llvm_type(), closure_ptr, 0, "fn_ptr_ptr")
            .expect("GEP to fn_ptr");
        let fn_ptr = self
            .builder
            .build_load(ptr_type, fn_ptr_ptr, "fn_ptr")
            .expect("load fn_ptr")
            .into_pointer_value();

        // Load env from closure struct (field 3 = env pointer)
        let env_ptr_ptr = self
            .builder
            .build_struct_gep(self.closure_llvm_type(), closure_ptr, 3, "env_ptr_ptr")
            .expect("GEP to env_ptr");
        let env_ptr = self
            .builder
            .build_load(ptr_type, env_ptr_ptr, "env_ptr")
            .expect("load env_ptr")
            .into_pointer_value();

        // Build arguments: env + lox args
        let mut args: Vec<BasicMetadataValueEnum> = vec![env_ptr.into()];
        for arg in &call.arguments {
            let val = self.compile_expr(arg)?;
            args.push(val.into());
        }

        // Build the function type for the indirect call
        let lv_type = self.lox_value.llvm_type();
        let mut param_types: Vec<inkwell::types::BasicMetadataTypeEnum> = vec![ptr_type.into()];
        for _ in &call.arguments {
            param_types.push(lv_type.into());
        }
        let call_fn_type = lv_type.fn_type(&param_types, false);

        let result = self
            .builder
            .build_indirect_call(call_fn_type, fn_ptr, &args, "call_result")
            .expect("build indirect call")
            .try_as_basic_value()
            .unwrap_basic()
            .into_struct_value();

        Ok(result)
    }

    fn compile_return(&mut self, ret: &ReturnStmt) -> anyhow::Result<()> {
        let value = match &ret.value {
            Some(expr) => self.compile_expr(expr)?,
            None => self.lox_value.build_nil(&self.builder),
        };

        let (ret_alloca, exit_bb) = self
            .return_target
            .expect("return must be inside a function");

        self.builder
            .build_store(ret_alloca, value)
            .expect("store return value");
        self.builder
            .build_unconditional_branch(exit_bb)
            .expect("branch to exit block");

        // Create a dead block for any code after return (LLVM requires
        // all instructions to be in a block)
        let current_fn = self.current_fn.expect("inside a function");
        let dead_bb = self.context.append_basic_block(current_fn, "after_ret");
        self.builder.position_at_end(dead_bb);

        Ok(())
    }

    /// Return the LLVM struct type matching the C LoxClosure struct layout.
    fn closure_llvm_type(&self) -> inkwell::types::StructType<'ctx> {
        let ptr_type = self.context.ptr_type(AddressSpace::default());
        let i32_type = self.context.i32_type();
        // { void*, i32, i32, LoxValue**, char* }
        // = { fn_ptr, arity, env_count, env, name }
        self.context.struct_type(
            &[
                ptr_type.into(), // fn_ptr
                i32_type.into(), // arity
                i32_type.into(), // env_count
                ptr_type.into(), // env (LoxValue**)
                ptr_type.into(), // name (char*)
            ],
            false,
        )
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
            Expr::Call(call) => self.compile_call(call),
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
        if self.locals.contains_key(&var.id) {
            if let Some(storage) = self.find_local(&var.name) {
                Ok(self.load_var_storage(&storage, &var.name))
            } else {
                // Resolved as local by resolver but not found in our scopes —
                // this can happen for globals referenced inside functions
                Ok(self.emit_global_get(&var.name))
            }
        } else {
            Ok(self.emit_global_get(&var.name))
        }
    }

    fn compile_assign(&mut self, assign: &AssignExpr) -> anyhow::Result<StructValue<'ctx>> {
        let value = self.compile_expr(&assign.value)?;
        if self.locals.contains_key(&assign.id) {
            if let Some(storage) = self.find_local(&assign.name) {
                self.store_var_storage(&storage, value);
            } else {
                self.emit_global_set(&assign.name, value);
            }
        } else {
            self.emit_global_set(&assign.name, value);
        }
        Ok(value)
    }

    // --- Scope management ---

    fn begin_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn end_scope(&mut self) {
        self.scopes.pop();
    }

    /// Search for a local variable by name in the scope stack (innermost first).
    /// Returns None if not found in any scope.
    fn find_local(&self, name: &str) -> Option<VarStorage<'ctx>> {
        for scope in self.scopes.iter().rev() {
            if let Some(storage) = scope.get(name) {
                return Some(storage.clone());
            }
        }
        None
    }

    /// Load a LoxValue from variable storage.
    fn load_var_storage(&self, storage: &VarStorage<'ctx>, name: &str) -> StructValue<'ctx> {
        match storage {
            VarStorage::Alloca(alloca) => self
                .builder
                .build_load(self.lox_value.llvm_type(), *alloca, name)
                .expect("load from alloca")
                .into_struct_value(),
            VarStorage::Cell(cell) => self
                .builder
                .build_call(self.runtime.lox_cell_get, &[(*cell).into()], name)
                .expect("call lox_cell_get")
                .try_as_basic_value()
                .unwrap_basic()
                .into_struct_value(),
        }
    }

    /// Store a LoxValue to variable storage.
    fn store_var_storage(&self, storage: &VarStorage<'ctx>, value: StructValue<'ctx>) {
        match storage {
            VarStorage::Alloca(alloca) => {
                self.builder
                    .build_store(*alloca, value)
                    .expect("store to alloca");
            }
            VarStorage::Cell(cell) => {
                self.builder
                    .build_call(
                        self.runtime.lox_cell_set,
                        &[(*cell).into(), value.into()],
                        "",
                    )
                    .expect("call lox_cell_set");
            }
        }
    }

    /// Create an alloca in the entry block of the current function.
    /// Placing all allocas in the entry block is LLVM best practice for mem2reg.
    fn create_entry_block_alloca(&self, name: &str) -> PointerValue<'ctx> {
        let current_fn = self.current_fn.expect("must be inside a function");
        let entry = current_fn
            .get_first_basic_block()
            .expect("function has entry block");

        // Create a temporary builder positioned at the start of the entry block
        let alloca_builder = self.context.create_builder();
        match entry.get_first_instruction() {
            Some(first_instr) => alloca_builder.position_before(&first_instr),
            None => alloca_builder.position_at_end(entry),
        }
        alloca_builder
            .build_alloca(self.lox_value.llvm_type(), name)
            .expect("build alloca for local var")
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
    use crate::interpreter::resolver::Resolver;
    use crate::parser::Parser;
    use crate::scanner;

    fn compile_to_ir(source: &str) -> String {
        let tokens = scanner::scan(source).expect("scan succeeds");
        let program = Parser::new(tokens).parse().expect("parse succeeds");
        let locals = Resolver::new().resolve(&program).expect("resolve succeeds");
        let captures = super::super::capture::analyze_captures(&program);
        let context = Context::create();
        let codegen = CodeGen::new(&context, "test", locals, captures);
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

    // --- Phase 3: Local variables and scoping ---

    #[test]
    fn local_var() {
        let ir = compile_to_ir("{ var x = 1; print x; }");
        // Local var should use alloca+store+load
        assert!(ir.contains("alloca"), "should use alloca for local var");
        assert!(ir.contains("store"), "should store to local var");
        assert!(ir.contains("load"), "should load from local var");
        // Should not call lox_global_get for "x" (declaration is ok)
        assert!(
            !ir.contains("call { i8, i64 } @lox_global_get"),
            "local var should not call global_get"
        );
    }

    #[test]
    fn scope_shadowing() {
        let ir = compile_to_ir(
            r#"var x = "global";
            {
                var x = "local";
                print x;
            }
            print x;"#,
        );
        // Should have both a global_set call (for outer x) and alloca (for inner x)
        assert!(
            ir.contains("call void @lox_global_set"),
            "outer x is global"
        );
        assert!(ir.contains("alloca"), "inner x uses alloca");
    }

    #[test]
    fn nested_blocks() {
        let ir = compile_to_ir("{ var a = 1; { var b = 2; print a; print b; } }");
        // Both local vars should use allocas
        let alloca_count = ir.matches("alloca").count();
        assert!(
            alloca_count >= 2,
            "should have allocas for both local vars, got {alloca_count}"
        );
    }

    #[test]
    fn local_var_assignment() {
        let ir = compile_to_ir("{ var x = 1; x = 2; print x; }");
        // Assignment to local should use store, not global_get/set for "x"
        let store_count = ir.matches("store").count();
        assert!(
            store_count >= 2,
            "should have stores for init and assignment, got {store_count}"
        );
        assert!(
            !ir.contains("call { i8, i64 } @lox_global_get"),
            "local assignment should not call global_get"
        );
    }

    #[test]
    fn mixed_global_and_local() {
        let ir = compile_to_ir("var g = 1; { var l = 2; print g; print l; }");
        // Global uses global_set/global_get calls, local uses alloca/load
        assert!(
            ir.contains("call void @lox_global_set"),
            "global var calls global_set"
        );
        assert!(
            ir.contains("@lox_global_get"),
            "reading global from block calls global_get"
        );
        assert!(ir.contains("alloca"), "local var uses alloca");
    }

    // --- Phase 4: Functions and closures ---

    #[test]
    fn simple_function() {
        let ir = compile_to_ir("fun f(x) { return x + 1; } print f(1);");
        assert!(ir.contains("@lox_fn_f"), "should have lox_fn_f function");
        assert!(
            ir.contains("lox_alloc_closure"),
            "should allocate a closure"
        );
    }

    #[test]
    fn function_call() {
        let ir = compile_to_ir("fun greet() { print 42; } greet();");
        assert!(ir.contains("@lox_fn_greet"), "should define greet fn");
        // Call should be indirect (through closure)
        assert!(ir.contains("call { i8, i64 }"), "should have call result");
    }

    #[test]
    fn function_return() {
        let ir = compile_to_ir("fun f() { return 42; }");
        assert!(ir.contains("@lox_fn_f"), "should define f function");
        assert!(
            ir.contains("ret { i8, i64 }"),
            "function should return LoxValue"
        );
    }

    #[test]
    fn implicit_nil_return() {
        let ir = compile_to_ir("fun f() { print 1; }");
        // Should still have a ret instruction (returning nil)
        assert!(
            ir.contains("ret { i8, i64 }"),
            "function should return LoxValue (nil)"
        );
    }

    #[test]
    fn closure_capture() {
        let ir = compile_to_ir(
            "fun make() { var x = 1; fun get() { return x; } return get; } print make()();",
        );
        assert!(ir.contains("lox_alloc_cell"), "captured var needs a cell");
        assert!(ir.contains("lox_cell_get"), "closure reads via cell_get");
    }

    #[test]
    fn closure_mutation() {
        let ir = compile_to_ir(
            "fun counter() { var n = 0; fun inc() { n = n + 1; return n; } return inc; }",
        );
        assert!(ir.contains("lox_alloc_cell"), "captured var needs a cell");
        assert!(ir.contains("lox_cell_set"), "closure writes via cell_set");
    }

    #[test]
    fn recursion() {
        let ir = compile_to_ir("fun fib(n) { if (n <= 1) return n; return fib(n-1) + fib(n-2); }");
        assert!(ir.contains("@lox_fn_fib"), "should define fib function");
    }

    #[test]
    fn native_clock() {
        let ir = compile_to_ir("var t = clock();");
        assert!(
            ir.contains("lox_clock_wrapper"),
            "should have clock wrapper"
        );
    }
}
