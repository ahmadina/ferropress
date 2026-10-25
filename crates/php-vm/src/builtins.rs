//! Built-in functions the VM binds by name.

use php_compiler::bytecode::Operand;
use php_value::{PhpString, Value};

use crate::Output;
use crate::calls::Argument;
use crate::machine::{Exec, Machine};

/// A built-in the call machinery can dispatch.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Native {
    Strlen,
    Strtolower,
    Strtoupper,
    Count,
    Gettype,
    IsNull,
    IsBool,
    IsInt,
    IsFloat,
    IsString,
    IsArray,
    IsObject,
    IsNumeric,
    FunctionExists,
    ClassExists,
}

/// The built-in named by `key`, compared lowercase.
pub(crate) fn lookup(key: &[u8]) -> Option<Native> {
    Some(match key {
        b"strlen" => Native::Strlen,
        b"strtolower" => Native::Strtolower,
        b"strtoupper" => Native::Strtoupper,
        b"count" => Native::Count,
        b"gettype" => Native::Gettype,
        b"is_null" => Native::IsNull,
        b"is_bool" => Native::IsBool,
        b"is_int" | b"is_integer" => Native::IsInt,
        b"is_float" | b"is_double" | b"is_real" => Native::IsFloat,
        b"is_string" => Native::IsString,
        b"is_array" => Native::IsArray,
        b"is_object" => Native::IsObject,
        b"is_numeric" => Native::IsNumeric,
        b"function_exists" => Native::FunctionExists,
        b"class_exists" => Native::ClassExists,
        _ => return None,
    })
}

/// Whether `key` names a bound built-in, compared lowercase.
pub(crate) fn is_bound(key: &[u8]) -> bool {
    lookup(key).is_some()
}

impl<O: Output> Machine<'_, '_, O> {
    /// Runs `native` with the arguments already sent.
    pub(crate) fn dispatch_native(
        &mut self,
        native: Native,
        args: &[Argument],
        result: Operand,
    ) -> Exec {
        match native {
            Native::Strlen => self.native_strlen(args, result),
            Native::Strtolower => self.native_case(args, result, false),
            Native::Strtoupper => self.native_case(args, result, true),
            Native::Count => self.native_count(args, result),
            Native::Gettype => self.native_gettype(args, result),
            Native::IsNull => self.native_is("is_null", args, result, |value| {
                matches!(value, Value::Null)
            }),
            Native::IsBool => self.native_is("is_bool", args, result, |value| {
                matches!(value, Value::Bool(_))
            }),
            Native::IsInt => self.native_is("is_int", args, result, |value| {
                matches!(value, Value::Int(_))
            }),
            Native::IsFloat => self.native_is("is_float", args, result, |value| {
                matches!(value, Value::Float(_))
            }),
            Native::IsString => self.native_is("is_string", args, result, |value| {
                matches!(value, Value::String(_))
            }),
            Native::IsArray => self.native_is("is_array", args, result, |value| {
                matches!(value, Value::Array(_))
            }),
            Native::IsObject => self.native_is("is_object", args, result, |value| {
                matches!(value, Value::Object(_))
            }),
            Native::IsNumeric => self.native_is("is_numeric", args, result, is_numeric),
            Native::FunctionExists => self.native_function_exists(args, result),
            Native::ClassExists => self.native_class_exists(args, result),
        }
    }
}

impl<O: Output> Machine<'_, '_, O> {
    fn native_one(&mut self, name: &'static str, args: &[Argument]) -> Exec<Value> {
        let Some(arg) = args.first() else {
            return Err(self.too_few(name, 0, 1));
        };
        Ok(arg.slot.value())
    }

    fn too_few(&self, name: &str, passed: usize, expected: usize) -> crate::machine::Stop {
        let mut message = name.as_bytes().to_vec();
        message.extend_from_slice(b"() expects exactly ");
        message.extend_from_slice(expected.to_string().as_bytes());
        message.extend_from_slice(if expected == 1 {
            b" argument, "
        } else {
            b" arguments, "
        });
        message.extend_from_slice(passed.to_string().as_bytes());
        message.extend_from_slice(b" given");
        self.throw_from("ArgumentCountError", message, name.as_bytes(), Vec::new())
    }

    fn native_strlen(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("strlen", args)?;
        let text = self.string_arg("strlen", &value)?;
        let len = i64::try_from(text.len()).unwrap_or(i64::MAX);
        self.write(result, Value::Int(len))
    }

    fn native_case(&mut self, args: &[Argument], result: Operand, upper: bool) -> Exec {
        let name = if upper { "strtoupper" } else { "strtolower" };
        let value = self.native_one(name, args)?;
        let text = self.string_arg(name, &value)?;
        let mapped: Vec<u8> = if upper {
            text.iter().map(u8::to_ascii_uppercase).collect()
        } else {
            text.iter().map(u8::to_ascii_lowercase).collect()
        };
        self.write(result, Value::String(PhpString::new(&mapped)))
    }

    fn native_count(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("count", args)?;
        match &value {
            Value::Array(array) => {
                let n = i64::try_from(array.len()).unwrap_or(i64::MAX);
                self.write(result, Value::Int(n))
            }
            other => {
                let mut message =
                    b"count(): Argument #1 ($value) must be of type Countable|array, ".to_vec();
                message.extend_from_slice(other.type_name().as_bytes());
                message.extend_from_slice(b" given");
                Err(self.throw("TypeError", message))
            }
        }
    }

    fn native_gettype(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("gettype", args)?;
        let name = match &value {
            Value::Null => "NULL",
            Value::Bool(_) => "boolean",
            Value::Int(_) => "integer",
            Value::Float(_) => "double",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        self.write(result, Value::String(PhpString::new(name.as_bytes())))
    }

    fn native_is(
        &mut self,
        name: &'static str,
        args: &[Argument],
        result: Operand,
        test: fn(&Value) -> bool,
    ) -> Exec {
        let value = self.native_one(name, args)?;
        self.write(result, Value::Bool(test(&value)))
    }

    fn native_function_exists(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("function_exists", args)?;
        let name = self.string_arg("function_exists", &value)?;
        let key = name.to_ascii_lowercase();
        let found = self.functions.contains_key(key.as_slice()) || is_bound(&key);
        self.write(result, Value::Bool(found))
    }

    fn native_class_exists(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("class_exists", args)?;
        let name = self.string_arg("class_exists", &value)?;
        let key = name.to_ascii_lowercase();
        self.write(
            result,
            Value::Bool(self.classes.contains_key(key.as_slice())),
        )
    }

    fn string_arg(&mut self, name: &str, value: &Value) -> Exec<Vec<u8>> {
        match value {
            Value::Array(_) | Value::Object(_) => {
                let mut message = name.as_bytes().to_vec();
                message.extend_from_slice(b"(): Argument #1 ($string) must be of type string, ");
                message.extend_from_slice(value.type_name().as_bytes());
                message.extend_from_slice(b" given");
                Err(self.throw("TypeError", message))
            }
            other => Ok(self.stringify(other)?.as_bytes().to_vec()),
        }
    }
}

fn is_numeric(value: &Value) -> bool {
    match value {
        Value::Int(_) | Value::Float(_) => true,
        Value::String(text) => {
            php_value::ops::numeric_prefix(text.as_bytes()).is_some_and(|prefix| !prefix.trailing)
        }
        _ => false,
    }
}
