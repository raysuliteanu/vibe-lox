use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::LoxError;
use crate::vm::chunk::{Chunk, Constant, OpCode};

#[derive(Debug, Clone)]
enum VmValue {
    Number(f64),
    Bool(bool),
    Nil,
    String(Rc<String>),
    #[allow(dead_code)]
    Function(Rc<VmFunction>),
    Closure(Rc<VmClosure>),
    NativeFunction(NativeFn),
    Class(Rc<RefCell<VmClass>>),
    Instance(Rc<RefCell<VmInstance>>),
    BoundMethod(Rc<VmBoundMethod>),
}

impl VmValue {
    fn is_falsey(&self) -> bool {
        matches!(self, Self::Nil | Self::Bool(false))
    }
}

impl std::fmt::Display for VmValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(n) => {
                if n.fract() == 0.0 {
                    write!(f, "{}", *n as i64)
                } else {
                    write!(f, "{n}")
                }
            }
            Self::Bool(b) => write!(f, "{b}"),
            Self::Nil => write!(f, "nil"),
            Self::String(s) => write!(f, "{s}"),
            Self::Function(func) => write!(f, "<fn {}>", func.name),
            Self::Closure(c) => write!(f, "<fn {}>", c.function.name),
            Self::NativeFunction(_) => write!(f, "<native fn>"),
            Self::Class(c) => write!(f, "{}", c.borrow().name),
            Self::Instance(i) => write!(f, "{} instance", i.borrow().class.borrow().name),
            Self::BoundMethod(bm) => write!(f, "<fn {}>", bm.method.function.name),
        }
    }
}

#[derive(Debug, Clone)]
struct VmFunction {
    name: String,
    arity: usize,
    #[allow(dead_code)]
    upvalue_count: usize,
    chunk: Chunk,
}

#[derive(Debug)]
struct VmClosure {
    function: Rc<VmFunction>,
    upvalues: Vec<Rc<RefCell<VmUpvalue>>>,
}

impl Clone for VmClosure {
    fn clone(&self) -> Self {
        Self {
            function: Rc::clone(&self.function),
            upvalues: self.upvalues.clone(),
        }
    }
}

#[derive(Debug, Clone)]
enum VmUpvalue {
    Open(usize), // stack index
    Closed(VmValue),
}

#[derive(Debug, Clone, Copy)]
enum NativeFn {
    Clock,
}

#[derive(Debug)]
struct VmClass {
    name: String,
    methods: HashMap<String, Rc<VmClosure>>,
}

#[derive(Debug)]
struct VmInstance {
    class: Rc<RefCell<VmClass>>,
    fields: HashMap<String, VmValue>,
}

#[derive(Debug)]
struct VmBoundMethod {
    receiver: VmValue,
    method: Rc<VmClosure>,
}

struct CallFrame {
    closure: Rc<VmClosure>,
    ip: usize,
    slot_offset: usize,
}

pub struct Vm {
    stack: Vec<VmValue>,
    frames: Vec<CallFrame>,
    globals: HashMap<String, VmValue>,
    open_upvalues: Vec<Rc<RefCell<VmUpvalue>>>,
    output: Vec<String>,
    writer: Box<dyn Write>,
}

impl Vm {
    pub fn new() -> Self {
        let mut globals = HashMap::new();
        globals.insert(
            "clock".to_string(),
            VmValue::NativeFunction(NativeFn::Clock),
        );
        Self {
            stack: Vec::with_capacity(256),
            frames: Vec::with_capacity(64),
            globals,
            open_upvalues: Vec::new(),
            output: Vec::new(),
            writer: Box::new(std::io::stdout()),
        }
    }

    #[cfg(test)]
    fn new_capturing() -> Self {
        let mut vm = Self::new();
        vm.writer = Box::new(Vec::<u8>::new());
        vm
    }

    pub fn output(&self) -> &[String] {
        &self.output
    }

    pub fn interpret(&mut self, chunk: Chunk) -> Result<(), LoxError> {
        let function = Rc::new(VmFunction {
            name: "script".to_string(),
            arity: 0,
            upvalue_count: 0,
            chunk,
        });
        let closure = Rc::new(VmClosure {
            function,
            upvalues: Vec::new(),
        });
        self.stack.push(VmValue::Closure(Rc::clone(&closure)));
        self.frames.push(CallFrame {
            closure,
            ip: 0,
            slot_offset: 0,
        });
        self.run()
    }

