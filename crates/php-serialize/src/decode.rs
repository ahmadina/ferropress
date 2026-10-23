//! Decoder for the serialized format.
//!
//! Reproduces which inputs PHP's `unserialize()` accepts, the offset it reports when it refuses
//! one, and the diagnostics it emits on the way, for every rule that depends only on the bytes.
//! Classes are treated as PHP treats them when none may be instantiated (`allowed_classes` set to
//! `false`): names must be well formed, objects keep their written properties, and `C:` payloads
//! stay uninterpreted. Class lookup, magic methods and reference resolution belong to the runtime.

use std::borrow::Cow;

use crate::document::{Document, KeyNode, Node};
use crate::error::{DecodeError, Diagnostic};

/// PHP's default `unserialize_max_depth`.
const DEFAULT_MAX_DEPTH: usize = 4096;
/// The most entries one PHP hash table can hold on 64-bit builds.
const MAX_TABLE_SIZE: u64 = 0x4000_0000;
/// The most significant digits a 64-bit integer token may have before it is out of range.
const MAX_INTEGER_DIGITS: usize = 19;

/// Options that change which inputs are accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DecodeOptions {
    /// How deeply arrays and objects may nest; `0` means unlimited. Defaults to 4096, PHP's
    /// default `unserialize_max_depth`.
    pub max_depth: usize,
}

impl DecodeOptions {
    /// Returns these options with `max_depth` replaced.
    #[must_use]
    pub const fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }
}

impl Default for DecodeOptions {
    fn default() -> Self {
        Self {
            max_depth: DEFAULT_MAX_DEPTH,
        }
    }
}

/// Decodes one serialized value with the default options.
///
/// Bytes after the value are left unread; compare [`Document::consumed`] with the input length to
/// detect them.
///
/// # Errors
///
/// Returns a [`DecodeError`] carrying PHP's error offset when PHP would refuse the input.
///
/// # Examples
///
/// ```
/// use php_serialize::{Key, Value, decode};
///
/// let document = decode(br#"a:1:{s:4:"name";s:3:"Ada";}"#).unwrap();
/// let Value::Array(mut entries) = document.root() else { unreachable!() };
/// assert_eq!(entries.next(), Some((Key::String(b"name"), Value::String(b"Ada"))));
/// assert_eq!(decode(b"i:5").unwrap_err().offset(), 0);
/// ```
pub fn decode(input: &[u8]) -> Result<Document<'_>, DecodeError<'_>> {
    decode_with(input, DecodeOptions::default())
}

/// Decodes one serialized value with explicit options.
///
/// # Errors
///
/// Returns a [`DecodeError`] carrying PHP's error offset when PHP would refuse the input.
pub fn decode_with(input: &[u8], options: DecodeOptions) -> Result<Document<'_>, DecodeError<'_>> {
    let mut decoder = Decoder {
        input,
        pos: 0,
        max_depth: options.max_depth,
        depth: 0,
        nodes: Vec::new(),
        keys: Vec::new(),
        slots: Vec::new(),
        frames: Vec::new(),
        diagnostics: Vec::new(),
    };
    match decoder.run() {
        Ok(()) => Ok(Document::new(
            decoder.nodes,
            decoder.keys,
            decoder.pos,
            decoder.diagnostics,
        )),
        Err(offset) => Err(DecodeError::new(offset, decoder.diagnostics)),
    }
}

/// Where a value is stored: the container node and the index of its key in `keys`.
type Bucket = Option<(usize, usize)>;

/// One numbered value that `R:` and `r:` may refer to.
#[derive(Debug, Clone, Copy)]
struct Slot {
    object: bool,
    bucket: Bucket,
}

/// An array or object whose entries are still being read.
#[derive(Debug, Clone, Copy)]
struct Frame {
    node: usize,
    remaining: usize,
}

/// A key normalized the way PHP compares keys of one container.
#[derive(Debug, PartialEq, Eq)]
enum NormalKey<'k> {
    Int(i64),
    String(&'k [u8]),
}

#[derive(Debug)]
struct Decoder<'a> {
    input: &'a [u8],
    /// PHP's read position; failures report the position each rule leaves it at.
    pos: usize,
    max_depth: usize,
    depth: usize,
    nodes: Vec<Node<'a>>,
    keys: Vec<KeyNode<'a>>,
    slots: Vec<Slot>,
    frames: Vec<Frame>,
    diagnostics: Vec<Diagnostic<'a>>,
}

/// The error offset of a refused input.
type Step<T = ()> = Result<T, usize>;

