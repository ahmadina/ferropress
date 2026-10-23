//! Properties that hold for every input: written values read back unchanged, and arbitrary bytes
//! never panic and always yield a self-consistent outcome.

use php_serialize::{Key, Value, Writer, decode};
use proptest::prelude::*;

/// A value tree the tests write with [`Writer`] and expect to read back. Floats are kept as bits,
/// with every NaN stored as the canonical NaN that `NAN` reads back as.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Tree {
    Null,
    Bool(bool),
    Int(i64),
    Float(u64),
    String(Vec<u8>),
    Array(Vec<(TreeKey, Tree)>),
    Object(Vec<u8>, Vec<(TreeKey, Tree)>),
    Custom(Vec<u8>, Vec<u8>),
    Enum(Vec<u8>, Vec<u8>),
    /// Generated only as `R:1;`, which is valid anywhere below the top-level array.
    Reference(usize),
    /// Never generated; present so that every decoded value converts back.
    ObjectReference(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TreeKey {
    Int(i64),
    String(Vec<u8>),
}

fn class_name() -> impl Strategy<Value = Vec<u8>> {
    "[A-Za-z_\u{80}-\u{ff}][A-Za-z0-9_\\\\\u{80}-\u{ff}]{0,8}".prop_map(String::into_bytes)
}

fn key() -> impl Strategy<Value = TreeKey> + Clone {
    prop_oneof![
        any::<i64>().prop_map(TreeKey::Int),
        proptest::collection::vec(any::<u8>(), 0..6).prop_map(TreeKey::String),
    ]
}

fn tree() -> impl Strategy<Value = Tree> {
    let leaf = prop_oneof![
        Just(Tree::Null),
        any::<bool>().prop_map(Tree::Bool),
        any::<i64>().prop_map(Tree::Int),
        any::<f64>().prop_map(|value| Tree::Float(float_bits(value))),
        proptest::collection::vec(any::<u8>(), 0..12).prop_map(Tree::String),
        (class_name(), proptest::collection::vec(any::<u8>(), 0..8))
            .prop_map(|(class, payload)| Tree::Custom(class, payload)),
        (class_name(), proptest::collection::vec(any::<u8>(), 0..6))
            .prop_map(|(class, case)| Tree::Enum(class, case)),
        Just(Tree::Reference(1)),
    ];
    leaf.prop_recursive(5, 64, 6, |inner| {
        let entries = proptest::collection::vec((key(), inner), 0..6);
        prop_oneof![
            entries.clone().prop_map(Tree::Array),
            (class_name(), entries).prop_map(|(class, entries)| Tree::Object(class, entries)),
        ]
    })
}

fn root() -> impl Strategy<Value = Tree> {
    proptest::collection::vec((key(), tree()), 0..6).prop_map(Tree::Array)
}

fn write(tree: &Tree, writer: &mut Writer) {
    let entries = |entries: &[(TreeKey, Tree)], writer: &mut Writer| {
        for (key, value) in entries {
            match key {
                TreeKey::Int(value) => writer.int(*value),
                TreeKey::String(bytes) => writer.string(bytes),
            }
            write(value, writer);
        }
        writer.end();
    };
    match tree {
        Tree::Null => writer.null(),
        Tree::Bool(value) => writer.bool(*value),
        Tree::Int(value) => writer.int(*value),
        Tree::Float(bits) => writer.float(f64::from_bits(*bits)),
        Tree::String(bytes) => writer.string(bytes),
        Tree::Array(items) => {
            writer.array(items.len());
            entries(items, writer);
        }
        Tree::Object(class, items) => {
            writer.object(class, items.len());
            entries(items, writer);
        }
        Tree::Custom(class, payload) => writer.custom(class, payload),
        Tree::Enum(class, case) => writer.enum_case(class, case),
        Tree::Reference(number) => writer.reference(*number),
        Tree::ObjectReference(number) => writer.object_reference(*number),
    }
}

fn float_bits(value: f64) -> u64 {
    if value.is_nan() {
        f64::NAN.to_bits()
    } else {
        value.to_bits()
    }
}

/// Converts a decoded view back into a [`Tree`].
fn read(value: Value<'_, '_>) -> Tree {
    let entries = |entries: php_serialize::Entries<'_, '_>| {
        entries
            .map(|(key, value)| {
                let key = match key {
                    Key::Int(value) => TreeKey::Int(value),
                    Key::String(bytes) => TreeKey::String(bytes.to_vec()),
                };
                (key, read(value))
            })
            .collect()
    };
    match value {
        Value::Null => Tree::Null,
        Value::Bool(value) => Tree::Bool(value),
        Value::Int(value) => Tree::Int(value),
        Value::Float(value) => Tree::Float(float_bits(value)),
        Value::String(bytes) => Tree::String(bytes.to_vec()),
        Value::Array(items) => Tree::Array(entries(items)),
        Value::Object { class, properties } => Tree::Object(class.to_vec(), entries(properties)),
        Value::Custom { class, payload } => Tree::Custom(class.to_vec(), payload.to_vec()),
        Value::Enum { class, case } => Tree::Enum(class.to_vec(), case.to_vec()),
        Value::Reference(number) => Tree::Reference(number),
        Value::ObjectReference(number) => Tree::ObjectReference(number),
    }
}

/// Checks the invariants every decoding outcome must satisfy.
fn check_outcome(input: &[u8]) {
    match decode(input) {
        Err(error) => assert!(error.offset() <= input.len()),
        Ok(document) => {
            assert!(document.consumed() <= input.len());
            let canonical = document.to_bytes();
            let again = decode(&canonical);
            assert!(again.is_ok(), "canonical form must decode: {again:?}");
            if let Ok(again) = again {
                assert_eq!(again.consumed(), canonical.len());
                assert_eq!(again.to_bytes(), canonical);
            }
        }
    }
}

#[derive(Debug, Clone)]
enum Mutation {
    Truncate(usize),
    Replace(usize, u8),
    Insert(usize, u8),
    Remove(usize),
}

fn mutation() -> impl Strategy<Value = Mutation> {
    let byte = prop_oneof![
        Just(b'0'),
        Just(b'9'),
        Just(b';'),
        Just(b':'),
        Just(b'{'),
        Just(b'}'),
        Just(b'"'),
        Just(b'-'),
        Just(b'R'),
        Just(b'r'),
        any::<u8>(),
    ];
    prop_oneof![
        any::<usize>().prop_map(Mutation::Truncate),
        (any::<usize>(), byte.clone()).prop_map(|(at, b)| Mutation::Replace(at, b)),
        (any::<usize>(), byte).prop_map(|(at, b)| Mutation::Insert(at, b)),
        any::<usize>().prop_map(Mutation::Remove),
    ]
}

fn apply(bytes: &mut Vec<u8>, mutation: &Mutation) {
    if bytes.is_empty() {
        return;
    }
    let len = bytes.len();
    match *mutation {
        Mutation::Truncate(at) => bytes.truncate(at % len),
        Mutation::Replace(at, byte) => bytes[at % len] = byte,
        Mutation::Insert(at, byte) => bytes.insert(at % (len + 1), byte),
        Mutation::Remove(at) => {
            bytes.remove(at % len);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn written_values_read_back_unchanged(tree in root()) {
        let mut writer = Writer::new();
        write(&tree, &mut writer);
        let bytes = writer.into_bytes();
        let document = decode(&bytes).expect("written value must decode");
        prop_assert_eq!(document.consumed(), bytes.len());
        prop_assert!(document.diagnostics().is_empty());
        prop_assert_eq!(&document.to_bytes(), &bytes);
        prop_assert_eq!(read(document.root()), tree);
    }

    #[test]
    fn arbitrary_bytes_never_panic(input in proptest::collection::vec(any::<u8>(), 0..64)) {
        check_outcome(&input);
    }

    #[test]
    fn mutated_values_never_panic(tree in root(), mutations in proptest::collection::vec(mutation(), 1..4)) {
        let mut writer = Writer::new();
        write(&tree, &mut writer);
        let mut bytes = writer.into_bytes();
        for mutation in &mutations {
            apply(&mut bytes, mutation);
        }
        check_outcome(&bytes);
    }
}
