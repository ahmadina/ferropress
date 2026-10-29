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
    SplAutoloadRegister,
    Abs,
    Min,
    Max,
    Floor,
    Ceil,
    Round,
    Intval,
    Floatval,
    Strval,
    Intdiv,
    Sqrt,
    Implode,
    Explode,
    JsonEncode,
    JsonDecode,
    ArrayKeyExists,
    InArray,
    ArrayMerge,
    ArrayKeys,
    ArrayValues,
    Substr,
    Strpos,
    StrContains,
    Trim,
    Ltrim,
    Rtrim,
    MbStrlen,
    MbSubstr,
    Gmdate,
    Date,
    Strtotime,
    Time,
    FileExists,
    FileGetContents,
    FilePutContents,
    IsFile,
    IsDir,
    Dirname,
    Basename,
    Serialize,
    Unserialize,
    StrReplace,
    HtmlSpecialChars,
    Sprintf,
    PregMatch,
    PregReplace,
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
        b"spl_autoload_register" => Native::SplAutoloadRegister,
        b"abs" => Native::Abs,
        b"min" => Native::Min,
        b"max" => Native::Max,
        b"floor" => Native::Floor,
        b"ceil" => Native::Ceil,
        b"round" => Native::Round,
        b"intval" => Native::Intval,
        b"floatval" | b"doubleval" => Native::Floatval,
        b"strval" => Native::Strval,
        b"intdiv" => Native::Intdiv,
        b"sqrt" => Native::Sqrt,
        b"implode" | b"join" => Native::Implode,
        b"explode" => Native::Explode,
        b"json_encode" => Native::JsonEncode,
        b"json_decode" => Native::JsonDecode,
        b"array_key_exists" | b"key_exists" => Native::ArrayKeyExists,
        b"in_array" => Native::InArray,
        b"array_merge" => Native::ArrayMerge,
        b"array_keys" => Native::ArrayKeys,
        b"array_values" => Native::ArrayValues,
        b"substr" => Native::Substr,
        b"strpos" => Native::Strpos,
        b"str_contains" => Native::StrContains,
        b"trim" => Native::Trim,
        b"ltrim" => Native::Ltrim,
        b"rtrim" => Native::Rtrim,
        b"mb_strlen" => Native::MbStrlen,
        b"mb_substr" => Native::MbSubstr,
        b"gmdate" => Native::Gmdate,
        b"date" => Native::Date,
        b"strtotime" => Native::Strtotime,
        b"time" => Native::Time,
        b"file_exists" => Native::FileExists,
        b"file_get_contents" => Native::FileGetContents,
        b"file_put_contents" => Native::FilePutContents,
        b"is_file" => Native::IsFile,
        b"is_dir" => Native::IsDir,
        b"dirname" => Native::Dirname,
        b"basename" => Native::Basename,
        b"serialize" => Native::Serialize,
        b"unserialize" => Native::Unserialize,
        b"str_replace" => Native::StrReplace,
        b"htmlspecialchars" => Native::HtmlSpecialChars,
        b"sprintf" => Native::Sprintf,
        b"preg_match" => Native::PregMatch,
        b"preg_replace" => Native::PregReplace,
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
            Native::SplAutoloadRegister => self.native_spl_autoload_register(args, result),
            Native::Abs => self.native_abs(args, result),
            Native::Min => self.native_minmax(args, result, true),
            Native::Max => self.native_minmax(args, result, false),
            Native::Floor => self.native_floor_ceil(args, result, true),
            Native::Ceil => self.native_floor_ceil(args, result, false),
            Native::Round => self.native_round(args, result),
            Native::Intval => self.native_intval(args, result),
            Native::Floatval => self.native_floatval(args, result),
            Native::Strval => self.native_strval(args, result),
            Native::Intdiv => self.native_intdiv(args, result),
            Native::Sqrt => self.native_sqrt(args, result),
            Native::Implode => self.native_implode(args, result),
            Native::Explode => self.native_explode(args, result),
            Native::JsonEncode => self.native_json_encode(args, result),
            Native::JsonDecode => self.native_json_decode(args, result),
            Native::ArrayKeyExists => self.native_array_key_exists(args, result),
            Native::InArray => self.native_in_array(args, result),
            Native::ArrayMerge => self.native_array_merge(args, result),
            Native::ArrayKeys => self.native_array_keys(args, result),
            Native::ArrayValues => self.native_array_values(args, result),
            Native::Substr => self.native_substr(args, result),
            Native::Strpos => self.native_strpos(args, result),
            Native::StrContains => self.native_str_contains(args, result),
            Native::Trim => self.native_trim(args, result, crate::strings::Trim::Both),
            Native::Ltrim => self.native_trim(args, result, crate::strings::Trim::Left),
            Native::Rtrim => self.native_trim(args, result, crate::strings::Trim::Right),
            Native::MbStrlen => self.native_mb_strlen(args, result),
            Native::MbSubstr => self.native_mb_substr(args, result),
            Native::Gmdate => self.native_gmdate(args, result),
            Native::Date => self.native_date(args, result),
            Native::Strtotime => self.native_strtotime(args, result),
            Native::Time => self.native_time(args, result),
            Native::FileExists => self.native_file_exists(args, result),
            Native::FileGetContents => self.native_file_get_contents(args, result),
            Native::FilePutContents => self.native_file_put_contents(args, result),
            Native::IsFile => self.native_is_file(args, result),
            Native::IsDir => self.native_is_dir(args, result),
            Native::Dirname => self.native_dirname(args, result),
            Native::Basename => self.native_basename(args, result),
            Native::Serialize => self.native_serialize(args, result),
            Native::Unserialize => self.native_unserialize(args, result),
            Native::StrReplace => self.native_str_replace(args, result),
            Native::HtmlSpecialChars => self.native_htmlspecialchars(args, result),
            Native::Sprintf => self.native_sprintf(args, result),
            Native::PregMatch => self.native_preg_match(args, result),
            Native::PregReplace => self.native_preg_replace(args, result),
        }
    }
}

