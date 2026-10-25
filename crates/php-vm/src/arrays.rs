//! Array literals and the first array built-ins.

use php_compiler::bytecode::{Instruction, Operand};
use php_value::ops::array_key;
use php_value::{PhpArray, PhpString, Value};

use crate::Output;
use crate::VmError;
use crate::calls::Argument;
use crate::machine::{Exec, Machine};

impl<O: Output> Machine<'_, '_, O> {
    /// `result` is a new empty array.
    pub(crate) fn new_array(&mut self, ins: &Instruction) -> Exec {
        self.write(ins.result, Value::Array(PhpArray::new()))
    }

    /// Stores `b` in the array `a`.
    pub(crate) fn array_item(&mut self, ins: &Instruction) -> Exec {
        let Value::Array(mut array) = self.read(ins.a)? else {
            return Err(VmError::Malformed("array item target is not an array").into());
        };
        let value = self.read(ins.b)?;
        if ins.c.is_unused() {
            array
                .append(value)
                .map_err(|_full| VmError::Unsupported("this many array items"))?;
        } else {
            let key_value = self.read(ins.c)?;
            let Some(key) = self.compute(|cx| Ok(array_key(&key_value, cx)))? else {
                self.display(b"Warning", b"Illegal offset type");
                return self.write(ins.a, Value::Array(array));
            };
            array.assign(key, value);
        }
        self.write(ins.a, Value::Array(array))
    }

    pub(crate) fn native_implode(&mut self, args: &[Argument], result: Operand) -> Exec {
        let (glue, array) = match args {
            [] => return Err(self.too_few("implode", 0, 1)),
            [one] => match one.slot.value() {
                Value::Array(array) => (PhpString::new(&[]), array),
                other => {
                    return Err(self.throw(
                        "TypeError",
                        format!(
                            "implode(): Argument #1 ($array) must be of type array, {} given",
                            other.type_name()
                        )
                        .into_bytes(),
                    ));
                }
            },
            [first, second, ..] => {
                let left = first.slot.value();
                let right = second.slot.value();
                match (left, right) {
                    (Value::Array(array), glue) | (glue, Value::Array(array)) => {
                        (self.stringify(&glue)?, array)
                    }
                    (_, other) => {
                        return Err(self.throw(
                            "TypeError",
                            format!(
                                "implode(): Argument #2 ($array) must be of type array, {} given",
                                other.type_name()
                            )
                            .into_bytes(),
                        ));
                    }
                }
            }
        };
        let mut out = Vec::new();
        for (index, (_key, slot)) in array.iter().enumerate() {
            if index > 0 {
                out.extend_from_slice(glue.as_bytes());
            }
            let text = self.stringify(&slot.value())?;
            out.extend_from_slice(text.as_bytes());
        }
        self.write(result, Value::String(PhpString::new(&out)))
    }

    pub(crate) fn native_explode(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() < 2 {
            return Err(self.too_few("explode", args.len(), 2));
        }
        let sep = self.string_arg("explode", &args[0].slot.value())?;
        let text = self.string_arg("explode", &args[1].slot.value())?;
        if sep.is_empty() {
            return Err(self.throw_from(
                "ValueError",
                b"explode(): Argument #1 ($separator) must not be empty".to_vec(),
                b"explode",
                vec![args[0].slot.value(), args[1].slot.value()],
            ));
        }
        let mut array = PhpArray::new();
        if sep.len() == 1 {
            let needle = sep[0];
            for part in text.split(|byte| *byte == needle) {
                array
                    .append(Value::String(PhpString::new(part)))
                    .map_err(|_full| VmError::Unsupported("this many array items"))?;
            }
        } else {
            let mut rest = text.as_slice();
            while let Some(at) = rest.windows(sep.len()).position(|window| window == sep) {
                array
                    .append(Value::String(PhpString::new(&rest[..at])))
                    .map_err(|_full| VmError::Unsupported("this many array items"))?;
                rest = &rest[at + sep.len()..];
            }
            array
                .append(Value::String(PhpString::new(rest)))
                .map_err(|_full| VmError::Unsupported("this many array items"))?;
        }
        self.write(result, Value::Array(array))
    }
}
