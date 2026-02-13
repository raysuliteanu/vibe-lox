use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::types::StructType;
use inkwell::values::{IntValue, StructValue};

/// Tag constants for the LoxValue tagged union.
/// These must match the C runtime's tag definitions exactly.
pub const TAG_NIL: u8 = 0;
pub const TAG_BOOL: u8 = 1;
pub const TAG_NUMBER: u8 = 2;
pub const TAG_STRING: u8 = 3;
pub const TAG_FUNCTION: u8 = 4;
pub const TAG_CLASS: u8 = 5;
pub const TAG_INSTANCE: u8 = 6;

/// Helper for building and extracting LoxValue structs in LLVM IR.
///
/// LoxValue is `{ i8, i64 }` where:
/// - field 0 (i8): type tag
/// - field 1 (i64): payload (interpretation depends on tag)
pub struct LoxValueType<'ctx> {
    context: &'ctx Context,
    struct_type: StructType<'ctx>,
}

impl<'ctx> LoxValueType<'ctx> {
    pub fn new(context: &'ctx Context) -> Self {
        let struct_type = context.struct_type(
            &[
                context.i8_type().into(),  // tag
                context.i64_type().into(), // payload
            ],
            false,
        );
        Self {
            context,
            struct_type,
        }
    }

    pub fn llvm_type(&self) -> StructType<'ctx> {
        self.struct_type
    }

    /// Build a nil LoxValue constant.
    pub fn build_nil(&self, builder: &Builder<'ctx>) -> StructValue<'ctx> {
        self.build_tagged_value(builder, TAG_NIL, self.context.i64_type().const_zero())
    }

    /// Build a bool LoxValue.
    pub fn build_bool(&self, builder: &Builder<'ctx>, value: bool) -> StructValue<'ctx> {
        let payload = self.context.i64_type().const_int(u64::from(value), false);
        self.build_tagged_value(builder, TAG_BOOL, payload)
    }

    /// Build a number LoxValue from an f64.
    pub fn build_number(&self, builder: &Builder<'ctx>, value: f64) -> StructValue<'ctx> {
        let f = self.context.f64_type().const_float(value);
        let payload = builder
            .build_bit_cast(f, self.context.i64_type(), "num_to_i64")
            .expect("bitcast f64 to i64")
            .into_int_value();
        self.build_tagged_value(builder, TAG_NUMBER, payload)
    }

    /// Build a string LoxValue from a pointer stored as i64.
    pub fn build_string(&self, builder: &Builder<'ctx>, ptr: IntValue<'ctx>) -> StructValue<'ctx> {
        self.build_tagged_value(builder, TAG_STRING, ptr)
    }

    /// Build a number LoxValue from a pre-bitcasted i64 payload.
    pub fn build_tagged_number(
        &self,
        builder: &Builder<'ctx>,
        payload: IntValue<'ctx>,
    ) -> StructValue<'ctx> {
        self.build_tagged_value(builder, TAG_NUMBER, payload)
    }

    /// Build a bool LoxValue from an LLVM i1 value.
    pub fn build_bool_from_i1(
        &self,
        builder: &Builder<'ctx>,
        value: IntValue<'ctx>,
    ) -> StructValue<'ctx> {
        let payload = builder
            .build_int_z_extend(value, self.context.i64_type(), "bool_ext")
            .expect("zero-extend i1 to i64");
        self.build_tagged_value(builder, TAG_BOOL, payload)
    }

    /// Extract the tag (i8) from a LoxValue.
    pub fn extract_tag(&self, builder: &Builder<'ctx>, value: StructValue<'ctx>) -> IntValue<'ctx> {
        builder
            .build_extract_value(value, 0, "tag")
            .expect("extract tag from LoxValue")
            .into_int_value()
    }

    /// Extract the payload (i64) from a LoxValue.
    pub fn extract_payload(
        &self,
        builder: &Builder<'ctx>,
        value: StructValue<'ctx>,
    ) -> IntValue<'ctx> {
        builder
            .build_extract_value(value, 1, "payload")
            .expect("extract payload from LoxValue")
            .into_int_value()
    }

    /// Extract the payload as an f64 (for number values).
    pub fn extract_number(
        &self,
        builder: &Builder<'ctx>,
        value: StructValue<'ctx>,
    ) -> inkwell::values::FloatValue<'ctx> {
        let payload = self.extract_payload(builder, value);
        builder
            .build_bit_cast(payload, self.context.f64_type(), "i64_to_f64")
            .expect("bitcast i64 to f64")
            .into_float_value()
    }

    /// Build a LoxValue from a tag constant and an i64 payload.
    fn build_tagged_value(
        &self,
        builder: &Builder<'ctx>,
        tag: u8,
        payload: IntValue<'ctx>,
    ) -> StructValue<'ctx> {
        let tag_val = self.context.i8_type().const_int(u64::from(tag), false);
        let undef = self.struct_type.get_undef();
        let with_tag = builder
            .build_insert_value(undef, tag_val, 0, "with_tag")
            .expect("insert tag into LoxValue")
            .into_struct_value();
        builder
            .build_insert_value(with_tag, payload, 1, "lox_value")
            .expect("insert payload into LoxValue")
            .into_struct_value()
    }
}
