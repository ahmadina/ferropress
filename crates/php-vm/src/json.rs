//! `json_encode` and `json_decode` for scalars and arrays.

use php_compiler::bytecode::Operand;
use php_value::{ArrayKey, PhpArray, PhpString, Value};

use crate::Output;
use crate::calls::Argument;
use crate::machine::{Exec, Machine};

impl<O: Output> Machine<'_, '_, O> {
    pub(crate) fn native_json_encode(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("json_encode", args)?;
        let mut out = Vec::new();
        encode(&value, &mut out);
        self.write(result, Value::String(PhpString::new(&out)))
    }

    pub(crate) fn native_json_decode(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("json_decode", args)?;
        let text = self.stringify(&value)?;
        let assoc = match args.get(1) {
            Some(arg) => self.compute(|cx| arg.slot.value().to_bool(cx))?,
            None => false,
        };
        let parsed = match decode(text.as_bytes(), assoc) {
            Some(value) => value,
            None => Value::Null,
        };
        self.write(result, parsed)
    }
}

fn encode(value: &Value, out: &mut Vec<u8>) {
    match value {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Int(n) => out.extend_from_slice(n.to_string().as_bytes()),
        Value::Float(n) => {
            if n.is_finite() {
                let text = format!("{n}");
                out.extend_from_slice(text.as_bytes());
                if !text.contains('.') && !text.contains('e') && !text.contains('E') {
                    out.extend_from_slice(b".0");
                }
            } else {
                out.extend_from_slice(b"0");
            }
        }
        Value::String(text) => encode_string(text.as_bytes(), out),
        Value::Array(array) => encode_array(array, out),
        Value::Object(object) => {
            out.push(b'{');
            let mut first = true;
            object.with_properties(|properties| {
                for (name, slot) in properties.iter() {
                    if name.as_bytes().first() == Some(&0) {
                        continue;
                    }
                    if !first {
                        out.push(b',');
                    }
                    first = false;
                    encode_string(name.as_bytes(), out);
                    out.push(b':');
                    encode(&slot.value(), out);
                }
            });
            out.push(b'}');
        }
    }
}

fn encode_array(array: &PhpArray, out: &mut Vec<u8>) {
    if is_list(array) {
        out.push(b'[');
        for (index, (_key, slot)) in array.iter().enumerate() {
            if index > 0 {
                out.push(b',');
            }
            encode(&slot.value(), out);
        }
        out.push(b']');
        return;
    }
    out.push(b'{');
    for (index, (key, slot)) in array.iter().enumerate() {
        if index > 0 {
            out.push(b',');
        }
        match key {
            ArrayKey::Int(n) => encode_string(n.to_string().as_bytes(), out),
            ArrayKey::String(text) => encode_string(text.as_bytes(), out),
        }
        out.push(b':');
        encode(&slot.value(), out);
    }
    out.push(b'}');
}

fn is_list(array: &PhpArray) -> bool {
    array.iter().enumerate().all(|(index, (key, _))| match key {
        ArrayKey::Int(n) => usize::try_from(*n).is_ok_and(|n| n == index),
        ArrayKey::String(_) => false,
    })
}

fn encode_string(bytes: &[u8], out: &mut Vec<u8>) {
    out.push(b'"');
    for &byte in bytes {
        match byte {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x00..=0x1f => {
                out.extend_from_slice(b"\\u00");
                out.push(hex(byte >> 4));
                out.push(hex(byte & 0x0f));
            }
            other => out.push(other),
        }
    }
    out.push(b'"');
}

const fn hex(nibble: u8) -> u8 {
    if nibble < 10 {
        b'0' + nibble
    } else {
        b'a' + nibble - 10
    }
}

