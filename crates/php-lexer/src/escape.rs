//! Escape sequences in string literals.

use crate::diagnostic::{LexError, LexWarning};

/// The literal an escaped body comes from; it decides whether `\"` and `` \` `` drop their backslash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quote {
    /// A `"…"` string.
    Double,
    /// A `` `…` `` command.
    Backtick,
    /// A heredoc body, where both quotes keep their backslash.
    Heredoc,
}

impl Quote {
    const fn escapes(self, byte: u8) -> bool {
        matches!((self, byte), (Self::Double, b'"') | (Self::Backtick, b'`'))
    }
}

/// A decoded double-quoted body and the warnings decoding raised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// The bytes the literal stands for.
    pub bytes: Vec<u8>,
    /// Octal overflow warnings, in source order.
    pub warnings: Vec<LexWarning>,
}

/// Decodes the body of a single-quoted literal (the bytes between the quotes): `\\` and `\'` lose
/// their backslash, every other byte is kept.
pub fn decode_single_quoted(body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(body.len());
    let mut index = 0;
    while let Some(&byte) = body.get(index) {
        if let (b'\\', Some(&next @ (b'\\' | b'\''))) = (byte, body.get(index + 1)) {
            bytes.push(next);
            index += 2;
        } else {
            bytes.push(byte);
            index += 1;
        }
    }
    bytes
}

/// Decodes the body of a double-quoted literal, a command or a heredoc segment.
///
/// # Errors
///
/// [`LexError::InvalidCodepointEscape`] or [`LexError::CodepointTooLarge`] for a malformed `\u{…}`.
pub fn decode_double_quoted(body: &[u8], quote: Quote) -> Result<Decoded, LexError> {
    let mut bytes = Vec::with_capacity(body.len());
    let mut warnings = Vec::new();
    scan(body, quote, Some(&mut bytes), &mut |warning| {
        warnings.push(warning);
    })
    .map_err(|failure| failure.error)?;
    Ok(Decoded { bytes, warnings })
}

/// Where escape decoding stopped and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EscapeFailure {
    pub(crate) error: LexError,
    /// Offset in the body of the backslash starting the bad sequence.
    pub(crate) at: usize,
}

/// Walks the escapes of `body`, writing the decoded bytes to `out` when given and reporting octal
/// overflows to `warn`.
pub(crate) fn scan(
    body: &[u8],
    quote: Quote,
    mut out: Option<&mut Vec<u8>>,
    warn: &mut dyn FnMut(LexWarning),
) -> Result<(), EscapeFailure> {
    let byte_at = |index: usize| body.get(index).copied();
    let mut emit = |bytes: &[u8]| {
        if let Some(out) = out.as_deref_mut() {
            out.extend_from_slice(bytes);
        }
    };
    let mut index = 0;
    while let Some(byte) = byte_at(index) {
        if byte != b'\\' {
            let run = body[index..]
                .iter()
                .position(|&byte| byte == b'\\')
                .map_or(body.len(), |offset| index + offset);
            emit(&body[index..run]);
            index = run;
            continue;
        }
        let Some(escaped) = byte_at(index + 1) else {
            emit(b"\\");
            break;
        };
        let start = index;
        index += 2;
        match escaped {
            b'n' => emit(b"\n"),
            b'r' => emit(b"\r"),
            b't' => emit(b"\t"),
            b'f' => emit(b"\x0c"),
            b'v' => emit(b"\x0b"),
            b'e' => emit(b"\x1b"),
            b'\\' | b'$' => emit(&[escaped]),
            b'"' | b'`' if quote.escapes(escaped) => emit(&[escaped]),
            b'x' | b'X' if byte_at(index).is_some_and(|byte| byte.is_ascii_hexdigit()) => {
                let digits = if byte_at(index + 1).is_some_and(|byte| byte.is_ascii_hexdigit()) {
                    2
                } else {
                    1
                };
                emit(&[parse_radix(&body[index..index + digits], 16)]);
                index += digits;
            }
            b'u' if byte_at(index) == Some(b'{') => {
                let digits_start = index + 1;
                let digits_len = body[digits_start..]
                    .iter()
                    .position(|byte| !byte.is_ascii_hexdigit())
                    .unwrap_or(body.len() - digits_start);
                let close = digits_start + digits_len;
                if digits_len == 0 || byte_at(close) != Some(b'}') {
                    return Err(EscapeFailure {
                        error: LexError::InvalidCodepointEscape,
                        at: start,
                    });
                }
                let value = codepoint(&body[digits_start..close]).ok_or(EscapeFailure {
                    error: LexError::CodepointTooLarge,
                    at: start,
                })?;
                let mut buffer = [0; 4];
                emit(encode_utf8(value, &mut buffer));
                index = close + 1;
            }
            b'0'..=b'7' => {
                let digits = 1 + body[index..]
                    .iter()
                    .take(2)
                    .take_while(|byte| matches!(byte, b'0'..=b'7'))
                    .count();
                let octal = &body[index - 1..index - 1 + digits];
                if let [first @ b'4'..=b'7', second, third] = *octal {
                    warn(LexWarning::OctalEscapeOverflow([first, second, third]));
                }
                emit(&[parse_radix(octal, 8)]);
                index += digits - 1;
            }
            _ => emit(&[b'\\', escaped]),
        }
    }
    Ok(())
}

