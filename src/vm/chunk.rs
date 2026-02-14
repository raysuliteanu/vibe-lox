use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt;

/// A bytecode instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, strum::AsRefStr)]
#[strum(serialize_all = "snake_case")]
#[repr(u8)]
pub enum OpCode {
    Constant,
    Nil,
    True,
    False,
    Pop,
    GetLocal,
    SetLocal,
    GetGlobal,
    SetGlobal,
    DefineGlobal,
    GetUpvalue,
    SetUpvalue,
    GetProperty,
    SetProperty,
    GetSuper,
    Equal,
    Greater,
    Less,
    Add,
    Subtract,
    Multiply,
    Divide,
    Not,
    Negate,
    Print,
    Jump,
    JumpIfFalse,
    Loop,
    Call,
    Invoke,
    SuperInvoke,
    Closure,
    CloseUpvalue,
    Return,
    Class,
    Inherit,
    Method,
}

impl fmt::Display for OpCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl TryFrom<u8> for OpCode {
    type Error = u8;

    fn try_from(byte: u8) -> Result<Self, Self::Error> {
        if byte <= OpCode::Method as u8 {
            // Safety: OpCode is repr(u8) and we've verified byte is in range
            Ok(unsafe { std::mem::transmute::<u8, OpCode>(byte) })
        } else {
            Err(byte)
        }
    }
}

/// A constant value that can appear in a chunk's constant pool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Constant {
    Number(f64),
    String(String),
    /// A function prototype with name, arity, upvalue count, and its own chunk.
    Function {
        name: String,
        arity: usize,
        upvalue_count: usize,
        chunk: Chunk,
    },
}

impl Constant {
    fn type_name(&self) -> &'static str {
        match self {
            Self::Number(_) => "Number",
            Self::String(_) => "String",
            Self::Function { .. } => "Function",
        }
    }

    fn pool_value(&self) -> String {
        match self {
            Self::Number(n) => format!("{n}"),
            Self::String(s) => format!("\"{s}\""),
            Self::Function { name, .. } => name.clone(),
        }
    }
}

impl fmt::Display for Constant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(n) => write!(f, "{n}"),
            Self::String(s) => write!(f, "\"{s}\""),
            Self::Function { name, .. } => write!(f, "<fn {name}>"),
        }
    }
}

/// A chunk of bytecode: instructions + constant pool + line info.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub code: Vec<u8>,
    pub constants: Vec<Constant>,
    pub lines: Vec<usize>,
}

impl Default for Chunk {
    fn default() -> Self {
        Self::new()
    }
}

impl Chunk {
    pub fn new() -> Self {
        Self {
            code: Vec::new(),
            constants: Vec::new(),
            lines: Vec::new(),
        }
    }

    pub fn write_op(&mut self, op: OpCode, line: usize) {
        self.code.push(op as u8);
        self.lines.push(line);
    }

    pub fn write_byte(&mut self, byte: u8, line: usize) {
        self.code.push(byte);
        self.lines.push(line);
    }

    pub fn write_u16(&mut self, value: u16, line: usize) {
        self.code.push((value >> 8) as u8);
        self.lines.push(line);
        self.code.push((value & 0xff) as u8);
        self.lines.push(line);
    }

    pub fn add_constant(&mut self, constant: Constant) -> u8 {
        self.constants.push(constant);
        (self.constants.len() - 1)
            .try_into()
            .expect("constant pool overflow (max 256)")
    }

    pub fn read_u16(&self, offset: usize) -> u16 {
        let hi = self.code[offset] as u16;
        let lo = self.code[offset + 1] as u16;
        (hi << 8) | lo
    }
}

/// Disassemble a chunk into structured, human-readable text with recursive
/// function output and constant pool display.
///
/// `source_name` is shown in the header (e.g. a file path or `"<script>"`).
pub fn disassemble(chunk: &Chunk, source_name: &str) -> Result<String> {
    let mut out = String::new();
    out.push_str(&format!("Compiled from \"{source_name}\"\n"));
    disassemble_chunk(chunk, "script", 0, &mut out)?;
    Ok(out)
}

