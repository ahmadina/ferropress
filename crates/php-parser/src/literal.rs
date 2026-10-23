//! Values of literal tokens: numbers, names, quoted strings and heredoc bodies.

use crate::ast::{Name, NameKind};
use php_lexer::{LexError, TokenKind};

/// A numeric literal's value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Number {
    /// It fits an integer.
    Int(i64),
    /// It is a float, or an integer literal too large for an integer.
    Float(f64),
}

/// The digits of a literal without the `_` separators.
fn without_separators(text: &[u8]) -> Vec<u8> {
    text.iter().copied().filter(|&byte| byte != b'_').collect()
}

/// Splits a prefixed or leading-zero literal into its radix and digits; `None` for decimal.
fn radix_digits(digits: &[u8]) -> Option<(u32, &[u8])> {
    match digits {
        [b'0', b'x' | b'X', rest @ ..] => Some((16, rest)),
        [b'0', b'b' | b'B', rest @ ..] => Some((2, rest)),
        [b'0', b'o' | b'O', rest @ ..] => Some((8, rest)),
        [b'0', rest @ ..] if !rest.is_empty() && rest.iter().all(u8::is_ascii_digit) => {
            Some((8, rest))
        }
        _ => None,
    }
}

/// The value of an integer or float token whose text is `text`.
pub(crate) fn number(kind: TokenKind, text: &[u8]) -> Number {
    let digits = without_separators(text);
    if let Some((radix, body)) = radix_digits(&digits) {
        if kind == TokenKind::Integer
            && let Some(value) = parse_radix(body, radix)
        {
            return Number::Int(value);
        }
        return Number::Float(accumulate(body, radix));
    }
    if kind == TokenKind::Integer
        && let Some(value) = std::str::from_utf8(&digits)
            .ok()
            .and_then(|text| text.parse().ok())
    {
        return Number::Int(value);
    }
    Number::Float(decimal_float(&digits))
}

fn parse_radix(digits: &[u8], radix: u32) -> Option<i64> {
    if digits.is_empty() {
        return Some(0);
    }
    std::str::from_utf8(digits)
        .ok()
        .and_then(|text| i64::from_str_radix(text, radix).ok())
}

/// A too-large prefixed literal's value, accumulated digit by digit in floating point as PHP does.
fn accumulate(digits: &[u8], radix: u32) -> f64 {
    digits.iter().fold(0.0, |value, &digit| {
        let digit = char::from(digit).to_digit(radix).unwrap_or(0);
        value * f64::from(radix) + f64::from(digit)
    })
}

/// A decimal literal's value, correctly rounded.
fn decimal_float(digits: &[u8]) -> f64 {
    std::str::from_utf8(digits)
        .ok()
        .and_then(|text| text.parse().ok())
        .unwrap_or(f64::INFINITY)
}

/// The value of an interpolation offset such as the `12` in `"$a[12]"`: an integer when it is a
/// canonical decimal (no leading zero, no separators) that fits, the text itself otherwise.
pub(crate) fn offset_number(text: &[u8]) -> Result<i64, Vec<u8>> {
    let canonical = text.iter().all(u8::is_ascii_digit) && (text == b"0" || text[0] != b'0');
    if canonical
        && let Some(value) = std::str::from_utf8(text)
            .ok()
            .and_then(|text| text.parse().ok())
    {
        return Ok(value);
    }
    Err(text.to_vec())
}

/// The name a name token stands for.
pub(crate) fn name(kind: TokenKind, text: &[u8]) -> Name {
    match kind {
        TokenKind::FullyQualifiedName => Name {
            kind: NameKind::FullyQualified,
            bytes: text.get(1..).unwrap_or_default().to_vec(),
        },
        TokenKind::RelativeName => Name {
            kind: NameKind::Relative,
            bytes: text
                .get(b"namespace\\".len()..)
                .unwrap_or_default()
                .to_vec(),
        },
        _ => Name {
            kind: NameKind::Unqualified,
            bytes: text.to_vec(),
        },
    }
}