impl<'a> Decoder<'a> {
    fn run(&mut self) -> Step {
        self.value(None)?;
        while let Some(frame) = self.frames.last_mut() {
            if frame.remaining == 0 {
                let node = frame.node;
                self.frames.pop();
                self.depth -= 1;
                self.close(node)?;
                continue;
            }
            frame.remaining -= 1;
            let node = frame.node;
            let key = self.key()?;
            self.keys.push(key);
            self.value(Some((node, self.keys.len() - 1)))?;
        }
        Ok(())
    }

    /// Reads the closing `}` of `node` and records where its subtree ends.
    fn close(&mut self, node: usize) -> Step {
        if self.at(self.pos) != b'}' || self.pos >= self.input.len() {
            return Err(self.pos);
        }
        self.pos += 1;
        let after = self.nodes.len();
        if let Some(Node::Array { end, .. } | Node::Object { end, .. }) = self.nodes.get_mut(node) {
            *end = after;
        }
        Ok(())
    }

    /// The byte at `index`, or NUL past the end, as PHP's terminated strings read.
    fn at(&self, index: usize) -> u8 {
        self.input.get(index).copied().unwrap_or(0)
    }

    fn digits_end(&self, mut index: usize) -> usize {
        while self.at(index).is_ascii_digit() {
            index += 1;
        }
        index
    }

    /// The end of one or more digits starting at `index`.
    fn unsigned_end(&self, index: usize) -> Option<usize> {
        let end = self.digits_end(index);
        (end > index).then_some(end)
    }

    /// The end of an optionally signed run of digits starting at `index`.
    fn signed_end(&self, index: usize) -> Option<usize> {
        let digits = if matches!(self.at(index), b'+' | b'-') {
            index + 1
        } else {
            index
        };
        self.unsigned_end(digits)
    }

    /// Reads digits as an unsigned machine word, wrapping on overflow as PHP's length parser does.
    fn parse_size(&self, index: usize) -> u64 {
        let end = self.digits_end(index);
        self.input[index..end].iter().fold(0_u64, |acc, digit| {
            acc.wrapping_mul(10).wrapping_add(u64::from(digit - b'0'))
        })
    }

    /// Reads an optionally signed integer, clamping with a diagnostic when it is out of range.
    /// Returns the value and the position after the digits; no digits read as zero.
    fn parse_long(&mut self, index: usize) -> (i64, usize) {
        let negative = self.at(index) == b'-';
        let mut cursor = if negative || self.at(index) == b'+' {
            index + 1
        } else {
            index
        };
        while self.at(cursor) == b'0' {
            cursor += 1;
        }
        let end = self.digits_end(cursor);
        let magnitude = self.input[cursor..end].iter().fold(0_u64, |acc, digit| {
            acc.wrapping_mul(10).wrapping_add(u64::from(digit - b'0'))
        });
        let limit = i64::MAX.unsigned_abs() + u64::from(negative);
        if end - cursor > MAX_INTEGER_DIGITS || magnitude > limit {
            self.diagnostics.push(Diagnostic::NumberOutOfRange);
            return (if negative { i64::MIN } else { i64::MAX }, end);
        }
        let value = if negative {
            0_i64.wrapping_sub_unsigned(magnitude)
        } else {
            0_i64.wrapping_add_unsigned(magnitude)
        };
        (value, end)
    }

    fn push(&mut self, node: Node<'a>) {
        self.nodes.push(node);
    }

    fn mark_object(&mut self, slot: usize) {
        if let Some(entry) = slot.checked_sub(1).and_then(|i| self.slots.get_mut(i)) {
            entry.object = true;
        }
    }

    /// Reads one value stored in `bucket` (`None` for the top-level value).
    fn value(&mut self, bucket: Bucket) -> Step {
        let start = self.pos;
        if start >= self.input.len() {
            return Err(start);
        }
        let tag = self.input[start];
        if tag != b'R' {
            self.slots.push(Slot {
                object: false,
                bucket,
            });
        }
        let slot = self.slots.len();
        match (tag, self.at(start + 1)) {
            (b'N', b';') => {
                self.pos = start + 2;
                self.push(Node::Null);
            }
            (b'b', b':') if self.bool_end(start).is_some() => {
                self.pos = start + 4;
                self.push(Node::Bool(self.at(start + 2) == b'1'));
            }
            (b'i', b':') => {
                let value = self.int_token(start)?;
                self.push(Node::Int(value));
            }
            (b'd', b':') => {
                let (value, end) = self.float_token(start).ok_or(start)?;
                self.pos = end;
                self.push(Node::Float(value));
            }
            (b's', b':') => {
                let bytes = self.plain_string(start)?;
                self.push(Node::String(Cow::Borrowed(bytes)));
            }
            (b'S', b':') => {
                let bytes = self.escaped_string(start)?;
                self.push(Node::String(Cow::Owned(bytes)));
            }
            (b'a', b':') => self.array(start)?,
            (b'O' | b'C', b':') => self.object(start, tag == b'C', slot)?,
            (b'E', b':') => {
                let (class, case) = self.enum_token(start)?;
                self.mark_object(slot);
                self.push(Node::Enum { class, case });
            }
            (b'R' | b'r', b':') => self.reference(start, tag == b'r', bucket, slot)?,
            (b'}', _) => {
                self.diagnostics.push(Diagnostic::UnexpectedEnd);
                return Err(start);
            }
            _ => return Err(start),
        }
        Ok(())
    }

