//! `define`, `defined`, `constant`, and bare constant names.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use php_compiler::bytecode::{Instruction, Operand};
use php_value::{PhpString, Value};

use crate::Output;
use crate::calls::Argument;
use crate::machine::{Exec, Machine, Stop};

/// User constants for one request, shared with included files.
pub(crate) type Table = Rc<RefCell<HashMap<Box<[u8]>, Value>>>;

/// An empty constant table.
pub(crate) fn new_table() -> Table {
    Rc::new(RefCell::new(HashMap::new()))
}

/// Drops a single leading `\` so `\\FOO` looks up `FOO`.
fn lookup_name(name: &[u8]) -> &[u8] {
    name.strip_prefix(b"\\").unwrap_or(name)
}

/// `true`, `false` or `null` in any case.
fn keyword_value(name: &[u8]) -> Option<Value> {
    if name.eq_ignore_ascii_case(b"true") {
        Some(Value::Bool(true))
    } else if name.eq_ignore_ascii_case(b"false") {
        Some(Value::Bool(false))
    } else if name.eq_ignore_ascii_case(b"null") {
        Some(Value::Null)
    } else {
        None
    }
}

/// Whether `name` is `true`, `false` or `null` in any case.
fn is_keyword(name: &[u8]) -> bool {
    keyword_value(name).is_some()
}

/// Class and member of `A::B`, when `name` holds `::`.
fn class_member(name: &[u8]) -> Option<(&[u8], &[u8])> {
    let start = name.windows(2).position(|pair| pair == b"::")?;
    Some((&name[..start], &name[start + 2..]))
}