    fn run(&mut self) -> Result<(), LoxError> {
        loop {
            let frame_idx = self.frames.len() - 1;
            let ip = self.frames[frame_idx].ip;
            let chunk = &self.frames[frame_idx].closure.function.chunk;

            if ip >= chunk.code.len() {
                return Ok(());
            }

            let op = chunk.code[ip];
            self.frames[frame_idx].ip += 1;

            match op_from_u8(op) {
                Some(OpCode::Constant) => {
                    let idx = self.read_byte();
                    let constant = self.current_chunk().constants[idx as usize].clone();
                    self.stack.push(constant_to_value(constant));
                }
                Some(OpCode::Nil) => self.stack.push(VmValue::Nil),
                Some(OpCode::True) => self.stack.push(VmValue::Bool(true)),
                Some(OpCode::False) => self.stack.push(VmValue::Bool(false)),
                Some(OpCode::Pop) => {
                    self.stack.pop();
                }
                Some(OpCode::GetLocal) => {
                    let slot = self.read_byte() as usize;
                    let offset = self.frames.last().expect("frame").slot_offset;
                    let value = self.stack[offset + slot].clone();
                    self.stack.push(value);
                }
                Some(OpCode::SetLocal) => {
                    let slot = self.read_byte() as usize;
                    let offset = self.frames.last().expect("frame").slot_offset;
                    let value = self.stack.last().expect("stack not empty").clone();
                    self.stack[offset + slot] = value;
                }
                Some(OpCode::GetGlobal) => {
                    let name = self.read_string_constant();
                    let value = self.globals.get(&name).cloned().ok_or_else(|| {
                        LoxError::runtime(format!("undefined variable '{name}'"), 0, 0)
                    })?;
                    self.stack.push(value);
                }
                Some(OpCode::SetGlobal) => {
                    let name = self.read_string_constant();
                    if !self.globals.contains_key(&name) {
                        return Err(LoxError::runtime(
                            format!("undefined variable '{name}'"),
                            0,
                            0,
                        ));
                    }
                    let value = self.stack.last().expect("stack not empty").clone();
                    self.globals.insert(name, value);
                }
                Some(OpCode::DefineGlobal) => {
                    let name = self.read_string_constant();
                    let value = self.stack.pop().expect("stack not empty");
                    self.globals.insert(name, value);
                }
                Some(OpCode::GetUpvalue) => {
                    let slot = self.read_byte() as usize;
                    let upvalue =
                        Rc::clone(&self.frames.last().expect("frame").closure.upvalues[slot]);
                    let value = match &*upvalue.borrow() {
                        VmUpvalue::Open(idx) => self.stack[*idx].clone(),
                        VmUpvalue::Closed(v) => v.clone(),
                    };
                    self.stack.push(value);
                }
                Some(OpCode::SetUpvalue) => {
                    let slot = self.read_byte() as usize;
                    let value = self.stack.last().expect("stack not empty").clone();
                    let upvalue =
                        Rc::clone(&self.frames.last().expect("frame").closure.upvalues[slot]);
                    match &mut *upvalue.borrow_mut() {
                        VmUpvalue::Open(idx) => {
                            self.stack[*idx] = value;
                        }
                        VmUpvalue::Closed(v) => {
                            *v = value;
                        }
                    }
                }
                Some(OpCode::GetProperty) => {
                    let name = self.read_string_constant();
                    let instance = self.stack.pop().expect("stack");
                    match instance {
                        VmValue::Instance(inst) => {
                            if let Some(val) = inst.borrow().fields.get(&name).cloned() {
                                self.stack.push(val);
                            } else if let Some(method) =
                                inst.borrow().class.borrow().methods.get(&name).cloned()
                            {
                                let bound = VmValue::BoundMethod(Rc::new(VmBoundMethod {
                                    receiver: VmValue::Instance(Rc::clone(&inst)),
                                    method,
                                }));
                                self.stack.push(bound);
                            } else {
                                return Err(LoxError::runtime(
                                    format!("undefined property '{name}'"),
                                    0,
                                    0,
                                ));
                            }
                        }
                        _ => {
                            return Err(LoxError::runtime("only instances have properties", 0, 0));
                        }
                    }
                }
                Some(OpCode::SetProperty) => {
                    let name = self.read_string_constant();
                    let value = self.stack.pop().expect("stack");
                    let instance = self.stack.pop().expect("stack");
                    match instance {
                        VmValue::Instance(inst) => {
                            inst.borrow_mut().fields.insert(name, value.clone());
                            self.stack.push(value);
                        }
                        _ => {
                            return Err(LoxError::runtime("only instances have fields", 0, 0));
                        }
                    }
                }
                Some(OpCode::GetSuper) => {
                    let name = self.read_string_constant();
                    let superclass = self.stack.pop().expect("stack");
                    let receiver = self.stack.pop().expect("stack");
                    if let VmValue::Class(sc) = superclass {
                        if let Some(method) = sc.borrow().methods.get(&name).cloned() {
                            let bound =
                                VmValue::BoundMethod(Rc::new(VmBoundMethod { receiver, method }));
                            self.stack.push(bound);
                        } else {
                            return Err(LoxError::runtime(
                                format!("undefined property '{name}'"),
                                0,
                                0,
                            ));
                        }
                    }
                }
                Some(OpCode::Equal) => {
                    let b = self.stack.pop().expect("stack");
                    let a = self.stack.pop().expect("stack");
                    self.stack.push(VmValue::Bool(values_equal(&a, &b)));
                }
                Some(OpCode::Greater) => {
                    self.binary_op(|a, b| VmValue::Bool(a > b))?;
                }
                Some(OpCode::Less) => {
                    self.binary_op(|a, b| VmValue::Bool(a < b))?;
                }
                Some(OpCode::Add) => {
                    let b = self.stack.pop().expect("stack");
                    let a = self.stack.pop().expect("stack");
                    match (&a, &b) {
                        (VmValue::Number(x), VmValue::Number(y)) => {
                            self.stack.push(VmValue::Number(x + y));
                        }
                        (VmValue::String(x), VmValue::String(y)) => {
                            self.stack.push(VmValue::String(Rc::new(format!("{x}{y}"))));
                        }
                        _ => {
                            return Err(LoxError::runtime(
                                "operands must be two numbers or two strings",
                                0,
                                0,
                            ));
                        }
                    }
                }
                Some(OpCode::Subtract) => {
                    self.binary_op(|a, b| VmValue::Number(a - b))?;
                }
                Some(OpCode::Multiply) => {
                    self.binary_op(|a, b| VmValue::Number(a * b))?;
                }
                Some(OpCode::Divide) => {
                    self.binary_op(|a, b| VmValue::Number(a / b))?;
                }
                Some(OpCode::Not) => {
                    let val = self.stack.pop().expect("stack");
                    self.stack.push(VmValue::Bool(val.is_falsey()));
                }
                Some(OpCode::Negate) => {
                    let val = self.stack.pop().expect("stack");
                    match val {
                        VmValue::Number(n) => self.stack.push(VmValue::Number(-n)),
                        _ => {
                            return Err(LoxError::runtime("operand must be a number", 0, 0));
                        }
                    }
                }
                Some(OpCode::Print) => {
                    let val = self.stack.pop().expect("stack");
                    let text = format!("{val}");
                    writeln!(self.writer, "{text}").expect("write should succeed");
                    self.output.push(text);
                }
                Some(OpCode::Jump) => {
                    let offset = self.read_u16();
                    self.frames.last_mut().expect("frame").ip += offset as usize;
                }
                Some(OpCode::JumpIfFalse) => {
                    let offset = self.read_u16();
                    if self.stack.last().expect("stack").is_falsey() {
                        self.frames.last_mut().expect("frame").ip += offset as usize;
                    }
                }
                Some(OpCode::Loop) => {
                    let offset = self.read_u16();
                    self.frames.last_mut().expect("frame").ip -= offset as usize;
                }
                Some(OpCode::Call) => {
                    let arg_count = self.read_byte() as usize;
                    let callee_idx = self.stack.len() - 1 - arg_count;
                    let callee = self.stack[callee_idx].clone();
                    self.call_value(callee, arg_count)?;
                }
                Some(OpCode::Invoke) => {
                    let name = self.read_string_constant();
                    let arg_count = self.read_byte() as usize;
                    let receiver_idx = self.stack.len() - 1 - arg_count;
                    let receiver = self.stack[receiver_idx].clone();
                    if let VmValue::Instance(inst) = &receiver {
                        if let Some(field) = inst.borrow().fields.get(&name).cloned() {
                            self.stack[receiver_idx] = field.clone();
                            self.call_value(field, arg_count)?;
                        } else {
                            let class = inst.borrow().class.clone();
                            self.invoke_from_class(&class, &name, arg_count)?;
                        }
                    } else {
                        return Err(LoxError::runtime("only instances have methods", 0, 0));
                    }
                }
                Some(OpCode::SuperInvoke) => {
                    let name = self.read_string_constant();
                    let arg_count = self.read_byte() as usize;
                    let superclass = self.stack.pop().expect("stack");
                    if let VmValue::Class(sc) = superclass {
                        self.invoke_from_class(&sc, &name, arg_count)?;
                    }
                }
                Some(OpCode::Closure) => {
                    let idx = self.read_byte();
                    let constant = self.current_chunk().constants[idx as usize].clone();
                    if let Constant::Function {
                        name,
                        arity,
                        upvalue_count,
                        chunk,
                    } = constant
                    {
                        let function = Rc::new(VmFunction {
                            name,
                            arity,
                            upvalue_count,
                            chunk,
                        });
                        let mut upvalues = Vec::with_capacity(upvalue_count);
                        for _ in 0..upvalue_count {
                            let is_local = self.read_byte();
                            let index = self.read_byte() as usize;
                            if is_local == 1 {
                                let abs_idx =
                                    self.frames.last().expect("frame").slot_offset + index;
                                let upvalue = self.capture_upvalue(abs_idx);
                                upvalues.push(upvalue);
                            } else {
                                let upvalue = Rc::clone(
                                    &self.frames.last().expect("frame").closure.upvalues[index],
                                );
                                upvalues.push(upvalue);
                            }
                        }
                        let closure = Rc::new(VmClosure { function, upvalues });
                        self.stack.push(VmValue::Closure(closure));
                    }
                }
                Some(OpCode::CloseUpvalue) => {
                    let idx = self.stack.len() - 1;
                    self.close_upvalues(idx);
                    self.stack.pop();
                }
                Some(OpCode::Return) => {
                    let result = self.stack.pop().expect("stack");
                    let frame = self.frames.pop().expect("frame");
                    if self.frames.is_empty() {
                        self.stack.pop(); // pop script closure
                        return Ok(());
                    }
                    self.close_upvalues(frame.slot_offset);
                    self.stack.truncate(frame.slot_offset);
                    self.stack.push(result);
                }
                Some(OpCode::Class) => {
                    let name = self.read_string_constant();
                    let class = Rc::new(RefCell::new(VmClass {
                        name,
                        methods: HashMap::new(),
                    }));
                    self.stack.push(VmValue::Class(class));
                }
                Some(OpCode::Inherit) => {
                    let superclass = self.stack[self.stack.len() - 2].clone();
                    let subclass = self.stack.last().expect("stack").clone();
                    if let (VmValue::Class(sc), VmValue::Class(sub)) = (&superclass, &subclass) {
                        let methods = sc.borrow().methods.clone();
                        sub.borrow_mut().methods.extend(methods);
                        self.stack.pop(); // pop subclass, leave super as local
                    } else {
                        return Err(LoxError::runtime("superclass must be a class", 0, 0));
                    }
                }
                Some(OpCode::Method) => {
                    let name = self.read_string_constant();
                    let method = self.stack.pop().expect("stack");
                    if let (VmValue::Closure(closure), Some(VmValue::Class(class))) =
                        (method, self.stack.last())
                    {
                        class.borrow_mut().methods.insert(name, closure);
                    }
                }
                None => {
                    return Err(LoxError::runtime(format!("unknown opcode {op}"), 0, 0));
                }
            }
        }
    }