    /// Reads one array key or property name. Only integer and string tokens are keys; any other
    /// token that PHP reads fully is refused at the position after it.
    fn key(&mut self) -> Step<KeyNode<'a>> {
        let start = self.pos;
        if start >= self.input.len() {
            return Err(start);
        }
        match (self.input[start], self.at(start + 1)) {
            (b'i', b':') => Ok(KeyNode::Int(self.int_token(start)?)),
            (b's', b':') => Ok(KeyNode::String(Cow::Borrowed(self.plain_string(start)?))),
            (b'S', b':') => Ok(KeyNode::String(Cow::Owned(self.escaped_string(start)?))),
            (b'N', b';') => Err(start + 2),
            (b'b', b':') => Err(self.bool_end(start).ok_or(start)?),
            (b'd', b':') => Err(self.float_token(start).ok_or(start)?.1),
            (b'R' | b'r', b':') => Err(self.reference_end(start).ok_or(start)?),
            (b'a', b':') => {
                let open = self.array_header(start).ok_or(start)?;
                self.parse_long(start + 2);
                Err(open)
            }
            (b'}', _) => {
                self.diagnostics.push(Diagnostic::UnexpectedEnd);
                Err(start)
            }
            _ => Err(start),
        }
    }

    fn bool_end(&self, start: usize) -> Option<usize> {
        (matches!(self.at(start + 2), b'0' | b'1') && self.at(start + 3) == b';')
            .then_some(start + 4)
    }

    /// `i:<signed>;`
    fn int_token(&mut self, start: usize) -> Step<i64> {
        let end = self.signed_end(start + 2).ok_or(start)?;
        if self.at(end) != b';' {
            return Err(start);
        }
        let (value, _) = self.parse_long(start + 2);
        self.pos = end + 1;
        Ok(value)
    }

    /// `d:NAN;`, `d:INF;`, `d:-INF;` or `d:<decimal>;`; returns the value and the end of the token.
    fn float_token(&self, start: usize) -> Option<(f64, usize)> {
        let body = start + 2;
        let rest = &self.input[body.min(self.input.len())..];
        for (text, value) in [
            (&b"NAN;"[..], f64::NAN),
            (b"INF;", f64::INFINITY),
            (b"-INF;", f64::NEG_INFINITY),
        ] {
            if rest.starts_with(text) {
                return Some((value, body + text.len()));
            }
        }
        let digits = if matches!(self.at(body), b'+' | b'-') {
            body + 1
        } else {
            body
        };
        let mut cursor = self.digits_end(digits);
        let mut count = cursor - digits;
        if self.at(cursor) == b'.' {
            let fraction = self.digits_end(cursor + 1);
            count += fraction - cursor - 1;
            cursor = fraction;
        }
        if count == 0 {
            return None;
        }
        if matches!(self.at(cursor), b'e' | b'E') {
            cursor = self.signed_end(cursor + 1)?;
        }
        if self.at(cursor) != b';' {
            return None;
        }
        let text = std::str::from_utf8(&self.input[body..cursor]).ok()?;
        Some((text.parse().ok()?, cursor + 1))
    }

    /// Reads `<len>:"` at `start + 2` and checks that `len` bytes remain; returns the payload start
    /// and length.
    fn length_header(&self, start: usize) -> Step<(usize, usize)> {
        let length_end = self.unsigned_end(start + 2).ok_or(start)?;
        if self.at(length_end) != b':' || self.at(length_end + 1) != b'"' {
            return Err(start);
        }
        let payload = length_end + 2;
        let length = self.parse_size(start + 2);
        let available = self.input.len() - payload;
        match usize::try_from(length) {
            Ok(length) if length <= available => Ok((payload, length)),
            _ => Err(start + 2),
        }
    }

    /// Checks the `"` and `terminator` that follow a length-prefixed text ending at `end`.
    fn close_quote(&self, end: usize, terminator: u8) -> Step {
        if self.at(end) != b'"' {
            return Err(end);
        }
        if self.at(end + 1) != terminator {
            return Err(end + 1);
        }
        Ok(())
    }

    /// `s:<len>:"<bytes>";`
    fn plain_string(&mut self, start: usize) -> Step<&'a [u8]> {
        let (payload, length) = self.length_header(start)?;
        let end = payload + length;
        self.close_quote(end, b';')?;
        self.pos = end + 2;
        Ok(&self.input[payload..end])
    }

    /// `S:<len>:"<bytes with \xx escapes>";`
    fn escaped_string(&mut self, start: usize) -> Step<Vec<u8>> {
        let (payload, length) = self.length_header(start)?;
        let mut bytes = Vec::with_capacity(length);
        let mut cursor = payload;
        for _ in 0..length {
            let byte = *self.input.get(cursor).ok_or(start)?;
            if byte == b'\\' {
                let high = hex_value(self.at(cursor + 1)).ok_or(start)?;
                let low = hex_value(self.at(cursor + 2)).ok_or(start)?;
                bytes.push((high << 4) | low);
                cursor += 3;
            } else {
                bytes.push(byte);
                cursor += 1;
            }
        }
        self.close_quote(cursor, b';')?;
        self.pos = cursor + 2;
        self.diagnostics.push(Diagnostic::EscapedStringDeprecated);
        Ok(bytes)
    }

    /// The position after `a:<len>:{`, if the header is well formed.
    fn array_header(&self, start: usize) -> Option<usize> {
        let count_end = self.unsigned_end(start + 2)?;
        (self.at(count_end) == b':' && self.at(count_end + 1) == b'{').then_some(count_end + 2)
    }

    /// `a:<len>:{...}`
    fn array(&mut self, start: usize) -> Step {
        let open = self.array_header(start).ok_or(start)?;
        let (elements, _) = self.parse_long(start + 2);
        let remaining = self.input.len() - open;
        let len = match usize::try_from(elements) {
            Ok(len) if (len as u64) < MAX_TABLE_SIZE && len <= remaining / 2 => len,
            _ => return Err(open),
        };
        self.pos = open;
        let node = self.nodes.len();
        self.push(Node::Array { len, end: 0 });
        if len == 0 {
            return self.close(node);
        }
        self.enter(node, len, open)
    }

    /// Starts reading the entries of `node`, unless that would exceed the depth limit.
    fn enter(&mut self, node: usize, remaining: usize, offset: usize) -> Step {
        if self.max_depth > 0 && self.depth >= self.max_depth {
            self.diagnostics.push(Diagnostic::MaxDepthExceeded {
                max_depth: self.max_depth,
            });
            return Err(offset);
        }
        self.depth += 1;
        self.frames.push(Frame { node, remaining });
        Ok(())
    }

    /// `O:<n>:"<class>":<len>:{...}` or `C:<n>:"<class>":<len>:{<payload>}`
    fn object(&mut self, start: usize, custom: bool, slot: usize) -> Step {
        let (name, length) = self.length_header(start)?;
        if length == 0 {
            return Err(start + 2);
        }
        let quote = name + length;
        self.close_quote(quote, b':')?;
        let class = &self.input[name..quote];
        if class.first() == Some(&b'\\') || !is_valid_class_name(class) {
            return Err(start);
        }
        if custom {
            let payload = self.custom_payload(quote)?;
            self.mark_object(slot);
            self.push(Node::Custom { class, payload });
            return Ok(());
        }
        if quote + 2 >= self.input.len() {
            self.diagnostics.push(Diagnostic::BadData);
            return Err(quote);
        }
        let (elements, count_end) = self.parse_long(quote + 2);
        let len = match usize::try_from(elements) {
            Ok(len) if len <= (self.input.len() - quote) / 2 => len,
            _ => return Err(count_end),
        };
        if self.at(count_end) != b':' {
            return Err(count_end);
        }
        if self.at(count_end + 1) != b'{' {
            return Err(count_end + 1);
        }
        let body = count_end + 2;
        self.pos = body;
        self.mark_object(slot);
        let node = self.nodes.len();
        self.push(Node::Object { class, len, end: 0 });
        self.enter(node, len, body)
    }

    /// `:<len>:{<payload>}` after a `C:` class name whose closing quote is at `quote`.
    fn custom_payload(&mut self, quote: usize) -> Step<&'a [u8]> {
        let (declared, cursor) = self.parse_long(quote + 2);
        if self.input.len() - cursor < 2 || self.at(cursor) != b':' {
            return Err(cursor);
        }
        if self.at(cursor + 1) != b'{' {
            return Err(cursor + 1);
        }
        let body = cursor + 2;
        let present = self.input.len() - body;
        let length = match usize::try_from(declared) {
            Ok(length) if length < present => length,
            _ => {
                self.diagnostics.push(Diagnostic::InsufficientData {
                    required: declared,
                    present,
                });
                return Err(body);
            }
        };
        let close = body + length;
        if self.at(close) != b'}' {
            return Err(close);
        }
        self.pos = close + 1;
        Ok(&self.input[body..close])
    }

    /// `E:<n>:"<class>:<case>";`; returns the enum and case names.
    fn enum_token(&mut self, start: usize) -> Step<(&'a [u8], &'a [u8])> {
        let (text_start, length) = self.length_header(start)?;
        if length == 0 {
            return Err(start + 2);
        }
        let end = text_start + length;
        self.close_quote(end, b';')?;
        let text = &self.input[text_start..end];
        let Some(colon) = text.iter().position(|&b| b == b':') else {
            self.diagnostics
                .push(Diagnostic::MissingEnumColon { name: text });
            return Err(start);
        };
        let (class, case) = (&text[..colon], &text[colon + 1..]);
        if !is_valid_class_name(class) {
            return Err(start);
        }
        self.pos = end + 2;
        Ok((class, case))
    }

    /// The position after `R:<n>;` or `r:<n>;`, if the token is well formed.
    fn reference_end(&self, start: usize) -> Option<usize> {
        let end = self.unsigned_end(start + 2)?;
        (self.at(end) == b';').then_some(end + 1)
    }

    /// `R:<n>;` or `r:<n>;` stored in `bucket`; `slot` is this value's own number for `r:`.
    fn reference(&mut self, start: usize, object: bool, bucket: Bucket, slot: usize) -> Step {
        let end = self.reference_end(start).ok_or(start)?;
        self.pos = end;
        let number = self.parse_size(start + 2).cast_signed();
        let target = usize::try_from(number.wrapping_sub(1))
            .ok()
            .filter(|&index| index < self.slots.len())
            .ok_or(end)?;
        let entry = self.slots[target];
        let is_self = object && target + 1 == slot;
        if is_self || self.same_bucket(entry.bucket, bucket) {
            return Err(end);
        }
        if object {
            if !entry.object {
                return Err(end);
            }
            self.mark_object(slot);
            self.push(Node::ObjectReference(target + 1));
        } else {
            self.push(Node::Reference(target + 1));
        }
        Ok(())
    }

    /// Whether two values are stored under the same key of the same container, where a later
    /// entry overwrites the earlier one.
    fn same_bucket(&self, left: Bucket, right: Bucket) -> bool {
        match (left, right) {
            (Some((left_node, left_key)), Some((right_node, right_key)))
                if left_node == right_node =>
            {
                match (self.keys.get(left_key), self.keys.get(right_key)) {
                    (Some(a), Some(b)) => normal_key(a) == normal_key(b),
                    _ => false,
                }
            }
            _ => false,
        }
    }
}