/// Recursively disassemble a single chunk (script or function body).
fn disassemble_chunk(chunk: &Chunk, name: &str, arity: usize, out: &mut String) -> Result<()> {
    // Function header
    if name == "script" {
        out.push_str("script;\n");
    } else {
        let params: Vec<String> = (0..arity).map(|i| format!("_{i}")).collect();
        out.push_str(&format!(
            "fun {name}({});  // arity={arity}\n",
            params.join(", ")
        ));
    }

    // Constants section
    if !chunk.constants.is_empty() {
        out.push_str("  Constants:\n");
        for (i, constant) in chunk.constants.iter().enumerate() {
            out.push_str(&format!(
                "    {:>3} = {:<14}  {}\n",
                format!("#{i}"),
                constant.type_name(),
                constant.pool_value()
            ));
        }
        out.push('\n');
    }

    // Code section
    out.push_str("  Code:\n");
    let mut offset = 0;
    while offset < chunk.code.len() {
        offset = disassemble_instruction(chunk, offset, out)?;
    }
    out.push('\n');

    // Recursively disassemble nested functions
    for constant in &chunk.constants {
        if let Constant::Function {
            name,
            arity,
            chunk: fn_chunk,
            ..
        } = constant
        {
            disassemble_chunk(fn_chunk, name, *arity, out)?;
        }
    }

    Ok(())
}

