//! `spl_autoload_register` and loading a missing class before `new` or `instanceof`.

use php_compiler::bytecode::Operand;
use php_value::{PhpString, Slot, Value};

use crate::Output;
use crate::calls::Argument;
use crate::machine::{Exec, Flow, Machine, Stop};

impl<O: Output> Machine<'_, '_, O> {
    /// Runs registered autoloaders for `name` when the class is not loaded.
    pub(crate) fn autoload(&mut self, name: &[u8]) -> Exec {
        let key = name.to_ascii_lowercase();
        if self.classes.contains_key(key.as_slice()) || self.loading.contains(key.as_slice()) {
            return Ok(());
        }
        if self.autoloaders.is_empty() {
            return Ok(());
        }
        self.loading.insert(key.clone().into());
        let loaders = self.autoloaders.clone();
        let named = Value::String(PhpString::new(name));
        let result = self.run_loaders(&loaders, &named, key.as_slice());
        self.loading.remove(key.as_slice());
        result
    }

    fn run_loaders(&mut self, loaders: &[Value], named: &Value, key: &[u8]) -> Exec {
        for loader in loaders {
            self.invoke_loader(loader, named.clone())?;
            if self.classes.contains_key(key) {
                break;
            }
        }
        Ok(())
    }

    fn invoke_loader(&mut self, loader: &Value, named: Value) -> Exec {
        match loader {
            Value::Object(object) => self.init_closure_call(object.clone())?,
            Value::String(name) => self.init_string_call(name.as_bytes())?,
            _ => return Ok(()),
        }
        if let Some(pending) = self.pending.last_mut() {
            pending.args = vec![Argument {
                slot: Slot::Value(named),
                from_call: false,
            }];
        }
        let depth = self.frames.len();
        self.call(Operand::UNUSED)?;
        self.run_until_depth(depth)
    }

    /// Steps until the frame stack is back to `depth`.
    pub(crate) fn run_until_depth(&mut self, depth: usize) -> Exec {
        while self.frames.len() > depth {
            match self.step() {
                Ok(Flow::Continue) => {}
                Ok(Flow::Finished) => return Ok(()),
                Err(Stop::Thrown(thrown)) => {
                    self.flush();
                    match self.catch_thrown(&thrown) {
                        Ok(true) => {}
                        Ok(false) => return Err(Stop::Thrown(thrown)),
                        Err(stop) => return Err(stop),
                    }
                }
                Err(stop) => return Err(stop),
            }
        }
        Ok(())
    }

    /// Registers `args[0]` as an autoload callback.
    pub(crate) fn native_spl_autoload_register(
        &mut self,
        args: &[Argument],
        result: Operand,
    ) -> Exec {
        let Some(arg) = args.first() else {
            return Err(self.throw_from(
                "ArgumentCountError",
                b"spl_autoload_register() expects at least 1 argument, 0 given".to_vec(),
                b"spl_autoload_register",
                Vec::new(),
            ));
        };
        let value = arg.slot.value();
        if !is_autoload_callable(&value) {
            return Err(self.throw_from(
                "TypeError",
                b"spl_autoload_register(): Argument #1 ($callback) must be a valid callback or null, no array or string given"
                    .to_vec(),
                b"spl_autoload_register",
                vec![value],
            ));
        }
        self.autoloaders.push(value);
        self.write(result, Value::Bool(true))
    }
}

fn is_autoload_callable(value: &Value) -> bool {
    match value {
        Value::Object(object) => object
            .class_name()
            .as_bytes()
            .eq_ignore_ascii_case(b"Closure"),
        Value::String(_) => true,
        _ => false,
    }
}

impl<O: Output> Machine<'_, '_, O> {
    /// Copies class declarations from an included unit into this machine.
    pub(crate) fn absorb_classes(&mut self, unit: &php_compiler::bytecode::Unit) -> Exec {
        for class in &unit.classes {
            if class.deferred {
                continue;
            }
            let key = class.name.to_ascii_lowercase();
            if self.classes.contains_key(key.as_slice()) {
                let mut message = b"Cannot declare class ".to_vec();
                message.extend_from_slice(&class.name);
                message.extend_from_slice(b", because the name is already in use");
                return Err(self.throw("Error", message));
            }
        }
        let mut added = crate::classes::bind_classes(&unit.classes, &mut self.codes);
        for class in &unit.classes {
            if class.deferred {
                continue;
            }
            let key = class.name.to_ascii_lowercase();
            if let Some(bound) = added.remove(key.as_slice()) {
                self.classes.insert(key.into(), bound);
            }
        }
        Ok(())
    }
}
