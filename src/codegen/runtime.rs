use inkwell::AddressSpace;
use inkwell::module::Module;
use inkwell::values::FunctionValue;

use super::types::LoxValueType;

/// Declares the external C runtime functions in the LLVM module.
///
/// These correspond to functions implemented in `runtime/lox_runtime.c`.
pub struct RuntimeDecls<'ctx> {
    pub lox_print: FunctionValue<'ctx>,
    pub lox_global_get: FunctionValue<'ctx>,
    pub lox_global_set: FunctionValue<'ctx>,
    pub lox_value_truthy: FunctionValue<'ctx>,
    pub lox_runtime_error: FunctionValue<'ctx>,
    pub lox_alloc_closure: FunctionValue<'ctx>,
    pub lox_alloc_cell: FunctionValue<'ctx>,
    pub lox_cell_get: FunctionValue<'ctx>,
    pub lox_cell_set: FunctionValue<'ctx>,
    pub lox_string_concat: FunctionValue<'ctx>,
    pub lox_string_equal: FunctionValue<'ctx>,
    pub lox_clock: FunctionValue<'ctx>,
}

impl<'ctx> RuntimeDecls<'ctx> {
    pub fn declare(module: &Module<'ctx>, lox_value: &LoxValueType<'ctx>) -> Self {
        let context = module.get_context();
        let void_type = context.void_type();
        let i1_type = context.bool_type();
        let i32_type = context.i32_type();
        let i64_type = context.i64_type();
        let ptr_type = context.ptr_type(AddressSpace::default());
        let lv_type = lox_value.llvm_type();

        // void lox_print(LoxValue value)
        let lox_print_ty = void_type.fn_type(&[lv_type.into()], false);
        let lox_print = module.add_function("lox_print", lox_print_ty, None);

        // LoxValue lox_global_get(i8* name, i64 name_len)
        let lox_global_get_ty = lv_type.fn_type(&[ptr_type.into(), i64_type.into()], false);
        let lox_global_get = module.add_function("lox_global_get", lox_global_get_ty, None);

        // void lox_global_set(i8* name, i64 name_len, LoxValue value)
        let lox_global_set_ty =
            void_type.fn_type(&[ptr_type.into(), i64_type.into(), lv_type.into()], false);
        let lox_global_set = module.add_function("lox_global_set", lox_global_set_ty, None);

        // i1 lox_value_truthy(LoxValue value)
        let lox_value_truthy_ty = i1_type.fn_type(&[lv_type.into()], false);
        let lox_value_truthy = module.add_function("lox_value_truthy", lox_value_truthy_ty, None);

        // void lox_runtime_error(i8* message, i64 message_len, i32 line)
        let lox_runtime_error_ty =
            void_type.fn_type(&[ptr_type.into(), i64_type.into(), i32_type.into()], false);
        let lox_runtime_error =
            module.add_function("lox_runtime_error", lox_runtime_error_ty, None);

        // LoxClosure* lox_alloc_closure(void* fn_ptr, i32 arity, i8* name,
        //                                LoxValue** env, i32 env_count)
        let lox_alloc_closure_ty = ptr_type.fn_type(
            &[
                ptr_type.into(),
                i32_type.into(),
                ptr_type.into(),
                ptr_type.into(),
                i32_type.into(),
            ],
            false,
        );
        let lox_alloc_closure =
            module.add_function("lox_alloc_closure", lox_alloc_closure_ty, None);

        // LoxCell* lox_alloc_cell(LoxValue initial)
        let lox_alloc_cell_ty = ptr_type.fn_type(&[lv_type.into()], false);
        let lox_alloc_cell = module.add_function("lox_alloc_cell", lox_alloc_cell_ty, None);

        // LoxValue lox_cell_get(LoxCell* cell)
        let lox_cell_get_ty = lv_type.fn_type(&[ptr_type.into()], false);
        let lox_cell_get = module.add_function("lox_cell_get", lox_cell_get_ty, None);

        // void lox_cell_set(LoxCell* cell, LoxValue value)
        let lox_cell_set_ty = void_type.fn_type(&[ptr_type.into(), lv_type.into()], false);
        let lox_cell_set = module.add_function("lox_cell_set", lox_cell_set_ty, None);

        // LoxValue lox_string_concat(LoxValue a, LoxValue b)
        let lox_string_concat_ty = lv_type.fn_type(&[lv_type.into(), lv_type.into()], false);
        let lox_string_concat =
            module.add_function("lox_string_concat", lox_string_concat_ty, None);

        // i1 lox_string_equal(LoxValue a, LoxValue b)
        let lox_string_equal_ty = i1_type.fn_type(&[lv_type.into(), lv_type.into()], false);
        let lox_string_equal = module.add_function("lox_string_equal", lox_string_equal_ty, None);

        // LoxValue lox_clock(void)
        let lox_clock_ty = lv_type.fn_type(&[], false);
        let lox_clock = module.add_function("lox_clock", lox_clock_ty, None);

        Self {
            lox_print,
            lox_global_get,
            lox_global_set,
            lox_value_truthy,
            lox_runtime_error,
            lox_alloc_closure,
            lox_alloc_cell,
            lox_cell_get,
            lox_cell_set,
            lox_string_concat,
            lox_string_equal,
            lox_clock,
        }
    }
}
