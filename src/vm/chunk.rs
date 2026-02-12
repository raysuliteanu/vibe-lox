use serde::{Deserialize, Serialize};

/// A bytecode instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

impl std::fmt::Display for OpCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
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

impl std::fmt::Display for Constant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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

/// Disassemble a chunk into human-readable text.
pub fn disassemble(chunk: &Chunk, name: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("== {name} ==\n"));
    let mut offset = 0;
    while offset < chunk.code.len() {
        offset = disassemble_instruction(chunk, offset, &mut out);
    }
    out
}

fn disassemble_instruction(chunk: &Chunk, offset: usize, out: &mut String) -> usize {
    out.push_str(&format!("{offset:04} "));

    if offset > 0 && chunk.lines[offset] == chunk.lines[offset - 1] {
        out.push_str("   | ");
    } else {
        out.push_str(&format!("{:4} ", chunk.lines[offset]));
    }

    let byte = chunk.code[offset];
    let op: Option<OpCode> = op_from_u8(byte);

    match op {
        Some(op) => match op {
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
                out.push_str(&format!(
                    "{op:<16} {idx:4} '{}'\n",
                    chunk.constants[idx as usize]
                ));
                offset + 2
            }
            OpCode::GetLocal
            | OpCode::SetLocal
            | OpCode::Call
            | OpCode::GetUpvalue
            | OpCode::SetUpvalue => {
                let slot = chunk.code[offset + 1];
                out.push_str(&format!("{op:<16} {slot:4}\n"));
                offset + 2
            }
            OpCode::Jump | OpCode::JumpIfFalse => {
                let jump = chunk.read_u16(offset + 1);
                let target = offset + 3 + jump as usize;
                out.push_str(&format!("{op:<16} {offset:4} -> {target}\n"));
                offset + 3
            }
            OpCode::Loop => {
                let jump = chunk.read_u16(offset + 1);
                let target = offset + 3 - jump as usize;
                out.push_str(&format!("{op:<16} {offset:4} -> {target}\n"));
                offset + 3
            }
            OpCode::Invoke | OpCode::SuperInvoke => {
                let name_idx = chunk.code[offset + 1];
                let arg_count = chunk.code[offset + 2];
                out.push_str(&format!(
                    "{op:<16} ({arg_count} args) {name_idx:4} '{}'\n",
                    chunk.constants[name_idx as usize]
                ));
                offset + 3
            }
            OpCode::Closure => {
                let mut off = offset + 1;
                let idx = chunk.code[off];
                off += 1;
                out.push_str(&format!(
                    "{op:<16} {idx:4} {}\n",
                    chunk.constants[idx as usize]
                ));
                if let Constant::Function { upvalue_count, .. } = &chunk.constants[idx as usize] {
                    for _ in 0..*upvalue_count {
                        let is_local = chunk.code[off];
                        let index = chunk.code[off + 1];
                        let kind = if is_local == 1 { "local" } else { "upvalue" };
                        out.push_str(&format!(
                            "{:04}    |                     {kind} {index}\n",
                            off
                        ));
                        off += 2;
                    }
                }
                off
            }
            _ => {
                out.push_str(&format!("{op}\n"));
                offset + 1
            }
        },
        None => {
            out.push_str(&format!("Unknown opcode {byte}\n"));
            offset + 1
        }
    }
}