    fn read_byte(&mut self) -> u8 {
        let frame = self.frames.last_mut().expect("frame");
        let byte = frame.closure.function.chunk.code[frame.ip];
        frame.ip += 1;
        byte
    }

    fn read_u16(&mut self) -> u16 {
        let frame = self.frames.last_mut().expect("frame");
        let value = frame.closure.function.chunk.read_u16(frame.ip);
        frame.ip += 2;
        value
    }

    fn read_string_constant(&mut self) -> String {
        let idx = self.read_byte();
        let constant = &self.current_chunk().constants[idx as usize];
        match constant {
            Constant::String(s) => s.clone(),
            _ => panic!("expected string constant"),
        }
    }

    fn current_chunk(&self) -> &Chunk {
        &self.frames.last().expect("frame").closure.function.chunk
    }

    fn binary_op(&mut self, op: fn(f64, f64) -> VmValue) -> Result<(), LoxError> {
        let b = self.stack.pop().expect("stack");
        let a = self.stack.pop().expect("stack");
        match (&a, &b) {
            (VmValue::Number(x), VmValue::Number(y)) => {
                self.stack.push(op(*x, *y));
                Ok(())
            }
            _ => Err(LoxError::runtime("operands must be numbers", 0, 0)),
        }
    }

    fn call_value(&mut self, callee: VmValue, arg_count: usize) -> Result<(), LoxError> {
        match callee {
            VmValue::Closure(closure) => {
                if arg_count != closure.function.arity {
                    return Err(LoxError::runtime(
                        format!(
                            "expected {} arguments but got {arg_count}",
                            closure.function.arity
                        ),
                        0,
                        0,
                    ));
                }
                let slot_offset = self.stack.len() - arg_count - 1;
                self.frames.push(CallFrame {
                    closure,
                    ip: 0,
                    slot_offset,
                });
                Ok(())
            }
            VmValue::NativeFunction(native) => {
                let result = match native {
                    NativeFn::Clock => {
                        let secs = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .expect("system clock should be after unix epoch")
                            .as_secs_f64();
                        VmValue::Number(secs)
                    }
                };
                // Remove callee + args, push result
                let start = self.stack.len() - arg_count - 1;
                self.stack.truncate(start);
                self.stack.push(result);
                Ok(())
            }
            VmValue::Class(class) => {
                let instance = Rc::new(RefCell::new(VmInstance {
                    class: Rc::clone(&class),
                    fields: HashMap::new(),
                }));
                let slot_offset = self.stack.len() - arg_count - 1;
                self.stack[slot_offset] = VmValue::Instance(Rc::clone(&instance));

                if let Some(init) = class.borrow().methods.get("init").cloned() {
                    if arg_count != init.function.arity {
                        return Err(LoxError::runtime(
                            format!(
                                "expected {} arguments but got {arg_count}",
                                init.function.arity
                            ),
                            0,
                            0,
                        ));
                    }
                    self.frames.push(CallFrame {
                        closure: init,
                        ip: 0,
                        slot_offset,
                    });
                } else if arg_count != 0 {
                    return Err(LoxError::runtime(
                        format!("expected 0 arguments but got {arg_count}"),
                        0,
                        0,
                    ));
                }
                Ok(())
            }
            VmValue::BoundMethod(bm) => {
                let slot_offset = self.stack.len() - arg_count - 1;
                self.stack[slot_offset] = bm.receiver.clone();
                if arg_count != bm.method.function.arity {
                    return Err(LoxError::runtime(
                        format!(
                            "expected {} arguments but got {arg_count}",
                            bm.method.function.arity
                        ),
                        0,
                        0,
                    ));
                }
                self.frames.push(CallFrame {
                    closure: Rc::clone(&bm.method),
                    ip: 0,
                    slot_offset,
                });
                Ok(())
            }
            _ => Err(LoxError::runtime(
                "can only call functions and classes",
                0,
                0,
            )),
        }
    }