impl<O: Output> Machine<'_, O> {
    /// `result` is the global constant named by `a`.
    pub(crate) fn fetch_const(&mut self, ins: &Instruction) -> Exec {
        let Value::String(name) = self.read(ins.a)? else {
            return Err(crate::VmError::Malformed("constant name is not a string").into());
        };
        let value = self.require_constant(name.as_bytes(), false)?;
        self.write(ins.result, value)
    }

    pub(crate) fn native_define(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() < 2 {
            return Err(self.too_few_at_least("define", args.len(), 2, args));
        }
        if args.len() > 3 {
            return Err(self.too_many_at_most("define", args.len(), 3, args));
        }
        let name = self.typed_string("define", 1, "constant_name", &args[0].slot.value(), args)?;
        if name.windows(2).any(|pair| pair == b"::") {
            return Err(self.define_class_constant(args));
        }
        if args.len() == 3 {
            let flag =
                self.typed_bool("define", 3, "case_insensitive", &args[2].slot.value(), args)?;
            if flag {
                self.display(
                    b"Warning",
                    b"define(): Argument #3 ($case_insensitive) is ignored since declaration of case-insensitive constants is no longer supported",
                );
            }
        }
        let value = args[1].slot.value();
        if is_keyword(&name) || self.constants.borrow().contains_key(name.as_slice()) {
            let mut message = b"Constant ".to_vec();
            message.extend_from_slice(&name);
            message.extend_from_slice(b" already defined");
            self.display(b"Warning", &message);
            return self.write(result, Value::Bool(false));
        }
        self.constants.borrow_mut().insert(name.into(), value);
        self.write(result, Value::Bool(true))
    }

    pub(crate) fn native_defined(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() != 1 {
            return if args.is_empty() {
                Err(self.too_few("defined", 0, 1, args))
            } else {
                Err(self.too_many("defined", args.len(), 1, args))
            };
        }
        let name = self.typed_string("defined", 1, "constant_name", &args[0].slot.value(), args)?;
        let found = self.has_constant(&name)?;
        self.write(result, Value::Bool(found))
    }

    pub(crate) fn native_constant(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() != 1 {
            return if args.is_empty() {
                Err(self.too_few("constant", 0, 1, args))
            } else {
                Err(self.too_many("constant", args.len(), 1, args))
            };
        }
        let name = self.typed_string("constant", 1, "name", &args[0].slot.value(), args)?;
        let value = self.require_constant(&name, true)?;
        self.write(result, value)
    }

    fn has_constant(&mut self, name: &[u8]) -> Exec<bool> {
        let name = lookup_name(name);
        if is_keyword(name) {
            return Ok(true);
        }
        if let Some((class, member)) = class_member(name) {
            return self.has_class_constant(class, member);
        }
        Ok(self.constants.borrow().contains_key(name))
    }

    fn require_constant(&mut self, name: &[u8], from_function: bool) -> Exec<Value> {
        let name = lookup_name(name);
        if let Some(value) = keyword_value(name) {
            return Ok(value);
        }
        if let Some((class, member)) = class_member(name) {
            return self.require_class_constant(class, member);
        }
        if let Some(value) = self.constants.borrow().get(name).cloned() {
            return Ok(value);
        }
        Err(self.undefined_constant(name, from_function, true))
    }

    fn has_class_constant(&mut self, class: &[u8], member: &[u8]) -> Exec<bool> {
        self.autoload(class)?;
        Ok(self.enum_case_defined(class, member))
    }

    fn require_class_constant(&mut self, class: &[u8], member: &[u8]) -> Exec<Value> {
        self.autoload(class)?;
        if let Some(value) = self.enum_case(class, member)? {
            return Ok(value);
        }
        let mut named = class.to_vec();
        named.extend_from_slice(b"::");
        named.extend_from_slice(member);
        let key = class.to_ascii_lowercase();
        if self.classes.contains_key(key.as_slice()) {
            return Err(self.undefined_constant(&named, true, false));
        }
        let mut message = b"Class \"".to_vec();
        message.extend_from_slice(class);
        message.extend_from_slice(b"\" not found");
        Err(self.throw_from(
            "Error",
            message,
            b"constant",
            vec![Value::String(PhpString::new(&named))],
        ))
    }

    fn enum_case_defined(&self, class: &[u8], member: &[u8]) -> bool {
        let key = class.to_ascii_lowercase();
        self.classes.get(key.as_slice()).is_some_and(|class| {
            class.kind == php_compiler::bytecode::ClassKind::Enum
                && class.case_names.iter().any(|name| name.as_ref() == member)
        })
    }

    fn define_class_constant(&self, args: &[Argument]) -> Stop {
        let values: Vec<Value> = args.iter().map(|arg| arg.slot.value()).collect();
        self.throw_from(
            "ValueError",
            b"define(): Argument #1 ($constant_name) cannot be a class constant".to_vec(),
            b"define",
            values,
        )
    }

    fn undefined_constant(&self, name: &[u8], from_function: bool, quoted: bool) -> Stop {
        let mut message = b"Undefined constant ".to_vec();
        if quoted {
            message.push(b'"');
        }
        message.extend_from_slice(name);
        if quoted {
            message.push(b'"');
        }
        if from_function {
            self.throw_from(
                "Error",
                message,
                b"constant",
                vec![Value::String(PhpString::new(name))],
            )
        } else {
            self.throw("Error", message)
        }
    }

    fn typed_bool(
        &mut self,
        name: &str,
        index: usize,
        param: &str,
        value: &Value,
        args: &[Argument],
    ) -> Exec<bool> {
        match value {
            Value::Bool(flag) => Ok(*flag),
            Value::Int(number) => Ok(*number != 0),
            Value::Null => {
                self.null_deprecated(name, index, param, b"bool");
                Ok(false)
            }
            Value::Array(_) | Value::Object(_) | Value::Float(_) | Value::String(_) => {
                Err(self.type_error_arg(name, index, param, b"bool", value, args))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{class_member, is_keyword, keyword_value, lookup_name};
    use php_value::Value;

    #[test]
    fn lookup_drops_one_leading_backslash() {
        assert_eq!(lookup_name(b"FOO"), b"FOO");
        assert_eq!(lookup_name(br"\FOO"), b"FOO");
        assert_eq!(lookup_name(br"A\B"), br"A\B");
    }

    #[test]
    fn keywords_are_true_false_and_null() {
        assert!(matches!(keyword_value(b"TRUE"), Some(Value::Bool(true))));
        assert!(matches!(keyword_value(b"False"), Some(Value::Bool(false))));
        assert!(matches!(keyword_value(b"null"), Some(Value::Null)));
        assert!(is_keyword(b"true"));
        assert!(!is_keyword(b"FOO"));
    }

    #[test]
    fn class_members_split_on_double_colon() {
        assert_eq!(
            class_member(b"A::B"),
            Some((b"A".as_slice(), b"B".as_slice()))
        );
        assert_eq!(class_member(b"FOO"), None);
        assert_eq!(
            class_member(b"A::"),
            Some((b"A".as_slice(), b"".as_slice()))
        );
    }
}
