use std::collections::HashMap;

use inkwell::AddressSpace;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::values::{
    BasicMetadataValueEnum, BasicValueEnum, FunctionValue, PointerValue, StructValue,
};

use crate::ast::{
    AssignExpr, BinaryExpr, BinaryOp, BlockStmt, CallExpr, ClassDecl, Decl, Expr, ExprId, ExprStmt,
    FunDecl, GetExpr, IfStmt, LiteralExpr, LiteralValue, LogicalExpr, LogicalOp, PrintStmt,
    Program, ReturnStmt, SetExpr, Stmt, SuperExpr, ThisExpr, UnaryExpr, UnaryOp, VarDecl,
    VariableExpr, WhileStmt,
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
            Decl::Class(class_decl) => self.compile_class_decl(class_decl),
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

    fn compile_class_decl(&mut self, class: &ClassDecl) -> anyhow::Result<()> {
        let ptr_type = self.context.ptr_type(AddressSpace::default());
        let i32_type = self.context.i32_type();

        // Resolve superclass pointer (or null)
        let superclass_ptr = if let Some(ref superclass_name) = class.superclass {
            // Load superclass LoxValue (TAG_CLASS), extract the class descriptor pointer
            let super_val = if let Some(storage) = self.find_local(superclass_name) {
                self.load_var_storage(&storage, superclass_name)
            } else {
                self.emit_global_get(superclass_name)
            };
            let super_payload = self.lox_value.extract_payload(&self.builder, super_val);
            self.builder
                .build_int_to_ptr(super_payload, ptr_type, "super_class_ptr")
                .expect("superclass payload to ptr")
        } else {
            ptr_type.const_null()
        };

        // Allocate class descriptor
        let method_count = i32_type.const_int(class.methods.len() as u64, false);
        let class_name_str = self
            .builder
            .build_global_string_ptr(&class.name, "class_name")
            .expect("class name string");
        let class_desc = self
            .builder
            .build_call(
                self.runtime.lox_alloc_class,
                &[
                    class_name_str.as_pointer_value().into(),
                    superclass_ptr.into(),
                    method_count.into(),
                ],
                "class_desc",
            )
            .expect("call lox_alloc_class")
            .try_as_basic_value()
            .unwrap_basic()
            .into_pointer_value();

        // Store class_desc pointer in a cell so methods of subclasses can capture "super"
        let class_desc_as_int = self
            .builder
            .build_ptr_to_int(class_desc, self.context.i64_type(), "class_desc_int")
            .expect("class desc to int");
        let class_val = self.lox_value.build_tagged_value_with_int(
            &self.builder,
            super::types::TAG_CLASS,
            class_desc_as_int,
        );

        // Compile each method
        let has_super = class.superclass.is_some();
        for method in &class.methods {
            let closure_ptr = self.compile_method(
                method,
                &class.name,
                has_super,
                class_desc,
                class_val,
                class.superclass.as_deref(),
            )?;

            // Add method to class descriptor
            let method_name_str = self
                .builder
                .build_global_string_ptr(&method.name, "method_name")
                .expect("method name string");
            self.builder
                .build_call(
                    self.runtime.lox_class_add_method,
                    &[
                        class_desc.into(),
                        method_name_str.as_pointer_value().into(),
                        closure_ptr.into(),
                    ],
                    "",
                )
                .expect("call lox_class_add_method");
        }

        // Store class as a global (or local if in a scope)
        if self.scopes.is_empty() {
            self.emit_global_set(&class.name, class_val);
        } else {
            let alloca = self.create_entry_block_alloca(&class.name);
            self.builder
                .build_store(alloca, class_val)
                .expect("store class to alloca");
            self.scopes
                .last_mut()
                .expect("have scope")
                .insert(class.name.clone(), VarStorage::Alloca(alloca));
        }

        Ok(())
    }

    /// Compile a method as an LLVM function. Methods have `this` at env[0]
    /// and optionally `super` at env[1] (for subclass methods).
    /// Returns the closure pointer for the method.
    fn compile_method(
        &mut self,
        method: &crate::ast::Function,
        class_name: &str,
        has_super: bool,
        _class_desc: PointerValue<'ctx>,
        _class_val: StructValue<'ctx>,
        superclass_name: Option<&str>,
    ) -> anyhow::Result<PointerValue<'ctx>> {
        let method_name = &method.name;
        let ptr_type = self.context.ptr_type(AddressSpace::default());
        let lv_type = self.lox_value.llvm_type();

        // Determine captured variables from enclosing scopes (excluding this/super)
        let captured_names = self
            .captures
            .function_captures
            .get(method_name)
            .cloned()
            .unwrap_or_default();

        // Method env layout: [this, super?, captured_var_0, ...]
        let this_env_idx = 0usize;
        let super_env_idx = if has_super { Some(1usize) } else { None };
        let capture_offset = if has_super { 2 } else { 1 };
        let total_env_count = capture_offset + captured_names.len();

        // Build LLVM function type: (ptr env, LoxValue arg0, ...) -> LoxValue
        let mut param_types: Vec<inkwell::types::BasicMetadataTypeEnum> = vec![ptr_type.into()];
        for _ in &method.params {
            param_types.push(lv_type.into());
        }
        let fn_type = lv_type.fn_type(&param_types, false);
        let llvm_fn_name = format!("lox_fn_{class_name}_{method_name}");
        let llvm_fn = self.module.add_function(&llvm_fn_name, fn_type, None);

        // Save compilation state
        let saved_fn = self.current_fn;
        let saved_lox_fn = self.current_lox_fn.clone();
        let saved_scopes = std::mem::take(&mut self.scopes);
        let saved_return_target = self.return_target.take();
        let saved_insert_block = self.builder.get_insert_block();

        // Set up for method body
        self.current_fn = Some(llvm_fn);
        self.current_lox_fn = method_name.clone();

        let entry_bb = self.context.append_basic_block(llvm_fn, "entry");
        let exit_bb = self.context.append_basic_block(llvm_fn, "exit");
        self.builder.position_at_end(entry_bb);

        let ret_alloca = self.create_entry_block_alloca("retval");
        self.builder
            .build_store(ret_alloca, self.lox_value.build_nil(&self.builder))
            .expect("store initial retval");
        self.return_target = Some((ret_alloca, exit_bb));

        self.begin_scope();

        // Load env parameter
        let env_param = llvm_fn
            .get_nth_param(0)
            .expect("env parameter exists")
            .into_pointer_value();

        // Load "this" from env[this_env_idx]
        let this_cell_ptr_ptr = unsafe {
            self.builder
                .build_gep(
                    ptr_type,
                    env_param,
                    &[self
                        .context
                        .i64_type()
                        .const_int(this_env_idx as u64, false)],
                    "env_this_ptr",
                )
                .expect("GEP for this in env")
        };
        let this_cell = self
            .builder
            .build_load(ptr_type, this_cell_ptr_ptr, "this_cell")
            .expect("load this cell from env")
            .into_pointer_value();
        self.scopes
            .last_mut()
            .expect("have scope")
            .insert("this".to_string(), VarStorage::Cell(this_cell));

        // Load "super" from env if subclass method
        if let Some(super_idx) = super_env_idx {
            let super_cell_ptr_ptr = unsafe {
                self.builder
                    .build_gep(
                        ptr_type,
                        env_param,
                        &[self.context.i64_type().const_int(super_idx as u64, false)],
                        "env_super_ptr",
                    )
                    .expect("GEP for super in env")
            };
            let super_cell = self
                .builder
                .build_load(ptr_type, super_cell_ptr_ptr, "super_cell")
                .expect("load super cell from env")
                .into_pointer_value();
            self.scopes
                .last_mut()
                .expect("have scope")
                .insert("super".to_string(), VarStorage::Cell(super_cell));
        }

        // Load captured variables from env
        for (i, cap_name) in captured_names.iter().enumerate() {
            let env_idx = capture_offset + i;
            let cell_ptr_ptr = unsafe {
                self.builder
                    .build_gep(
                        ptr_type,
                        env_param,
                        &[self.context.i64_type().const_int(env_idx as u64, false)],
                        &format!("env_{cap_name}_ptr"),
                    )
                    .expect("GEP into env for capture")
            };
            let cell_ptr = self
                .builder
                .build_load(ptr_type, cell_ptr_ptr, &format!("env_{cap_name}"))
                .expect("load capture cell from env")
                .into_pointer_value();
            self.scopes
                .last_mut()
                .expect("have scope")
                .insert(cap_name.clone(), VarStorage::Cell(cell_ptr));
        }

        // Bind parameters as local variables
        for (i, param_name) in method.params.iter().enumerate() {
            let param_val = llvm_fn
                .get_nth_param((i + 1) as u32)
                .expect("parameter exists")
                .into_struct_value();

            let is_captured = self.captures.captured_vars.contains(&CapturedVar {
                var_name: param_name.clone(),
                declaring_function: method_name.clone(),
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

        // Compile method body
        for decl in &method.body {
            self.compile_decl(decl)?;
        }

        // Branch to exit if body didn't terminate
        if self
            .builder
            .get_insert_block()
            .expect("have insert block")
            .get_terminator()
            .is_none()
        {
            // For init methods, implicit return is "this"
            if method.name == "init" {
                let this_val = self.load_var_storage(
                    &self.find_local("this").expect("this in method scope"),
                    "this",
                );
                self.builder
                    .build_store(ret_alloca, this_val)
                    .expect("store this as init return");
            }
            self.builder
                .build_unconditional_branch(exit_bb)
                .expect("branch to exit");
        }

        // Exit block
        self.builder.position_at_end(exit_bb);
        let ret_val = self
            .builder
            .build_load(self.lox_value.llvm_type(), ret_alloca, "retval")
            .expect("load return value");

        // For init methods, always return "this" regardless of what was stored
        if method.name == "init" {
            let this_val = self.load_var_storage(
                &self.find_local("this").expect("this in method scope"),
                "this_ret",
            );
            self.builder
                .build_return(Some(&this_val))
                .expect("return this from init");
        } else {
            self.builder
                .build_return(Some(&ret_val))
                .expect("build return");
        }

        self.end_scope();

        // Restore state
        self.current_fn = saved_fn;
        self.current_lox_fn = saved_lox_fn;
        self.scopes = saved_scopes;
        self.return_target = saved_return_target;
        if let Some(bb) = saved_insert_block {
            self.builder.position_at_end(bb);
        }

        // Build the method closure with env: [this_cell, super_cell?, captures...]
        // For "this": placeholder null cell (filled by lox_bind_method at call time)
        let i32_type = self.context.i32_type();
        let null_cell = ptr_type.const_null();

        let arr_alloca = self
            .builder
            .build_array_alloca(
                ptr_type,
                i32_type.const_int(total_env_count as u64, false),
                "method_env_arr",
            )
            .expect("alloca for method env array");

        // env[0] = null (placeholder for this, filled by bind_method)
        let slot0 = unsafe {
            self.builder
                .build_gep(
                    ptr_type,
                    arr_alloca,
                    &[self.context.i64_type().const_int(0, false)],
                    "env_slot_this",
                )
                .expect("GEP for this slot")
        };
        self.builder
            .build_store(slot0, null_cell)
            .expect("store null this cell");

        // env[1] = super cell (if subclass)
        if let Some(sc_name) = superclass_name {
            let slot1 = unsafe {
                self.builder
                    .build_gep(
                        ptr_type,
                        arr_alloca,
                        &[self.context.i64_type().const_int(1, false)],
                        "env_slot_super",
                    )
                    .expect("GEP for super slot")
            };

            let super_val = self.emit_global_get(sc_name);
            let super_cell = self
                .builder
                .build_call(
                    self.runtime.lox_alloc_cell,
                    &[super_val.into()],
                    "super_cell",
                )
                .expect("alloc super cell")
                .try_as_basic_value()
                .unwrap_basic()
                .into_pointer_value();
            self.builder
                .build_store(slot1, super_cell)
                .expect("store super cell");
        }

        // env[capture_offset..] = captured variable cells
        for (i, cap_name) in captured_names.iter().enumerate() {
            let env_idx = capture_offset + i;
            let cell_ptr = self.find_cell_for_capture(cap_name);
            let slot = unsafe {
                self.builder
                    .build_gep(
                        ptr_type,
                        arr_alloca,
                        &[self.context.i64_type().const_int(env_idx as u64, false)],
                        &format!("env_slot_{env_idx}"),
                    )
                    .expect("GEP for capture slot")
            };
            self.builder
                .build_store(slot, cell_ptr)
                .expect("store capture cell in method env");
        }

        let fn_ptr = llvm_fn.as_global_value().as_pointer_value();
        let arity = i32_type.const_int(method.params.len() as u64, false);
        let name_str = self
            .builder
            .build_global_string_ptr(method_name, "method_fn_name")
            .expect("method fn name string");
        let env_count = i32_type.const_int(total_env_count as u64, false);

        let closure_ptr = self
            .builder
            .build_call(
                self.runtime.lox_alloc_closure,
                &[
                    fn_ptr.into(),
                    arity.into(),
                    name_str.as_pointer_value().into(),
                    arr_alloca.into(),
                    env_count.into(),
                ],
                "method_closure",
            )
            .expect("call lox_alloc_closure for method")
            .try_as_basic_value()
            .unwrap_basic()
            .into_pointer_value();

        Ok(closure_ptr)
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
        let current_fn = self.current_fn.expect("must be inside a function");
        let callee = self.compile_expr(&call.callee)?;
        let lv_type = self.lox_value.llvm_type();

        // Evaluate arguments upfront (needed for both function and class calls)
        let mut arg_vals = Vec::new();
        for arg in &call.arguments {
            arg_vals.push(self.compile_expr(arg)?);
        }

        // Check if callee is a class (TAG_CLASS) or function (TAG_FUNCTION)
        let callee_tag = self.lox_value.extract_tag(&self.builder, callee);
        let class_tag = self
            .context
            .i8_type()
            .const_int(u64::from(super::types::TAG_CLASS), false);
        let is_class = self
            .builder
            .build_int_compare(inkwell::IntPredicate::EQ, callee_tag, class_tag, "is_class")
            .expect("check if class call");

        let fn_call_bb = self.context.append_basic_block(current_fn, "call_fn");
        let class_call_bb = self.context.append_basic_block(current_fn, "call_class");
        let call_merge_bb = self.context.append_basic_block(current_fn, "call_merge");

        self.builder
            .build_conditional_branch(is_class, class_call_bb, fn_call_bb)
            .expect("branch on call type");

        // --- Function call path (existing logic) ---
        self.builder.position_at_end(fn_call_bb);
        let fn_result = self.emit_closure_call(callee, &arg_vals)?;
        let fn_exit_bb = self.builder.get_insert_block().expect("have block");
        self.builder
            .build_unconditional_branch(call_merge_bb)
            .expect("branch to call merge from fn");

        // --- Class instantiation path ---
        self.builder.position_at_end(class_call_bb);
        let class_result = self.emit_class_instantiation(callee, &arg_vals)?;
        let class_exit_bb = self.builder.get_insert_block().expect("have block");
        self.builder
            .build_unconditional_branch(call_merge_bb)
            .expect("branch to call merge from class");

        // Merge
        self.builder.position_at_end(call_merge_bb);
        let phi = self
            .builder
            .build_phi(lv_type, "call_result")
            .expect("phi for call result");
        phi.add_incoming(&[(&fn_result, fn_exit_bb), (&class_result, class_exit_bb)]);
        Ok(phi.as_basic_value().into_struct_value())
    }

    /// Emit an indirect call through a closure struct (TAG_FUNCTION path).
    fn emit_closure_call(
        &mut self,
        callee: StructValue<'ctx>,
        args: &[StructValue<'ctx>],
    ) -> anyhow::Result<StructValue<'ctx>> {
        let ptr_type = self.context.ptr_type(AddressSpace::default());
        let lv_type = self.lox_value.llvm_type();

        let closure_ptr_int = self.lox_value.extract_payload(&self.builder, callee);
        let closure_ptr = self
            .builder
            .build_int_to_ptr(closure_ptr_int, ptr_type, "closure_ptr")
            .expect("int to closure ptr");

        let fn_ptr_ptr = self
            .builder
            .build_struct_gep(self.closure_llvm_type(), closure_ptr, 0, "fn_ptr_ptr")
            .expect("GEP to fn_ptr");
        let fn_ptr = self
            .builder
            .build_load(ptr_type, fn_ptr_ptr, "fn_ptr")
            .expect("load fn_ptr")
            .into_pointer_value();

        let env_ptr_ptr = self
            .builder
            .build_struct_gep(self.closure_llvm_type(), closure_ptr, 3, "env_ptr_ptr")
            .expect("GEP to env_ptr");
        let env_ptr = self
            .builder
            .build_load(ptr_type, env_ptr_ptr, "env_ptr")
            .expect("load env_ptr")
            .into_pointer_value();

        let mut call_args: Vec<BasicMetadataValueEnum> = vec![env_ptr.into()];
        for arg in args {
            call_args.push((*arg).into());
        }

        let mut param_types: Vec<inkwell::types::BasicMetadataTypeEnum> = vec![ptr_type.into()];
        for _ in args {
            param_types.push(lv_type.into());
        }
        let call_fn_type = lv_type.fn_type(&param_types, false);

        let result = self
            .builder
            .build_indirect_call(call_fn_type, fn_ptr, &call_args, "fn_call_result")
            .expect("build indirect call")
            .try_as_basic_value()
            .unwrap_basic()
            .into_struct_value();
        Ok(result)
    }

    /// Emit class instantiation: allocate instance, call init if present, return instance.
    fn emit_class_instantiation(
        &mut self,
        callee: StructValue<'ctx>,
        args: &[StructValue<'ctx>],
    ) -> anyhow::Result<StructValue<'ctx>> {
        let current_fn = self.current_fn.expect("inside a function");
        let ptr_type = self.context.ptr_type(AddressSpace::default());

        // Extract class descriptor pointer
        let class_ptr_int = self.lox_value.extract_payload(&self.builder, callee);
        let class_ptr = self
            .builder
            .build_int_to_ptr(class_ptr_int, ptr_type, "class_desc_ptr")
            .expect("int to class desc ptr");

        // Allocate instance
        let instance = self
            .builder
            .build_call(
                self.runtime.lox_alloc_instance,
                &[class_ptr.into()],
                "instance",
            )
            .expect("call lox_alloc_instance")
            .try_as_basic_value()
            .unwrap_basic()
            .into_struct_value();

        // Find init method
        let init_name = self
            .builder
            .build_global_string_ptr("init", "init_name")
            .expect("init string");
        let init_closure = self
            .builder
            .build_call(
                self.runtime.lox_class_find_method,
                &[class_ptr.into(), init_name.as_pointer_value().into()],
                "init_closure",
            )
            .expect("call lox_class_find_method")
            .try_as_basic_value()
            .unwrap_basic()
            .into_pointer_value();

        // Check if init exists
        let has_init = self
            .builder
            .build_int_compare(
                inkwell::IntPredicate::NE,
                self.builder
                    .build_ptr_to_int(init_closure, self.context.i64_type(), "init_int")
                    .expect("init closure to int"),
                self.context.i64_type().const_zero(),
                "has_init",
            )
            .expect("check init null");

        let call_init_bb = self.context.append_basic_block(current_fn, "call_init");
        let skip_init_bb = self.context.append_basic_block(current_fn, "skip_init");

        self.builder
            .build_conditional_branch(has_init, call_init_bb, skip_init_bb)
            .expect("branch on init check");

        // Call init: bind to instance and call with args
        self.builder.position_at_end(call_init_bb);
        let bound_init = self
            .builder
            .build_call(
                self.runtime.lox_bind_method,
                &[instance.into(), init_closure.into()],
                "bound_init",
            )
            .expect("call lox_bind_method for init")
            .try_as_basic_value()
            .unwrap_basic()
            .into_struct_value();

        // Call the bound init as a regular closure
        self.emit_closure_call(bound_init, args)?;
        self.builder
            .build_unconditional_branch(skip_init_bb)
            .expect("branch to skip init");

        // Return the instance (not the init return value)
        self.builder.position_at_end(skip_init_bb);
        Ok(instance)
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

    fn compile_get(&mut self, get: &GetExpr) -> anyhow::Result<StructValue<'ctx>> {
        let object = self.compile_expr(&get.object)?;
        let (name_ptr, name_len) = self.build_string_constant(&get.name);
        let result = self
            .builder
            .build_call(
                self.runtime.lox_instance_get_property,
                &[object.into(), name_ptr.into(), name_len.into()],
                "get_prop",
            )
            .expect("call lox_instance_get_property")
            .try_as_basic_value()
            .unwrap_basic()
            .into_struct_value();
        Ok(result)
    }

    fn compile_set(&mut self, set: &SetExpr) -> anyhow::Result<StructValue<'ctx>> {
        let object = self.compile_expr(&set.object)?;
        let value = self.compile_expr(&set.value)?;
        let (name_ptr, name_len) = self.build_string_constant(&set.name);
        self.builder
            .build_call(
                self.runtime.lox_instance_set_field,
                &[
                    object.into(),
                    name_ptr.into(),
                    name_len.into(),
                    value.into(),
                ],
                "",
            )
            .expect("call lox_instance_set_field");
        Ok(value)
    }

    fn compile_this(&mut self, _this: &ThisExpr) -> anyhow::Result<StructValue<'ctx>> {
        // "this" is a local variable in method scope (loaded from env[0])
        if let Some(storage) = self.find_local("this") {
            Ok(self.load_var_storage(&storage, "this"))
        } else {
            anyhow::bail!("'this' used outside of a method")
        }
    }

    fn compile_super(&mut self, sup: &SuperExpr) -> anyhow::Result<StructValue<'ctx>> {
        let ptr_type = self.context.ptr_type(AddressSpace::default());

        // Load the superclass LoxValue from "super" local
        let super_val = if let Some(storage) = self.find_local("super") {
            self.load_var_storage(&storage, "super")
        } else {
            anyhow::bail!("'super' used outside of a subclass method")
        };

        // Extract class descriptor pointer from superclass value
        let super_payload = self.lox_value.extract_payload(&self.builder, super_val);
        let super_class_ptr = self
            .builder
            .build_int_to_ptr(super_payload, ptr_type, "super_class_ptr")
            .expect("super payload to ptr");

        // Find the method on the superclass
        let method_name_str = self
            .builder
            .build_global_string_ptr(&sup.method, "super_method_name")
            .expect("super method name string");
        let method_closure = self
            .builder
            .build_call(
                self.runtime.lox_class_find_method,
                &[
                    super_class_ptr.into(),
                    method_name_str.as_pointer_value().into(),
                ],
                "super_method",
            )
            .expect("call lox_class_find_method")
            .try_as_basic_value()
            .unwrap_basic()
            .into_pointer_value();

        // Bind the method to "this"
        let this_val = self.load_var_storage(
            &self.find_local("this").expect("this in method scope"),
            "this_for_super",
        );
        let bound = self
            .builder
            .build_call(
                self.runtime.lox_bind_method,
                &[this_val.into(), method_closure.into()],
                "bound_super",
            )
            .expect("call lox_bind_method")
            .try_as_basic_value()
            .unwrap_basic()
            .into_struct_value();
        Ok(bound)
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
            Expr::Get(get) => self.compile_get(get),
            Expr::Set(set) => self.compile_set(set),
            Expr::This(this) => self.compile_this(this),
            Expr::Super(sup) => self.compile_super(sup),
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
        let current_fn = self.current_fn.expect("must be inside a function");
        let left_tag = self.lox_value.extract_tag(&self.builder, left);
        let string_tag = self
            .context
            .i8_type()
            .const_int(u64::from(super::types::TAG_STRING), false);
        let is_string = self
            .builder
            .build_int_compare(inkwell::IntPredicate::EQ, left_tag, string_tag, "is_str")
            .expect("compare tag to TAG_STRING");

        let num_bb = self.context.append_basic_block(current_fn, "add_num");
        let str_bb = self.context.append_basic_block(current_fn, "add_str");
        let merge_bb = self.context.append_basic_block(current_fn, "add_merge");

        self.builder
            .build_conditional_branch(is_string, str_bb, num_bb)
            .expect("branch on add type");

        // Number addition
        self.builder.position_at_end(num_bb);
        let lhs = self.lox_value.extract_number(&self.builder, left);
        let rhs = self.lox_value.extract_number(&self.builder, right);
        let num_result = self
            .builder
            .build_float_add(lhs, rhs, "add")
            .expect("float add");
        let num_payload = self
            .builder
            .build_bit_cast(num_result, self.context.i64_type(), "add_i64")
            .expect("bitcast add result")
            .into_int_value();
        let num_val = self
            .lox_value
            .build_tagged_number(&self.builder, num_payload);
        let num_exit_bb = self.builder.get_insert_block().expect("have block");
        self.builder
            .build_unconditional_branch(merge_bb)
            .expect("branch to merge from num add");

        // String concatenation
        self.builder.position_at_end(str_bb);
        let str_val = self
            .builder
            .build_call(
                self.runtime.lox_string_concat,
                &[left.into(), right.into()],
                "concat",
            )
            .expect("call lox_string_concat")
            .try_as_basic_value()
            .unwrap_basic()
            .into_struct_value();
        let str_exit_bb = self.builder.get_insert_block().expect("have block");
        self.builder
            .build_unconditional_branch(merge_bb)
            .expect("branch to merge from str concat");

        // Merge
        self.builder.position_at_end(merge_bb);
        let phi = self
            .builder
            .build_phi(self.lox_value.llvm_type(), "add_result")
            .expect("build phi for add");
        phi.add_incoming(&[(&num_val, num_exit_bb), (&str_val, str_exit_bb)]);
        Ok(phi.as_basic_value().into_struct_value())
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
        let current_fn = self.current_fn.expect("must be inside a function");
        let left_tag = self.lox_value.extract_tag(&self.builder, left);
        let right_tag = self.lox_value.extract_tag(&self.builder, right);
        let tags_equal = self
            .builder
            .build_int_compare(inkwell::IntPredicate::EQ, left_tag, right_tag, "tags_eq")
            .expect("compare tags");

        // If tags differ, values are not equal — skip to merge with false
        let same_tag_bb = self.context.append_basic_block(current_fn, "eq_same_tag");
        let merge_bb = self.context.append_basic_block(current_fn, "eq_merge");
        let entry_bb = self.builder.get_insert_block().expect("have block");
        self.builder
            .build_conditional_branch(tags_equal, same_tag_bb, merge_bb)
            .expect("branch on tag equality");

        // Same-tag path: check if strings (need content comparison) or other (payload compare)
        self.builder.position_at_end(same_tag_bb);
        let string_tag = self
            .context
            .i8_type()
            .const_int(u64::from(super::types::TAG_STRING), false);
        let is_string = self
            .builder
            .build_int_compare(inkwell::IntPredicate::EQ, left_tag, string_tag, "is_str")
            .expect("check if string tag");

        let str_eq_bb = self.context.append_basic_block(current_fn, "eq_str");
        let payload_eq_bb = self.context.append_basic_block(current_fn, "eq_payload");
        self.builder
            .build_conditional_branch(is_string, str_eq_bb, payload_eq_bb)
            .expect("branch on string check");

        // String equality: call lox_string_equal
        self.builder.position_at_end(str_eq_bb);
        let str_eq = self
            .builder
            .build_call(
                self.runtime.lox_string_equal,
                &[left.into(), right.into()],
                "str_eq",
            )
            .expect("call lox_string_equal")
            .try_as_basic_value()
            .unwrap_basic()
            .into_int_value();
        let str_eq_exit = self.builder.get_insert_block().expect("have block");
        self.builder
            .build_unconditional_branch(merge_bb)
            .expect("branch from str eq");

        // Payload equality: compare i64 payloads directly
        self.builder.position_at_end(payload_eq_bb);
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
        let payload_exit = self.builder.get_insert_block().expect("have block");
        self.builder
            .build_unconditional_branch(merge_bb)
            .expect("branch from payload eq");

        // Merge: phi for equality result
        self.builder.position_at_end(merge_bb);
        let false_val = self.context.bool_type().const_zero();
        let phi = self
            .builder
            .build_phi(self.context.bool_type(), "eq_result")
            .expect("build phi for equality");
        phi.add_incoming(&[
            (&false_val, entry_bb),          // tags differ → false
            (&str_eq, str_eq_exit),          // string comparison result
            (&payloads_equal, payload_exit), // payload comparison result
        ]);
        let equal = phi.as_basic_value().into_int_value();

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

    // --- Phase 5: String operations ---

    #[test]
    fn string_concat() {
        let ir = compile_to_ir(r#"var a = "hello"; var b = " world"; print a + b;"#);
        assert!(
            ir.contains("lox_string_concat"),
            "should call string concat for string +"
        );
        assert!(ir.contains("add_str"), "should have string add branch");
        assert!(ir.contains("add_num"), "should have number add branch");
    }

    #[test]
    fn string_equality() {
        let ir = compile_to_ir(r#"var a = "abc"; var b = "abc"; print a == b;"#);
        assert!(
            ir.contains("lox_string_equal"),
            "should call string_equal for string =="
        );
        assert!(ir.contains("eq_str"), "should have string equality branch");
    }

    #[test]
    fn string_inequality() {
        let ir = compile_to_ir(r#"var a = "abc"; var b = "def"; print a != b;"#);
        assert!(
            ir.contains("lox_string_equal"),
            "should call string_equal for string !="
        );
    }

    // --- Phase 6: Classes and inheritance ---

    #[test]
    fn class_declaration() {
        let ir = compile_to_ir("class Cake {}");
        assert!(ir.contains("lox_alloc_class"), "should allocate class");
    }

    #[test]
    fn class_with_method() {
        let ir = compile_to_ir("class Cake { taste() { return 42; } }");
        assert!(
            ir.contains("@lox_fn_Cake_taste"),
            "should define method function"
        );
        assert!(
            ir.contains("lox_class_add_method"),
            "should add method to class"
        );
    }

    #[test]
    fn class_instantiation() {
        let ir = compile_to_ir("class Cake {} var c = Cake();");
        assert!(
            ir.contains("lox_alloc_instance"),
            "should allocate instance"
        );
        assert!(
            ir.contains("call_class"),
            "should have class instantiation branch"
        );
    }

    #[test]
    fn class_field_access() {
        let ir = compile_to_ir(
            "class Cake { init(f) { this.flavor = f; } } var c = Cake(1); print c.flavor;",
        );
        assert!(ir.contains("lox_instance_set_field"), "should set field");
        assert!(
            ir.contains("lox_instance_get_property"),
            "should get property"
        );
    }

    #[test]
    fn class_method_call() {
        let ir = compile_to_ir(
            r#"class Greeter { greet() { return "hi"; } } var g = Greeter(); print g.greet();"#,
        );
        assert!(ir.contains("@lox_fn_Greeter_greet"), "should define method");
        assert!(
            ir.contains("lox_instance_get_property"),
            "method access via get_property"
        );
    }

    #[test]
    fn class_inheritance() {
        let ir = compile_to_ir("class A {} class B < A {}");
        // Should pass superclass to lox_alloc_class
        let alloc_count = ir.matches("lox_alloc_class").count();
        assert!(
            alloc_count >= 2,
            "should allocate both classes, got {alloc_count}"
        );
    }

    #[test]
    fn class_super_call() {
        let ir = compile_to_ir(
            "class A { foo() { return 1; } } class B < A { bar() { return super.foo(); } }",
        );
        assert!(
            ir.contains("lox_class_find_method"),
            "super should find method on superclass"
        );
        assert!(
            ir.contains("lox_bind_method"),
            "super method should be bound"
        );
    }
}