/// PHP stores a string key that is a canonical decimal integer as that integer.
fn normal_key<'k>(key: &'k KeyNode<'_>) -> NormalKey<'k> {
    match key {
        KeyNode::Int(value) => NormalKey::Int(*value),
        KeyNode::String(bytes) => {
            canonical_integer(bytes).map_or(NormalKey::String(bytes), NormalKey::Int)
        }
    }
}

/// The integer a string spells in canonical form: optional `-`, no leading zeros, in range.
fn canonical_integer(bytes: &[u8]) -> Option<i64> {
    let (negative, digits) = match bytes.split_first() {
        Some((b'-', rest)) => (true, rest),
        _ => (false, bytes),
    };
    let leading_zero = digits.first() == Some(&b'0') && bytes.len() > 1;
    if digits.is_empty()
        || leading_zero
        || digits.len() > MAX_INTEGER_DIGITS
        || !digits.iter().all(u8::is_ascii_digit)
    {
        return None;
    }
    let magnitude = digits
        .iter()
        .fold(0_u64, |acc, digit| acc * 10 + u64::from(digit - b'0'));
    if negative {
        (magnitude <= i64::MIN.unsigned_abs()).then(|| 0_i64.wrapping_sub_unsigned(magnitude))
    } else {
        i64::try_from(magnitude).ok()
    }
}

/// Class names may only contain ASCII letters, digits, `_`, `\` and bytes from 0x80 up.
fn is_valid_class_name(name: &[u8]) -> bool {
    name.iter()
        .all(|&b| b.is_ascii_alphanumeric() || b == b'_' || b == b'\\' || b >= 0x80)
}

fn hex_value(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
}
