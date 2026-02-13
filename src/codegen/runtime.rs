use inkwell::AddressSpace;
use inkwell::module::Module;
use inkwell::values::FunctionValue;

use super::types::LoxValueType;

/// Declares the external C runtime functions in the LLVM module.
///
/// These correspond to functions implemented in `runtime/lox_runtime.c`.
/// Only Phase 1 functions are declared here; later phases add more.
pub struct RuntimeDecls<'ctx> {
    pub lox_print: FunctionValue<'ctx>,
    pub lox_global_get: FunctionValue<'ctx>,
    pub lox_global_set: FunctionValue<'ctx>,
    pub lox_value_truthy: FunctionValue<'ctx>,
    pub lox_runtime_error: FunctionValue<'ctx>,
}

impl<'ctx> RuntimeDecls<'ctx> {
    pub fn declare(module: &Module<'ctx>, lox_value: &LoxValueType<'ctx>) -> Self {
        let context = module.get_context();
        let void_type = context.void_type();
        let i1_type = context.bool_type();
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
        let i32_type = context.i32_type();
        let lox_runtime_error_ty =
            void_type.fn_type(&[ptr_type.into(), i64_type.into(), i32_type.into()], false);
        let lox_runtime_error =
            module.add_function("lox_runtime_error", lox_runtime_error_ty, None);

        Self {
            lox_print,
            lox_global_get,
            lox_global_set,
            lox_value_truthy,
            lox_runtime_error,
        }
    }
}
