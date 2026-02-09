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
}