impl<O: Output> Machine<'_, '_, O> {
    pub(crate) fn native_one(&mut self, name: &'static str, args: &[Argument]) -> Exec<Value> {
        let Some(arg) = args.first() else {
            return Err(self.too_few(name, 0, 1, args));
        };
        Ok(arg.slot.value())
    }

    pub(crate) fn too_few(
        &self,
        name: &str,
        passed: usize,
        expected: usize,
        args: &[Argument],
    ) -> crate::machine::Stop {
        self.count_error(name, passed, expected, b"() expects exactly ", args)
    }

    pub(crate) fn too_few_at_least(
        &self,
        name: &str,
        passed: usize,
        expected: usize,
        args: &[Argument],
    ) -> crate::machine::Stop {
        self.count_error(name, passed, expected, b"() expects at least ", args)
    }

    pub(crate) fn too_many(
        &self,
        name: &str,
        passed: usize,
        expected: usize,
        args: &[Argument],
    ) -> crate::machine::Stop {
        self.count_error(name, passed, expected, b"() expects exactly ", args)
    }

    pub(crate) fn too_many_at_most(
        &self,
        name: &str,
        passed: usize,
        expected: usize,
        args: &[Argument],
    ) -> crate::machine::Stop {
        self.count_error(name, passed, expected, b"() expects at most ", args)
    }

    fn count_error(
        &self,
        name: &str,
        passed: usize,
        expected: usize,
        phrase: &[u8],
        args: &[Argument],
    ) -> crate::machine::Stop {
        let mut message = name.as_bytes().to_vec();
        message.extend_from_slice(phrase);
        message.extend_from_slice(expected.to_string().as_bytes());
        message.extend_from_slice(if expected == 1 {
            b" argument, "
        } else {
            b" arguments, "
        });
        message.extend_from_slice(passed.to_string().as_bytes());
        message.extend_from_slice(b" given");
        let values: Vec<Value> = args.iter().map(|arg| arg.slot.value()).collect();
        self.throw_from("ArgumentCountError", message, name.as_bytes(), values)
    }

    pub(crate) fn typed_string(
        &mut self,
        name: &str,
        index: usize,
        param: &str,
        value: &Value,
        args: &[Argument],
    ) -> Exec<Vec<u8>> {
        match value {
            Value::Array(_) | Value::Object(_) => {
                Err(self.type_error_arg(name, index, param, b"string", value, args))
            }
            Value::Null => {
                self.null_deprecated(name, index, param, b"string");
                Ok(Vec::new())
            }
            other => Ok(self.stringify(other)?.as_bytes().to_vec()),
        }
    }

    pub(crate) fn typed_int(
        &mut self,
        name: &str,
        index: usize,
        param: &str,
        value: &Value,
        args: &[Argument],
    ) -> Exec<i64> {
        match value {
            Value::Int(number) => Ok(*number),
            Value::Bool(flag) => Ok(i64::from(*flag)),
            Value::Float(_) => self.compute(|cx| value.to_int(cx)),
            Value::Null => {
                self.null_deprecated(name, index, param, b"int");
                Ok(0)
            }
            Value::String(text) if php_value::ops::whole_numeric(text.as_bytes()).is_some() => {
                self.compute(|cx| value.to_int(cx))
            }
            other => Err(self.type_error_arg(name, index, param, b"int", other, args)),
        }
    }

    fn null_deprecated(&mut self, name: &str, index: usize, param: &str, ty: &[u8]) {
        let mut message = name.as_bytes().to_vec();
        message.extend_from_slice(b"(): Passing null to parameter #");
        message.extend_from_slice(index.to_string().as_bytes());
        message.extend_from_slice(b" ($");
        message.extend_from_slice(param.as_bytes());
        message.extend_from_slice(b") of type ");
        message.extend_from_slice(ty);
        message.extend_from_slice(b" is deprecated");
        self.display(b"Deprecated", &message);
    }

    fn type_error_arg(
        &self,
        name: &str,
        index: usize,
        param: &str,
        expected: &[u8],
        value: &Value,
        args: &[Argument],
    ) -> crate::machine::Stop {
        let mut message = name.as_bytes().to_vec();
        message.extend_from_slice(b"(): Argument #");
        message.extend_from_slice(index.to_string().as_bytes());
        message.extend_from_slice(b" ($");
        message.extend_from_slice(param.as_bytes());
        message.extend_from_slice(b") must be of type ");
        message.extend_from_slice(expected);
        message.extend_from_slice(b", ");
        message.extend_from_slice(value.type_name().as_bytes());
        message.extend_from_slice(b" given");
        let values: Vec<Value> = args.iter().map(|arg| arg.slot.value()).collect();
        self.throw_from("TypeError", message, name.as_bytes(), values)
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
        let autoload = match args.get(1) {
            Some(arg) => self.compute(|cx| arg.slot.value().to_bool(cx))?,
            None => true,
        };
        if autoload {
            self.autoload(&name)?;
        }
        let key = name.to_ascii_lowercase();
        self.write(
            result,
            Value::Bool(self.classes.contains_key(key.as_slice())),
        )
    }

    pub(crate) fn string_arg(&mut self, name: &str, value: &Value) -> Exec<Vec<u8>> {
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
