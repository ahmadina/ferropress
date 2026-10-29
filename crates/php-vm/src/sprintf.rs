//! `sprintf` with the conversion specifiers plugins use most.

use php_compiler::bytecode::Operand;
use php_value::{PhpString, Value};

use crate::Output;
use crate::VmError;
use crate::calls::Argument;
use crate::machine::{Exec, Machine};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Percent,
    String,
    Int,
    Uint,
    Hex(bool),
    Float,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Spec {
    left: bool,
    zero: bool,
    plus: bool,
    width: usize,
    precision: Option<usize>,
    kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormatError {
    Incomplete,
    Unknown(u8),
    Unsupported(u8),
}

impl<O: Output> Machine<'_, '_, O> {
    pub(crate) fn native_sprintf(&mut self, args: &[Argument], result: Operand) -> Exec {
        if args.is_empty() {
            return Err(self.too_few_at_least("sprintf", 0, 1, args));
        }
        let format = self.string_arg("sprintf", &args[0].slot.value())?;
        let needed = 1 + count_values(&format);
        if args.len() < needed {
            let mut message = needed.to_string().into_bytes();
            message.extend_from_slice(b" arguments are required, ");
            message.extend_from_slice(args.len().to_string().as_bytes());
            message.extend_from_slice(b" given");
            let values: Vec<Value> = args.iter().map(|arg| arg.slot.value()).collect();
            return Err(self.throw_from("ArgumentCountError", message, b"sprintf", values));
        }
        let rendered = self.render_sprintf(&format, args)?;
        self.write(result, Value::String(PhpString::new(&rendered)))
    }

    fn render_sprintf(&mut self, format: &[u8], args: &[Argument]) -> Exec<Vec<u8>> {
        let mut out = Vec::new();
        let mut index = 0;
        let mut arg = 0;
        while index < format.len() {
            if format[index] != b'%' {
                out.push(format[index]);
                index += 1;
                continue;
            }
            let (spec, next) = match parse_spec(format, index) {
                Ok(parsed) => parsed,
                Err(error) => return Err(self.sprintf_error(error, args)),
            };
            index = next;
            if spec.kind == Kind::Percent {
                out.push(b'%');
                continue;
            }
            let value = args[arg + 1].slot.value();
            arg += 1;
            let piece = self.format_value(spec, &value)?;
            out.extend_from_slice(&piece);
        }
        Ok(out)
    }

    fn format_value(&mut self, spec: Spec, value: &Value) -> Exec<Vec<u8>> {
        match spec.kind {
            Kind::Percent => Ok(b"%".to_vec()),
            Kind::String => {
                let text = self.stringify(value)?;
                Ok(pad(text.as_bytes(), spec.width, spec.left, false, false))
            }
            Kind::Int => {
                let n = self.compute(|cx| value.to_int(cx))?;
                Ok(pad_number(&signed_int(n, spec.plus), spec))
            }
            Kind::Uint => {
                let n = self.compute(|cx| value.to_int(cx))?;
                let text = n.cast_unsigned().to_string();
                Ok(pad(
                    text.as_bytes(),
                    spec.width,
                    spec.left,
                    spec.zero,
                    false,
                ))
            }
            Kind::Hex(upper) => {
                let n = self.compute(|cx| value.to_int(cx))?;
                let bits = n.cast_unsigned();
                let text = if upper {
                    format!("{bits:X}")
                } else {
                    format!("{bits:x}")
                };
                Ok(pad(
                    text.as_bytes(),
                    spec.width,
                    spec.left,
                    spec.zero,
                    false,
                ))
            }
            Kind::Float => {
                let n = self.compute(|_cx| value.to_float())?;
                let precision = spec.precision.unwrap_or(6);
                let text = format!("{n:.precision$}");
                Ok(pad(
                    text.as_bytes(),
                    spec.width,
                    spec.left,
                    spec.zero,
                    false,
                ))
            }
        }
    }

    fn sprintf_error(&self, error: FormatError, args: &[Argument]) -> crate::machine::Stop {
        let values: Vec<Value> = args.iter().map(|arg| arg.slot.value()).collect();
        match error {
            FormatError::Incomplete => self.throw_from(
                "ValueError",
                b"Missing format specifier at end of string".to_vec(),
                b"sprintf",
                values,
            ),
            FormatError::Unknown(byte) => {
                let mut message = b"Unknown format specifier \"".to_vec();
                message.push(byte);
                message.push(b'"');
                self.throw_from("ValueError", message, b"sprintf", values)
            }
            FormatError::Unsupported(_) => VmError::Unsupported("this sprintf specifier").into(),
        }
    }
}

fn count_values(format: &[u8]) -> usize {
    let mut index = 0;
    let mut count = 0;
    while index < format.len() {
        if format[index] != b'%' {
            index += 1;
            continue;
        }
        if let Ok((spec, next)) = parse_spec(format, index) {
            if spec.kind != Kind::Percent {
                count += 1;
            }
            index = next;
        } else {
            count += 1;
            break;
        }
    }
    count
}

fn parse_spec(format: &[u8], start: usize) -> Result<(Spec, usize), FormatError> {
    let mut index = start + 1;
    if index >= format.len() {
        return Err(FormatError::Incomplete);
    }
    if format[index] == b'%' {
        return Ok((
            Spec {
                left: false,
                zero: false,
                plus: false,
                width: 0,
                precision: None,
                kind: Kind::Percent,
            },
            index + 1,
        ));
    }
    let mut left = false;
    let mut zero = false;
    let mut plus = false;
    while index < format.len() {
        match format[index] {
            b'-' => left = true,
            b'0' => zero = true,
            b'+' => plus = true,
            b' ' => {}
            _ => break,
        }
        index += 1;
    }
    let (width, next) = take_number(format, index);
    index = next;
    let precision = (format.get(index) == Some(&b'.')).then(|| {
        let (digits, next) = take_number(format, index + 1);
        index = next;
        digits
    });
    let kind = match format.get(index).copied() {
        None => return Err(FormatError::Incomplete),
        Some(b's') => Kind::String,
        Some(b'd') => Kind::Int,
        Some(b'u') => Kind::Uint,
        Some(b'x') => Kind::Hex(false),
        Some(b'X') => Kind::Hex(true),
        Some(b'f') => Kind::Float,
        Some(byte) if matches!(byte, b'b' | b'c' | b'e' | b'E' | b'F' | b'g' | b'G' | b'o') => {
            return Err(FormatError::Unsupported(byte));
        }
        Some(byte) => return Err(FormatError::Unknown(byte)),
    };
    Ok((
        Spec {
            left,
            zero,
            plus,
            width,
            precision,
            kind,
        },
        index + 1,
    ))
}

fn take_number(bytes: &[u8], start: usize) -> (usize, usize) {
    let mut index = start;
    let mut value = 0usize;
    while let Some(&byte) = bytes.get(index) {
        if !byte.is_ascii_digit() {
            break;
        }
        value = value
            .saturating_mul(10)
            .saturating_add(usize::from(byte - b'0'));
        index += 1;
    }
    (value, index)
}

fn signed_int(n: i64, plus: bool) -> Vec<u8> {
    if plus && n >= 0 {
        format!("+{n}").into_bytes()
    } else {
        n.to_string().into_bytes()
    }
}

fn pad_number(body: &[u8], spec: Spec) -> Vec<u8> {
    pad(body, spec.width, spec.left, spec.zero, true)
}

fn pad(body: &[u8], width: usize, left: bool, zero: bool, signed: bool) -> Vec<u8> {
    if body.len() >= width {
        return body.to_vec();
    }
    let fill = if zero && !left { b'0' } else { b' ' };
    let extra = width - body.len();
    let mut out = Vec::with_capacity(width);
    if left {
        out.extend_from_slice(body);
        out.extend(std::iter::repeat_n(b' ', extra));
        return out;
    }
    if zero && signed && matches!(body.first(), Some(b'+' | b'-')) {
        out.push(body[0]);
        out.extend(std::iter::repeat_n(fill, extra));
        out.extend_from_slice(&body[1..]);
        return out;
    }
    out.extend(std::iter::repeat_n(fill, extra));
    out.extend_from_slice(body);
    out
}

#[cfg(test)]
mod tests {
    use super::{Kind, count_values, pad, parse_spec, signed_int};

    #[test]
    fn count_skips_escaped_percents() {
        assert_eq!(count_values(b"%% %s %d"), 2);
        assert_eq!(count_values(b"%"), 1);
        assert_eq!(count_values(b"plain"), 0);
    }

    #[test]
    fn parse_reads_flags_width_and_type() {
        let (spec, next) = parse_spec(b"%05d", 0).expect("spec");
        assert_eq!(next, 4);
        assert!(spec.zero);
        assert_eq!(spec.width, 5);
        assert_eq!(spec.kind, Kind::Int);
        assert!(parse_spec(b"%", 0).is_err());
        assert_eq!(signed_int(3, true), b"+3");
        assert_eq!(pad(b"ab", 5, false, false, false), b"   ab");
        assert_eq!(pad(b"-3", 5, false, true, true), b"-0003");
    }
}