/// The identifier a keyword stands for where any identifier is accepted: its leading letters and
/// underscores. `None` for `<?=`, which reads as `echo` but cannot be an identifier.
pub(crate) fn keyword_identifier(text: &[u8]) -> Option<Vec<u8>> {
    let end = text
        .iter()
        .position(|&byte| !(byte.is_ascii_alphabetic() || byte == b'_'))
        .unwrap_or(text.len());
    (end > 0).then(|| text[..end].to_vec())
}

/// A failure while removing heredoc indentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StripFailure {
    /// The error.
    pub(crate) error: LexError,
    /// Lines of the text before the offending one.
    pub(crate) lines_before: u32,
}

/// Where the next line break at or after `start` is, and how long it is.
fn next_newline(text: &[u8], start: usize) -> Option<(usize, usize)> {
    let offset = text[start..]
        .iter()
        .position(|&byte| byte == b'\n' || byte == b'\r')?;
    let at = start + offset;
    let length = if text[at] == b'\r' && text.get(at + 1) == Some(&b'\n') {
        2
    } else {
        1
    };
    Some((at, length))
}

/// The layout of a heredoc or nowdoc segment being stripped of its indentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Indentation {
    /// Columns removed from each line.
    pub(crate) width: usize,
    /// Whether the indentation consists of spaces (otherwise tabs).
    pub(crate) uses_spaces: bool,
    /// Whether the segment starts at the beginning of a line.
    pub(crate) at_line_start: bool,
    /// Whether the segment ends right before the closing label.
    pub(crate) at_closing_label: bool,
}

/// Removes `layout.width` columns of indentation from every line of a heredoc or nowdoc segment
/// that starts a line. Lines consisting only of whitespace may be shorter.
pub(crate) fn strip_indentation(text: &[u8], layout: Indentation) -> Result<Vec<u8>, StripFailure> {
    let mut out = Vec::with_capacity(text.len());
    let mut position = 0;
    let mut lines_before = 0;
    if !layout.at_line_start {
        let Some((at, length)) = next_newline(text, 0) else {
            return Ok(text.to_vec());
        };
        position = at + length;
        out.extend_from_slice(&text[..position]);
        lines_before = 1;
    }
    while position < text.len() {
        let line_end = next_newline(text, position);
        let stop = match line_end {
            Some((at, _)) => Some(at),
            None if layout.at_closing_label => Some(text.len()),
            None => None,
        };
        for _ in 0..layout.width {
            if Some(position) == stop {
                break;
            }
            let failure = |error| StripFailure {
                error,
                lines_before,
            };
            match text.get(position) {
                Some(b' ') if layout.uses_spaces => {}
                Some(b'\t') if !layout.uses_spaces => {}
                Some(b' ' | b'\t') => return Err(failure(LexError::MixedIndentation)),
                _ => return Err(failure(LexError::BodyIndentation(layout.width))),
            }
            position += 1;
        }
        if position == text.len() {
            break;
        }
        if let Some((at, length)) = line_end {
            out.extend_from_slice(&text[position..at + length]);
            position = at + length;
            lines_before += 1;
        } else {
            out.extend_from_slice(&text[position..]);
            break;
        }
    }
    Ok(out)
}

/// The indentation that opens the last line of `text` as `(width, spaces only)`, and whether nothing
/// but that indentation follows the last line break; `None` when `text` has no line break.
pub(crate) fn last_line_indentation(text: &[u8]) -> Option<((usize, bool), bool)> {
    let start = text
        .iter()
        .rposition(|&byte| byte == b'\n' || byte == b'\r')?
        + 1;
    let rest = &text[start..];
    let width = rest
        .iter()
        .take_while(|&&byte| byte == b' ' || byte == b'\t')
        .count();
    let uses_spaces = width > 0 && rest[..width].iter().all(|&byte| byte == b' ');
    Some(((width, uses_spaces), width == rest.len()))
}

