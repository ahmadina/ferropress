//! Query-context title bytes used before a path lookup.

/// Query-context dash sanitizer (`sanitize_title_for_query`).
///
/// Accent folding and the `sanitize_title` filter are not dispatched. The
/// default dash sanitizer runs in the query context, not save.
#[must_use]
pub(crate) fn sanitize_title_for_query(bytes: &[u8]) -> Box<[u8]> {
    let mut title = preserve_octets(bytes);
    title.retain(|&byte| byte != b'%');
    restore_octets(&mut title);
    if let Ok(text) = std::str::from_utf8(&title) {
        let lowered: String = text.chars().flat_map(char::to_lowercase).collect();
        title = utf8_uri_encode(lowered.as_bytes(), 200);
    }
    for byte in &mut title {
        byte.make_ascii_lowercase();
    }
    title = strip_entities(&title);
    for byte in &mut title {
        if *byte == b'.' {
            *byte = b'-';
        }
    }
    title.retain(|&byte| matches!(byte, b'%' | b'a'..=b'z' | b'0'..=b'9' | b' ' | b'_' | b'-'));
    title = collapse_ws_and_dashes(&title);
    trim_dashes(&title).into()
}

fn preserve_octets(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && bytes.get(index + 1).is_some_and(u8::is_ascii_hexdigit)
            && bytes.get(index + 2).is_some_and(u8::is_ascii_hexdigit)
        {
            out.extend_from_slice(b"---");
            out.push(bytes[index + 1]);
            out.push(bytes[index + 2]);
            out.extend_from_slice(b"---");
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    out
}

fn restore_octets(bytes: &mut Vec<u8>) {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"---")
            && bytes.get(index + 3).is_some_and(u8::is_ascii_hexdigit)
            && bytes.get(index + 4).is_some_and(u8::is_ascii_hexdigit)
            && bytes[index + 5..].starts_with(b"---")
        {
            out.push(b'%');
            out.push(bytes[index + 3]);
            out.push(bytes[index + 4]);
            index += 8;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    *bytes = out;
}

fn utf8_uri_encode(bytes: &[u8], max: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte < 128 {
            if out.len() + 1 > max {
                break;
            }
            out.push(byte);
            index += 1;
            continue;
        }
        let width = utf8_width(byte);
        if out.len() + width * 3 > max {
            break;
        }
        if index + width > bytes.len() {
            break;
        }
        for &part in &bytes[index..index + width] {
            out.push(b'%');
            out.extend_from_slice(format!("{part:x}").as_bytes());
        }
        index += width;
    }
    out
}

fn utf8_width(lead: u8) -> usize {
    if lead < 224 {
        2
    } else if lead < 240 {
        3
    } else {
        4
    }
}

fn strip_entities(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'&'
            && let Some(rel) = bytes[index + 1..].iter().position(|&b| b == b';')
            && !bytes[index + 1..index + 1 + rel].contains(&b'\n')
        {
            index += rel + 2;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    out
}

fn collapse_ws_and_dashes(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut in_ws = false;
    for &byte in bytes {
        if byte.is_ascii_whitespace() {
            if !in_ws {
                out.push(b'-');
                in_ws = true;
            }
            continue;
        }
        in_ws = false;
        if byte == b'-' && out.last() == Some(&b'-') {
            continue;
        }
        out.push(byte);
    }
    out
}

fn trim_dashes(bytes: &[u8]) -> &[u8] {
    let start = bytes.iter().position(|&b| b != b'-').unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|&b| b != b'-')
        .map_or(start, |index| index + 1);
    if start >= end {
        &[]
    } else {
        &bytes[start..end]
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize_title_for_query;

    #[test]
    fn ascii_query_titles() {
        assert_eq!(&*sanitize_title_for_query(b"about"), b"about");
        assert_eq!(&*sanitize_title_for_query(b"Hello World"), b"hello-world");
        assert_eq!(&*sanitize_title_for_query(b"foo.bar"), b"foo-bar");
        assert_eq!(&*sanitize_title_for_query(b"0"), b"0");
        assert_eq!(&*sanitize_title_for_query(b""), b"");
        assert_eq!(&*sanitize_title_for_query(b"---"), b"");
        assert_eq!(&*sanitize_title_for_query(b"a&amp;b"), b"ab");
        assert_eq!(&*sanitize_title_for_query(b"a%2fb"), b"a%2fb");
        assert_eq!(&*sanitize_title_for_query(b"a%b"), b"ab");
        assert_eq!(&*sanitize_title_for_query(b"Hello!"), b"hello");
        assert_eq!(&*sanitize_title_for_query(b" a--b "), b"a-b");
    }

    #[test]
    fn utf8_is_percent_encoded() {
        assert_eq!(&*sanitize_title_for_query("é".as_bytes()), b"%c3%a9");
        assert_eq!(&*sanitize_title_for_query(&[0xff, b'a']), b"a");
        assert_eq!(&*sanitize_title_for_query("€".as_bytes()), b"%e2%82%ac");
        assert_eq!(&*sanitize_title_for_query(b"a&\nb;c"), b"abc");
    }
}
