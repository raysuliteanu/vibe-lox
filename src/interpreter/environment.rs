use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::interpreter::value::Value;

#[derive(Debug)]
pub struct Environment {
    values: HashMap<String, Value>,
    enclosing: Option<Rc<RefCell<Environment>>>,
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}

impl Environment {
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
            enclosing: None,
        }
    }

    pub fn with_enclosing(enclosing: Rc<RefCell<Environment>>) -> Self {
        Self {
            values: HashMap::new(),
            enclosing: Some(enclosing),
        }
    }

    pub fn define(&mut self, name: String, value: Value) {
        self.values.insert(name, value);
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        if let Some(val) = self.values.get(name) {
            return Some(val.clone());
        }
        if let Some(ref enclosing) = self.enclosing {
            return enclosing.borrow().get(name);
        }
        None
    }

    pub fn get_at(&self, distance: usize, name: &str) -> Option<Value> {
        if distance == 0 {
            self.values.get(name).cloned()
        } else {
            self.enclosing
                .as_ref()
                .expect("resolver guarantees valid distance")
                .borrow()
                .get_at(distance - 1, name)
        }
    }

    pub fn assign(&mut self, name: &str, value: Value) -> bool {
        if self.values.contains_key(name) {
            self.values.insert(name.to_string(), value);
            return true;
        }
        if let Some(ref enclosing) = self.enclosing {
            return enclosing.borrow_mut().assign(name, value);
        }
        false
    }

    pub fn assign_at(&mut self, distance: usize, name: &str, value: Value) {
        if distance == 0 {
            self.values.insert(name.to_string(), value);
        } else {
            self.enclosing
                .as_ref()
                .expect("resolver guarantees valid distance")
                .borrow_mut()
                .assign_at(distance - 1, name, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn define_and_get() {
        let mut env = Environment::new();
        env.define("x".to_string(), Value::Number(42.0));
        assert!(matches!(env.get("x"), Some(Value::Number(n)) if n == 42.0));
    }

    #[test]
    fn get_undefined_returns_none() {
        let env = Environment::new();
        assert!(env.get("x").is_none());
    }

    #[test]
    fn enclosing_scope() {
        let outer = Rc::new(RefCell::new(Environment::new()));
        outer
            .borrow_mut()
            .define("x".to_string(), Value::Number(1.0));
        let inner = Environment::with_enclosing(Rc::clone(&outer));
        assert!(matches!(inner.get("x"), Some(Value::Number(n)) if n == 1.0));
    }

    #[test]
    fn assign_updates_value() {
        let mut env = Environment::new();
        env.define("x".to_string(), Value::Number(1.0));
        assert!(env.assign("x", Value::Number(2.0)));
        assert!(matches!(env.get("x"), Some(Value::Number(n)) if n == 2.0));
    }

    #[test]
    fn assign_undefined_returns_false() {
        let mut env = Environment::new();
        assert!(!env.assign("x", Value::Number(1.0)));
    }

    #[test]
    fn get_at_depth() {
        let outer = Rc::new(RefCell::new(Environment::new()));
        outer
            .borrow_mut()
            .define("x".to_string(), Value::Number(10.0));
        let inner = Rc::new(RefCell::new(Environment::with_enclosing(Rc::clone(&outer))));
        assert!(matches!(inner.borrow().get_at(1, "x"), Some(Value::Number(n)) if n == 10.0));
    }
}
