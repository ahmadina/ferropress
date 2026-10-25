//! Classes: constructing objects, reading and writing properties, and calling methods.

use std::collections::HashMap;

use php_compiler::bytecode::{ClassDecl, Instruction};
use php_value::{PhpString, Slot, Value};

use crate::calls::Pending;
use crate::machine::{Exec, Machine, value_of};
use crate::{Output, VmError};

/// A class bound for this request.
pub(crate) struct Class {
    pub(crate) name: Box<[u8]>,
    pub(crate) properties: Vec<(Box<[u8]>, Value)>,
    pub(crate) methods: HashMap<Box<[u8]>, usize>,
}

impl<O: Output> Machine<'_, '_, O> {
    /// `result` is a new instance of the class named by `a`.
    pub(crate) fn new_object(&mut self, ins: &Instruction) -> Exec {
        let Value::String(name) = self.read(ins.a)? else {
            return Err(VmError::Malformed("class name is not a string").into());
        };
        let key = name.as_bytes().to_ascii_lowercase();
        let Some(class) = self.classes.get(key.as_slice()) else {
            let mut message = b"Class \"".to_vec();
            message.extend_from_slice(name.as_bytes());
            message.extend_from_slice(b"\" not found");
            return Err(self.throw("Error", message));
        };
        let class_name = class.name.clone();
        let properties = class.properties.clone();
        let object = self
            .objects
            .create(PhpString::new(&class_name))
            .map_err(|_full| VmError::Unsupported("this many objects"))?;
        for (prop, default) in properties {
            object.with_properties_mut(|table| {
                table.assign(PhpString::new(&prop), default);
            });
        }
        self.write(ins.result, Value::Object(object))
    }

    /// `result = a->b`.
    pub(crate) fn load_prop(&mut self, ins: &Instruction) -> Exec {
        let object = self.read(ins.a)?;
        let Value::String(name) = self.read(ins.b)? else {
            return Err(VmError::Malformed("property name is not a string").into());
        };
        let Value::Object(handle) = object else {
            self.display(
                b"Warning",
                &property_message(b"Attempt to read property \"", name.as_bytes(), &object),
            );
            return self.write(ins.result, Value::Null);
        };
        let value =
            handle.with_properties(|properties| properties.get(name.as_bytes()).map(Slot::value));
        let Some(value) = value else {
            let mut message = b"Undefined property: ".to_vec();
            message.extend_from_slice(handle.class_name().as_bytes());
            message.extend_from_slice(b"::$");
            message.extend_from_slice(name.as_bytes());
            self.display(b"Warning", &message);
            return self.write(ins.result, Value::Null);
        };
        self.write(ins.result, value)
    }

    /// `a->b = c`.
    pub(crate) fn store_prop(&mut self, ins: &Instruction) -> Exec {
        let object = self.read(ins.a)?;
        let Value::String(name) = self.read(ins.b)? else {
            return Err(VmError::Malformed("property name is not a string").into());
        };
        let value = self.read(ins.c)?;
        let Value::Object(handle) = object else {
            return Err(self.throw(
                "Error",
                property_message(b"Attempt to assign property \"", name.as_bytes(), &object),
            ));
        };
        handle.with_properties_mut(|properties| {
            properties.assign(PhpString::new(name.as_bytes()), value);
        });
        Ok(())
    }

    /// Starts a call of method `b` on the object `a`.
    pub(crate) fn init_method_call(&mut self, ins: &Instruction) -> Exec {
        let object = self.read(ins.a)?;
        let Value::String(key) = self.read(ins.b)? else {
            return Err(VmError::Malformed("method name is not a string").into());
        };
        let Value::String(written) = self.read(ins.c)? else {
            return Err(VmError::Malformed("method name is not a string").into());
        };
        let Value::Object(handle) = object else {
            let mut message = b"Call to a member function ".to_vec();
            message.extend_from_slice(written.as_bytes());
            message.extend_from_slice(b"() on ");
            message.extend_from_slice(value_type_name(&object).as_bytes());
            return Err(self.throw("Error", message));
        };
        let class_key = handle.class_name().as_bytes().to_ascii_lowercase();
        if let Some(&code) = self
            .classes
            .get(class_key.as_slice())
            .and_then(|class| class.methods.get(key.as_bytes()))
        {
            self.pending.push(Pending {
                code,
                env: None,
                this: Some(handle),
                implicit_ctor: None,
                args: Vec::new(),
            });
            return Ok(());
        }
        if ins.flags & 1 != 0 {
            let class_name = handle.class_name().as_bytes().into();
            self.pending.push(Pending {
                code: 0,
                env: None,
                this: Some(handle),
                implicit_ctor: Some(class_name),
                args: Vec::new(),
            });
            return Ok(());
        }
        let mut message = b"Call to undefined method ".to_vec();
        message.extend_from_slice(handle.class_name().as_bytes());
        message.extend_from_slice(b"::");
        message.extend_from_slice(written.as_bytes());
        message.extend_from_slice(b"()");
        Err(self.throw("Error", message))
    }
}

/// Loads methods of `classes` onto `codes` and returns the class table.
pub(crate) fn bind_classes<'u>(
    classes: &'u [ClassDecl],
    codes: &mut Vec<crate::machine::Loaded<'u>>,
) -> HashMap<Box<[u8]>, Class> {
    let mut table = HashMap::with_capacity(classes.len());
    for class in classes {
        let mut methods = HashMap::with_capacity(class.methods.len());
        for method in &class.methods {
            methods.insert(method.name.to_ascii_lowercase().into(), codes.len());
            codes.push(crate::machine::load(&method.code));
        }
        table.insert(
            class.name.to_ascii_lowercase().into(),
            Class {
                name: class.name.clone(),
                properties: class
                    .properties
                    .iter()
                    .map(|property| (property.name.clone(), value_of(&property.default)))
                    .collect(),
                methods,
            },
        );
    }
    table
}

fn property_message(prefix: &[u8], name: &[u8], value: &Value) -> Vec<u8> {
    let mut message = prefix.to_vec();
    message.extend_from_slice(name);
    message.extend_from_slice(b"\" on ");
    message.extend_from_slice(value_type_name(value).as_bytes());
    message
}

fn value_type_name(value: &Value) -> String {
    match value {
        Value::Bool(true) => "true".into(),
        Value::Bool(false) => "false".into(),
        other => other.type_name().into(),
    }
}
