//! Classes, interfaces, traits and enums: construction, members, `instanceof` and cases.

use std::collections::{HashMap, HashSet};

use php_compiler::bytecode::{ClassDecl, ClassKind, Instruction};
use php_value::{PhpString, Slot, Value};

use crate::calls::Pending;
use crate::exceptions::catch_matches;
use crate::machine::{Exec, Machine, value_of};
use crate::{Output, VmError};

/// A class-like bound for this request.
pub(crate) struct Class {
    pub(crate) name: Box<[u8]>,
    pub(crate) kind: ClassKind,
    /// Lowercase parent class name, when this type extends another.
    pub(crate) parent: Option<Box<[u8]>>,
    pub(crate) properties: Vec<(Box<[u8]>, Value)>,
    pub(crate) methods: HashMap<Box<[u8]>, usize>,
    /// Native methods on a Rust-owned class, keyed lowercase.
    pub(crate) native_methods: HashMap<Box<[u8]>, NativeMethod>,
    pub(crate) implements: Vec<Box<[u8]>>,
    pub(crate) uses: Vec<Box<[u8]>>,
    pub(crate) case_names: Vec<Box<[u8]>>,
    pub(crate) case_objects: HashMap<Box<[u8]>, Value>,
}

/// A method implemented in Rust on a core-owned class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeMethod {
    /// `CoreNode::label()`.
    Label,
    /// `CoreNode::tag($value)`.
    Tag,
}

enum FoundMethod {
    Code(usize),
    Native(NativeMethod),
}

