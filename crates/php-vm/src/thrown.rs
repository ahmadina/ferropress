//! Thrown errors, their stack traces and how an uncaught one is displayed.

use php_value::{Precision, Value, write_float_with};

/// How many bytes of a string argument a stack trace shows before cutting it with `...`.
const STRING_ARGUMENT_LIMIT: usize = 15;

/// One call on the stack when an error was thrown.
#[derive(Debug, Clone)]
pub(crate) struct TraceEntry {
    /// The file the call was made from.
    pub(crate) file: Box<[u8]>,
    /// The line of the call.
    pub(crate) line: u32,
    /// The function called, as declared.
    pub(crate) function: Box<[u8]>,
    /// The arguments as the trace shows them.
    pub(crate) args: Vec<Value>,
}

/// A thrown error, as an object of its class plus the location it was raised.
#[derive(Debug, Clone)]
pub(crate) struct Thrown {
    pub(crate) class: Box<[u8]>,
    pub(crate) message: Vec<u8>,
    pub(crate) object: php_value::Object,
    pub(crate) file: Box<[u8]>,
    pub(crate) line: u32,
    /// The calls active when it was thrown, innermost first.
    pub(crate) trace: Vec<TraceEntry>,
}

impl Thrown {
    /// What the reference displays when this error is not caught.
    pub(crate) fn uncaught(&self, precision: Precision) -> Vec<u8> {
        let mut out = b"\nFatal error: Uncaught ".to_vec();
        out.extend_from_slice(&self.class);
        out.extend_from_slice(b": ");
        out.extend_from_slice(&self.message);
        out.extend_from_slice(b" in ");
        out.extend_from_slice(&self.file);
        out.push(b':');
        out.extend_from_slice(self.line.to_string().as_bytes());
        out.extend_from_slice(b"\nStack trace:\n");
        for (index, entry) in self.trace.iter().enumerate() {
            out.push(b'#');
            out.extend_from_slice(index.to_string().as_bytes());
            out.push(b' ');
            out.extend_from_slice(&entry.file);
            out.push(b'(');
            out.extend_from_slice(entry.line.to_string().as_bytes());
            out.extend_from_slice(b"): ");
            out.extend_from_slice(&entry.function);
            out.push(b'(');
            for (position, arg) in entry.args.iter().enumerate() {
                if position > 0 {
                    out.extend_from_slice(b", ");
                }
                write_argument(&mut out, arg, precision);
            }
            out.extend_from_slice(b")\n");
        }
        out.push(b'#');
        out.extend_from_slice(self.trace.len().to_string().as_bytes());
        out.extend_from_slice(b" {main}\n  thrown in ");
        out.extend_from_slice(&self.file);
        out.extend_from_slice(b" on line ");
        out.extend_from_slice(self.line.to_string().as_bytes());
        out.push(b'\n');
        out
    }
}

/// Writes `value` as a stack trace shows an argument.
fn write_argument(out: &mut Vec<u8>, value: &Value, precision: Precision) {
    match value {
        Value::Null => out.extend_from_slice(b"NULL"),
        Value::Bool(value) => out.extend_from_slice(if *value { b"true" } else { b"false" }),
        Value::Int(value) => out.extend_from_slice(value.to_string().as_bytes()),
        Value::Float(value) => {
            let start = out.len();
            write_float_with(out, *value, precision);
            if value.is_finite() && !out[start..].contains(&b'.') {
                out.extend_from_slice(b".0");
            }
        }
        Value::String(text) => {
            let bytes = text.as_bytes();
            out.push(b'\'');
            write_escaped(out, &bytes[..bytes.len().min(STRING_ARGUMENT_LIMIT)]);
            if bytes.len() > STRING_ARGUMENT_LIMIT {
                out.extend_from_slice(b"...");
            }
            out.push(b'\'');
        }
        Value::Array(_) => out.extend_from_slice(b"Array"),
        Value::Object(object) => {
            out.extend_from_slice(b"Object(");
            out.extend_from_slice(object.class_name().as_bytes());
            out.push(b')');
        }
    }
}

/// Writes `bytes` with control bytes, backslashes and bytes above `~` escaped.
fn write_escaped(out: &mut Vec<u8>, bytes: &[u8]) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for &byte in bytes {
        let escape = match byte {
            b'\n' => b'n',
            b'\r' => b'r',
            b'\t' => b't',
            0x0c => b'f',
            0x0b => b'v',
            b'\\' => b'\\',
            0x1b => b'e',
            0x20..=0x7e => {
                out.push(byte);
                continue;
            }
            _ => {
                out.extend_from_slice(&[
                    b'\\',
                    b'x',
                    HEX[usize::from(byte >> 4)],
                    HEX[usize::from(byte & 0xf)],
                ]);
                continue;
            }
        };
        out.extend_from_slice(&[b'\\', escape]);
    }
}

#[cfg(test)]
mod tests {
    use php_value::{ObjectStore, PhpArray, Precision, Value};

    use super::{Thrown, TraceEntry, write_argument};

    fn shown(value: &Value) -> String {
        let mut out = Vec::new();
        write_argument(&mut out, value, Precision::from_setting(14));
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn arguments_show_as_the_reference_shows_them() {
        let store = ObjectStore::new();
        let object = store.create("Some\\Class".into()).unwrap();
        let cases = [
            (Value::Null, "NULL"),
            (Value::Bool(true), "true"),
            (Value::Bool(false), "false"),
            (Value::Int(-3), "-3"),
            (Value::Float(1.0), "1.0"),
            (Value::Float(2.5), "2.5"),
            (Value::Float(1e25), "1.0E+25"),
            (Value::Float(f64::INFINITY), "INF"),
            (Value::from("short"), "'short'"),
            (
                Value::from("changed and long string"),
                "'changed and lon...'",
            ),
            (
                Value::from("\n\r\t\x0c\x0b\\\x1b\x01\x7f"),
                "'\\n\\r\\t\\f\\v\\\\\\e\\x01\\x7F'",
            ),
            (Value::from(PhpArray::new()), "Array"),
            (Value::from(object), "Object(Some\\Class)"),
        ];
        for (value, expected) in cases {
            assert_eq!(shown(&value), expected, "{value:?}");
        }
    }

    #[test]
    fn uncaught_errors_show_message_trace_and_location() {
        let store = ObjectStore::new();
        let thrown = Thrown {
            class: b"DivisionByZeroError".to_vec().into(),
            message: b"Modulo by zero".to_vec(),
            object: store.create("DivisionByZeroError".into()).unwrap(),
            file: b"/t.php".to_vec().into(),
            line: 3,
            trace: vec![TraceEntry {
                file: b"/t.php".to_vec().into(),
                line: 4,
                function: b"f".to_vec().into(),
                args: vec![Value::Int(1), Value::Int(0)],
            }],
        };
        assert_eq!(
            String::from_utf8(thrown.uncaught(Precision::from_setting(14))).unwrap(),
            "\nFatal error: Uncaught DivisionByZeroError: Modulo by zero in /t.php:3\nStack \
             trace:\n#0 /t.php(4): f(1, 0)\n#1 {main}\n  thrown in /t.php on line 3\n"
        );
    }
}
