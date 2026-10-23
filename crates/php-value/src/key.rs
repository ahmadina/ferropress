//! Array keys and PHP's integer-string rule.

use crate::PhpString;

/// The key of an array element: an integer or a string.
///
/// A string that is the canonical decimal form of a signed 64-bit integer (an optional `-`, no leading
/// zeros, not `-0`) is always stored as that integer, so `"5"` and `5` name the same element while
/// `"05"`, `"-0"`, `" 5"` and `"9223372036854775808"` stay strings.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ArrayKey {
    /// An integer key.
    Int(i64),
    /// A string key that is not a canonical integer.
    String(PhpString),
}

impl ArrayKey {
    /// The key for `bytes`, as an integer when they are a canonical integer.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        canonical_integer(bytes).map_or_else(|| Self::String(PhpString::new(bytes)), Self::Int)
    }

    /// The key for `text`, reusing its storage when it stays a string.
    pub fn from_string(text: PhpString) -> Self {
        canonical_integer(text.as_bytes()).map_or(Self::String(text), Self::Int)
    }
}

impl From<i64> for ArrayKey {
    fn from(number: i64) -> Self {
        Self::Int(number)
    }
}

impl From<PhpString> for ArrayKey {
    fn from(text: PhpString) -> Self {
        Self::from_string(text)
    }
}

impl From<&str> for ArrayKey {
    fn from(text: &str) -> Self {
        Self::from_bytes(text.as_bytes())
    }
}

/// The integer `bytes` spell in canonical decimal form, if any.
fn canonical_integer(bytes: &[u8]) -> Option<i64> {
    let (negative, digits) = match bytes.split_first() {
        Some((b'-', rest)) => (true, rest),
        _ => (false, bytes),
    };
    let leading_zero = digits.len() > 1 && digits[0] == b'0';
    if digits.is_empty() || leading_zero || (negative && digits == b"0") {
        return None;
    }
    // Accumulate downwards so that the most negative value is reachable.
    let mut value: i64 = 0;
    for &byte in digits {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_sub(i64::from(byte - b'0'))?;
    }
    if negative {
        Some(value)
    } else {
        value.checked_neg()
    }
}

#[cfg(test)]
mod tests {
    use super::ArrayKey;
    use crate::PhpString;
    use proptest::prelude::*;

    #[test]
    fn canonical_integers_become_integer_keys() {
        let integers = [
            ("0", 0),
            ("-1", -1),
            ("42", 42),
            ("9223372036854775807", i64::MAX),
            ("-9223372036854775808", i64::MIN),
        ];
        for (text, number) in integers {
            assert_eq!(ArrayKey::from(text), ArrayKey::Int(number), "{text}");
        }
    }

    #[test]
    fn other_strings_stay_strings() {
        let strings = [
            "",
            "-",
            "-0",
            "00",
            "01",
            "-01",
            " 1",
            "1 ",
            "+1",
            "1e3",
            "1.0",
            "0x1",
            "١",
            "9223372036854775808",
            "-9223372036854775809",
            "99999999999999999999",
        ];
        for text in strings {
            assert_eq!(
                ArrayKey::from(text),
                ArrayKey::String(PhpString::from(text)),
                "{text}"
            );
        }
    }

    #[test]
    fn string_keys_reuse_their_storage() {
        let text = PhpString::from("name");
        let ArrayKey::String(stored) = ArrayKey::from(text.clone()) else {
            panic!("a name is a string key");
        };
        assert!(std::ptr::eq(stored.as_bytes(), text.as_bytes()));
        assert_eq!(ArrayKey::from(7), ArrayKey::Int(7));
        assert_eq!(ArrayKey::from(PhpString::from("7")), ArrayKey::Int(7));
    }

    proptest! {
        #[test]
        fn every_integer_round_trips_through_its_decimal_form(number in any::<i64>()) {
            prop_assert_eq!(ArrayKey::from(number.to_string().as_str()), ArrayKey::Int(number));
        }

        #[test]
        fn integer_keys_are_exactly_the_canonical_forms(bytes in proptest::collection::vec(any::<u8>(), 0..24)) {
            let key = ArrayKey::from_bytes(&bytes);
            let canonical = std::str::from_utf8(&bytes)
                .ok()
                .and_then(|text| text.parse::<i64>().ok())
                .filter(|number| number.to_string().as_bytes() == bytes.as_slice());
            let expected = canonical.map_or_else(|| ArrayKey::String(PhpString::new(&bytes)), ArrayKey::Int);
            prop_assert_eq!(key, expected);
        }
    }
}
