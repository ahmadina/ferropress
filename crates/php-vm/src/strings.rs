//! Extra string built-ins: slice, search and trim.

use php_compiler::bytecode::Operand;
use php_value::{PhpString, Value};

use crate::Output;
use crate::calls::Argument;
use crate::machine::{Exec, Machine};

const TRIM_DEFAULT: &[u8] = b" \t\n\r\0\x0b";

impl<O: Output> Machine<'_, '_, O> {
    pub(crate) fn native_substr(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() < 2 {
            return Err(self.too_few("substr", args.len(), 2));
        }
        let text = self.string_arg("substr", &args[0].slot.value())?;
        let offset = self.compute(|cx| args[1].slot.value().to_int(cx))?;
        let length = match args.get(2) {
            Some(arg) => Some(self.compute(|cx| arg.slot.value().to_int(cx))?),
            None => None,
        };
        let slice = substr_range(text.len(), offset, length)
            .map_or(&[][..], |(start, end)| &text[start..end]);
        self.write(result, Value::String(PhpString::new(slice)))
    }

    pub(crate) fn native_strpos(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() < 2 {
            return Err(self.too_few("strpos", args.len(), 2));
        }
        let haystack = self.string_arg("strpos", &args[0].slot.value())?;
        let needle = self.string_arg("strpos", &args[1].slot.value())?;
        let offset = match args.get(2) {
            Some(arg) => self.compute(|cx| arg.slot.value().to_int(cx))?,
            None => 0,
        };
        let found = strpos(&haystack, &needle, offset);
        self.write(result, found.map_or(Value::Bool(false), Value::Int))
    }

    pub(crate) fn native_str_contains(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() < 2 {
            return Err(self.too_few("str_contains", args.len(), 2));
        }
        let haystack = self.string_arg("str_contains", &args[0].slot.value())?;
        let needle = self.string_arg("str_contains", &args[1].slot.value())?;
        self.write(result, Value::Bool(contains(&haystack, &needle)))
    }

    pub(crate) fn native_trim(&mut self, args: &[Argument], result: Operand, mode: Trim) -> Exec {
        let name = mode.name();
        let value = self.native_one(name, args)?;
        let text = self.string_arg(name, &value)?;
        let cut = match args.get(1) {
            Some(arg) => self.string_arg(name, &arg.slot.value())?,
            None => TRIM_DEFAULT.to_vec(),
        };
        let slice = trim_bytes(&text, &cut, mode);
        self.write(result, Value::String(PhpString::new(slice)))
    }
}

/// Which ends `trim` / `ltrim` / `rtrim` cut.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Trim {
    Both,
    Left,
    Right,
}

impl Trim {
    const fn name(self) -> &'static str {
        match self {
            Self::Both => "trim",
            Self::Left => "ltrim",
            Self::Right => "rtrim",
        }
    }
}

pub(crate) fn substr_range(len: usize, offset: i64, length: Option<i64>) -> Option<(usize, usize)> {
    let len_i = i64::try_from(len).ok()?;
    let start = if offset >= 0 {
        if offset >= len_i {
            return None;
        }
        offset
    } else if offset.checked_neg().is_some_and(|n| n > len_i) {
        0
    } else {
        len_i.saturating_add(offset)
    };
    let end = match length {
        None => len_i,
        Some(n) if n >= 0 => start.saturating_add(n).min(len_i),
        Some(n) => len_i.saturating_add(n).max(start),
    };
    if end <= start {
        return None;
    }
    Some((usize::try_from(start).ok()?, usize::try_from(end).ok()?))
}

fn strpos(haystack: &[u8], needle: &[u8], offset: i64) -> Option<i64> {
    if needle.is_empty() {
        return Some(offset.max(0).min(i64::try_from(haystack.len()).ok()?));
    }
    let start = start_index(haystack.len(), offset)?;
    let rest = haystack.get(start..)?;
    let at = rest
        .windows(needle.len())
        .position(|window| window == needle)?;
    i64::try_from(start + at).ok()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty()
        || haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn start_index(len: usize, offset: i64) -> Option<usize> {
    if offset >= 0 {
        let start = usize::try_from(offset).ok()?;
        (start <= len).then_some(start)
    } else {
        let from_end = usize::try_from(offset.checked_neg()?).ok()?;
        Some(len.saturating_sub(from_end))
    }
}

fn trim_bytes<'a>(text: &'a [u8], cut: &[u8], mode: Trim) -> &'a [u8] {
    let start = if matches!(mode, Trim::Both | Trim::Left) {
        text.iter()
            .position(|byte| !cut.contains(byte))
            .unwrap_or(text.len())
    } else {
        0
    };
    let end = if matches!(mode, Trim::Both | Trim::Right) {
        text.iter()
            .rposition(|byte| !cut.contains(byte))
            .map_or(start, |index| index + 1)
    } else {
        text.len()
    };
    text.get(start..end.max(start)).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::{Trim, contains, strpos, substr_range, trim_bytes};

    #[test]
    fn substr_range_follows_php_offsets() {
        assert_eq!(substr_range(4, 1, Some(2)), Some((1, 3)));
        assert_eq!(substr_range(4, -2, None), Some((2, 4)));
        assert_eq!(substr_range(4, 4, None), None);
        assert_eq!(substr_range(4, 1, Some(-1)), Some((1, 3)));
    }

    #[test]
    fn strpos_and_contains_find_needles() {
        assert_eq!(strpos(b"abcd", b"c", 0), Some(2));
        assert_eq!(strpos(b"abcd", b"z", 0), None);
        assert_eq!(strpos(b"abcd", b"", 2), Some(2));
        assert!(contains(b"abcd", b"bc"));
        assert!(contains(b"abcd", b""));
        assert!(!contains(b"abcd", b"z"));
    }

    #[test]
    fn trim_cuts_default_ends() {
        assert_eq!(trim_bytes(b" a\n", b" \n", Trim::Both), b"a");
        assert_eq!(trim_bytes(b" a ", b" ", Trim::Left), b"a ");
        assert_eq!(trim_bytes(b" a ", b" ", Trim::Right), b" a");
    }
}
