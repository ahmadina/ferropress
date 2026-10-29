//! `str_replace` and `htmlspecialchars`.

use php_compiler::bytecode::Operand;
use php_value::{PhpArray, PhpString, Value};

use crate::Output;
use crate::VmError;
use crate::calls::Argument;
use crate::machine::{Exec, Machine};

/// PHP 8.1+ default: `ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML401`.
const HTML_DEFAULT_FLAGS: i64 = 11;

impl<O: Output> Machine<'_, '_, O> {
    pub(crate) fn native_str_replace(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() < 3 {
            return Err(self.too_few_at_least("str_replace", args.len(), 3));
        }
        if args.len() > 3 {
            return Err(VmError::Unsupported("str_replace count").into());
        }
        let search = args[0].slot.value();
        let replace = args[1].slot.value();
        let subject = args[2].slot.value();
        if !matches!(search, Value::Array(_)) && matches!(replace, Value::Array(_)) {
            return Err(self.throw_from(
                "TypeError",
                b"str_replace(): Argument #2 ($replace) must be of type string when argument #1 ($search) is a string".to_vec(),
                b"str_replace",
                vec![search, replace, subject],
            ));
        }
        let pairs = self.replace_pairs(&search, &replace)?;
        let out = self.replace_subject(&pairs, subject)?;
        self.write(result, out)
    }

    pub(crate) fn native_htmlspecialchars(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.is_empty() {
            return Err(self.too_few_at_least("htmlspecialchars", 0, 1));
        }
        let value = args[0].slot.value();
        if matches!(value, Value::Null) {
            self.display(
                b"Deprecated",
                b"htmlspecialchars(): Passing null to parameter #1 ($string) of type string is deprecated",
            );
        }
        let text = self.string_arg("htmlspecialchars", &value)?;
        let flags = match args.get(1) {
            Some(arg) => self.compute(|cx| arg.slot.value().to_int(cx))?,
            None => HTML_DEFAULT_FLAGS,
        };
        if let Some(arg) = args.get(2) {
            self.html_charset(&arg.slot.value())?;
        }
        let double_encode = match args.get(3) {
            Some(arg) => self.compute(|cx| arg.slot.value().to_bool(cx))?,
            None => true,
        };
        let encoded = htmlspecialchars(&text, flags, double_encode);
        self.write(result, Value::String(PhpString::new(&encoded)))
    }

    fn replace_pairs(&mut self, search: &Value, replace: &Value) -> Exec<Vec<(Vec<u8>, Vec<u8>)>> {
        let Value::Array(array) = search else {
            let needle = self.stringify(search)?.as_bytes().to_vec();
            let with = self.stringify(replace)?.as_bytes().to_vec();
            return Ok(vec![(needle, with)]);
        };
        let replacements = match replace {
            Value::Array(items) => Some(items),
            _ => None,
        };
        let mut pairs = Vec::new();
        for (index, (_key, slot)) in array.iter().enumerate() {
            let needle = self.stringify(&slot.value())?.as_bytes().to_vec();
            let with = match replacements {
                Some(items) => match items.entry_from(index) {
                    Some((_, _, slot)) => self.stringify(&slot.value())?.as_bytes().to_vec(),
                    None => Vec::new(),
                },
                None => self.stringify(replace)?.as_bytes().to_vec(),
            };
            pairs.push((needle, with));
        }
        Ok(pairs)
    }

    fn replace_subject(&mut self, pairs: &[(Vec<u8>, Vec<u8>)], subject: Value) -> Exec<Value> {
        match subject {
            Value::Array(array) => {
                let mut out = PhpArray::new();
                for (key, slot) in array.iter() {
                    let mut text = self.stringify(&slot.value())?.as_bytes().to_vec();
                    for (needle, with) in pairs {
                        text = replace_all(&text, needle, with);
                    }
                    out.assign(key.clone(), Value::String(PhpString::new(&text)));
                }
                Ok(Value::Array(out))
            }
            Value::Object(object) => {
                let mut message =
                    b"str_replace(): Argument #3 ($subject) must be of type array|string, "
                        .to_vec();
                message.extend_from_slice(object.class_name().as_bytes());
                message.extend_from_slice(b" given");
                Err(self.throw("TypeError", message))
            }
            other => {
                let mut text = self.stringify(&other)?.as_bytes().to_vec();
                for (needle, with) in pairs {
                    text = replace_all(&text, needle, with);
                }
                Ok(Value::String(PhpString::new(&text)))
            }
        }
    }

