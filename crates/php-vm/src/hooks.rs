//! Reentrant bridge hooks and `call_user_func` (ADR-0003).

use php_compiler::bytecode::Operand;
use php_value::{Slot, Value};

use crate::Output;
use crate::calls::Argument;
use crate::machine::{Exec, Machine};

impl<O: Output> Machine<'_, O> {
    pub(crate) fn native_call_user_func(&mut self, args: &[Argument], result: Operand) -> Exec {
        let Some(callee) = args.first() else {
            return Err(self.too_few_at_least("call_user_func", 0, 1, args));
        };
        let callee = callee.slot.value();
        let mut rest: Vec<Slot> = args[1..]
            .iter()
            .map(|arg| crate::closures::copy_slot(&arg.slot))
            .collect();
        let value = self.invoke_callable(&callee, &mut rest)?;
        self.write(result, value)
    }

    pub(crate) fn native_bridge_add(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("bridge_add", args)?;
        if !is_callable(&value) {
            return Err(self.not_callable(&value));
        }
        self.hooks.push(value);
        self.write(result, Value::Bool(true))
    }

    pub(crate) fn native_bridge_remove(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("bridge_remove", args)?;
        let found = self
            .hooks
            .iter()
            .position(|hook| same_callback(hook, &value));
        let removed = found.is_some_and(|index| {
            self.hooks.remove(index);
            true
        });
        self.write(result, Value::Bool(removed))
    }

    pub(crate) fn native_bridge_apply(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() > 1 {
            return Err(self.too_many_at_most("bridge_apply", args.len(), 1, args));
        }
        let mut payload = args.first().map_or(Value::Null, |arg| arg.slot.value());
        let mut index = 0;
        while index < self.hooks.len() {
            let callee = self.hooks[index].clone();
            payload = self.invoke_callable(&callee, &mut [Slot::from(payload)])?;
            index += 1;
        }
        self.write(result, payload)
    }

    pub(crate) fn native_get_class(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("get_class", args)?;
        let Value::Object(object) = value else {
            return Err(self.throw(
                "TypeError",
                b"get_class(): Argument #1 ($object) must be of type object".to_vec(),
            ));
        };
        self.write(result, Value::String(object.class_name().as_bytes().into()))
    }

    pub(crate) fn native_get_parent_class(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("get_parent_class", args)?;
        let key = class_key_of(&value);
        let parent = key.and_then(|key| {
            let parent_key = self.classes.get(key.as_slice())?.parent.as_ref()?;
            Some(self.classes.get(parent_key.as_ref())?.name.clone())
        });
        match parent {
            Some(name) => self.write(result, Value::String(name.as_ref().into())),
            None => self.write(result, Value::Bool(false)),
        }
    }

    pub(crate) fn native_spl_object_id(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("spl_object_id", args)?;
        let Value::Object(object) = value else {
            return Err(self.throw(
                "TypeError",
                b"spl_object_id(): Argument #1 ($object) must be of type object".to_vec(),
            ));
        };
        self.write(result, Value::Int(i64::from(object.id().get())))
    }
}

fn is_callable(value: &Value) -> bool {
    matches!(value, Value::String(_) | Value::Object(_))
}

fn same_callback(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Object(a), Value::Object(b)) => a == b,
        (Value::String(a), Value::String(b)) => a.as_bytes().eq_ignore_ascii_case(b.as_bytes()),
        _ => false,
    }
}

fn class_key_of(value: &Value) -> Option<Vec<u8>> {
    match value {
        Value::Object(object) => Some(object.class_name().as_bytes().to_ascii_lowercase()),
        Value::String(name) => Some(name.as_bytes().to_ascii_lowercase()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{is_callable, same_callback};
    use php_value::{ObjectStore, PhpString, Value};

    #[test]
    fn callables_and_identity_are_told_apart() -> Result<(), String> {
        assert!(is_callable(&Value::from("inc")));
        assert!(!is_callable(&Value::Null));
        let store = ObjectStore::new();
        let first = store
            .create(PhpString::from("Closure"))
            .map_err(|error| error.to_string())?;
        let second = store
            .create(PhpString::from("Closure"))
            .map_err(|error| error.to_string())?;
        assert!(same_callback(
            &Value::Object(first.clone()),
            &Value::Object(first)
        ));
        assert!(!same_callback(&Value::Object(second), &Value::from("inc")));
        assert!(same_callback(&Value::from("INC"), &Value::from("inc")));
        Ok(())
    }
}