    fn invoke_from_class(
        &mut self,
        class: &Rc<RefCell<VmClass>>,
        name: &str,
        arg_count: usize,
    ) -> Result<(), LoxError> {
        let method = class
            .borrow()
            .methods
            .get(name)
            .cloned()
            .ok_or_else(|| LoxError::runtime(format!("undefined property '{name}'"), 0, 0))?;
        let slot_offset = self.stack.len() - arg_count - 1;
        self.frames.push(CallFrame {
            closure: method,
            ip: 0,
            slot_offset,
        });
        Ok(())
    }

    fn capture_upvalue(&mut self, stack_idx: usize) -> Rc<RefCell<VmUpvalue>> {
        for uv in &self.open_upvalues {
            if let VmUpvalue::Open(idx) = &*uv.borrow()
                && *idx == stack_idx
            {
                return Rc::clone(uv);
            }
        }
        let upvalue = Rc::new(RefCell::new(VmUpvalue::Open(stack_idx)));
        self.open_upvalues.push(Rc::clone(&upvalue));
        upvalue
    }

    fn close_upvalues(&mut self, last: usize) {
        let mut i = 0;
        while i < self.open_upvalues.len() {
            let should_close = {
                let uv = self.open_upvalues[i].borrow();
                if let VmUpvalue::Open(idx) = &*uv {
                    *idx >= last
                } else {
                    false
                }
            };
            if should_close {
                let uv = self.open_upvalues.remove(i);
                let value = {
                    let borrowed = uv.borrow();
                    if let VmUpvalue::Open(idx) = &*borrowed {
                        self.stack[*idx].clone()
                    } else {
                        unreachable!()
                    }
                };
                *uv.borrow_mut() = VmUpvalue::Closed(value);
            } else {
                i += 1;
            }
        }
    }
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

fn constant_to_value(constant: Constant) -> VmValue {
    match constant {
        Constant::Number(n) => VmValue::Number(n),
        Constant::String(s) => VmValue::String(Rc::new(s)),
        Constant::Function { .. } => {
            panic!("function constants should be handled by Closure opcode")
        }
    }
}

fn values_equal(a: &VmValue, b: &VmValue) -> bool {
    match (a, b) {
        (VmValue::Nil, VmValue::Nil) => true,
        (VmValue::Bool(a), VmValue::Bool(b)) => a == b,
        (VmValue::Number(a), VmValue::Number(b)) => a == b,
        (VmValue::String(a), VmValue::String(b)) => a == b,
        _ => false,
    }
}

fn op_from_u8(byte: u8) -> Option<OpCode> {
    if byte <= OpCode::Method as u8 {
        Some(unsafe { std::mem::transmute::<u8, OpCode>(byte) })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::Parser;
    use crate::scanner;
    use crate::vm::compiler::Compiler;
    use rstest::rstest;

    fn run_vm(source: &str) -> Vec<String> {
        let tokens = scanner::scan(source).expect("scan");
        let program = Parser::new(tokens).parse().expect("parse");
        let chunk = Compiler::new().compile(&program).expect("compile");
        let mut vm = Vm::new_capturing();
        vm.interpret(chunk).expect("interpret");
        vm.output.clone()
    }

    fn run_vm_err(source: &str) -> LoxError {
        let tokens = scanner::scan(source).expect("scan");
        let program = Parser::new(tokens).parse().expect("parse");
        let chunk = Compiler::new().compile(&program).expect("compile");
        let mut vm = Vm::new_capturing();
        vm.interpret(chunk).unwrap_err()
    }

    #[rstest]
    #[case("print 1 + 2;", "3")]
    #[case("print 10 - 3;", "7")]
    #[case("print 2 * 3;", "6")]
    #[case("print 10 / 4;", "2.5")]
    #[case("print -5;", "-5")]
    fn vm_arithmetic(#[case] source: &str, #[case] expected: &str) {
        assert_eq!(run_vm(source), vec![expected]);
    }

    #[test]
    fn vm_string_concat() {
        assert_eq!(run_vm("print \"hello\" + \" world\";"), vec!["hello world"]);
    }

    #[test]
    fn vm_variables() {
        assert_eq!(run_vm("var x = 10; print x;"), vec!["10"]);
    }

    #[test]
    fn vm_blocks_scoping() {
        assert_eq!(
            run_vm("var x = 1; { var x = 2; print x; } print x;"),
            vec!["2", "1"]
        );
    }

    #[test]
    fn vm_if_else() {
        assert_eq!(run_vm("if (true) print 1; else print 2;"), vec!["1"]);
    }

    #[test]
    fn vm_while_loop() {
        assert_eq!(
            run_vm("var i = 0; while (i < 3) { print i; i = i + 1; }"),
            vec!["0", "1", "2"]
        );
    }

    #[test]
    fn vm_for_loop() {
        assert_eq!(
            run_vm("for (var i = 0; i < 3; i = i + 1) print i;"),
            vec!["0", "1", "2"]
        );
    }

    #[test]
    fn vm_functions() {
        assert_eq!(
            run_vm("fun add(a, b) { return a + b; } print add(1, 2);"),
            vec!["3"]
        );
    }

    #[test]
    fn vm_closures() {
        assert_eq!(
            run_vm(
                "fun makeCounter() { var i = 0; fun count() { i = i + 1; return i; } return count; } var c = makeCounter(); print c(); print c();"
            ),
            vec!["1", "2"]
        );
    }

    #[test]
    fn vm_classes() {
        assert_eq!(
            run_vm("class Foo { bar() { return 42; } } var foo = Foo(); print foo.bar();"),
            vec!["42"]
        );
    }

    #[test]
    fn vm_class_fields() {
        assert_eq!(
            run_vm("class Foo {} var foo = Foo(); foo.x = 10; print foo.x;"),
            vec!["10"]
        );
    }

    #[test]
    fn vm_fibonacci() {
        assert_eq!(
            run_vm(
                "fun fib(n) { if (n <= 1) return n; return fib(n - 1) + fib(n - 2); } for (var i = 0; i < 10; i = i + 1) { print fib(i); }"
            ),
            vec!["0", "1", "1", "2", "3", "5", "8", "13", "21", "34"]
        );
    }

    #[test]
    fn vm_undefined_variable() {
        let err = run_vm_err("print x;");
        assert!(err.to_string().contains("undefined variable"));
    }

    // ========== Additional VM Execution Tests ==========

    // ========== Boolean and Comparison Operations ==========

    #[test]
    fn vm_comparison_less() {
        assert_eq!(run_vm("print 1 < 2;"), vec!["true"]);
        assert_eq!(run_vm("print 2 < 1;"), vec!["false"]);
    }

    #[test]
    fn vm_comparison_greater() {
        assert_eq!(run_vm("print 2 > 1;"), vec!["true"]);
        assert_eq!(run_vm("print 1 > 2;"), vec!["false"]);
    }

    #[test]
    fn vm_comparison_equal() {
        assert_eq!(run_vm("print 1 == 1;"), vec!["true"]);
        assert_eq!(run_vm("print 1 == 2;"), vec!["false"]);
        assert_eq!(run_vm("print nil == nil;"), vec!["true"]);
    }

    #[test]
    fn vm_comparison_not_equal() {
        assert_eq!(run_vm("print 1 != 2;"), vec!["true"]);
        assert_eq!(run_vm("print 1 != 1;"), vec!["false"]);
    }

    #[test]
    fn vm_boolean_not() {
        assert_eq!(run_vm("print !true;"), vec!["false"]);
        assert_eq!(run_vm("print !false;"), vec!["true"]);
        assert_eq!(run_vm("print !nil;"), vec!["true"]);
    }

    #[test]
    fn vm_truthiness() {
        assert_eq!(run_vm("if (false) print 1; else print 2;"), vec!["2"]);
        assert_eq!(run_vm("if (nil) print 1; else print 2;"), vec!["2"]);
        assert_eq!(run_vm("if (0) print 1; else print 2;"), vec!["1"]);
        assert_eq!(run_vm("if (\"\") print 1; else print 2;"), vec!["1"]);
    }

    // ========== Variable Scoping ==========

    #[test]
    fn vm_global_variables() {
        assert_eq!(
            run_vm("var a = 1; var b = 2; print a + b;"),
            vec!["3"]
        );
    }

    #[test]
    fn vm_local_variables_in_block() {
        assert_eq!(run_vm("{ var x = 5; print x; }"), vec!["5"]);
    }

    #[test]
    fn vm_variable_shadowing() {
        assert_eq!(
            run_vm("var x = 1; { var x = 2; { var x = 3; print x; } print x; } print x;"),
            vec!["3", "2", "1"]
        );
    }

    #[test]
    fn vm_global_reassignment() {
        assert_eq!(run_vm("var x = 1; x = 2; print x;"), vec!["2"]);
    }

    #[test]
    fn vm_local_reassignment() {
        assert_eq!(run_vm("{ var x = 1; x = 2; print x; }"), vec!["2"]);
    }

    // ========== Control Flow ==========

    #[test]
    fn vm_if_without_else() {
        assert_eq!(run_vm("if (true) print 1;"), vec!["1"]);
        assert_eq!(run_vm("if (false) print 1;"), Vec::<String>::new());
    }

    #[test]
    fn vm_nested_if() {
        assert_eq!(
            run_vm("if (true) { if (true) print 1; }"),
            vec!["1"]
        );
    }

    #[test]
    fn vm_while_with_break_simulation() {
        // Lox doesn't have break, but we can test loop exit via condition
        assert_eq!(
            run_vm("var i = 0; var done = false; while (!done) { print i; i = i + 1; if (i >= 3) done = true; }"),
            vec!["0", "1", "2"]
        );
    }

    #[test]
    fn vm_nested_loops() {
        assert_eq!(
            run_vm("for (var i = 0; i < 2; i = i + 1) { for (var j = 0; j < 2; j = j + 1) { print i * 10 + j; } }"),
            vec!["0", "1", "10", "11"]
        );
    }

    // ========== Functions ==========

    #[test]
    fn vm_function_no_params() {
        assert_eq!(
            run_vm("fun greet() { return \"hello\"; } print greet();"),
            vec!["hello"]
        );
    }

    #[test]
    fn vm_function_multiple_params() {
        assert_eq!(
            run_vm("fun add3(a, b, c) { return a + b + c; } print add3(1, 2, 3);"),
            vec!["6"]
        );
    }

    #[test]
    fn vm_function_no_return() {
        assert_eq!(
            run_vm("fun test() { 42; } print test();"),
            vec!["nil"]
        );
    }

    #[test]
    fn vm_function_early_return() {
        assert_eq!(
            run_vm("fun test() { return 1; return 2; } print test();"),
            vec!["1"]
        );
    }

    #[test]
    fn vm_recursive_function() {
        assert_eq!(
            run_vm("fun countdown(n) { if (n <= 0) return; print n; countdown(n - 1); } countdown(3);"),
            vec!["3", "2", "1"]
        );
    }

    #[test]
    fn vm_nested_function_calls() {
        assert_eq!(
            run_vm("fun double(x) { return x * 2; } print double(double(5));"),
            vec!["20"]
        );
    }

    #[test]
    fn vm_function_as_value() {
        assert_eq!(
            run_vm("fun identity(x) { return x; } var f = identity; print f(42);"),
            vec!["42"]
        );
    }

    // ========== Closures ==========

    #[test]
    fn vm_simple_closure() {
        assert_eq!(
            run_vm("fun outer() { var x = 1; fun inner() { return x; } return inner; } var f = outer(); print f();"),
            vec!["1"]
        );
    }

    #[test]
    fn vm_closure_mutates_captured_var() {
        assert_eq!(
            run_vm("fun outer() { var x = 0; fun inner() { x = x + 1; return x; } return inner; } var f = outer(); print f(); print f();"),
            vec!["1", "2"]
        );
    }

    #[test]
    fn vm_multiple_closures_share_variable() {
        assert_eq!(
            run_vm(r#"
                fun outer() {
                    var x = 0;
                    fun inc() { x = x + 1; return x; }
                    fun get() { return x; }
                    inc();
                    print get();
                }
                outer();
            "#),
            vec!["1"]
        );
    }

    // ========== Classes ==========

    #[test]
    fn vm_class_instantiation() {
        assert_eq!(
            run_vm("class Foo {} var foo = Foo(); print foo;"),
            vec!["Foo instance"]
        );
    }

    #[test]
    fn vm_class_field_get_set() {
        assert_eq!(
            run_vm("class Foo {} var f = Foo(); f.bar = \"baz\"; print f.bar;"),
            vec!["baz"]
        );
    }

    #[test]
    fn vm_class_method() {
        assert_eq!(
            run_vm("class Foo { greet() { return \"hello\"; } } var f = Foo(); print f.greet();"),
            vec!["hello"]
        );
    }

    #[test]
    fn vm_class_this() {
        assert_eq!(
            run_vm("class Foo { getThis() { return this; } } var f = Foo(); print f.getThis();"),
            vec!["Foo instance"]
        );
    }

    #[test]
    fn vm_class_initializer() {
        assert_eq!(
            run_vm("class Foo { init(x) { this.x = x; } } var f = Foo(42); print f.x;"),
            vec!["42"]
        );
    }

    #[test]
    fn vm_class_initializer_returns_instance() {
        assert_eq!(
            run_vm("class Foo { init() { } } var f = Foo(); print f;"),
            vec!["Foo instance"]
        );
    }

    #[test]
    fn vm_class_inheritance() {
        assert_eq!(
            run_vm("class Base { method() { return \"base\"; } } class Derived < Base {} var d = Derived(); print d.method();"),
            vec!["base"]
        );
    }

    #[test]
    fn vm_class_method_override() {
        assert_eq!(
            run_vm("class Base { method() { return \"base\"; } } class Derived < Base { method() { return \"derived\"; } } var d = Derived(); print d.method();"),
            vec!["derived"]
        );
    }

    #[test]
    fn vm_class_super_call() {
        assert_eq!(
            run_vm(r#"
                class Base { greet() { return "hello"; } }
                class Derived < Base {
                    greet() { return super.greet(); }
                }
                var d = Derived();
                print d.greet();
            "#),
            vec!["hello"]
        );
    }

    // ========== Error Cases ==========

    #[test]
    fn vm_undefined_global_get() {
        let err = run_vm_err("print x;");
        assert!(err.to_string().contains("undefined variable"));
    }

    #[test]
    fn vm_undefined_global_set() {
        let err = run_vm_err("x = 1;");
        assert!(err.to_string().contains("undefined variable"));
    }

    #[test]
    fn vm_wrong_arity_too_few() {
        let err = run_vm_err("fun f(a, b) {} f(1);");
        assert!(err.to_string().contains("expected 2"));
    }

    #[test]
    fn vm_wrong_arity_too_many() {
        let err = run_vm_err("fun f(a) {} f(1, 2);");
        assert!(err.to_string().contains("expected 1"));
    }

    #[test]
    fn vm_call_non_function() {
        let err = run_vm_err("var x = 42; x();");
        assert!(err.to_string().contains("can only call"));
    }

    #[test]
    fn vm_type_error_negate_string() {
        let err = run_vm_err("print -\"hello\";");
        assert!(err.to_string().contains("operand must be a number"));
    }

    #[test]
    fn vm_type_error_add_number_bool() {
        let err = run_vm_err("print 1 + true;");
        assert!(err.to_string().contains("operands must be"));
    }

    #[test]
    fn vm_type_error_subtract_strings() {
        let err = run_vm_err("print \"a\" - \"b\";");
        assert!(err.to_string().contains("operands must be numbers"));
    }

    #[test]
    fn vm_undefined_property() {
        let err = run_vm_err("class Foo {} var f = Foo(); print f.bar;");
        assert!(err.to_string().contains("undefined property"));
    }

    #[test]
    fn vm_property_on_non_instance() {
        let err = run_vm_err("var x = 42; print x.foo;");
        assert!(err.to_string().contains("only instances have properties"));
    }

    #[test]
    fn vm_set_property_on_non_instance() {
        let err = run_vm_err("var x = 42; x.foo = 1;");
        assert!(err.to_string().contains("only instances have fields"));
    }

    #[test]
    fn vm_inherit_from_non_class() {
        let err = run_vm_err("var NotAClass = 42; class Foo < NotAClass {}");
        assert!(err.to_string().contains("superclass must be a class"));
    }

    // ========== Native Functions ==========

    #[test]
    fn vm_clock_function() {
        let output = run_vm("print clock();");
        assert_eq!(output.len(), 1);
        // Clock should return a number (unix timestamp)
        assert!(output[0].parse::<f64>().is_ok());
    }

    // ========== Edge Cases ==========

    #[test]
    fn vm_string_equality() {
        assert_eq!(run_vm("print \"hello\" == \"hello\";"), vec!["true"]);
        assert_eq!(run_vm("print \"hello\" == \"world\";"), vec!["false"]);
    }

    #[test]
    fn vm_nil_operations() {
        assert_eq!(run_vm("print nil == nil;"), vec!["true"]);
        assert_eq!(run_vm("print nil == false;"), vec!["false"]);
        assert_eq!(run_vm("print !nil;"), vec!["true"]);
    }

    #[test]
    fn vm_zero_and_false_distinct() {
        assert_eq!(run_vm("print 0 == false;"), vec!["false"]);
        assert_eq!(run_vm("print 0 == 0;"), vec!["true"]);
    }

    #[test]
    fn vm_empty_string_truthy() {
        assert_eq!(run_vm("if (\"\") print \"yes\"; else print \"no\";"), vec!["yes"]);
    }

    #[test]
    fn vm_multiple_statements() {
        assert_eq!(
            run_vm("print 1; print 2; print 3;"),
            vec!["1", "2", "3"]
        );
    }

    #[test]
    fn vm_expression_statements() {
        // Expression statements should not print
        assert_eq!(run_vm("1 + 2; \"hello\"; 3;"), Vec::<String>::new());
    }
}