    fn html_charset(&mut self, value: &Value) -> Exec {
        if matches!(value, Value::Null) {
            return Ok(());
        }
        let name = self.stringify(value)?;
        if name.as_bytes().eq_ignore_ascii_case(b"utf-8")
            || name.as_bytes().eq_ignore_ascii_case(b"utf8")
        {
            return Ok(());
        }
        let mut message = b"htmlspecialchars(): Charset \"".to_vec();
        message.extend_from_slice(name.as_bytes());
        message.extend_from_slice(b"\" is not supported, assuming UTF-8");
        self.display(b"Warning", &message);
        Ok(())
    }
}

fn replace_all(haystack: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    if needle.is_empty() {
        return haystack.to_vec();
    }
    let mut out = Vec::new();
    let mut rest = haystack;
    while let Some(at) = rest
        .windows(needle.len())
        .position(|window| window == needle)
    {
        out.extend_from_slice(&rest[..at]);
        out.extend_from_slice(with);
        rest = &rest[at + needle.len()..];
    }
    out.extend_from_slice(rest);
    out
}

fn htmlspecialchars(text: &[u8], flags: i64, double_encode: bool) -> Vec<u8> {
    let quotes = flags & 2 != 0;
    let apostrophe = flags & 1 != 0;
    let mut out = Vec::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        match text[index] {
            b'<' => out.extend_from_slice(b"&lt;"),
            b'>' => out.extend_from_slice(b"&gt;"),
            b'&' if !double_encode => {
                if let Some(len) = entity_len(&text[index..]) {
                    out.extend_from_slice(&text[index..index + len]);
                    index += len;
                    continue;
                }
                out.extend_from_slice(b"&amp;");
            }
            b'&' => out.extend_from_slice(b"&amp;"),
            b'"' if quotes => out.extend_from_slice(b"&quot;"),
            b'\'' if apostrophe => out.extend_from_slice(b"&#039;"),
            byte => out.push(byte),
        }
        index += 1;
    }
    out
}

fn entity_len(bytes: &[u8]) -> Option<usize> {
    let rest = bytes.get(1..)?;
    if rest.first() == Some(&b'#') {
        let body = rest.get(1..)?;
        let (digits, hex) = if body
            .first()
            .is_some_and(|byte| byte.eq_ignore_ascii_case(&b'x'))
        {
            (body.get(1..)?, true)
        } else {
            (body, false)
        };
        let end = digits.iter().position(|byte| *byte == b';')?;
        let taken = &digits[..end];
        if taken.is_empty() {
            return None;
        }
        let ok = if hex {
            taken.iter().all(u8::is_ascii_hexdigit)
        } else {
            taken.iter().all(u8::is_ascii_digit)
        };
        ok.then_some(2 + usize::from(hex) + end + 1)
    } else {
        let end = rest.iter().position(|byte| *byte == b';')?;
        let name = &rest[..end];
        let first = name.first()?;
        if !first.is_ascii_alphabetic() || !name.iter().all(u8::is_ascii_alphanumeric) {
            return None;
        }
        Some(end + 2)
    }
}

#[cfg(test)]
mod tests {
    use super::{entity_len, htmlspecialchars, replace_all};

    #[test]
    fn replace_all_walks_non_overlapping_matches() {
        assert_eq!(replace_all(b"aAa", b"a", b"b"), b"bAb");
        assert_eq!(replace_all(b"aaa", b"aa", b"x"), b"xa");
        assert_eq!(replace_all(b"ab", b"", b"x"), b"ab");
    }

    #[test]
    fn htmlspecialchars_encodes_default_quotes() {
        assert_eq!(
            htmlspecialchars(br#"<a & "b" 'c'>"#, 11, true),
            b"&lt;a &amp; &quot;b&quot; &#039;c&#039;&gt;"
        );
        assert_eq!(htmlspecialchars(b"&amp;", 11, true), b"&amp;amp;");
        assert_eq!(htmlspecialchars(b"&amp;", 11, false), b"&amp;");
        assert_eq!(htmlspecialchars(br#"a "b""#, 0, true), br#"a "b""#);
    }

    #[test]
    fn entity_len_accepts_named_and_numeric_forms() {
        assert_eq!(entity_len(b"&amp;x"), Some(5));
        assert_eq!(entity_len(b"&#39;"), Some(5));
        assert_eq!(entity_len(b"&#x27;"), Some(6));
        assert_eq!(entity_len(b"&;"), None);
        assert_eq!(entity_len(b"& foo;"), None);
    }
}