/// Format a single instruction into `out`, returning the next offset.
fn disassemble_instruction(chunk: &Chunk, offset: usize, out: &mut String) -> Result<usize> {
    let byte = chunk.code[offset];
    let op = OpCode::try_from(byte)
        .map_err(|b| anyhow::anyhow!("invalid opcode {b} at offset {offset}"))?;
    let name = op.as_ref();

    match op {
        OpCode::Constant
        | OpCode::DefineGlobal
        | OpCode::GetGlobal
        | OpCode::SetGlobal
        | OpCode::Class
        | OpCode::GetProperty
        | OpCode::SetProperty
        | OpCode::Method
        | OpCode::GetSuper => {
            let idx = chunk.code[offset + 1];
            let comment = &chunk.constants[idx as usize];
            out.push_str(&format!(
                "    {:>3}: {:<18} #{:<5} // {comment}\n",
                offset, name, idx
            ));
            Ok(offset + 2)
        }
        OpCode::GetLocal
        | OpCode::SetLocal
        | OpCode::Call
        | OpCode::GetUpvalue
        | OpCode::SetUpvalue => {
            let slot = chunk.code[offset + 1];
            out.push_str(&format!("    {:>3}: {:<18} {slot}\n", offset, name));
            Ok(offset + 2)
        }
        OpCode::Jump | OpCode::JumpIfFalse => {
            let jump = chunk.read_u16(offset + 1);
            let target = offset + 3 + jump as usize;
            out.push_str(&format!("    {:>3}: {:<18} -> {target}\n", offset, name));
            Ok(offset + 3)
        }
        OpCode::Loop => {
            let jump = chunk.read_u16(offset + 1);
            let target = offset + 3 - jump as usize;
            out.push_str(&format!("    {:>3}: {:<18} -> {target}\n", offset, name));
            Ok(offset + 3)
        }
        OpCode::Invoke | OpCode::SuperInvoke => {
            let name_idx = chunk.code[offset + 1];
            let arg_count = chunk.code[offset + 2];
            let comment = &chunk.constants[name_idx as usize];
            out.push_str(&format!(
                "    {:>3}: {:<18} #{:<5} // ({arg_count} args) {comment}\n",
                offset, name, name_idx
            ));
            Ok(offset + 3)
        }
        OpCode::Closure => {
            let idx = chunk.code[offset + 1];
            let comment = &chunk.constants[idx as usize];
            out.push_str(&format!(
                "    {:>3}: {:<18} #{:<5} // {comment}\n",
                offset, name, idx
            ));
            let mut off = offset + 2;
            if let Constant::Function { upvalue_count, .. } = &chunk.constants[idx as usize] {
                for _ in 0..*upvalue_count {
                    let is_local = chunk.code[off];
                    let index = chunk.code[off + 1];
                    let kind = if is_local == 1 { "local" } else { "upvalue" };
                    out.push_str(&format!("           | {kind} {index}\n"));
                    off += 2;
                }
            }
            Ok(off)
        }
        _ => {
            out.push_str(&format!("    {:>3}: {name}\n", offset));
            Ok(offset + 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read_constant() {
        let mut chunk = Chunk::new();
        let idx = chunk.add_constant(Constant::Number(1.2));
        chunk.write_op(OpCode::Constant, 1);
        chunk.write_byte(idx, 1);

        assert_eq!(chunk.code.len(), 2);
        assert_eq!(chunk.code[0], OpCode::Constant as u8);
        assert_eq!(chunk.constants[idx as usize], Constant::Number(1.2));
    }

    #[test]
    fn disassemble_simple() {
        let mut chunk = Chunk::new();
        let idx = chunk.add_constant(Constant::Number(42.0));
        chunk.write_op(OpCode::Constant, 1);
        chunk.write_byte(idx, 1);
        chunk.write_op(OpCode::Return, 1);

        let text = disassemble(&chunk, "test").expect("valid bytecode");
        assert!(text.contains("constant"));
        assert!(text.contains("42"));
        assert!(text.contains("return"));
    }

    #[test]
    fn serialize_deserialize_chunk() {
        let mut chunk = Chunk::new();
        #[allow(clippy::approx_constant)]
        chunk.add_constant(Constant::Number(3.14));
        chunk.write_op(OpCode::Constant, 1);
        chunk.write_byte(0, 1);
        chunk.write_op(OpCode::Return, 1);

        let serialized = rmp_serde::to_vec(&chunk).expect("serialize");
        let deserialized: Chunk = rmp_serde::from_slice(&serialized).expect("deserialize");
        assert_eq!(chunk, deserialized);
    }

    // ========== Additional Chunk Tests ==========

    // ========== Basic Operations ==========

    #[test]
    fn new_chunk_is_empty() {
        let chunk = Chunk::new();
        assert!(chunk.code.is_empty());
        assert!(chunk.constants.is_empty());
        assert!(chunk.lines.is_empty());
    }

    #[test]
    fn write_multiple_opcodes() {
        let mut chunk = Chunk::new();
        chunk.write_op(OpCode::Nil, 1);
        chunk.write_op(OpCode::True, 1);
        chunk.write_op(OpCode::False, 1);

        assert_eq!(chunk.code.len(), 3);
        assert_eq!(chunk.code[0], OpCode::Nil as u8);
        assert_eq!(chunk.code[1], OpCode::True as u8);
        assert_eq!(chunk.code[2], OpCode::False as u8);
    }

    #[test]
    fn line_numbers_tracked() {
        let mut chunk = Chunk::new();
        chunk.write_op(OpCode::Constant, 1);
        chunk.write_byte(0, 1);
        chunk.write_op(OpCode::Return, 2);

        assert_eq!(chunk.lines.len(), 3);
        assert_eq!(chunk.lines[0], 1);
        assert_eq!(chunk.lines[1], 1);
        assert_eq!(chunk.lines[2], 2);
    }

    // ========== Constant Pool ==========

    #[test]
    fn add_number_constant() {
        let mut chunk = Chunk::new();
        let idx = chunk.add_constant(Constant::Number(42.0));
        assert_eq!(idx, 0);
        assert_eq!(chunk.constants.len(), 1);
        assert_eq!(chunk.constants[0], Constant::Number(42.0));
    }

    #[test]
    fn add_string_constant() {
        let mut chunk = Chunk::new();
        let idx = chunk.add_constant(Constant::String("hello".to_string()));
        assert_eq!(idx, 0);
        assert!(matches!(&chunk.constants[0], Constant::String(s) if s == "hello"));
    }

    #[test]
    fn add_multiple_constants() {
        let mut chunk = Chunk::new();
        let idx1 = chunk.add_constant(Constant::Number(1.0));
        let idx2 = chunk.add_constant(Constant::Number(2.0));
        let idx3 = chunk.add_constant(Constant::String("test".to_string()));

        assert_eq!(idx1, 0);
        assert_eq!(idx2, 1);
        assert_eq!(idx3, 2);
        assert_eq!(chunk.constants.len(), 3);
    }

    #[test]
    fn constant_pool_max_255() {
        let mut chunk = Chunk::new();
        for i in 0..255 {
            let idx = chunk.add_constant(Constant::Number(i as f64));
            assert_eq!(idx, i as u8);
        }
        assert_eq!(chunk.constants.len(), 255);
    }

    #[test]
    #[should_panic(expected = "constant pool overflow")]
    fn constant_pool_overflow() {
        let mut chunk = Chunk::new();
        for i in 0..257 {
            chunk.add_constant(Constant::Number(i as f64));
        }
    }

    // ========== U16 Operations ==========

    #[test]
    fn write_and_read_u16_small() {
        let mut chunk = Chunk::new();
        chunk.write_u16(42, 1);
        assert_eq!(chunk.code.len(), 2);
        assert_eq!(chunk.read_u16(0), 42);
    }

    #[test]
    fn write_and_read_u16_large() {
        let mut chunk = Chunk::new();
        chunk.write_u16(0xABCD, 1);
        assert_eq!(chunk.read_u16(0), 0xABCD);
    }

    #[test]
    fn write_and_read_u16_max() {
        let mut chunk = Chunk::new();
        chunk.write_u16(0xFFFF, 1);
        assert_eq!(chunk.read_u16(0), 0xFFFF);
    }

    #[test]
    fn write_and_read_u16_zero() {
        let mut chunk = Chunk::new();
        chunk.write_u16(0, 1);
        assert_eq!(chunk.read_u16(0), 0);
    }

    // ========== Disassembly ==========

    #[test]
    fn disassemble_header_and_structure() {
        let mut chunk = Chunk::new();
        chunk.write_op(OpCode::Return, 1);
        let text = disassemble(&chunk, "test_chunk.lox").expect("valid bytecode");
        assert!(text.contains("Compiled from \"test_chunk.lox\""));
        assert!(text.contains("script;"));
        assert!(text.contains("Code:"));
    }

    #[test]
    fn disassemble_all_simple_opcodes() {
        let mut chunk = Chunk::new();
        chunk.write_op(OpCode::Nil, 1);
        chunk.write_op(OpCode::True, 1);
        chunk.write_op(OpCode::False, 1);
        chunk.write_op(OpCode::Pop, 1);
        chunk.write_op(OpCode::Add, 1);
        chunk.write_op(OpCode::Subtract, 1);
        chunk.write_op(OpCode::Multiply, 1);
        chunk.write_op(OpCode::Divide, 1);
        chunk.write_op(OpCode::Negate, 1);
        chunk.write_op(OpCode::Not, 1);
        chunk.write_op(OpCode::Equal, 1);
        chunk.write_op(OpCode::Greater, 1);
        chunk.write_op(OpCode::Less, 1);
        chunk.write_op(OpCode::Return, 1);

        let text = disassemble(&chunk, "test").expect("valid bytecode");
        assert!(text.contains("nil"));
        assert!(text.contains("add"));
        assert!(text.contains("subtract"));
        assert!(text.contains("return"));
    }

    #[test]
    fn disassemble_constant_instruction() {
        let mut chunk = Chunk::new();
        let idx = chunk.add_constant(Constant::Number(123.45));
        chunk.write_op(OpCode::Constant, 1);
        chunk.write_byte(idx, 1);

        let text = disassemble(&chunk, "test").expect("valid bytecode");
        assert!(text.contains("constant"));
        assert!(text.contains("123.45"));
        assert!(text.contains("#0"));
    }

    #[test]
    fn disassemble_string_constant() {
        let mut chunk = Chunk::new();
        let idx = chunk.add_constant(Constant::String("hello world".to_string()));
        chunk.write_op(OpCode::Constant, 1);
        chunk.write_byte(idx, 1);

        let text = disassemble(&chunk, "test").expect("valid bytecode");
        assert!(text.contains("hello world"));
    }

    #[test]
    fn disassemble_jump_instruction() {
        let mut chunk = Chunk::new();
        chunk.write_op(OpCode::Jump, 1);
        chunk.write_u16(10, 1);

        let text = disassemble(&chunk, "test").expect("valid bytecode");
        assert!(text.contains("jump"));
        assert!(text.contains("-> 13"));
    }

    #[test]
    fn disassemble_loop_instruction() {
        let mut chunk = Chunk::new();
        chunk.write_op(OpCode::Nil, 1);
        chunk.write_op(OpCode::Nil, 1);
        chunk.write_op(OpCode::Loop, 1);
        chunk.write_u16(2, 1);

        let text = disassemble(&chunk, "test").expect("valid bytecode");
        assert!(text.contains("loop"));
        assert!(text.contains("-> 3"));
    }

    #[test]
    fn disassemble_local_variable_instructions() {
        let mut chunk = Chunk::new();
        chunk.write_op(OpCode::GetLocal, 1);
        chunk.write_byte(3, 1);
        chunk.write_op(OpCode::SetLocal, 1);
        chunk.write_byte(5, 1);

        let text = disassemble(&chunk, "test").expect("valid bytecode");
        assert!(text.contains("get_local"));
        assert!(text.contains("set_local"));
    }

    #[test]
    fn disassemble_constants_section() {
        let mut chunk = Chunk::new();
        chunk.add_constant(Constant::Number(42.0));
        chunk.add_constant(Constant::String("hello".to_string()));
        chunk.write_op(OpCode::Return, 1);

        let text = disassemble(&chunk, "test").expect("valid bytecode");
        assert!(text.contains("Constants:"));
        assert!(text.contains("#0 = Number"));
        assert!(text.contains("42"));
        assert!(text.contains("#1 = String"));
        assert!(text.contains("\"hello\""));
    }

    // ========== Serialization Edge Cases ==========

    #[test]
    fn serialize_empty_chunk() {
        let chunk = Chunk::new();
        let serialized = rmp_serde::to_vec(&chunk).expect("serialize");
        let deserialized: Chunk = rmp_serde::from_slice(&serialized).expect("deserialize");
        assert_eq!(chunk, deserialized);
    }

    #[test]
    fn serialize_chunk_with_many_constants() {
        let mut chunk = Chunk::new();
        for i in 0..10 {
            chunk.add_constant(Constant::Number(i as f64));
        }
        chunk.write_op(OpCode::Return, 1);

        let serialized = rmp_serde::to_vec(&chunk).expect("serialize");
        let deserialized: Chunk = rmp_serde::from_slice(&serialized).expect("deserialize");
        assert_eq!(chunk, deserialized);
    }

    #[test]
    fn serialize_chunk_with_function() {
        let mut inner_chunk = Chunk::new();
        inner_chunk.write_op(OpCode::Return, 1);

        let mut chunk = Chunk::new();
        chunk.add_constant(Constant::Function {
            name: "test".to_string(),
            arity: 2,
            upvalue_count: 0,
            chunk: inner_chunk,
        });
        chunk.write_op(OpCode::Return, 1);

        let serialized = rmp_serde::to_vec(&chunk).expect("serialize");
        let deserialized: Chunk = rmp_serde::from_slice(&serialized).expect("deserialize");
        assert_eq!(chunk, deserialized);
    }

    // ========== OpCode Conversion ==========

    #[test]
    fn opcode_try_from_valid() {
        assert_eq!(OpCode::try_from(OpCode::Nil as u8), Ok(OpCode::Nil));
        assert_eq!(OpCode::try_from(OpCode::True as u8), Ok(OpCode::True));
        assert_eq!(OpCode::try_from(OpCode::Return as u8), Ok(OpCode::Return));
        assert_eq!(OpCode::try_from(OpCode::Method as u8), Ok(OpCode::Method));
    }

    #[test]
    fn opcode_try_from_invalid() {
        assert_eq!(OpCode::try_from(255), Err(255));
        assert_eq!(OpCode::try_from(200), Err(200));
    }

    // ========== Default Trait ==========

    #[test]
    fn default_chunk_is_empty() {
        let chunk = Chunk::default();
        assert!(chunk.code.is_empty());
        assert!(chunk.constants.is_empty());
        assert!(chunk.lines.is_empty());
    }

    // ========== New Disassembly Format Tests ==========

    #[test]
    fn test_snake_case_names() {
        assert_eq!(OpCode::Constant.as_ref(), "constant");
        assert_eq!(OpCode::JumpIfFalse.as_ref(), "jump_if_false");
        assert_eq!(OpCode::GetLocal.as_ref(), "get_local");
        assert_eq!(OpCode::DefineGlobal.as_ref(), "define_global");
        assert_eq!(OpCode::CloseUpvalue.as_ref(), "close_upvalue");
        assert_eq!(OpCode::SuperInvoke.as_ref(), "super_invoke");
        assert_eq!(OpCode::GetSuper.as_ref(), "get_super");
        assert_eq!(OpCode::Return.as_ref(), "return");
    }

    #[test]
    fn test_nested_function_disassembly() {
        let mut inner_chunk = Chunk::new();
        inner_chunk.add_constant(Constant::Number(1.0));
        inner_chunk.write_op(OpCode::Constant, 1);
        inner_chunk.write_byte(0, 1);
        inner_chunk.write_op(OpCode::Return, 1);

        let mut chunk = Chunk::new();
        let fn_idx = chunk.add_constant(Constant::Function {
            name: "add".to_string(),
            arity: 2,
            upvalue_count: 0,
            chunk: inner_chunk,
        });
        chunk.write_op(OpCode::Closure, 1);
        chunk.write_byte(fn_idx, 1);
        chunk.write_op(OpCode::Return, 1);

        let text = disassemble(&chunk, "test.lox").expect("valid bytecode");
        // Top-level script section
        assert!(text.contains("script;"));
        assert!(text.contains("closure"));
        // Nested function section
        assert!(text.contains("fun add(_0, _1);  // arity=2"));
        assert!(text.contains("constant"));
    }

    #[test]
    fn test_jump_target_format() {
        let mut chunk = Chunk::new();
        // jump_if_false at offset 0, jumping over 7 bytes → target = 0 + 3 + 7 = 10
        chunk.write_op(OpCode::JumpIfFalse, 1);
        chunk.write_u16(7, 1);
        // some filler
        for _ in 0..7 {
            chunk.write_op(OpCode::Pop, 1);
        }

        let text = disassemble(&chunk, "test").expect("valid bytecode");
        assert!(text.contains("jump_if_false"));
        assert!(text.contains("-> 10"));
    }

    #[test]
    fn test_closure_upvalue_display() {
        let mut inner_chunk = Chunk::new();
        inner_chunk.write_op(OpCode::Return, 1);

        let mut chunk = Chunk::new();
        let fn_idx = chunk.add_constant(Constant::Function {
            name: "closure_fn".to_string(),
            arity: 0,
            upvalue_count: 2,
            chunk: inner_chunk,
        });
        chunk.write_op(OpCode::Closure, 1);
        chunk.write_byte(fn_idx, 1);
        // upvalue 0: local slot 1
        chunk.write_byte(1, 1);
        chunk.write_byte(1, 1);
        // upvalue 1: captured upvalue index 0
        chunk.write_byte(0, 1);
        chunk.write_byte(0, 1);
        chunk.write_op(OpCode::Return, 1);

        let text = disassemble(&chunk, "test").expect("valid bytecode");
        assert!(text.contains("closure"));
        assert!(text.contains("| local 1"));
        assert!(text.contains("| upvalue 0"));
    }
}
