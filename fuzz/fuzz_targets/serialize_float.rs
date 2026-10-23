//! Every `f64` bit pattern through the writer and back through the reader.
//!
//! The written `d:` token must read back as the same bits (any NaN as a NaN), and a finite value's text
//! must carry exactly as many significant digits as Rust's shortest round-trip form.

#![no_main]

use libfuzzer_sys::fuzz_target;
use php_serialize::{Value, Writer, decode};

/// Significant digits of a decimal mantissa: digits before any exponent, without leading or trailing zeros.
fn significant(text: &[u8]) -> &[u8] {
    let mantissa_end = text
        .iter()
        .position(|&b| b == b'e' || b == b'E')
        .unwrap_or(text.len());
    let mantissa = &text[..mantissa_end];
    let first = mantissa.iter().position(|&b| (b'1'..=b'9').contains(&b));
    let last = mantissa.iter().rposition(|&b| (b'1'..=b'9').contains(&b));
    match (first, last) {
        (Some(first), Some(last)) => &mantissa[first..=last],
        _ => &[],
    }
}

fn digit_count(text: &[u8]) -> usize {
    significant(text)
        .iter()
        .filter(|b| b.is_ascii_digit())
        .count()
}

fuzz_target!(|bits: u64| {
    let value = f64::from_bits(bits);
    let mut writer = Writer::new();
    writer.float(value);
    let written = writer.as_bytes();
    let document = decode(written);
    assert!(document.is_ok(), "written float must decode: {document:?}");
    let Ok(document) = document else {
        return;
    };
    let root = document.root();
    assert!(
        matches!(root, Value::Float(_)),
        "a d: token reads as a float"
    );
    let Value::Float(read) = root else {
        return;
    };
    if value.is_nan() {
        assert!(read.is_nan());
        return;
    }
    assert_eq!(read.to_bits(), bits);
    if value.is_finite() {
        let text = &written[2..written.len() - 1];
        let shortest = format!("{value:e}");
        assert_eq!(
            digit_count(text),
            digit_count(shortest.as_bytes()),
            "{} against {shortest}",
            String::from_utf8_lossy(text)
        );
    }
});
