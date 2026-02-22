use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::ast::Function;
use crate::interpreter::environment::Environment;
use crate::interpreter::value::{LoxInstance, Value};

/// Represents something callable in Lox.
#[derive(Debug, Clone)]
pub enum Callable {
    Native(NativeFunction),
    User(LoxFunction),
}

impl Callable {
    pub fn name(&self) -> &str {
        match self {
            Self::Native(n) => n.name(),
            Self::User(u) => &u.declaration.name,
        }
    }

    pub fn arity(&self) -> usize {
        match self {
            Self::Native(n) => n.arity(),
            Self::User(u) => u.declaration.params.len(),
        }
    }

    pub fn bind(&self, instance: Rc<RefCell<LoxInstance>>) -> Self {
        match self {
            Self::Native(_) => panic!("cannot bind native function"),
            Self::User(u) => {
                let env = Rc::new(RefCell::new(Environment::with_enclosing(Rc::clone(
                    &u.closure,
                ))));
                env.borrow_mut()
                    .define("this".to_string(), Value::Instance(instance));
                Self::User(LoxFunction {
                    declaration: u.declaration.clone(),
                    closure: env,
                    is_initializer: u.is_initializer,
                })
            }
        }
    }
}

impl fmt::Display for Callable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "<fn {}>", self.name())
    }
}

/// A user-defined Lox function.
#[derive(Debug, Clone)]
pub struct LoxFunction {
    pub declaration: Function,
    pub closure: Rc<RefCell<Environment>>,
    pub is_initializer: bool,
}

/// Native function types.
#[derive(Debug, Clone, Copy)]
pub enum NativeFunction {
    Clock,
    ReadLine,
    ToNumber,
}

impl NativeFunction {
    pub fn name(&self) -> &str {
        match self {
            Self::Clock => "clock",
            Self::ReadLine => "readLine",
            Self::ToNumber => "toNumber",
        }
    }

    pub fn arity(&self) -> usize {
        match self {
            Self::Clock => 0,
            Self::ReadLine => 0,
            Self::ToNumber => 1,
        }
    }

    pub fn call(&self, _args: &[Value]) -> Value {
        match self {
            Self::Clock => {
                use std::time::{SystemTime, UNIX_EPOCH};
                let secs = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("system clock should be after unix epoch")
                    .as_secs_f64();
                Value::Number(secs)
            }
            Self::ReadLine => match crate::stdlib::read_line_from(&mut std::io::stdin().lock()) {
                Some(s) => Value::Str(s),
                None => Value::Nil,
            },
            Self::ToNumber => match &_args[0] {
                Value::Number(n) => Value::Number(*n),
                Value::Str(s) => match crate::stdlib::parse_lox_number(s) {
                    Some(n) => Value::Number(n),
                    None => Value::Nil,
                },
                _ => Value::Nil,
            },
        }
    }
}
