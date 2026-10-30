//! `isset` and `empty`.

use php_compiler::bytecode::Instruction;
use php_value::ops::array_key;
use php_value::{ArrayKey, PhpString, Value};

use crate::Output;
use crate::machine::{Exec, Machine};

impl<O: Output> Machine<'_, O> {
    /// Whether `a` is set, or empty when flag bit 0 is set.
    pub(crate) fn isset(&mut self, ins: &Instruction) -> Exec {
        let value = self.read_quiet(ins.a)?;
        self.write(
            ins.result,
            Value::Bool(isset_or_empty(&value, ins.flags & 1 != 0)),
        )
    }

    /// Whether `a[b]` is set, or empty when flag bit 0 is set.
    pub(crate) fn isset_dim(&mut self, ins: &Instruction) -> Exec {
        let container = self.read_quiet(ins.a)?;
        let index = self.read(ins.b)?;
        let result = self.dim_isset(&container, &index, ins.flags & 1 != 0)?;
        self.write(ins.result, Value::Bool(result))
    }

    fn dim_isset(&mut self, container: &Value, index: &Value, empty: bool) -> Exec<bool> {
        match container {
            Value::Array(array) => {
                let Some(key) = self.compute(|cx| Ok(array_key(index, cx)))? else {
                    self.display(b"Warning", b"Illegal offset type");
                    return Ok(empty);
                };
                match array.get(&key) {
                    Some(slot) => Ok(isset_or_empty(&slot.value(), empty)),
                    None => Ok(empty),
                }
            }
            Value::String(text) => self.string_dim_isset(text.as_bytes(), index, empty),
            Value::Object(object) => {
                let mut message = b"Cannot use object of type ".to_vec();
                message.extend_from_slice(object.class_name().as_bytes());
                message.extend_from_slice(b" as array");
                Err(self.throw("Error", message))
            }
            _ => Ok(empty),
        }
    }

    fn string_dim_isset(&mut self, bytes: &[u8], index: &Value, empty: bool) -> Exec<bool> {
        let Some(key) = self.compute(|cx| Ok(array_key(index, cx)))? else {
            return Ok(empty);
        };
        match key {
            ArrayKey::Int(offset) => match string_offset(bytes, offset) {
                Some(byte) => Ok(isset_or_empty(
                    &Value::String(PhpString::new(&[byte])),
                    empty,
                )),
                None => Ok(empty),
            },
            ArrayKey::String(_) => Ok(empty),
        }
    }
}

fn isset_or_empty(value: &Value, empty: bool) -> bool {
    if empty {
        is_empty(value)
    } else {
        !matches!(value, Value::Null)
    }
}

fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(value) => !*value,
        Value::Int(number) => *number == 0,
        Value::Float(number) => *number == 0.0,
        Value::String(text) => matches!(text.as_bytes(), b"" | b"0"),
        Value::Array(array) => array.is_empty(),
        Value::Object(_) => false,
    }
}

fn string_offset(bytes: &[u8], offset: i64) -> Option<u8> {
    let index = if offset >= 0 {
        usize::try_from(offset).ok()?
    } else {
        let from_end = usize::try_from(offset.checked_neg()?).ok()?;
        bytes.len().checked_sub(from_end)?
    };
    bytes.get(index).copied()
}

#[cfg(test)]
mod tests {
    use super::{is_empty, isset_or_empty, string_offset};
    use php_value::{PhpArray, Value};

    #[test]
    fn emptiness_matches_php() {
        assert!(is_empty(&Value::Null));
        assert!(is_empty(&Value::Bool(false)));
        assert!(!is_empty(&Value::Bool(true)));
        assert!(is_empty(&Value::Int(0)));
        assert!(!is_empty(&Value::Int(1)));
        assert!(is_empty(&Value::Float(0.0)));
        assert!(is_empty(&Value::from("")));
        assert!(is_empty(&Value::from("0")));
        assert!(!is_empty(&Value::from("00")));
        assert!(is_empty(&Value::Array(PhpArray::new())));
        assert!(!isset_or_empty(&Value::Null, false));
        assert!(isset_or_empty(&Value::Int(0), false));
        assert!(isset_or_empty(&Value::Null, true));
    }

    #[test]
    fn string_offsets_accept_negative_indexes() {
        assert_eq!(string_offset(b"ab", 0), Some(b'a'));
        assert_eq!(string_offset(b"ab", 1), Some(b'b'));
        assert_eq!(string_offset(b"ab", -1), Some(b'b'));
        assert_eq!(string_offset(b"ab", -2), Some(b'a'));
        assert_eq!(string_offset(b"ab", -3), None);
        assert_eq!(string_offset(b"ab", 2), None);
    }
}