impl<O: Output> Machine<'_, O> {
    /// Registers the deferred class named by `a`.
    pub(crate) fn bind_class(&mut self, ins: &Instruction) -> Exec {
        let Value::Int(index) = self.read(ins.a)? else {
            return Err(VmError::Malformed("class index is not an integer").into());
        };
        let Ok(index) = u32::try_from(index) else {
            return Err(VmError::Malformed("class index out of range").into());
        };
        let Some(class) = self.deferred_classes.remove(&index) else {
            return Err(self.throw(
                "Error",
                b"Cannot declare class, because the name is already in use".to_vec(),
            ));
        };
        let key = class.name.to_ascii_lowercase();
        if self.classes.contains_key(key.as_slice()) {
            let mut message = b"Cannot declare class ".to_vec();
            message.extend_from_slice(&class.name);
            message.extend_from_slice(b", because the name is already in use");
            return Err(self.throw("Error", message));
        }
        self.classes.insert(key.into(), class);
        Ok(())
    }

    /// `result` is a new instance of the class named by `a`.
    pub(crate) fn new_object(&mut self, ins: &Instruction) -> Exec {
        let Value::String(name) = self.read(ins.a)? else {
            return Err(VmError::Malformed("class name is not a string").into());
        };
        self.autoload(name.as_bytes())?;
        let key = name.as_bytes().to_ascii_lowercase();
        let Some(class) = self.classes.get(key.as_slice()) else {
            let mut message = b"Class \"".to_vec();
            message.extend_from_slice(name.as_bytes());
            message.extend_from_slice(b"\" not found");
            return Err(self.throw("Error", message));
        };
        if class.name.eq_ignore_ascii_case(b"Generator") {
            return Err(self.throw(
                "Error",
                b"The \"Generator\" class is reserved for internal use and cannot be manually instantiated".to_vec(),
            ));
        }
        if let Some(message) = cannot_instantiate(class) {
            return Err(self.throw("Error", message));
        }
        let class_name = class.name.clone();
        let properties = self.inherited_properties(&key);
        let object = self
            .objects
            .create(PhpString::new(&class_name))
            .map_err(|_full| VmError::Unsupported("this many objects"))?;
        for (prop, default) in properties {
            object.with_properties_mut(|table| {
                table.assign(PhpString::new(&prop), default);
            });
        }
        if crate::exceptions::is_throwable(&class_name) {
            object.with_properties_mut(|table| {
                table.assign(
                    PhpString::new(b"message"),
                    Value::String(PhpString::new(&[])),
                );
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
        if self.init_runtime_method(&handle, key.as_bytes(), written.as_bytes())? {
            return Ok(());
        }
        self.init_declared_method(handle, key.as_bytes(), written.as_bytes(), ins.flags)
    }

    fn init_runtime_method(
        &mut self,
        handle: &php_value::Object,
        key: &[u8],
        written: &[u8],
    ) -> Exec<bool> {
        if handle
            .class_name()
            .as_bytes()
            .eq_ignore_ascii_case(b"Generator")
        {
            let Some(method) = crate::generators::GenMethod::from_name(key) else {
                return Err(self.undefined_method(b"Generator", written));
            };
            self.pending.push(Pending {
                code: 0,
                env: None,
                this: Some(handle.clone()),
                implicit_ctor: None,
                generator_method: Some((handle.clone(), method)),
                native: None,
                host: None,
                fiber_method: None,
                native_method: None,
                args: Vec::new(),
            });
            return Ok(true);
        }
        if handle
            .class_name()
            .as_bytes()
            .eq_ignore_ascii_case(b"Fiber")
        {
            let Some(method) = crate::fibers::FiberMethod::from_name(key) else {
                return Err(self.undefined_method(b"Fiber", written));
            };
            self.pending.push(Pending {
                code: 0,
                env: None,
                this: Some(handle.clone()),
                implicit_ctor: None,
                generator_method: None,
                native: None,
                host: None,
                fiber_method: Some(crate::fibers::FiberCall::Instance(handle.clone(), method)),
                native_method: None,
                args: Vec::new(),
            });
            return Ok(true);
        }
        Ok(false)
    }

    fn init_declared_method(
        &mut self,
        handle: php_value::Object,
        key: &[u8],
        written: &[u8],
        flags: u8,
    ) -> Exec {
        let class_key = handle.class_name().as_bytes().to_ascii_lowercase();
        match self.find_method(&class_key, key) {
            Some(FoundMethod::Code(code)) => {
                self.pending.push(Pending {
                    code,
                    env: None,
                    this: Some(handle),
                    implicit_ctor: None,
                    generator_method: None,
                    native: None,
                    host: None,
                    fiber_method: None,
                    native_method: None,
                    args: Vec::new(),
                });
                Ok(())
            }
            Some(FoundMethod::Native(method)) => {
                self.pending.push(Pending {
                    code: 0,
                    env: None,
                    this: Some(handle.clone()),
                    implicit_ctor: None,
                    generator_method: None,
                    native: None,
                    host: None,
                    fiber_method: None,
                    native_method: Some((handle, method)),
                    args: Vec::new(),
                });
                Ok(())
            }
            None if flags & 1 != 0 => {
                let class_name = handle.class_name().as_bytes().into();
                self.pending.push(Pending {
                    code: 0,
                    env: None,
                    this: Some(handle),
                    implicit_ctor: Some(class_name),
                    generator_method: None,
                    native: None,
                    host: None,
                    fiber_method: None,
                    native_method: None,
                    args: Vec::new(),
                });
                Ok(())
            }
            None => Err(self.undefined_method(handle.class_name().as_bytes(), written)),
        }
    }

    fn undefined_method(&self, class: &[u8], written: &[u8]) -> crate::machine::Stop {
        let mut message = b"Call to undefined method ".to_vec();
        message.extend_from_slice(class);
        message.extend_from_slice(b"::");
        message.extend_from_slice(written);
        message.extend_from_slice(b"()");
        self.throw("Error", message)
    }

    /// `result = a instanceof b`.
    pub(crate) fn instance_of(&mut self, ins: &Instruction) -> Exec {
        let value = self.read(ins.a)?;
        let Value::String(name) = self.read(ins.b)? else {
            return Err(VmError::Malformed("class name is not a string").into());
        };
        let matched = match value {
            Value::Object(handle) => {
                self.is_instance(handle.class_name().as_bytes(), name.as_bytes())
            }
            _ => false,
        };
        self.write(ins.result, Value::Bool(matched))
    }

    /// `result = a::b` for an enum case.
    pub(crate) fn class_const(&mut self, ins: &Instruction) -> Exec {
        let Value::String(class_name) = self.read(ins.a)? else {
            return Err(VmError::Malformed("class name is not a string").into());
        };
        let Value::String(member) = self.read(ins.b)? else {
            return Err(VmError::Malformed("constant name is not a string").into());
        };
        if let Some(value) = self.enum_case(class_name.as_bytes(), member.as_bytes())? {
            return self.write(ins.result, value);
        }
        let mut message = b"Undefined constant ".to_vec();
        message.extend_from_slice(class_name.as_bytes());
        message.extend_from_slice(b"::");
        message.extend_from_slice(member.as_bytes());
        Err(self.throw("Error", message))
    }

    fn enum_case(&mut self, class_name: &[u8], member: &[u8]) -> Exec<Option<Value>> {
        let key = class_name.to_ascii_lowercase();
        let Some(class) = self.classes.get(key.as_slice()) else {
            return Ok(None);
        };
        if class.kind != ClassKind::Enum
            || !class.case_names.iter().any(|name| name.as_ref() == member)
        {
            return Ok(None);
        }
        if let Some(value) = class.case_objects.get(member) {
            return Ok(Some(value.clone()));
        }
        let declared = class.name.clone();
        let object = self
            .objects
            .create(PhpString::new(&declared))
            .map_err(|_full| VmError::Unsupported("this many objects"))?;
        object.with_properties_mut(|table| {
            table.assign(
                PhpString::new(b"name"),
                Value::String(PhpString::new(member)),
            );
        });
        let value = Value::Object(object);
        if let Some(class) = self.classes.get_mut(key.as_slice()) {
            class.case_objects.insert(member.into(), value.clone());
        }
        Ok(Some(value))
    }

    fn is_instance(&self, object_class: &[u8], target: &[u8]) -> bool {
        let object = object_class.to_ascii_lowercase();
        let target = target.to_ascii_lowercase();
        object == target
            || catch_matches(&target, &object)
            || self.walks_implements(&object, &target)
            || self.walks_parent(&object, &target)
    }

    fn walks_parent(&self, class_key: &[u8], target: &[u8]) -> bool {
        let mut current = class_key.to_vec();
        let mut seen = HashSet::new();
        while seen.insert(current.clone()) {
            let Some(class) = self.classes.get(current.as_slice()) else {
                return false;
            };
            let Some(parent) = &class.parent else {
                return false;
            };
            if parent.as_ref() == target {
                return true;
            }
            current = parent.to_vec();
        }
        false
    }

    fn inherited_properties(&self, class_key: &[u8]) -> Vec<(Box<[u8]>, Value)> {
        let mut chain = Vec::new();
        let mut current = Some(class_key.to_vec());
        let mut seen = HashSet::new();
        while let Some(key) = current {
            if !seen.insert(key.clone()) {
                break;
            }
            let Some(class) = self.classes.get(key.as_slice()) else {
                break;
            };
            chain.push(class.properties.clone());
            current = class.parent.as_ref().map(|parent| parent.to_vec());
        }
        chain.reverse();
        let mut properties = Vec::new();
        for group in chain {
            for (name, value) in group {
                if let Some((_, existing)) = properties.iter_mut().find(|(held, _)| held == &name) {
                    *existing = value;
                } else {
                    properties.push((name, value));
                }
            }
        }
        properties
    }

    fn find_method(&self, class_key: &[u8], method: &[u8]) -> Option<FoundMethod> {
        let mut current = Some(class_key.to_vec());
        let mut seen = HashSet::new();
        while let Some(key) = current {
            if !seen.insert(key.clone()) {
                break;
            }
            let Some(class) = self.classes.get(key.as_slice()) else {
                break;
            };
            if let Some(&code) = class.methods.get(method) {
                return Some(FoundMethod::Code(code));
            }
            if let Some(&native) = class.native_methods.get(method) {
                return Some(FoundMethod::Native(native));
            }
            current = class.parent.as_ref().map(|parent| parent.to_vec());
        }
        None
    }

    pub(crate) fn init_class_static_call(
        &mut self,
        class: &[u8],
        key: &[u8],
        written: &[u8],
    ) -> Exec {
        let resolved = self.resolve_static_class(class)?;
        let this = self.current_this();
        match self.find_method(&resolved, key) {
            Some(FoundMethod::Code(code)) => {
                self.pending.push(Pending {
                    code,
                    env: None,
                    this,
                    implicit_ctor: None,
                    generator_method: None,
                    native: None,
                    host: None,
                    fiber_method: None,
                    native_method: None,
                    args: Vec::new(),
                });
                Ok(())
            }
            Some(FoundMethod::Native(method)) => {
                let Some(this) = this else {
                    let name = match method {
                        NativeMethod::Label => "label",
                        NativeMethod::Tag => "tag",
                    };
                    let mut message = b"Non-static method ".to_vec();
                    message.extend_from_slice(&resolved);
                    message.extend_from_slice(b"::");
                    message.extend_from_slice(name.as_bytes());
                    message.extend_from_slice(b"() cannot be called statically");
                    return Err(self.throw("Error", message));
                };
                self.pending.push(Pending {
                    code: 0,
                    env: None,
                    this: Some(this.clone()),
                    implicit_ctor: None,
                    generator_method: None,
                    native: None,
                    host: None,
                    fiber_method: None,
                    native_method: Some((this, method)),
                    args: Vec::new(),
                });
                Ok(())
            }
            None => {
                let mut message = b"Call to undefined method ".to_vec();
                message.extend_from_slice(class);
                message.extend_from_slice(b"::");
                message.extend_from_slice(written);
                message.extend_from_slice(b"()");
                Err(self.throw("Error", message))
            }
        }
    }

    fn resolve_static_class(&self, written: &[u8]) -> Exec<Vec<u8>> {
        if written.eq_ignore_ascii_case(b"parent") {
            let Some(current) = self.current_class() else {
                return Err(self.throw(
                    "Error",
                    b"Cannot use \"parent\" when no class scope is active".to_vec(),
                ));
            };
            let key = current.to_ascii_lowercase();
            let Some(parent) = self
                .classes
                .get(key.as_slice())
                .and_then(|class| class.parent.clone())
            else {
                return Err(self.throw(
                    "Error",
                    b"Cannot use \"parent\" when current class scope has no parent".to_vec(),
                ));
            };
            return Ok(parent.into());
        }
        if written.eq_ignore_ascii_case(b"self")
            && let Some(current) = self.current_class()
        {
            return Ok(current.to_ascii_lowercase());
        }
        Ok(written.to_ascii_lowercase())
    }

    fn current_class(&self) -> Option<&[u8]> {
        let frame = self.frames.last()?;
        let name = self.codes.get(frame.code)?.code.name.as_ref();
        let split = name.windows(2).position(|pair| pair == b"->")?;
        Some(&name[..split])
    }

    fn current_this(&self) -> Option<php_value::Object> {
        let frame = self.frames.last()?;
        let names = &self.codes.get(frame.code)?.code.slots;
        let index = names.iter().position(|name| name.as_ref() == b"this")?;
        match frame.slots.get(index)?.as_ref()? {
            Slot::Value(Value::Object(object)) => Some(object.clone()),
            Slot::Reference(reference) => match reference.get() {
                Value::Object(object) => Some(object),
                _ => None,
            },
            Slot::Value(_) => None,
        }
    }

    pub(crate) fn dispatch_core_method(
        &mut self,
        this: &php_value::Object,
        method: NativeMethod,
        args: &[crate::calls::Argument],
        result: php_compiler::bytecode::Operand,
    ) -> Exec {
        match method {
            NativeMethod::Label => {
                let tag = this
                    .with_properties(|properties| properties.get(b"tag").map(Slot::value))
                    .unwrap_or(Value::Null);
                let text = self.stringify(&tag)?;
                self.write(result, Value::String(text))
            }
            NativeMethod::Tag => {
                let value = args.first().map_or(Value::Null, |arg| arg.slot.value());
                this.with_properties_mut(|properties| {
                    properties.assign(php_value::PhpString::from("tag"), value);
                });
                self.write(result, Value::Object(this.clone()))
            }
        }
    }

    fn walks_implements(&self, class_key: &[u8], target: &[u8]) -> bool {
        let mut pending = vec![class_key.to_vec().into_boxed_slice()];
        let mut seen = HashSet::new();
        while let Some(current) = pending.pop() {
            if !seen.insert(current.clone()) {
                continue;
            }
            let Some(class) = self.classes.get(current.as_ref()) else {
                continue;
            };
            for iface in &class.implements {
                let key = iface.to_ascii_lowercase().into_boxed_slice();
                if key.as_ref() == target {
                    return true;
                }
                pending.push(key);
            }
        }
        false
    }
}

fn cannot_instantiate(class: &Class) -> Option<Vec<u8>> {
    let kind = match class.kind {
        ClassKind::Class => return None,
        ClassKind::Interface => "interface",
        ClassKind::Trait => "trait",
        ClassKind::Enum => "enum",
    };
    let mut message = b"Cannot instantiate ".to_vec();
    message.extend_from_slice(kind.as_bytes());
    message.push(b' ');
    message.extend_from_slice(&class.name);
    Some(message)
}

fn empty_class(name: &[u8], kind: ClassKind) -> Class {
    Class {
        name: name.into(),
        kind,
        parent: None,
        properties: Vec::new(),
        methods: HashMap::new(),
        native_methods: HashMap::new(),
        implements: Vec::new(),
        uses: Vec::new(),
        case_names: Vec::new(),
        case_objects: HashMap::new(),
    }
}

/// Registers the built-in throwable classes so `new Exception` and `new Error` resolve.
pub(crate) fn register_throwables(table: &mut HashMap<Box<[u8]>, Class>) {
    for name in [
        &b"Exception"[..],
        b"Error",
        b"TypeError",
        b"ArgumentCountError",
        b"ArithmeticError",
        b"DivisionByZeroError",
        b"ParseError",
        b"FiberError",
        b"ValueError",
    ] {
        table
            .entry(name.to_ascii_lowercase().into())
            .or_insert_with(|| {
                let mut class = empty_class(name, ClassKind::Class);
                class.properties = vec![(
                    b"message".to_vec().into_boxed_slice(),
                    Value::String(PhpString::new(&[])),
                )];
                class
            });
    }
}

/// Registers the Rust-owned class PHP types may extend (ADR-0003).
pub(crate) fn register_core_node(table: &mut HashMap<Box<[u8]>, Class>) {
    let mut class = empty_class(b"CoreNode", ClassKind::Class);
    class.properties = vec![(
        b"tag".to_vec().into_boxed_slice(),
        Value::String(PhpString::new(b"core")),
    )];
    class
        .native_methods
        .insert(b"label".to_vec().into(), NativeMethod::Label);
    class
        .native_methods
        .insert(b"tag".to_vec().into(), NativeMethod::Tag);
    table
        .entry(b"corenode".to_vec().into_boxed_slice())
        .or_insert(class);
}

/// Loads methods of `classes` onto `codes` and returns the class table.
pub(crate) fn bind_classes(
    classes: &[ClassDecl],
    codes: &mut Vec<crate::machine::Loaded>,
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
                kind: class.kind,
                parent: class
                    .extends
                    .as_ref()
                    .map(|name| name.to_ascii_lowercase().into()),
                properties: class
                    .properties
                    .iter()
                    .map(|property| (property.name.clone(), value_of(&property.default)))
                    .collect(),
                methods,
                native_methods: HashMap::new(),
                implements: class
                    .implements
                    .iter()
                    .map(|name| name.to_ascii_lowercase().into())
                    .collect(),
                uses: class
                    .uses
                    .iter()
                    .map(|name| name.to_ascii_lowercase().into())
                    .collect(),
                case_names: class.cases.iter().map(|case| case.name.clone()).collect(),
                case_objects: HashMap::new(),
            },
        );
    }
    apply_trait_uses(&mut table);
    table
}

fn apply_trait_uses(table: &mut HashMap<Box<[u8]>, Class>) {
    let mut changed = true;
    while changed {
        changed = false;
        let keys: Vec<Box<[u8]>> = table.keys().cloned().collect();
        for key in keys {
            let uses = table.get(key.as_ref()).map(|class| class.uses.clone());
            let Some(uses) = uses else {
                continue;
            };
            let extra: Vec<(Box<[u8]>, usize)> = uses
                .iter()
                .filter_map(|name| table.get(name.as_ref()))
                .flat_map(|used| used.methods.iter().map(|(n, code)| (n.clone(), *code)))
                .collect();
            let Some(dest) = table.get_mut(key.as_ref()) else {
                continue;
            };
            for (name, code) in extra {
                if dest.methods.contains_key(&name) {
                    continue;
                }
                dest.methods.insert(name, code);
                changed = true;
            }
        }
    }
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
