//! Decoder behavior for representative inputs. Outcomes marked "observed" were checked against
//! PHP 8.5.12 in the reference lab; the full differential corpus lives in `tests/oracle.rs`.

use php_serialize::{DecodeOptions, Diagnostic, Key, Value, decode, decode_with};

/// Decodes `input` and returns its canonical re-encoding, or the error offset.
fn outcome(input: &[u8]) -> Result<Vec<u8>, usize> {
    decode(input)
        .map(|document| document.to_bytes())
        .map_err(|error| error.offset())
}

#[track_caller]
fn accepts(input: &[u8], canonical: &[u8]) {
    assert_eq!(
        outcome(input),
        Ok(canonical.to_vec()),
        "{}",
        String::from_utf8_lossy(input)
    );
}

#[track_caller]
fn refuses(input: &[u8], offset: usize) {
    assert_eq!(
        outcome(input),
        Err(offset),
        "{}",
        String::from_utf8_lossy(input)
    );
}

#[test]
fn observed_acceptances_and_canonical_forms() {
    accepts(br#"O:8:"stdClass"::{}"#, br#"O:8:"stdClass":0:{}"#);
    accepts(
        br#"O:8:"stdClass":+1:{s:1:"a";i:1;}"#,
        br#"O:8:"stdClass":1:{s:1:"a";i:1;}"#,
    );
    accepts(b"i:+05;", b"i:5;");
    accepts(b"d:1.;", b"d:1;");
    accepts(b"d:.5;", b"d:0.5;");
    accepts(b"d:+.5e3;", b"d:500;");
    accepts(b"d:1e999;", b"d:INF;");
    accepts(br#"s:18446744073709551617:"a";"#, br#"s:1:"a";"#);
    accepts(br#"C:3:"Foo":3:{abc}"#, br#"C:3:"Foo":3:{abc}"#);
    accepts(b"a:1:{i:0;R:1;}", b"a:1:{i:0;R:1;}");
    accepts(br#"S:3:"\61bc";"#, br#"s:3:"abc";"#);
}

#[test]
fn observed_refusals_report_php_offsets() {
    refuses(b"a:2:{i:0;i:1;i:0;R:2;}", 21);
    refuses(b"a:2:{i:0;N;i:0;R:2;}", 19);
    refuses(br#"E:9:"Foo:Bar";"#, 14);
    refuses(b"a:2:{i:0;i:1;}", 13);
    refuses(b"b:2;", 0);
    refuses(b"}", 0);
    refuses(b"r:1;", 4);
    refuses(b"a:1:{i:0;r:1;}", 13);
}

#[test]
fn diagnostics_are_reported_in_order() {
    let document = decode(b"a:2:{i:0;i:99999999999999999999;i:1;S:1:\"x\";}").unwrap();
    assert_eq!(
        document.diagnostics(),
        [
            Diagnostic::NumberOutOfRange,
            Diagnostic::EscapedStringDeprecated
        ]
    );
    let error = decode(b"a:2:{i:0;i:1;}").unwrap_err();
    assert_eq!(error.diagnostics(), [Diagnostic::UnexpectedEnd]);
    assert_eq!(
        decode(b"i:-9223372036854775809;").unwrap().diagnostics(),
        [Diagnostic::NumberOutOfRange]
    );
    assert!(
        decode(b"i:-9223372036854775808;")
            .unwrap()
            .diagnostics()
            .is_empty()
    );
}

#[test]
fn values_are_navigable() {
    let input = br#"a:5:{i:-1;N;s:1:"b";b:1;i:2;a:1:{i:0;d:0.5;}i:3;O:4:"Item":1:{s:1:"x";E:6:"Suit:A";}i:4;r:6;}trailing"#;
    let document = decode(input).unwrap();
    assert_eq!(document.consumed(), input.len() - b"trailing".len());
    let Value::Array(entries) = document.root() else {
        panic!("not an array")
    };
    assert_eq!(entries.len(), 5);
    let collected: Vec<_> = entries.collect();
    assert_eq!(collected[0], (Key::Int(-1), Value::Null));
    assert_eq!(collected[1], (Key::String(b"b"), Value::Bool(true)));
    let Value::Array(mut nested) = collected[2].1.clone() else {
        panic!("not an array")
    };
    assert_eq!(nested.next(), Some((Key::Int(0), Value::Float(0.5))));
    assert_eq!(nested.next(), None);
    let Value::Object { class, properties } = collected[3].1.clone() else {
        panic!("not an object")
    };
    assert_eq!(class, b"Item");
    assert_eq!(
        properties.collect::<Vec<_>>(),
        [(
            Key::String(b"x"),
            Value::Enum {
                class: b"Suit",
                case: b"A"
            }
        )]
    );
    assert_eq!(collected[4], (Key::Int(4), Value::ObjectReference(6)));

    let custom = decode(br#"C:4:"Blob":2:{xy}"#).unwrap();
    assert_eq!(
        custom.root(),
        Value::Custom {
            class: b"Blob",
            payload: b"xy"
        }
    );
    let with_reference = decode(b"a:1:{i:0;R:1;}").unwrap();
    let Value::Array(mut references) = with_reference.root() else {
        panic!("not an array")
    };
    assert_eq!(references.next(), Some((Key::Int(0), Value::Reference(1))));
    assert_eq!(decode(b"s:2:\"hi\";").unwrap().root(), Value::String(b"hi"));
    assert_eq!(decode(b"i:7;").unwrap().root(), Value::Int(7));
}

#[test]
fn scalar_tokens_must_be_complete() {
    for input in [
        &b""[..],
        b"N",
        b"b:1",
        b"i:",
        b"i:5",
        b"i:x;",
        b"d:;",
        b"d:.;",
        b"d:1e;",
        b"d:1.5",
        b"d:-NAN;",
        b"d:inf;",
        b"x",
        b"a:1:",
        b"R:1",
    ] {
        refuses(input, 0);
    }
    refuses(b"s:1:\"a\"", 7);
    accepts(b"d:NAN;", b"d:NAN;");
    accepts(b"d:-INF;", b"d:-INF;");
    accepts(b"d:-1.5E-3;", b"d:-0.0015;");
    accepts(b"b:0;", b"b:0;");
}

#[test]
fn string_lengths_are_checked_against_the_input() {
    refuses(br#"s:5:"abc";"#, 10);
    refuses(br#"s:6:"abc";"#, 2);
    refuses(br#"s:2:"abc";"#, 7);
    refuses(br#"s:3:"abc"x"#, 9);
    refuses(br#"s:3:"abc""#, 9);
    refuses(br#"S:5:"abc";"#, 10);
    refuses(br#"S:6:"abc";"#, 2);
    refuses(br#"S:3:"\6"#, 2);
    refuses(br#"S:2:"\6"#, 0);
    refuses(br#"S:3:"\6xbc";"#, 0);
    refuses(br#"S:4:"\61bc";"#, 11);
    refuses(br#"S:2:"\61bc";"#, 9);
    refuses(br#"S:3:"\61bc"x"#, 11);
}

#[test]
fn keys_must_be_integers_or_strings() {
    refuses(b"a:1:{N;i:1;}", 7);
    refuses(b"a:1:{b:1;i:1;}", 9);
    refuses(b"a:1:{d:1;i:1;}", 9);
    refuses(b"a:1:{R:1;i:1;}", 9);
    refuses(b"a:1:{a:0:{}i:1;}", 10);
    refuses(br#"a:1:{O:1:"A":0:{}i:1;}"#, 5);
    refuses(br#"a:1:{E:3:"A:B";i:1;}"#, 5);
    refuses(b"a:1:{x;i:1;}", 5);
    refuses(b"a:1:{b:2;i:1;}", 5);
    refuses(b"a:1:{d:x;i:1;}", 5);
    refuses(b"a:1:{R:x;i:1;}", 5);
    refuses(b"a:1:{a:x;i:1;}", 5);
    refuses(b"a:1:{i:0;}", 9);
    accepts(br#"a:1:{S:1:"\41";i:1;}"#, br#"a:1:{s:1:"A";i:1;}"#);
    let error = decode(b"a:1:{a:99999999999999999999:{i:1;}").unwrap_err();
    assert_eq!(error.offset(), 29);
    assert_eq!(error.diagnostics(), [Diagnostic::NumberOutOfRange]);
    let error = decode(b"a:2:{i:0;i:0;}").unwrap_err();
    assert_eq!(
        (error.offset(), error.diagnostics()),
        (13, &[Diagnostic::UnexpectedEnd][..])
    );
    assert!(decode(b"a:1:{}").unwrap_err().diagnostics().is_empty());
}

#[test]
fn array_counts_are_checked() {
    refuses(b"a:0:{", 5);
    refuses(b"a:0:{x", 5);
    refuses(b"a:3:{i:0;i:0;}", 13);
    refuses(b"a:3:{i:0;}", 5);
    refuses(b"a:99999999999999999999:{}", 24);
    refuses(b"a:1:{i:0;i:0;", 13);
    refuses(b"a:1:{i:0;i:0;i:1;i:1;}", 13);
    accepts(b"a:0:{}", b"a:0:{}");
    refuses(b"a:+1:{i:0;i:0;}", 0);
}

#[test]
fn object_headers_are_checked() {
    refuses(br#"O:0:"":0:{}"#, 2);
    refuses(br#"O:9:"A":0:{}"#, 2);
    refuses(br#"O:1:"AB":0:{}"#, 6);
    refuses(br#"O:1:"A";0:{}"#, 7);
    refuses(br#"O:2:"\A":0:{}"#, 0);
    refuses(br#"O:3:"A-B":0:{}"#, 0);
    refuses(b"O:1:\"A\":", 6);
    refuses(br#"O:1:"A":-1:{}"#, 10);
    refuses(br#"O:1:"A":9:{}"#, 9);
    refuses(br#"O:1:"A":0;{}"#, 9);
    refuses(br#"O:1:"A":0:}"#, 10);
    refuses(br#"O:1:"A":0:{"#, 11);
    refuses(br#"O:1:"A":1:{i:0;i:0;"#, 19);
    accepts(br#"O:1:"A":1:{i:0;i:0;}"#, br#"O:1:"A":1:{i:0;i:0;}"#);
    accepts(
        "O:5:\"Caf\u{e9}\":0:{}".as_bytes(),
        "O:5:\"Caf\u{e9}\":0:{}".as_bytes(),
    );
    let error = decode(br#"O:1:"A":"#).unwrap_err();
    assert_eq!(
        (error.offset(), error.diagnostics()),
        (6, &[Diagnostic::BadData][..])
    );
}

#[test]
fn custom_payloads_are_checked() {
    refuses(br#"C:1:"A":"#, 8);
    refuses("O:4:\"Caf\u{e9}\":0:{}".as_bytes(), 9);
    refuses(br#"C:1:"A":0;{}"#, 9);
    refuses(br#"C:1:"A":0:}"#, 10);
    refuses(br#"C:1:"A":2:{ab"#, 11);
    refuses(br#"C:1:"A":1:{ab}"#, 12);
    refuses(br#"C:2:"\A":0:{}"#, 0);
    let error = decode(br#"C:1:"A":-1:{}"#).unwrap_err();
    assert_eq!(
        (error.offset(), error.diagnostics()),
        (
            12,
            &[Diagnostic::InsufficientData {
                required: -1,
                present: 1
            }][..]
        )
    );
    accepts(br#"C:1:"A":0:{}"#, br#"C:1:"A":0:{}"#);
}

#[test]
fn enum_tokens_are_checked() {
    refuses(br#"E:0:"";"#, 2);
    refuses(br#"E:3:"A:B":"#, 9);
    refuses(br#"E:3:"A-B";"#, 0);
    refuses(br#"E:3:"A.B:C";"#, 8);
    refuses(br#"E:5:"A.B:C";"#, 0);
    let error = decode(br#"E:3:"ABC";"#).unwrap_err();
    assert_eq!(
        (error.offset(), error.diagnostics()),
        (0, &[Diagnostic::MissingEnumColon { name: b"ABC" }][..])
    );
    accepts(br#"E:5:":Case";"#, br#"E:5:":Case";"#);
}

#[test]
fn references_must_point_back() {
    refuses(b"R:1;", 4);
    refuses(b"a:1:{i:0;R:0;}", 13);
    refuses(b"a:1:{i:0;R:3;}", 13);
    refuses(b"a:1:{i:0;R:18446744073709551615;}", 32);
    refuses(b"a:2:{i:0;i:1;s:1:\"0\";R:2;}", 25);
    refuses(b"O:1:\"A\":2:{i:0;i:1;s:1:\"0\";r:2;}", 31);
    refuses(b"a:2:{i:0;i:1;i:1;r:2;}", 21);
    refuses(b"a:1:{i:0;r:3;}", 13);
    accepts(
        b"a:2:{i:0;i:1;s:2:\"00\";R:2;}",
        b"a:2:{i:0;i:1;s:2:\"00\";R:2;}",
    );
    accepts(
        b"a:2:{i:0;i:1;i:1;R:18446744073709551618;}",
        b"a:2:{i:0;i:1;i:1;R:2;}",
    );
    accepts(
        br#"a:2:{i:0;O:1:"A":0:{}i:1;r:2;}"#,
        br#"a:2:{i:0;O:1:"A":0:{}i:1;r:2;}"#,
    );
    accepts(
        br#"a:2:{i:0;E:3:"A:B";i:1;r:2;}"#,
        br#"a:2:{i:0;E:3:"A:B";i:1;r:2;}"#,
    );
    accepts(
        br#"a:2:{i:0;C:1:"A":0:{}i:1;r:2;}"#,
        br#"a:2:{i:0;C:1:"A":0:{}i:1;r:2;}"#,
    );
    accepts(
        br#"a:3:{i:0;O:1:"A":0:{}i:1;r:2;i:2;r:3;}"#,
        br#"a:3:{i:0;O:1:"A":0:{}i:1;r:2;i:2;r:3;}"#,
    );
    accepts(br#"O:1:"A":1:{i:0;r:1;}"#, br#"O:1:"A":1:{i:0;r:1;}"#);
    accepts(
        b"a:2:{s:2:\"-0\";i:1;i:0;R:2;}",
        b"a:2:{s:2:\"-0\";i:1;i:0;R:2;}",
    );
    accepts(
        b"a:2:{s:20:\"99999999999999999999\";i:1;i:0;R:2;}",
        b"a:2:{s:20:\"99999999999999999999\";i:1;i:0;R:2;}",
    );
    refuses(
        b"a:2:{s:20:\"-9223372036854775808\";i:1;i:-9223372036854775808;R:2;}",
        64,
    );
    accepts(
        b"a:2:{s:20:\"-9223372036854775809\";i:1;i:0;R:2;}",
        b"a:2:{s:20:\"-9223372036854775809\";i:1;i:0;R:2;}",
    );
    accepts(
        b"a:2:{s:1:\"-\";i:1;s:0:\"\";R:2;}",
        b"a:2:{s:1:\"-\";i:1;s:0:\"\";R:2;}",
    );
}

#[test]
fn nesting_depth_is_limited() {
    let nested = |depth: usize| {
        let mut input = b"a:1:{i:0;".repeat(depth);
        input.extend_from_slice(b"N;");
        input.extend(std::iter::repeat_n(b'}', depth));
        input
    };
    assert!(decode(&nested(4096)).is_ok());
    let too_deep = nested(4097);
    let error = decode(&too_deep).unwrap_err();
    assert_eq!(error.offset(), 4096 * 9 + 5);
    assert_eq!(
        error.diagnostics(),
        [Diagnostic::MaxDepthExceeded { max_depth: 4096 }]
    );
    let shallow = DecodeOptions::default().with_max_depth(1);
    assert!(decode_with(b"a:0:{}", shallow).is_ok());
    assert_eq!(
        decode_with(br#"a:1:{i:0;O:1:"A":0:{}}"#, shallow)
            .unwrap_err()
            .offset(),
        20
    );
    let unlimited = DecodeOptions::default().with_max_depth(0);
    let deep = nested(100_000);
    assert_eq!(decode_with(&deep, unlimited).unwrap().to_bytes(), deep);
}
