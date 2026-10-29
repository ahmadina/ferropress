//! `preg_match` and `preg_replace` over PCRE2 (ADR-0009).

use pcre2::bytes::{Captures, Regex, RegexBuilder};
use php_compiler::bytecode::Operand;
use php_value::{PhpString, Value};

use crate::Output;
use crate::VmError;
use crate::calls::Argument;
use crate::machine::{Exec, Machine, Stop};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Flags(u8);

impl Flags {
    const CASELESS: u8 = 1 << 0;
    const MULTILINE: u8 = 1 << 1;
    const DOTALL: u8 = 1 << 2;
    const EXTENDED: u8 = 1 << 3;
    const UTF: u8 = 1 << 4;
    const ANCHORED: u8 = 1 << 5;
    const UNGREEDY: u8 = 1 << 6;
    const DUPNAMES: u8 = 1 << 7;

    const fn has(self, bit: u8) -> bool {
        self.0 & bit != 0
    }

    fn set(&mut self, bit: u8) {
        self.0 |= bit;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PatternError {
    Empty,
    BadDelimiter,
    NoEnd(u8),
    Unknown(u8),
    Unsupported(&'static str),
}

impl<O: Output> Machine<'_, '_, O> {
    pub(crate) fn native_preg_match(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() < 2 {
            return Err(self.too_few_at_least("preg_match", args.len(), 2, args));
        }
        if args.len() > 2 {
            return Err(VmError::Unsupported("preg_match matches").into());
        }
        let pattern = self.typed_string("preg_match", 1, "pattern", &args[0].slot.value(), args)?;
        let subject = self.typed_string("preg_match", 2, "subject", &args[1].slot.value(), args)?;
        let Some((regex, flags)) = self.compile_preg("preg_match", &pattern)? else {
            return self.write(result, Value::Bool(false));
        };
        match regex.find(&subject) {
            Ok(Some(found)) if !(flags.has(Flags::ANCHORED) && found.start() != 0) => {
                self.write(result, Value::Int(1))
            }
            Ok(_) => self.write(result, Value::Int(0)),
            Err(error) => {
                self.preg_warning("preg_match", error.to_string().as_bytes());
                self.write(result, Value::Bool(false))
            }
        }
    }

    pub(crate) fn native_preg_replace(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.len() < 3 {
            return Err(self.too_few_at_least("preg_replace", args.len(), 3, args));
        }
        if args.len() > 3 {
            return Err(VmError::Unsupported("preg_replace limit").into());
        }
        if matches!(args[0].slot.value(), Value::Array(_) | Value::Object(_))
            || matches!(args[1].slot.value(), Value::Array(_) | Value::Object(_))
        {
            return Err(VmError::Unsupported("preg_replace arrays").into());
        }
        let pattern =
            self.typed_string("preg_replace", 1, "pattern", &args[0].slot.value(), args)?;
        let replace = self.typed_string(
            "preg_replace",
            2,
            "replacement",
            &args[1].slot.value(),
            args,
        )?;
        let subject =
            self.typed_string("preg_replace", 3, "subject", &args[2].slot.value(), args)?;
        let Some((regex, _)) = self.compile_preg("preg_replace", &pattern)? else {
            return self.write(result, Value::Null);
        };
        match replace_all(&regex, &subject, &replace) {
            Ok(text) => self.write(result, Value::String(PhpString::new(&text))),
            Err(error) => {
                self.preg_warning("preg_replace", error.to_string().as_bytes());
                self.write(result, Value::Null)
            }
        }
    }

    fn compile_preg(&mut self, name: &str, pattern: &[u8]) -> Result<Option<(Regex, Flags)>, Stop> {
        let (body, flags) = match parse_pattern(pattern) {
            Ok(parsed) => parsed,
            Err(PatternError::Unsupported(reason)) => {
                return Err(VmError::Unsupported(reason).into());
            }
            Err(error) => {
                self.preg_warning(name, &pattern_message(&error));
                return Ok(None);
            }
        };
        let Some(text) = std::str::from_utf8(body).ok() else {
            return Err(VmError::Unsupported("this preg pattern encoding").into());
        };
        match compile_pattern(text, flags) {
            Ok(regex) => Ok(Some((regex, flags))),
            Err(error) => {
                self.preg_warning(name, &compile_message(&error));
                Ok(None)
            }
        }
    }

    fn preg_warning(&mut self, name: &str, detail: &[u8]) {
        let mut message = name.as_bytes().to_vec();
        message.extend_from_slice(b"(): ");
        message.extend_from_slice(detail);
        self.display(b"Warning", &message);
    }
}

fn compile_pattern(body: &str, flags: Flags) -> Result<Regex, pcre2::Error> {
    let mut source = String::new();
    if flags.has(Flags::UNGREEDY) {
        source.push_str("(?U)");
    }
    if flags.has(Flags::DUPNAMES) {
        source.push_str("(?J)");
    }
    source.push_str(body);
    RegexBuilder::new()
        .caseless(flags.has(Flags::CASELESS))
        .multi_line(flags.has(Flags::MULTILINE))
        .dotall(flags.has(Flags::DOTALL))
        .extended(flags.has(Flags::EXTENDED))
        .utf(flags.has(Flags::UTF))
        .ucp(flags.has(Flags::UTF))
        .build(&source)
}

fn parse_pattern(pattern: &[u8]) -> Result<(&[u8], Flags), PatternError> {
    let first = *pattern.first().ok_or(PatternError::Empty)?;
    if first.is_ascii_alphanumeric() || first == b'\\' || first == 0 {
        return Err(PatternError::BadDelimiter);
    }
    let close = match first {
        b'{' => b'}',
        b'(' => b')',
        b'[' => b']',
        b'<' => b'>',
        other => other,
    };
    let mut index = 1;
    while index < pattern.len() {
        if pattern[index] == b'\\' {
            index = index.saturating_add(2);
            continue;
        }
        if pattern[index] == close {
            let body = &pattern[1..index];
            if body.is_empty() {
                return Err(PatternError::Empty);
            }
            let flags = parse_flags(&pattern[index + 1..])?;
            return Ok((body, flags));
        }
        index += 1;
    }
    Err(PatternError::NoEnd(first))
}

fn parse_flags(bytes: &[u8]) -> Result<Flags, PatternError> {
    let mut flags = Flags::default();
    for &byte in bytes {
        match byte {
            b'i' => flags.set(Flags::CASELESS),
            b'm' => flags.set(Flags::MULTILINE),
            b's' => flags.set(Flags::DOTALL),
            b'x' => flags.set(Flags::EXTENDED),
            b'u' => flags.set(Flags::UTF),
            b'A' => flags.set(Flags::ANCHORED),
            b'U' => flags.set(Flags::UNGREEDY),
            b'J' => flags.set(Flags::DUPNAMES),
            b'S' | b'X' => {}
            b'D' => return Err(PatternError::Unsupported("this preg modifier")),
            other => return Err(PatternError::Unknown(other)),
        }
    }
    Ok(flags)
}

fn pattern_message(error: &PatternError) -> Vec<u8> {
    match error {
        PatternError::Empty => b"Empty regular expression".to_vec(),
        PatternError::BadDelimiter => {
            b"Delimiter must not be alphanumeric, backslash, or NUL byte".to_vec()
        }
        PatternError::NoEnd(delimiter) => {
            let mut message = b"No ending delimiter '".to_vec();
            message.push(*delimiter);
            message.extend_from_slice(b"' found");
            message
        }
        PatternError::Unknown(modifier) => {
            let mut message = b"Unknown modifier '".to_vec();
            message.push(*modifier);
            message.push(b'\'');
            message
        }
        PatternError::Unsupported(_) => Vec::new(),
    }
}

fn compile_message(error: &pcre2::Error) -> Vec<u8> {
    let text = error.to_string();
    let mut message = b"Compilation failed: ".to_vec();
    if let Some(offset) = error.offset()
        && let Some((_, detail)) = text.rsplit_once(": ")
    {
        message.extend_from_slice(detail.as_bytes());
        message.extend_from_slice(b" at offset ");
        message.extend_from_slice(offset.to_string().as_bytes());
        return message;
    }
    message.extend_from_slice(text.as_bytes());
    message
}

fn replace_all(regex: &Regex, subject: &[u8], replace: &[u8]) -> Result<Vec<u8>, pcre2::Error> {
    let mut out = Vec::with_capacity(subject.len());
    let mut last = 0;
    for result in regex.captures_iter(subject) {
        let captures = result?;
        let Some(full) = captures.get(0) else {
            continue;
        };
        out.extend_from_slice(&subject[last..full.start()]);
        interpolate(&captures, replace, &mut out);
        last = full.end();
    }
    out.extend_from_slice(&subject[last..]);
    Ok(out)
}

fn interpolate(captures: &Captures<'_>, replace: &[u8], out: &mut Vec<u8>) {
    let mut index = 0;
    while index < replace.len() {
        match replace[index] {
            b'\\' if index + 1 < replace.len() && replace[index + 1].is_ascii_digit() => {
                let (number, next) = take_digits(replace, index + 1);
                push_group(captures, number, out);
                index = next;
            }
            b'$' if index + 1 < replace.len() => match replace[index + 1] {
                b'{' => {
                    let digits = index + 2;
                    let (number, end) = take_digits(replace, digits);
                    if end > digits && replace.get(end) == Some(&b'}') {
                        push_group(captures, number, out);
                        index = end + 1;
                    } else {
                        out.push(b'$');
                        index += 1;
                    }
                }
                byte if byte.is_ascii_digit() => {
                    let (number, next) = take_digits(replace, index + 1);
                    push_group(captures, number, out);
                    index = next;
                }
                _ => {
                    out.push(b'$');
                    index += 1;
                }
            },
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
}

fn take_digits(bytes: &[u8], start: usize) -> (usize, usize) {
    let mut end = start;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    let number = std::str::from_utf8(&bytes[start..end])
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(usize::MAX);
    (number, end)
}

fn push_group(captures: &Captures<'_>, number: usize, out: &mut Vec<u8>) {
    if let Some(group) = captures.get(number) {
        out.extend_from_slice(group.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::{Flags, PatternError, parse_pattern};

    #[test]
    fn delimiters_and_modifiers_parse() {
        let (body, flags) = parse_pattern(br"/a+/i").unwrap();
        assert_eq!(body, b"a+");
        assert!(flags.has(Flags::CASELESS));
        let (body, flags) = parse_pattern(br"#a#ms").unwrap();
        assert_eq!(body, b"a");
        assert!(flags.has(Flags::MULTILINE) && flags.has(Flags::DOTALL));
        let (body, _) = parse_pattern(br"{a}").unwrap();
        assert_eq!(body, b"a");
        assert!(matches!(
            parse_pattern(br"/a"),
            Err(PatternError::NoEnd(b'/'))
        ));
        assert!(matches!(
            parse_pattern(b"a"),
            Err(PatternError::BadDelimiter)
        ));
        assert!(matches!(
            parse_pattern(br"/a/z"),
            Err(PatternError::Unknown(b'z'))
        ));
        assert!(matches!(
            parse_pattern(br"/a/D"),
            Err(PatternError::Unsupported(_))
        ));
    }
}
