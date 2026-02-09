use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

use crate::interpreter::callable::Callable;

#[derive(Clone, Debug)]
pub enum Value {
    Number(f64),
    Str(String),
    Bool(bool),
    Nil,
    Function(Callable),
    Class(Rc<LoxClass>),
    Instance(Rc<RefCell<LoxInstance>>),
}

impl Value {
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Nil => false,
            Self::Bool(b) => *b,
            _ => true,
        }
    }

    pub fn is_equal(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Nil, Self::Nil) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::Str(a), Self::Str(b)) => a == b,
            _ => false,
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(n) => {
                if n.fract() == 0.0 {
                    write!(f, "{}", *n as i64)
                } else {
                    write!(f, "{n}")
                }
            }
            Self::Str(s) => write!(f, "{s}"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::Nil => write!(f, "nil"),
            Self::Function(func) => write!(f, "{func}"),
            Self::Class(class) => write!(f, "{class}"),
            Self::Instance(inst) => write!(f, "{}", inst.borrow()),
        }
    }
}

#[derive(Debug)]
pub struct LoxClass {
    pub name: String,
    pub superclass: Option<Rc<LoxClass>>,
    pub methods: HashMap<String, Callable>,
}

impl LoxClass {
    pub fn find_method(&self, name: &str) -> Option<Callable> {
        self.methods
            .get(name)
            .cloned()
            .or_else(|| self.superclass.as_ref().and_then(|sc| sc.find_method(name)))
    }
}

impl fmt::Display for LoxClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

#[derive(Debug)]
pub struct LoxInstance {
    pub class: Rc<LoxClass>,
    pub fields: HashMap<String, Value>,
}

impl LoxInstance {
    pub fn new(class: Rc<LoxClass>) -> Self {
        Self {
            class,
            fields: HashMap::new(),
        }
    }

    pub fn get(&self, name: &str, this: Rc<RefCell<LoxInstance>>) -> Option<Value> {
        if let Some(val) = self.fields.get(name) {
            return Some(val.clone());
        }
        if let Some(method) = self.class.find_method(name) {
            return Some(Value::Function(method.bind(this)));
        }
        None
    }

    pub fn set(&mut self, name: String, value: Value) {
        self.fields.insert(name, value);
    }
}

impl fmt::Display for LoxInstance {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} instance", self.class.name)
    }
}
