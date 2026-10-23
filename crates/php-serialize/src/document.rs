//! Decoded serialized data, stored flat in pre-order.

use std::borrow::Cow;

use crate::error::Diagnostic;
use crate::write::Writer;

/// One decoded token. Containers record their entry count and the index one past their last
/// descendant, so a subtree is skipped in constant time.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Node<'a> {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(Cow<'a, [u8]>),
    Array {
        len: usize,
        end: usize,
    },
    Object {
        class: &'a [u8],
        len: usize,
        end: usize,
    },
    Custom {
        class: &'a [u8],
        payload: &'a [u8],
    },
    Enum {
        class: &'a [u8],
        case: &'a [u8],
    },
    Reference(usize),
    ObjectReference(usize),
}

/// The key of an array entry or object property as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum KeyNode<'a> {
    Int(i64),
    String(Cow<'a, [u8]>),
}

/// A value PHP's `unserialize()` accepts, decoded without running any PHP semantics.
///
/// Strings, class names and payloads borrow from the input except for legacy `S:` strings,
/// whose escapes are resolved into owned bytes. Navigate with [`Document::root`]; write the value
/// back with [`Document::to_bytes`], which yields PHP's canonical form of the same tokens.
#[derive(Debug, Clone, PartialEq)]
pub struct Document<'a> {
    nodes: Vec<Node<'a>>,
    /// `keys[i]` is the key of `nodes[i + 1]`; the root has no key.
    keys: Vec<KeyNode<'a>>,
    consumed: usize,
    diagnostics: Vec<Diagnostic<'a>>,
}

impl<'a> Document<'a> {
    pub(crate) const fn new(
        nodes: Vec<Node<'a>>,
        keys: Vec<KeyNode<'a>>,
        consumed: usize,
        diagnostics: Vec<Diagnostic<'a>>,
    ) -> Self {
        Self {
            nodes,
            keys,
            consumed,
            diagnostics,
        }
    }

    /// The decoded value.
    pub fn root(&self) -> Value<'_, 'a> {
        value_at(&self.nodes, &self.keys, 0)
    }

    /// How many input bytes the value occupies. PHP warns about extra data when this is less than
    /// the input length, but still returns the value.
    pub const fn consumed(&self) -> usize {
        self.consumed
    }

    /// The diagnostics PHP emits while reading the value, in order.
    pub fn diagnostics(&self) -> &[Diagnostic<'a>] {
        &self.diagnostics
    }

    /// Writes the value with `writer`.
    pub fn write_to(&self, writer: &mut Writer) {
        let mut open: Vec<usize> = Vec::new();
        for (index, node) in self.nodes.iter().enumerate() {
            while open.last() == Some(&index) {
                open.pop();
                writer.end();
            }
            if let Some(key) = index.checked_sub(1).and_then(|k| self.keys.get(k)) {
                match key {
                    KeyNode::Int(value) => writer.int(*value),
                    KeyNode::String(bytes) => writer.string(bytes),
                }
            }
            match node {
                Node::Null => writer.null(),
                Node::Bool(value) => writer.bool(*value),
                Node::Int(value) => writer.int(*value),
                Node::Float(value) => writer.float(*value),
                Node::String(bytes) => writer.string(bytes),
                Node::Array { len, end } => {
                    writer.array(*len);
                    open.push(*end);
                }
                Node::Object { class, len, end } => {
                    writer.object(class, *len);
                    open.push(*end);
                }
                Node::Custom { class, payload } => writer.custom(class, payload),
                Node::Enum { class, case } => writer.enum_case(class, case),
                Node::Reference(number) => writer.reference(*number),
                Node::ObjectReference(number) => writer.object_reference(*number),
            }
        }
        for _ in open {
            writer.end();
        }
    }

    /// The value in PHP's canonical serialized form.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut writer = Writer::with_capacity(self.consumed);
        self.write_to(&mut writer);
        writer.into_bytes()
    }
}

/// A borrowed view of one decoded value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value<'d, 'a> {
    /// `N;`
    Null,
    /// `b:0;` or `b:1;`
    Bool(bool),
    /// `i:<n>;`
    Int(i64),
    /// `d:<n>;`
    Float(f64),
    /// `s:` or `S:`
    String(&'d [u8]),
    /// `a:<n>:{...}`
    Array(Entries<'d, 'a>),
    /// `O:<n>:"<class>":<n>:{...}`
    Object {
        /// The class name as written.
        class: &'a [u8],
        /// The properties as written, including mangled visibility prefixes.
        properties: Entries<'d, 'a>,
    },
    /// `C:<n>:"<class>":<n>:{<payload>}`
    Custom {
        /// The class name as written.
        class: &'a [u8],
        /// The class-defined payload, uninterpreted.
        payload: &'a [u8],
    },
    /// `E:<n>:"<class>:<case>";`
    Enum {
        /// The enum name.
        class: &'a [u8],
        /// The case name.
        case: &'a [u8],
    },
    /// `R:<n>;`, a PHP reference to the value numbered `n`.
    Reference(usize),
    /// `r:<n>;`, another occurrence of the object numbered `n`.
    ObjectReference(usize),
}

/// The key of an array entry or object property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key<'d> {
    /// `i:<n>;`
    Int(i64),
    /// `s:` or `S:`
    String(&'d [u8]),
}

/// The entries of an array or object, in written order.
#[derive(Debug, Clone, PartialEq)]
pub struct Entries<'d, 'a> {
    nodes: &'d [Node<'a>],
    keys: &'d [KeyNode<'a>],
    next: usize,
    remaining: usize,
}

impl<'d, 'a> Iterator for Entries<'d, 'a> {
    type Item = (Key<'d>, Value<'d, 'a>);

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let index = self.next;
        let key = match self.keys.get(index.checked_sub(1)?)? {
            KeyNode::Int(value) => Key::Int(*value),
            KeyNode::String(bytes) => Key::String(bytes),
        };
        self.remaining -= 1;
        self.next = match self.nodes.get(index)? {
            Node::Array { end, .. } | Node::Object { end, .. } => *end,
            _ => index + 1,
        };
        Some((key, value_at(self.nodes, self.keys, index)))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for Entries<'_, '_> {}

fn value_at<'d, 'a>(nodes: &'d [Node<'a>], keys: &'d [KeyNode<'a>], index: usize) -> Value<'d, 'a> {
    let entries = |len: usize| Entries {
        nodes,
        keys,
        next: index + 1,
        remaining: len,
    };
    match &nodes[index] {
        Node::Null => Value::Null,
        Node::Bool(value) => Value::Bool(*value),
        Node::Int(value) => Value::Int(*value),
        Node::Float(value) => Value::Float(*value),
        Node::String(bytes) => Value::String(bytes),
        Node::Array { len, .. } => Value::Array(entries(*len)),
        Node::Object { class, len, .. } => Value::Object {
            class,
            properties: entries(*len),
        },
        Node::Custom { class, payload } => Value::Custom { class, payload },
        Node::Enum { class, case } => Value::Enum { class, case },
        Node::Reference(number) => Value::Reference(*number),
        Node::ObjectReference(number) => Value::ObjectReference(*number),
    }
}
