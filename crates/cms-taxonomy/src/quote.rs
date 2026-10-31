//! `MySQL` string literals matching the bytes the lab actually sent.

/// `'…'` with the escapes `$wpdb->prepare` applies before the query runs.
#[must_use]
pub(crate) fn quote(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 2);
    out.push(b'\'');
    for &byte in bytes {
        match byte {
            0 => out.extend_from_slice(br"\0"),
            b'\n' => out.extend_from_slice(br"\n"),
            b'\r' => out.extend_from_slice(br"\r"),
            0x1a => out.extend_from_slice(br"\Z"),
            b'\\' => out.extend_from_slice(br"\\"),
            b'\'' => {
                out.push(b'\\');
                out.push(b'\'');
            }
            b'"' => {
                out.push(b'\\');
                out.push(b'"');
            }
            other => out.push(other),
        }
    }
    out.push(b'\'');
    out
}

#[cfg(test)]
mod tests {
    use super::quote;

    #[test]
    fn literals_match_the_lab_prepare_then_execute_bytes() {
        assert_eq!(quote(b"home"), br"'home'");
        assert_eq!(quote(b""), br"''");
        assert_eq!(quote(b"a'b"), br"'a\'b'");
        assert_eq!(quote(b"a\"b"), b"'a\\\"b'");
        assert_eq!(quote(b"a\\b"), br"'a\\b'");
        assert_eq!(quote(b"a%b"), br"'a%b'");
        assert_eq!(quote(b"a`b"), br"'a`b'");
        assert_eq!(quote(b"a\0b"), br"'a\0b'");
        assert_eq!(quote(b"a\nb"), br"'a\nb'");
        assert_eq!(quote(b"a\rb"), br"'a\rb'");
        assert_eq!(quote(&[b'a', 0x1a, b'b']), br"'a\Zb'");
        assert_eq!(quote(b"a\tb"), b"'a\tb'");
        assert_eq!(quote("caf\u{e9}".as_bytes()), "'caf\u{e9}'".as_bytes());
    }

    proptest::proptest! {
        #[test]
        fn quoted_text_stays_wrapped_and_round_trips_plain_ascii(
            text in "[A-Za-z0-9._-]{0,32}",
        ) {
            let quoted = quote(text.as_bytes());
            proptest::prop_assert!(quoted.first() == Some(&b'\''));
            proptest::prop_assert!(quoted.last() == Some(&b'\''));
            proptest::prop_assert_eq!(&quoted[1..quoted.len() - 1], text.as_bytes());
        }
    }
}
