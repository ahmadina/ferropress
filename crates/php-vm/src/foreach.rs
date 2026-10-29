//! `foreach` over arrays and object properties.

use php_compiler::bytecode::Instruction;
use php_value::{ArrayKey, PhpString, Value};

use crate::Output;
use crate::VmError;
use crate::machine::{Exec, Machine};

impl<O: Output> Machine<'_, '_, O> {
    /// Whether `a` has an element at or after cursor `b`; `b` becomes that position.
    pub(crate) fn iter_ready(&mut self, ins: &Instruction) -> Exec {
        let container = self.read(ins.a)?;
        let cursor = match self.read(ins.b)? {
            Value::Int(n) => usize::try_from(n).unwrap_or(usize::MAX),
            _ => return Err(VmError::Malformed("foreach cursor is not an integer").into()),
        };
        match next_entry(&container, cursor) {
            Some((at, _, _)) => {
                let at = i64::try_from(at).unwrap_or(i64::MAX);
                self.write(ins.b, Value::Int(at))?;
                self.write(ins.result, Value::Bool(true))
            }
            None => match &container {
                Value::Array(_) | Value::Object(_) => self.write(ins.result, Value::Bool(false)),
                other => {
                    let mut message = b"foreach() argument must be of type array|object, ".to_vec();
                    message.extend_from_slice(other.type_name().as_bytes());
                    message.extend_from_slice(b" given");
                    self.display(b"Warning", &message);
                    self.write(ins.result, Value::Bool(false))
                }
            },
        }
    }

    /// The key or value of the entry at cursor `b` of `a`.
    pub(crate) fn iter_pick(&mut self, ins: &Instruction) -> Exec {
        let container = self.read(ins.a)?;
        let cursor = match self.read(ins.b)? {
            Value::Int(n) => usize::try_from(n).unwrap_or(usize::MAX),
            _ => return Err(VmError::Malformed("foreach cursor is not an integer").into()),
        };
        let Some((_, key, value)) = next_entry(&container, cursor) else {
            return Err(VmError::Malformed("foreach pick past the last element").into());
        };
        let picked = if ins.flags & 1 != 0 { key } else { value };
        self.write(ins.result, picked)
    }
}

fn next_entry(container: &Value, cursor: usize) -> Option<(usize, Value, Value)> {
    match container {
        Value::Array(array) => array.entry_from(cursor).map(|(at, key, slot)| {
            let key = match key {
                ArrayKey::Int(number) => Value::Int(*number),
                ArrayKey::String(text) => Value::String(text.clone()),
            };
            (at, key, slot.value())
        }),
        Value::Object(object) => object.with_properties(|properties| {
            properties
                .iter()
                .enumerate()
                .skip(cursor)
                .find(|(_, (name, _))| name.as_bytes().first() != Some(&0))
                .map(|(at, (name, slot))| {
                    (
                        at,
                        Value::String(PhpString::new(name.as_bytes())),
                        slot.value(),
                    )
                })
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::next_entry;
    use php_value::{PhpArray, Value};

    #[test]
    fn next_entry_walks_an_array() {
        let mut array = PhpArray::new();
        array.append(Value::Int(1)).expect("append");
        array.append(Value::Int(2)).expect("append");
        let container = Value::Array(array);
        let (at, _key, value) = next_entry(&container, 0).expect("first");
        assert_eq!(at, 0);
        assert!(matches!(value, Value::Int(1)));
        assert!(next_entry(&container, at + 1).is_some());
        assert!(next_entry(&container, 2).is_none());
    }

    #[test]
    fn next_entry_walks_visible_object_properties() {
        let store = php_value::ObjectStore::new();
        let object = store
            .create(php_value::PhpString::new(b"C"))
            .expect("object");
        object.with_properties_mut(|properties| {
            properties.assign(php_value::PhpString::new(b"a"), Value::Int(1));
            properties.assign(php_value::PhpString::new(&[0, b'x']), Value::Int(9));
            properties.assign(php_value::PhpString::new(b"b"), Value::Int(2));
        });
        let container = Value::Object(object);
        let (at, _key, value) = next_entry(&container, 0).expect("first");
        assert_eq!(at, 0);
        assert!(matches!(value, Value::Int(1)));
        let (at, _key, value) = next_entry(&container, at + 1).expect("visible after hidden");
        assert_eq!(at, 2);
        assert!(matches!(value, Value::Int(2)));
        assert!(next_entry(&container, at + 1).is_none());
    }

    #[test]
    fn next_entry_skips_scalars() {
        assert!(next_entry(&Value::Int(1), 0).is_none());
        assert!(next_entry(&Value::Null, 0).is_none());
    }
}
