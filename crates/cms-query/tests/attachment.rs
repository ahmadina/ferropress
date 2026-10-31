//! Parse the attachment query vars (`contracts/query/attachment.md`).

use cms_query::{AttachmentInput, AttachmentVar};

#[test]
fn missing_empty_and_zero_id_are_not_attachment() {
    let missing = AttachmentVar::parse(AttachmentInput::new());
    assert_eq!(missing.as_bytes(), b"");
    assert_eq!(missing.id(), 0);
    assert!(!missing.is_attachment());
    assert!(!missing.is_single());
    let empty = AttachmentVar::parse(AttachmentInput::new().attachment(b"").attachment_id(b"0"));
    assert!(!empty.is_attachment());
}

#[test]
fn slug_and_spaces_are_attachment() {
    let parsed = AttachmentVar::parse(AttachmentInput::new().attachment(b" file"));
    assert_eq!(parsed.as_bytes(), b" file");
    assert!(parsed.is_attachment());
    assert!(parsed.is_single());
    let zero = AttachmentVar::parse(AttachmentInput::new().attachment(b"0"));
    assert_eq!(zero.as_bytes(), b"0");
    assert!(zero.is_attachment());
}

#[test]
fn positive_id_is_attachment() {
    let parsed = AttachmentVar::parse(AttachmentInput::new().attachment_id(b"-4"));
    assert_eq!(parsed.id(), 4);
    assert!(parsed.is_attachment());
    assert_eq!(
        AttachmentVar::parse(AttachmentInput::new().attachment_id(b"12abc")).id(),
        12
    );
}

#[test]
fn subpost_replaces_when_loosely_nonempty() {
    let remap = AttachmentVar::parse(
        AttachmentInput::new()
            .attachment(b"old")
            .subpost(b"new")
            .attachment_id(b"1")
            .subpost_id(b"9"),
    );
    assert_eq!(remap.as_bytes(), b"new");
    assert_eq!(remap.id(), 9);
    let zero_compat = AttachmentVar::parse(
        AttachmentInput::new()
            .attachment(b"keep")
            .subpost(b"0")
            .attachment_id(b"3")
            .subpost_id(b"0"),
    );
    assert_eq!(zero_compat.as_bytes(), b"0");
    assert_eq!(zero_compat.id(), 0);
    assert!(zero_compat.is_attachment());
    let keep = AttachmentVar::parse(
        AttachmentInput::new()
            .attachment(b"keep")
            .subpost(b"")
            .attachment_id(b"3")
            .subpost_id(b""),
    );
    assert_eq!(keep.as_bytes(), b"keep");
    assert_eq!(keep.id(), 3);
}
