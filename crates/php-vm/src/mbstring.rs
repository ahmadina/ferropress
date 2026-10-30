//! UTF-8 `mb_strlen` and `mb_substr`.

use php_compiler::bytecode::Operand;
use php_value::{PhpString, Value};

use crate::Output;
use crate::calls::Argument;
use crate::machine::{Exec, Machine};
use crate::strings::substr_range;

impl<O: Output> Machine<'_, O> {
    pub(crate) fn native_mb_strlen(&mut self, args: &[Argument], result: Operand) -> Exec {
        let value = self.native_one("mb_strlen", args)?;
        let text = self.string_arg("mb_strlen", &value)?;
        let n = i64::try_from(utf8_chars(&text).len()).unwrap_or(i64::MAX);
        self.write(result, Value::Int(n))
    }

    pub(crate) fn native_mb_substr(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() < 2 {
            return Err(self.too_few("mb_substr", args.len(), 2, args));
        }
        let text = self.string_arg("mb_substr", &args[0].slot.value())?;
        let offset = self.compute(|cx| args[1].slot.value().to_int(cx))?;
        let length = match args.get(2) {
            Some(arg) => Some(self.compute(|cx| arg.slot.value().to_int(cx))?),
            None => None,
        };
        let chars = utf8_chars(&text);
        let slice = substr_range(chars.len(), offset, length)
            .map_or(String::new(), |(start, end)| {
                chars[start..end].iter().collect()
            });
        self.write(result, Value::String(PhpString::new(slice.as_bytes())))
    }
}

fn utf8_chars(bytes: &[u8]) -> Vec<char> {
    String::from_utf8_lossy(bytes).chars().collect()
}

#[cfg(test)]
mod tests {
    use super::utf8_chars;

    #[test]
    fn utf8_chars_count_codepoints() {
        assert_eq!(utf8_chars("é".as_bytes()).len(), 1);
        assert_eq!(utf8_chars(b"ab").len(), 2);
    }
}