fn op_from_u8(byte: u8) -> Option<OpCode> {
    // Safety: OpCode is repr(u8), so this is safe for valid values
    if byte <= OpCode::Method as u8 {
        Some(unsafe { std::mem::transmute::<u8, OpCode>(byte) })
    } else {
        None
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

        let text = disassemble(&chunk, "test");
        assert!(text.contains("Constant"));
        assert!(text.contains("42"));
        assert!(text.contains("Return"));
    }

    #[test]
    fn serialize_deserialize_chunk() {
        let mut chunk = Chunk::new();
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
    fn disassemble_with_name() {
        let mut chunk = Chunk::new();
        chunk.write_op(OpCode::Return, 1);
        let text = disassemble(&chunk, "test_chunk");
        assert!(text.contains("== test_chunk =="));
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

        let text = disassemble(&chunk, "test");
        assert!(text.contains("Nil"));
        assert!(text.contains("True"));
        assert!(text.contains("False"));
        assert!(text.contains("Add"));
        assert!(text.contains("Return"));
    }

    #[test]
    fn disassemble_constant_instruction() {
        let mut chunk = Chunk::new();
        let idx = chunk.add_constant(Constant::Number(123.45));
        chunk.write_op(OpCode::Constant, 1);
        chunk.write_byte(idx, 1);

        let text = disassemble(&chunk, "test");
        assert!(text.contains("Constant"));
        assert!(text.contains("123.45"));
    }

    #[test]
    fn disassemble_string_constant() {
        let mut chunk = Chunk::new();
        let idx = chunk.add_constant(Constant::String("hello world".to_string()));
        chunk.write_op(OpCode::Constant, 1);
        chunk.write_byte(idx, 1);

        let text = disassemble(&chunk, "test");
        assert!(text.contains("hello world"));
    }

    #[test]
    fn disassemble_jump_instruction() {
        let mut chunk = Chunk::new();
        chunk.write_op(OpCode::Jump, 1);
        chunk.write_u16(10, 1);

        let text = disassemble(&chunk, "test");
        assert!(text.contains("Jump"));
        assert!(text.contains("->"));
    }

    #[test]
    fn disassemble_loop_instruction() {
        let mut chunk = Chunk::new();
        // Write some code first so loop has something to jump back to
        chunk.write_op(OpCode::Nil, 1);
        chunk.write_op(OpCode::Nil, 1);
        chunk.write_op(OpCode::Loop, 1);
        chunk.write_u16(2, 1);

        let text = disassemble(&chunk, "test");
        assert!(text.contains("Loop"));
    }

    #[test]
    fn disassemble_local_variable_instructions() {
        let mut chunk = Chunk::new();
        chunk.write_op(OpCode::GetLocal, 1);
        chunk.write_byte(3, 1);
        chunk.write_op(OpCode::SetLocal, 1);
        chunk.write_byte(5, 1);

        let text = disassemble(&chunk, "test");
        assert!(text.contains("GetLocal"));
        assert!(text.contains("SetLocal"));
    }

    #[test]
    fn disassemble_line_numbers_shown() {
        let mut chunk = Chunk::new();
        chunk.write_op(OpCode::Nil, 1);
        chunk.write_op(OpCode::True, 2);
        chunk.write_op(OpCode::False, 3);

        let text = disassemble(&chunk, "test");
        // First instruction shows line number
        assert!(text.contains("   1"));
        // Subsequent different lines show new numbers
        assert!(text.contains("   2"));
        assert!(text.contains("   3"));
    }

    #[test]
    fn disassemble_same_line_shows_pipe() {
        let mut chunk = Chunk::new();
        chunk.write_op(OpCode::Nil, 1);
        chunk.write_op(OpCode::True, 1);

        let text = disassemble(&chunk, "test");
        // Second instruction on same line should show |
        assert!(text.contains("   |"));
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
    fn op_from_u8_valid() {
        assert_eq!(op_from_u8(OpCode::Nil as u8), Some(OpCode::Nil));
        assert_eq!(op_from_u8(OpCode::True as u8), Some(OpCode::True));
        assert_eq!(op_from_u8(OpCode::Return as u8), Some(OpCode::Return));
        assert_eq!(op_from_u8(OpCode::Method as u8), Some(OpCode::Method));
    }

    #[test]
    fn op_from_u8_invalid() {
        assert_eq!(op_from_u8(255), None);
        assert_eq!(op_from_u8(200), None);
    }

    // ========== Default Trait ==========

    #[test]
    fn default_chunk_is_empty() {
        let chunk = Chunk::default();
        assert!(chunk.code.is_empty());
        assert!(chunk.constants.is_empty());
        assert!(chunk.lines.is_empty());
    }
}
