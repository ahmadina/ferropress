//! Hook names stay exact bytes (`contracts/hooks/dispatch.md`).

use cms_types::{ArgCount, HookName, Priority};

#[test]
fn two_spellings_are_different_hooks() {
    assert_ne!(HookName::from("init"), HookName::from("Init"));
    assert_eq!(HookName::from("init").as_bytes(), b"init");
    assert_eq!(HookName::from(&b"init"[..]).as_ref(), b"init");
    assert!(HookName::new(HookName::ALL).is_all());
    assert_eq!(format!("{}", HookName::new([0xff])), "\u{FFFD}");
}

#[test]
fn defaults_match_the_contract() {
    assert_eq!(Priority::default().get(), 10);
    assert_eq!(ArgCount::default().get(), 1);
    assert_eq!(Priority::NULL.get(), 0);
    assert_eq!(ArgCount::NONE.get(), 0);
}
