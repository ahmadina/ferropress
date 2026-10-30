//! `serialize` and `unserialize` for scalars, arrays and objects.

use php_compiler::bytecode::Operand;
use php_serialize::{Diagnostic, Entries, Key, Severity, Value as Token, Writer, decode};
use php_value::{ArrayKey, PhpArray, PhpString, Value};

use crate::Output;
use crate::VmError;
use crate::calls::Argument;
use crate::machine::{Exec, Machine};

impl<O: Output> Machine<'_, O> {
    pub(crate) fn native_serialize(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() != 1 {
            return Err(if args.is_empty() {
                self.too_few("serialize", 0, 1, args)
            } else {
                self.too_many("serialize", args.len(), 1, args)
            });
        }
        let mut writer = Writer::new();
        write_value(&mut writer, &args[0].slot.value())?;
        self.write(result, Value::String(PhpString::new(writer.as_bytes())))
    }

    pub(crate) fn native_unserialize(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.is_empty() {
            return Err(self.too_few_at_least("unserialize", 0, 1, args));
        }
        if args.len() > 1 {
            return Err(VmError::Unsupported("unserialize options").into());
        }
        let data = self.typed_string("unserialize", 1, "data", &args[0].slot.value(), args)?;
        if data.is_empty() {
            return self.write(result, Value::Bool(false));
        }
        match decode(&data) {
            Ok(document) => {
                self.emit_serialize_diagnostics(document.diagnostics());
                if document.consumed() < data.len() {
                    self.unserialize_warning(
                        b"Extra data starting at offset ",
                        document.consumed(),
                        data.len(),
                    );
                }
                let value = self.read_token(document.root())?;
                self.write(result, value)
            }
            Err(error) => {
                self.emit_serialize_diagnostics(error.diagnostics());
                self.unserialize_warning(b"Error at offset ", error.offset(), data.len());
                self.write(result, Value::Bool(false))
            }
        }
    }

    fn emit_serialize_diagnostics(&mut self, diagnostics: &[Diagnostic<'_>]) {
        for diagnostic in diagnostics {
            let mut message = if diagnostic.has_function_prefix() {
                b"unserialize(): ".to_vec()
            } else {
                Vec::new()
            };
            message.extend_from_slice(&diagnostic.message());
            let label = match diagnostic.severity() {
                Severity::Warning => b"Warning".as_slice(),
                Severity::Deprecated => b"Deprecated".as_slice(),
            };
            self.display(label, &message);
        }
    }

    fn unserialize_warning(&mut self, prefix: &[u8], offset: usize, total: usize) {
        let mut message = b"unserialize(): ".to_vec();
        message.extend_from_slice(prefix);
        message.extend_from_slice(offset.to_string().as_bytes());
        message.extend_from_slice(b" of ");
        message.extend_from_slice(total.to_string().as_bytes());
        message.extend_from_slice(b" bytes");
        self.display(b"Warning", &message);
    }

    fn read_token(&mut self, token: Token<'_, '_>) -> Exec<Value> {
        match token {
            Token::Null => Ok(Value::Null),
            Token::Bool(flag) => Ok(Value::Bool(flag)),
            Token::Int(number) => Ok(Value::Int(number)),
            Token::Float(number) => Ok(Value::Float(number)),
            Token::String(bytes) => Ok(Value::String(PhpString::new(bytes))),
            Token::Array(entries) => self.read_array(entries),
            Token::Object { class, properties } => self.read_object(class, properties),
            Token::Custom { .. } => Err(VmError::Unsupported("unserialize custom objects").into()),
            Token::Enum { .. } => Err(VmError::Unsupported("unserialize enums").into()),
            Token::Reference(_) | Token::ObjectReference(_) => {
                Err(VmError::Unsupported("unserialize references").into())
            }
        }
    }

    fn read_array(&mut self, entries: Entries<'_, '_>) -> Exec<Value> {
        let mut array = PhpArray::new();
        for (key, token) in entries {
            let value = self.read_token(token)?;
            array.assign(array_key(key), value);
        }
        Ok(Value::Array(array))
    }

    fn read_object(&mut self, class: &[u8], properties: Entries<'_, '_>) -> Exec<Value> {
        let object = self
            .objects
            .create(PhpString::new(class))
            .map_err(|_full| VmError::Unsupported("this many objects"))?;
        for (key, token) in properties {
            let value = self.read_token(token)?;
            let name = match key {
                Key::Int(number) => PhpString::new(number.to_string().as_bytes()),
                Key::String(bytes) => PhpString::new(bytes),
            };
            object.with_properties_mut(|table| {
                table.assign(name, value);
            });
        }
        Ok(Value::Object(object))
    }
}

fn write_value(writer: &mut Writer, value: &Value) -> Result<(), VmError> {
    match value {
        Value::Null => writer.null(),
        Value::Bool(flag) => writer.bool(*flag),
        Value::Int(number) => writer.int(*number),
        Value::Float(number) => writer.float(*number),
        Value::String(text) => writer.string(text.as_bytes()),
        Value::Array(array) => write_array(writer, array)?,
        Value::Object(object) => write_object(writer, object)?,
    }
    Ok(())
}

fn write_array(writer: &mut Writer, array: &PhpArray) -> Result<(), VmError> {
    writer.array(array.len());
    for (key, slot) in array.iter() {
        match key {
            ArrayKey::Int(number) => writer.int(number),
            ArrayKey::String(text) => writer.string(text.as_bytes()),
        }
        write_value(writer, &slot.value())?;
    }
    writer.end();
    Ok(())
}

fn write_object(writer: &mut Writer, object: &php_value::Object) -> Result<(), VmError> {
    let class = object.class_name().as_bytes().to_vec();
    let mut pairs = Vec::new();
    object.with_properties(|properties| {
        for (name, slot) in properties.iter() {
            pairs.push((name.as_bytes().to_vec(), slot.value()));
        }
    });
    writer.object(&class, pairs.len());
    for (name, value) in pairs {
        writer.string(&name);
        write_value(writer, &value)?;
    }
    writer.end();
    Ok(())
}

fn array_key(key: Key<'_>) -> ArrayKey {
    match key {
        Key::Int(number) => ArrayKey::Int(number),
        Key::String(bytes) => ArrayKey::from_bytes(bytes),
    }
}

#[cfg(test)]
mod tests {
    use php_serialize::Writer;
    use php_value::{PhpArray, Value};

    use super::write_value;

    #[test]
    fn scalars_and_arrays_write_php_tokens() {
        let mut writer = Writer::new();
        write_value(&mut writer, &Value::Null).unwrap();
        write_value(&mut writer, &Value::Bool(true)).unwrap();
        write_value(&mut writer, &Value::Int(5)).unwrap();
        write_value(&mut writer, &Value::from("ab")).unwrap();
        write_value(&mut writer, &Value::Float(1.5)).unwrap();
        let mut array = PhpArray::new();
        array.assign(0.into(), Value::Int(1));
        write_value(&mut writer, &Value::Array(array)).unwrap();
        assert_eq!(
            writer.as_bytes(),
            br#"N;b:1;i:5;s:2:"ab";d:1.5;a:1:{i:0;i:1;}"#
        );
    }
}