fn decode(bytes: &[u8], assoc: bool) -> Option<Value> {
    let mut parser = Parser {
        bytes,
        pos: skip_ws(bytes, 0),
        assoc,
    };
    let value = parser.value()?;
    parser.pos = skip_ws(parser.bytes, parser.pos);
    if parser.pos != parser.bytes.len() {
        return None;
    }
    Some(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    assoc: bool,
}

impl Parser<'_> {
    fn value(&mut self) -> Option<Value> {
        self.pos = skip_ws(self.bytes, self.pos);
        match self.bytes.get(self.pos)? {
            b'n' => self.exact(b"null", Value::Null),
            b't' => self.exact(b"true", Value::Bool(true)),
            b'f' => self.exact(b"false", Value::Bool(false)),
            b'"' => self
                .string()
                .map(|text| Value::String(PhpString::new(&text))),
            b'[' => self.list(),
            b'{' => self.object(),
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn exact(&mut self, token: &[u8], value: Value) -> Option<Value> {
        self.bytes.get(self.pos..)?.starts_with(token).then(|| {
            self.pos += token.len();
            value
        })
    }

    fn list(&mut self) -> Option<Value> {
        self.pos += 1;
        let mut array = PhpArray::new();
        self.pos = skip_ws(self.bytes, self.pos);
        if self.bytes.get(self.pos) == Some(&b']') {
            self.pos += 1;
            return Some(Value::Array(array));
        }
        loop {
            let item = self.value()?;
            array.append(item).ok()?;
            self.pos = skip_ws(self.bytes, self.pos);
            match self.bytes.get(self.pos)? {
                b']' => {
                    self.pos += 1;
                    return Some(Value::Array(array));
                }
                b',' => {
                    self.pos += 1;
                    self.pos = skip_ws(self.bytes, self.pos);
                }
                _ => return None,
            }
        }
    }

    fn object(&mut self) -> Option<Value> {
        self.pos += 1;
        let mut array = PhpArray::new();
        self.pos = skip_ws(self.bytes, self.pos);
        if self.bytes.get(self.pos) == Some(&b'}') {
            self.pos += 1;
            return Some(self.finish_object(array));
        }
        loop {
            self.pos = skip_ws(self.bytes, self.pos);
            let key = self.string()?;
            self.pos = skip_ws(self.bytes, self.pos);
            if self.bytes.get(self.pos)? != &b':' {
                return None;
            }
            self.pos += 1;
            let value = self.value()?;
            array.assign(ArrayKey::from_string(PhpString::new(&key)), value);
            self.pos = skip_ws(self.bytes, self.pos);
            match self.bytes.get(self.pos)? {
                b'}' => {
                    self.pos += 1;
                    return Some(self.finish_object(array));
                }
                b',' => {
                    self.pos += 1;
                }
                _ => return None,
            }
        }
    }

    fn finish_object(&self, array: PhpArray) -> Value {
        let _ = self.assoc;
        Value::Array(array)
    }

    fn string(&mut self) -> Option<Vec<u8>> {
        if self.bytes.get(self.pos)? != &b'"' {
            return None;
        }
        self.pos += 1;
        let mut out = Vec::new();
        while let Some(&byte) = self.bytes.get(self.pos) {
            self.pos += 1;
            match byte {
                b'"' => return Some(out),
                b'\\' => {
                    let esc = self.bytes.get(self.pos)?;
                    self.pos += 1;
                    match esc {
                        b'"' | b'\\' | b'/' => out.push(*esc),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0c),
                        b'u' => {
                            let hex = self.bytes.get(self.pos..self.pos + 4)?;
                            let code =
                                u32::from_str_radix(core::str::from_utf8(hex).ok()?, 16).ok()?;
                            self.pos += 4;
                            let ch = char::from_u32(code)?;
                            let mut buf = [0; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return None,
                    }
                }
                other => out.push(other),
            }
        }
        None
    }

    fn number(&mut self) -> Option<Value> {
        let start = self.pos;
        if self.bytes.get(self.pos) == Some(&b'-') {
            self.pos += 1;
        }
        if self.bytes.get(self.pos) == Some(&b'0') {
            self.pos += 1;
        } else if matches!(self.bytes.get(self.pos), Some(b'1'..=b'9')) {
            while matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        } else {
            return None;
        }
        let mut is_float = false;
        if self.bytes.get(self.pos) == Some(&b'.') {
            is_float = true;
            self.pos += 1;
            if !matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                return None;
            }
            while matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.bytes.get(self.pos), Some(b'e' | b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.bytes.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                return None;
            }
            while matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        let text = core::str::from_utf8(&self.bytes[start..self.pos]).ok()?;
        if is_float {
            text.parse().ok().map(Value::Float)
        } else {
            text.parse().ok().map(Value::Int)
        }
    }
}

fn skip_ws(bytes: &[u8], mut pos: usize) -> usize {
    while matches!(bytes.get(pos), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        pos += 1;
    }
    pos
}

#[cfg(test)]
mod tests {
    use super::{decode, encode, hex};
    use php_value::{ArrayKey, PhpArray, PhpString, Value};

    fn encoded(value: &Value) -> Vec<u8> {
        let mut out = Vec::new();
        encode(value, &mut out);
        out
    }

    #[test]
    fn encode_escapes_and_control_bytes() {
        assert_eq!(
            encoded(&Value::String(PhpString::new(b"a\"\\\n\r\t\x01"))),
            br#""a\"\\\n\r\t\u0001""#
        );
        assert_eq!(hex(10), b'a');
    }

    #[test]
    fn encode_lists_and_maps() {
        let mut list = PhpArray::new();
        list.append(Value::Int(1)).expect("append");
        list.append(Value::Int(2)).expect("append");
        assert_eq!(encoded(&Value::Array(list)), b"[1,2]");

        let mut map = PhpArray::new();
        map.assign(ArrayKey::from_string(PhpString::new(b"a")), Value::Int(1));
        assert_eq!(encoded(&Value::Array(map)), br#"{"a":1}"#);
    }

    #[test]
    fn decode_reads_escapes_and_rejects_junk() {
        let Some(Value::String(text)) = decode(br#""a\"\n\u0041""#, false) else {
            panic!("expected a string");
        };
        assert_eq!(text.as_bytes(), b"a\"\nA");
        assert!(decode(b"[1]x", false).is_none());
        assert!(decode(b"  true  ", false).is_some());
        assert!(matches!(decode(b"false", false), Some(Value::Bool(false))));
    }
}
