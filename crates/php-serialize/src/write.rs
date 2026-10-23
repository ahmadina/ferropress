//! Token-by-token writer for the serialized format.

use crate::float::write_float;

/// Writes values in PHP's serialized format, one token at a time, into an owned buffer.
///
/// The writer emits exactly the bytes PHP's `serialize()` produces for each token. It does not
/// track nesting: after [`Writer::array`] or [`Writer::object`] announces `len` entries, the caller
/// writes `len` key/value pairs (keys with [`Writer::int`] or [`Writer::string`]) and then calls
/// [`Writer::end`]. Reference numbers are 1-based positions in PHP's value numbering.
///
/// # Examples
///
/// ```
/// use php_serialize::Writer;
///
/// let mut writer = Writer::new();
/// writer.array(2);
/// writer.int(0);
/// writer.string(b"one");
/// writer.string(b"pi");
/// writer.float(3.14);
/// writer.end();
/// assert_eq!(writer.as_bytes(), br#"a:2:{i:0;s:3:"one";s:2:"pi";d:3.14;}"#);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Writer {
    out: Vec<u8>,
}

impl Writer {
    /// Creates an empty writer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an empty writer whose buffer can hold `capacity` bytes without reallocating.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            out: Vec::with_capacity(capacity),
        }
    }

    /// The bytes written so far.
    pub fn as_bytes(&self) -> &[u8] {
        &self.out
    }

    /// Consumes the writer and returns its bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.out
    }

    /// Writes `N;`.
    pub fn null(&mut self) {
        self.out.extend_from_slice(b"N;");
    }

    /// Writes `b:0;` or `b:1;`.
    pub fn bool(&mut self, value: bool) {
        self.out
            .extend_from_slice(if value { b"b:1;" } else { b"b:0;" });
    }

    /// Writes `i:<value>;`; also used for integer keys.
    pub fn int(&mut self, value: i64) {
        self.out.extend_from_slice(b"i:");
        push_signed(&mut self.out, value);
        self.out.push(b';');
    }

    /// Writes `d:<value>;` in PHP's shortest round-trip notation.
    pub fn float(&mut self, value: f64) {
        self.out.extend_from_slice(b"d:");
        write_float(&mut self.out, value);
        self.out.push(b';');
    }

    /// Writes `s:<len>:"<bytes>";`; also used for string keys.
    pub fn string(&mut self, value: &[u8]) {
        self.out.extend_from_slice(b"s:");
        self.quoted(value);
        self.out.push(b';');
    }

    /// Opens an array of `len` entries: `a:<len>:{`.
    pub fn array(&mut self, len: usize) {
        self.out.extend_from_slice(b"a:");
        push_decimal(&mut self.out, len as u64);
        self.out.extend_from_slice(b":{");
    }

    /// Opens an object of class `class` with `len` properties: `O:<n>:"<class>":<len>:{`.
    pub fn object(&mut self, class: &[u8], len: usize) {
        self.out.extend_from_slice(b"O:");
        self.quoted(class);
        self.out.push(b':');
        push_decimal(&mut self.out, len as u64);
        self.out.extend_from_slice(b":{");
    }

    /// Closes the innermost array or object: `}`.
    pub fn end(&mut self) {
        self.out.push(b'}');
    }

    /// Writes an object with a class-defined payload: `C:<n>:"<class>":<len>:{<payload>}`.
    pub fn custom(&mut self, class: &[u8], payload: &[u8]) {
        self.out.extend_from_slice(b"C:");
        self.quoted(class);
        self.out.push(b':');
        push_decimal(&mut self.out, payload.len() as u64);
        self.out.extend_from_slice(b":{");
        self.out.extend_from_slice(payload);
        self.out.push(b'}');
    }

    /// Writes an enum case: `E:<n>:"<class>:<case>";`.
    pub fn enum_case(&mut self, class: &[u8], case: &[u8]) {
        self.out.extend_from_slice(b"E:");
        push_decimal(&mut self.out, (class.len() + 1 + case.len()) as u64);
        self.out.extend_from_slice(b":\"");
        self.out.extend_from_slice(class);
        self.out.push(b':');
        self.out.extend_from_slice(case);
        self.out.extend_from_slice(b"\";");
    }

    /// Writes a PHP reference to an earlier value: `R:<number>;`.
    pub fn reference(&mut self, number: usize) {
        self.out.extend_from_slice(b"R:");
        push_decimal(&mut self.out, number as u64);
        self.out.push(b';');
    }

    /// Writes a repeated occurrence of an earlier object: `r:<number>;`.
    pub fn object_reference(&mut self, number: usize) {
        self.out.extend_from_slice(b"r:");
        push_decimal(&mut self.out, number as u64);
        self.out.push(b';');
    }

    /// Writes `<len>:"<bytes>"`.
    fn quoted(&mut self, bytes: &[u8]) {
        push_decimal(&mut self.out, bytes.len() as u64);
        self.out.extend_from_slice(b":\"");
        self.out.extend_from_slice(bytes);
        self.out.push(b'"');
    }
}

/// Appends `value` in decimal without allocating.
pub(crate) fn push_decimal(out: &mut Vec<u8>, mut value: u64) {
    let mut buffer = [0_u8; 20];
    let mut start = buffer.len();
    loop {
        start -= 1;
        buffer[start] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    out.extend_from_slice(&buffer[start..]);
}

/// Appends a signed decimal.
fn push_signed(out: &mut Vec<u8>, value: i64) {
    if value < 0 {
        out.push(b'-');
    }
    push_decimal(out, value.unsigned_abs());
}

#[cfg(test)]
mod tests {
    use super::{Writer, push_decimal};

    #[test]
    fn scalars_use_php_token_forms() {
        let mut writer = Writer::with_capacity(64);
        writer.null();
        writer.bool(true);
        writer.bool(false);
        writer.int(0);
        writer.int(-42);
        writer.int(i64::MIN);
        writer.int(i64::MAX);
        writer.float(-0.0);
        writer.string(b"");
        writer.string(b"a\"b\0");
        assert_eq!(
            writer.as_bytes(),
            b"N;b:1;b:0;i:0;i:-42;i:-9223372036854775808;i:9223372036854775807;d:-0;\
              s:0:\"\";s:4:\"a\"b\0\";"
        );
    }

    #[test]
    fn containers_and_references_use_php_token_forms() {
        let mut writer = Writer::new();
        writer.object(b"Shop\\Item", 3);
        writer.string(b"\0*\0tags");
        writer.array(0);
        writer.end();
        writer.int(7);
        writer.custom(b"Blob", b"x:{}");
        writer.string(b"suit");
        writer.enum_case(b"Suit", b"Hearts");
        writer.end();
        writer.reference(2);
        writer.object_reference(1);
        assert_eq!(
            writer.into_bytes(),
            b"O:9:\"Shop\\Item\":3:{s:7:\"\0*\0tags\";a:0:{}i:7;C:4:\"Blob\":4:{x:{}}\
              s:4:\"suit\";E:11:\"Suit:Hearts\";}R:2;r:1;"
        );
    }

    #[test]
    fn decimals_cover_the_full_range() {
        let mut out = Vec::new();
        push_decimal(&mut out, 0);
        out.push(b' ');
        push_decimal(&mut out, u64::MAX);
        assert_eq!(out, b"0 18446744073709551615");
    }
}
