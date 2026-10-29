//! Array literals and the first array built-ins.

use php_compiler::bytecode::{Instruction, Operand};
use php_value::ops::array_key;
use php_value::{ArrayKey, PhpArray, PhpString, Value};

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

    /// `result = a[b]`.
    pub(crate) fn fetch_dim(&mut self, ins: &Instruction) -> Exec {
        let container = self.read(ins.a)?;
        let index = self.read(ins.b)?;
        let value = self.dim_get(&container, &index)?;
        self.write(ins.result, value)
    }

    /// `a[b] = c`; unused `b` appends.
    pub(crate) fn store_dim(&mut self, ins: &Instruction) -> Exec {
        let value = self.read(ins.c)?;
        let container = self.read(ins.a)?;
        match container {
            Value::Array(mut array) => {
                self.store_array_dim(&mut array, ins.b, value)?;
                self.write(ins.a, Value::Array(array))
            }
            Value::Null => {
                let mut array = PhpArray::new();
                self.store_array_dim(&mut array, ins.b, value)?;
                self.write(ins.a, Value::Array(array))
            }
            Value::Object(object) => {
                let mut message = b"Cannot use object of type ".to_vec();
                message.extend_from_slice(object.class_name().as_bytes());
                message.extend_from_slice(b" as array");
                Err(self.throw("Error", message))
            }
            _ => Err(self.throw("Error", b"Cannot use a scalar value as an array".to_vec())),
        }
    }

    fn store_array_dim(&mut self, array: &mut PhpArray, index: Operand, value: Value) -> Exec {
        if index.is_unused() {
            array
                .append(value)
                .map_err(|_full| VmError::Unsupported("this many array items"))?;
            return Ok(());
        }
        let key_value = self.read(index)?;
        let Some(key) = self.compute(|cx| Ok(array_key(&key_value, cx)))? else {
            self.display(b"Warning", b"Illegal offset type");
            return Ok(());
        };
        array.assign(key, value);
        Ok(())
    }

    fn dim_get(&mut self, container: &Value, index: &Value) -> Exec<Value> {
        match container {
            Value::Array(array) => {
                let Some(key) = self.compute(|cx| Ok(array_key(index, cx)))? else {
                    self.display(b"Warning", b"Illegal offset type");
                    return Ok(Value::Null);
                };
                if let Some(slot) = array.get(&key) {
                    return Ok(slot.value());
                }
                self.display(b"Warning", &undefined_key_message(&key));
                Ok(Value::Null)
            }
            Value::String(text) => self.string_offset(text.as_bytes(), index),
            other => {
                self.display(b"Warning", &offset_on(other));
                Ok(Value::Null)
            }
        }
    }

    fn string_offset(&mut self, bytes: &[u8], index: &Value) -> Exec<Value> {
        let Some(key) = self.compute(|cx| Ok(array_key(index, cx)))? else {
            self.display(b"Warning", b"Illegal offset type");
            return Ok(Value::String(PhpString::new(&[])));
        };
        match key {
            ArrayKey::Int(offset) => string_byte(bytes, offset).map_or_else(
                || {
                    self.display(b"Warning", &uninitialized_offset(offset));
                    Ok(Value::String(PhpString::new(&[])))
                },
                |byte| Ok(Value::String(PhpString::new(&[byte]))),
            ),
            ArrayKey::String(text) => {
                let mut message = b"Illegal string offset \"".to_vec();
                message.extend_from_slice(text.as_bytes());
                message.push(b'"');
                self.display(b"Warning", &message);
                Ok(Value::String(PhpString::new(&[])))
            }
        }
    }

    pub(crate) fn native_implode(&mut self, args: &[Argument], result: Operand) -> Exec {
        let (glue, array) = match args {
            [] => return Err(self.too_few("implode", 0, 1, args)),
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
            return Err(self.too_few("explode", args.len(), 2, args));
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

    pub(crate) fn native_array_key_exists(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() < 2 {
            return Err(self.too_few("array_key_exists", args.len(), 2, args));
        }
        let key_value = args[0].slot.value();
        let Some(key) = self.compute(|cx| Ok(array_key(&key_value, cx)))? else {
            return Err(self.throw(
                "TypeError",
                format!(
                    "array_key_exists(): Argument #1 ($key) must be of type string|int, {} given",
                    key_value.type_name()
                )
                .into_bytes(),
            ));
        };
        let array = self.need_array("array_key_exists", 2, "array", args[1].slot.value())?;
        self.write(result, Value::Bool(array.contains_key(&key)))
    }

    pub(crate) fn native_in_array(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() < 2 {
            return Err(self.too_few("in_array", args.len(), 2, args));
        }
        let needle = args[0].slot.value();
        let array = self.need_array("in_array", 2, "haystack", args[1].slot.value())?;
        let strict = match args.get(2) {
            Some(arg) => self.compute(|cx| arg.slot.value().to_bool(cx))?,
            None => false,
        };
        let mut found = false;
        for (_key, slot) in array.iter() {
            let item = slot.value();
            found = if strict {
                self.compute(|_cx| php_value::ops::identical(&needle, &item))?
            } else {
                self.compute(|cx| php_value::ops::loose_equals(&needle, &item, cx))?
            };
            if found {
                break;
            }
        }
        self.write(result, Value::Bool(found))
    }

    pub(crate) fn native_array_merge(&mut self, args: &[Argument], result: Operand) -> Exec {
        let mut out = PhpArray::new();
        for (index, arg) in args.iter().enumerate() {
            let array = self.need_array("array_merge", index + 1, "array", arg.slot.value())?;
            for (key, slot) in array.iter() {
                match key {
                    ArrayKey::Int(_) => {
                        out.append(slot.value())
                            .map_err(|_full| VmError::Unsupported("this many array items"))?;
                    }
                    ArrayKey::String(name) => {
                        out.assign(ArrayKey::String(name.clone()), slot.value());
                    }
                }
            }
        }
        self.write(result, Value::Array(out))
    }

    pub(crate) fn native_array_keys(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("array_keys", args)?;
        let array = self.need_array("array_keys", 1, "array", value)?;
        let mut out = PhpArray::new();
        for (key, _slot) in array.iter() {
            let item = match key {
                ArrayKey::Int(number) => Value::Int(*number),
                ArrayKey::String(text) => Value::String(text.clone()),
            };
            out.append(item)
                .map_err(|_full| VmError::Unsupported("this many array items"))?;
        }
        self.write(result, Value::Array(out))
    }

    pub(crate) fn native_array_values(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("array_values", args)?;
        let array = self.need_array("array_values", 1, "array", value)?;
        let mut out = PhpArray::new();
        for (_key, slot) in array.iter() {
            out.append(slot.value())
                .map_err(|_full| VmError::Unsupported("this many array items"))?;
        }
        self.write(result, Value::Array(out))
    }

    fn need_array(&mut self, name: &str, nth: usize, param: &str, value: Value) -> Exec<PhpArray> {
        match value {
            Value::Array(array) => Ok(array),
            other => Err(self.throw(
                "TypeError",
                format!(
                    "{name}(): Argument #{nth} (${param}) must be of type array, {} given",
                    other.type_name()
                )
                .into_bytes(),
            )),
        }
    }
}

fn undefined_key_message(key: &ArrayKey) -> Vec<u8> {
    let mut message = b"Undefined array key ".to_vec();
    match key {
        ArrayKey::Int(number) => {
            message.extend_from_slice(number.to_string().as_bytes());
        }
        ArrayKey::String(text) => {
            message.push(b'"');
            message.extend_from_slice(text.as_bytes());
            message.push(b'"');
        }
    }
    message
}

fn offset_on(value: &Value) -> Vec<u8> {
    let mut message = b"Trying to access array offset on ".to_vec();
    message.extend_from_slice(value.value_name().as_bytes());
    message
}

fn uninitialized_offset(offset: i64) -> Vec<u8> {
    let mut message = b"Uninitialized string offset ".to_vec();
    message.extend_from_slice(offset.to_string().as_bytes());
    message
}

fn string_byte(bytes: &[u8], offset: i64) -> Option<u8> {
    let index = usize::try_from(offset).ok()?;
    bytes.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::{offset_on, string_byte, undefined_key_message, uninitialized_offset};
    use php_value::{ArrayKey, PhpString, Value};

    #[test]
    fn key_and_offset_messages() {
        assert_eq!(
            undefined_key_message(&ArrayKey::Int(0)),
            b"Undefined array key 0"
        );
        assert_eq!(
            undefined_key_message(&ArrayKey::String(PhpString::new(b"k"))),
            br#"Undefined array key "k""#
        );
        assert_eq!(
            offset_on(&Value::Null),
            b"Trying to access array offset on null"
        );
        assert_eq!(
            offset_on(&Value::Bool(true)),
            b"Trying to access array offset on true"
        );
        assert_eq!(uninitialized_offset(-1), b"Uninitialized string offset -1");
        assert_eq!(string_byte(b"ab", 1), Some(b'b'));
        assert_eq!(string_byte(b"ab", -1), None);
    }
}