/// Line breaks in `text` counted the way the reference scanner counts them while decoding: `\n`,
/// and `\r` not followed by `\n`.
pub(crate) fn count_lines(text: &[u8]) -> u32 {
    let mut count = 0;
    for (index, &byte) in text.iter().enumerate() {
        if byte == b'\n' || (byte == b'\r' && text.get(index + 1) != Some(&b'\n')) {
            count += 1;
        }
    }
    count
}

/// `text` without one trailing line break.
pub(crate) fn without_final_newline(text: &[u8]) -> &[u8] {
    text.strip_suffix(b"\r\n")
        .or_else(|| text.strip_suffix(b"\n"))
        .or_else(|| text.strip_suffix(b"\r"))
        .unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::{
        Indentation, Number, StripFailure, count_lines, keyword_identifier, last_line_indentation,
        name, number, offset_number, strip_indentation, without_final_newline,
    };
    use crate::ast::NameKind;
    use php_lexer::{LexError, TokenKind};

    #[test]
    fn integers_in_every_base() {
        let int = |text: &str| number(TokenKind::Integer, text.as_bytes());
        assert_eq!(int("123"), Number::Int(123));
        assert_eq!(int("1_000"), Number::Int(1000));
        assert_eq!(int("0x1F"), Number::Int(31));
        assert_eq!(int("0b101"), Number::Int(5));
        assert_eq!(int("0o17"), Number::Int(15));
        assert_eq!(int("017"), Number::Int(15));
        assert_eq!(int("0"), Number::Int(0));
        assert_eq!(int("0x"), Number::Int(0));
        assert_eq!(int("9223372036854775807"), Number::Int(i64::MAX));
    }

    #[test]
    fn floats_and_overflowing_integers() {
        let float = |text: &str| number(TokenKind::Float, text.as_bytes());
        assert_eq!(float("1.5"), Number::Float(1.5));
        assert_eq!(float("1."), Number::Float(1.0));
        assert_eq!(float(".5"), Number::Float(0.5));
        assert_eq!(float("1e3"), Number::Float(1000.0));
        assert_eq!(float("1.e3"), Number::Float(1000.0));
        assert_eq!(float("1_0.5"), Number::Float(10.5));
        assert_eq!(
            float("9223372036854775808"),
            Number::Float(9_223_372_036_854_775_808.0)
        );
        assert_eq!(
            float("0x8000000000000000"),
            Number::Float(9_223_372_036_854_775_808.0)
        );
        assert_eq!(float("0b1"), Number::Float(1.0));
        assert_eq!(
            float("01777777777777777777777"),
            Number::Float(18_446_744_073_709_551_615.0)
        );
        assert_eq!(float("0.5"), Number::Float(0.5));
        assert_eq!(float("0e1"), Number::Float(0.0));
        assert_eq!(float("1e999"), Number::Float(f64::INFINITY));
        assert_eq!(
            number(TokenKind::Integer, b"99999999999999999999"),
            Number::Float(1e20)
        );
        assert_eq!(number(TokenKind::Integer, b"0x"), Number::Int(0));
        assert_eq!(
            number(TokenKind::Integer, b"0xFFFFFFFFFFFFFFFFF"),
            Number::Float(295_147_905_179_352_825_855.0)
        );
    }

    #[test]
    fn offsets_are_integers_only_when_canonical() {
        assert_eq!(offset_number(b"12"), Ok(12));
        assert_eq!(offset_number(b"0"), Ok(0));
        assert_eq!(offset_number(b"012"), Err(b"012".to_vec()));
        assert_eq!(offset_number(b"0x1"), Err(b"0x1".to_vec()));
        assert_eq!(offset_number(b"1_0"), Err(b"1_0".to_vec()));
        assert_eq!(
            offset_number(b"9223372036854775808"),
            Err(b"9223372036854775808".to_vec())
        );
    }

    #[test]
    fn names_drop_their_qualifier() {
        let full = name(TokenKind::FullyQualifiedName, b"\\A\\B");
        assert_eq!(
            (full.kind, full.bytes),
            (NameKind::FullyQualified, b"A\\B".to_vec())
        );
        let relative = name(TokenKind::RelativeName, b"NameSpace\\C");
        assert_eq!(
            (relative.kind, relative.bytes),
            (NameKind::Relative, b"C".to_vec())
        );
        let plain = name(TokenKind::QualifiedName, b"A\\B");
        assert_eq!(
            (plain.kind, plain.bytes),
            (NameKind::Unqualified, b"A\\B".to_vec())
        );
        assert_eq!(keyword_identifier(b"Exit"), Some(b"Exit".to_vec()));
        assert_eq!(keyword_identifier(b"die"), Some(b"die".to_vec()));
        assert_eq!(keyword_identifier(b"<?="), None);
    }

    fn layout(width: usize, at_line_start: bool, at_closing_label: bool) -> Indentation {
        Indentation {
            width,
            uses_spaces: true,
            at_line_start,
            at_closing_label,
        }
    }

    #[test]
    fn strips_indentation_line_by_line() {
        let strip = |text: &[u8], layout| strip_indentation(text, layout);
        assert_eq!(
            strip(b"    a\n      b", layout(4, true, true)),
            Ok(b"a\n  b".to_vec())
        );
        assert_eq!(
            strip(b"    a\n\n    b", layout(4, true, true)),
            Ok(b"a\n\nb".to_vec())
        );
        assert_eq!(
            strip(b"  \n    b", layout(4, true, true)),
            Ok(b"\nb".to_vec())
        );
        assert_eq!(
            strip(b"x\n    b", layout(4, false, true)),
            Ok(b"x\nb".to_vec())
        );
        assert_eq!(strip(b"x", layout(4, false, false)), Ok(b"x".to_vec()));
        assert_eq!(
            strip(b"    a\r\n    b\r", layout(4, true, false)),
            Ok(b"a\r\nb\r".to_vec())
        );
        assert_eq!(strip(b"    a", layout(4, true, false)), Ok(b"a".to_vec()));
        assert_eq!(strip(b"", layout(4, true, true)), Ok(Vec::new()));
        assert_eq!(
            strip(b"    a\n  b", layout(4, true, true)),
            Err(StripFailure {
                error: LexError::BodyIndentation(4),
                lines_before: 1
            })
        );
        assert_eq!(
            strip(b"x\n  ", layout(4, false, false)),
            Err(StripFailure {
                error: LexError::BodyIndentation(4),
                lines_before: 1
            })
        );
        let tabs = Indentation {
            uses_spaces: false,
            ..layout(1, true, true)
        };
        assert_eq!(strip(b"\ta", tabs), Ok(b"a".to_vec()));
        assert_eq!(
            strip(b" a", tabs),
            Err(StripFailure {
                error: LexError::MixedIndentation,
                lines_before: 0
            })
        );
        assert_eq!(
            strip(b"\ta", layout(1, true, true)),
            Err(StripFailure {
                error: LexError::MixedIndentation,
                lines_before: 0
            })
        );
    }

    #[test]
    fn line_counting_and_final_newlines() {
        assert_eq!(count_lines(b"a\nb\r\nc\rd"), 3);
        assert_eq!(without_final_newline(b"a\r\n"), b"a");
        assert_eq!(without_final_newline(b"a\n"), b"a");
        assert_eq!(without_final_newline(b"a\r"), b"a");
        assert_eq!(without_final_newline(b"a"), b"a");
    }

    #[test]
    fn last_line_indentation_reads_after_the_last_break() {
        assert_eq!(last_line_indentation(b"abc"), None);
        assert_eq!(last_line_indentation(b"a\n  b"), Some(((2, true), false)));
        assert_eq!(
            last_line_indentation(b"a\r\n\tb"),
            Some(((1, false), false))
        );
        assert_eq!(last_line_indentation(b"a\n \tb"), Some(((2, false), false)));
        assert_eq!(last_line_indentation(b"a\n  "), Some(((2, true), true)));
        assert_eq!(last_line_indentation(b"a\r"), Some(((0, false), true)));
    }
}
