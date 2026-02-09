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

        let serialized = serde_json::to_string(&chunk).expect("serialize");
        let deserialized: Chunk = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(chunk, deserialized);
    }
}
