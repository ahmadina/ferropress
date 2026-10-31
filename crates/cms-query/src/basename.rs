//! Last path segment (`wp_basename`) before an attachment slug lookup.

const UNRESERVED: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._";

/// Last segment after `wp_basename` (no suffix).
#[must_use]
pub(crate) fn wp_basename(path: &[u8]) -> Box<[u8]> {
    let encoded = urlencode(path);
    let slashed = replace_encoded_slashes(&encoded);
    let base = last_slash(&slashed);
    urldecode(base).into()
}

fn urlencode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    for &byte in bytes {
        if UNRESERVED.contains(&byte) {
            out.push(byte);
        } else if byte == b' ' {
            out.push(b'+');
        } else {
            out.extend_from_slice(format!("%{byte:02X}").as_bytes());
        }
    }
    out
}

fn replace_encoded_slashes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"%2F") || bytes[index..].starts_with(b"%5C") {
            out.push(b'/');
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    out
}

fn last_slash(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|&b| b != b'/')
        .map_or(0, |index| index + 1);
    let start = bytes[..end]
        .iter()
        .rposition(|&b| b == b'/')
        .map_or(0, |index| index + 1);
    if start >= end {
        &[]
    } else {
        &bytes[start..end]
    }
}

fn urldecode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if bytes.get(index + 1).is_some_and(u8::is_ascii_hexdigit)
                && bytes.get(index + 2).is_some_and(u8::is_ascii_hexdigit) =>
            {
                out.push(hex_byte(bytes[index + 1], bytes[index + 2]));
                index += 3;
            }
            other => {
                out.push(other);
                index += 1;
            }
        }
    }
    out
}

fn hex_byte(hi: u8, lo: u8) -> u8 {
    (hex_val(hi) << 4) | hex_val(lo)
}

fn hex_val(byte: u8) -> u8 {
    match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::wp_basename;

    #[test]
    fn last_segment_and_encoded_slashes() {
        assert_eq!(&*wp_basename(b"file"), b"file");
        assert_eq!(&*wp_basename(b"dir/file"), b"file");
        assert_eq!(&*wp_basename(b"dir/file/"), b"file");
        assert_eq!(&*wp_basename(b""), b"");
        assert_eq!(&*wp_basename(b"a%2Fb"), b"a%2Fb");
        assert_eq!(&*wp_basename(b"a%5Cb"), b"a%5Cb");
        assert_eq!(&*wp_basename(b"a b/c d"), b"c d");
    }
}