/// The low byte of a short run of digits in `radix`; PHP keeps only that byte.
fn parse_radix(digits: &[u8], radix: u32) -> u8 {
    let value = digits.iter().fold(0_u32, |value, &digit| {
        value * radix + char::from(digit).to_digit(radix).unwrap_or(0)
    });
    value.to_le_bytes()[0]
}

/// The code point spelled by hexadecimal `digits`, or `None` above U+10FFFF.
fn codepoint(digits: &[u8]) -> Option<u32> {
    let significant = digits
        .iter()
        .position(|&digit| digit != b'0')
        .map_or(&digits[digits.len()..], |offset| &digits[offset..]);
    if significant.len() > 6 {
        return None;
    }
    let value = significant.iter().fold(0_u32, |value, &digit| {
        value * 16 + char::from(digit).to_digit(16).unwrap_or(0)
    });
    (value <= 0x10_FFFF).then_some(value)
}

/// Encodes `value` (at most U+10FFFF) with the UTF-8 bit layout. Surrogates are encoded like any other
/// three-byte value because PHP strings may hold them, which is why `char` is not used.
fn encode_utf8(value: u32, buffer: &mut [u8; 4]) -> &[u8] {
    let [_, plane, high, low] = value.to_be_bytes();
    let continuation = |bits: u8| 0x80 | (bits & 0x3F);
    match value {
        0..0x80 => {
            buffer[0] = low;
            &buffer[..1]
        }
        0x80..0x800 => {
            buffer[0] = 0xC0 | (high << 2) | (low >> 6);
            buffer[1] = continuation(low);
            &buffer[..2]
        }
        0x800..0x1_0000 => {
            buffer[0] = 0xE0 | (high >> 4);
            buffer[1] = continuation((high << 2) | (low >> 6));
            buffer[2] = continuation(low);
            &buffer[..3]
        }
        _ => {
            buffer[0] = 0xF0 | (plane >> 2);
            buffer[1] = continuation((plane << 4) | (high >> 4));
            buffer[2] = continuation((high << 2) | (low >> 6));
            buffer[3] = continuation(low);
            &buffer[..4]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Decoded, Quote, decode_double_quoted, decode_single_quoted, scan};
    use crate::diagnostic::{LexError, LexWarning};

    fn decode(body: &[u8]) -> Vec<u8> {
        match decode_double_quoted(body, Quote::Double) {
            Ok(Decoded { bytes, .. }) => bytes,
            Err(error) => panic!("{error}"),
        }
    }

    #[test]
    fn single_quoted_keeps_unknown_escapes() {
        assert_eq!(decode_single_quoted(br"a\\b\'c\nd\"), br"a\b'c\nd\");
        assert_eq!(decode_single_quoted(b""), b"");
    }

    #[test]
    fn simple_escapes() {
        assert_eq!(decode(br"\n\r\t\f\v\e\\\$"), b"\n\r\t\x0c\x0b\x1b\\$");
        assert_eq!(decode(br"\q\8"), br"\q\8");
        assert_eq!(decode(b"plain"), b"plain");
        assert_eq!(decode(br"end\"), br"end\");
    }

    #[test]
    fn quotes_depend_on_the_literal() {
        assert_eq!(decode(br#"\"\`"#), br#""\`"#);
        let backtick = decode_double_quoted(br#"\"\`"#, Quote::Backtick).map(|d| d.bytes);
        assert_eq!(backtick, Ok(br#"\"`"#.to_vec()));
        let heredoc = decode_double_quoted(br#"\"\`"#, Quote::Heredoc).map(|d| d.bytes);
        assert_eq!(heredoc, Ok(br#"\"\`"#.to_vec()));
    }

    #[test]
    fn hex_escapes_take_one_or_two_digits() {
        assert_eq!(decode(br"\x41\X4g\x\xfff"), b"A\x04g\\x\xfff");
    }

    #[test]
    fn octal_escapes_take_up_to_three_digits_and_wrap() {
        assert_eq!(decode(br"\0\101\7777"), b"\0A\xff7");
        let decoded = decode_double_quoted(br"\400\377\1234", Quote::Double);
        assert_eq!(
            decoded,
            Ok(Decoded {
                bytes: b"\0\xffS4".to_vec(),
                warnings: vec![LexWarning::OctalEscapeOverflow(*b"400")],
            })
        );
    }

    #[test]
    fn codepoint_escapes() {
        assert_eq!(
            decode(br"\u{41}\u{00e9}\u{20AC}\u{1F600}"),
            "Aé€😀".as_bytes()
        );
        assert_eq!(decode(br"\u{0000000041}\u41"), br"A\u41");
        assert_eq!(decode(br"\u{D800}"), b"\xed\xa0\x80");
        assert_eq!(decode(br"\u{10FFFF}"), "\u{10FFFF}".as_bytes());
    }

    #[test]
    fn bad_codepoint_escapes_fail_at_the_backslash() {
        for body in [&br"ab\u{}"[..], br"ab\u{zz}", br"ab\u{41", br"ab\u{"] {
            let failure = scan(body, Quote::Double, None, &mut |_| {});
            assert_eq!(
                failure.map_err(|failure| (failure.error, failure.at)),
                Err((LexError::InvalidCodepointEscape, 2)),
                "{body:?}"
            );
        }
        for body in [&br"\u{110000}"[..], br"\u{1000000000000000000000}"] {
            assert_eq!(
                decode_double_quoted(body, Quote::Double),
                Err(LexError::CodepointTooLarge)
            );
        }
    }
}
