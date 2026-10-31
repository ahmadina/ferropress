//! The `attachment` and `attachment_id` query vars after `parse_query`.

use php_value::PhpString;
use php_value::Value;
use php_value::ops::Collect;
use php_value::ops::loose_equals;

/// Raw attachment keys before remap and absint (`contracts/query/attachment.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttachmentInput<'a> {
    attachment: Option<&'a [u8]>,
    attachment_id: Option<&'a [u8]>,
    subpost: Option<&'a [u8]>,
    subpost_id: Option<&'a [u8]>,
}

impl<'a> AttachmentInput<'a> {
    /// No keys set (filled as empty).
    #[must_use]
    pub const fn new() -> Self {
        Self {
            attachment: None,
            attachment_id: None,
            subpost: None,
            subpost_id: None,
        }
    }

    /// Sets the `attachment` slug bytes.
    #[must_use]
    pub const fn attachment(self, bytes: &'a [u8]) -> Self {
        Self {
            attachment: Some(bytes),
            ..self
        }
    }

    /// Sets the `attachment_id` bytes.
    #[must_use]
    pub const fn attachment_id(self, bytes: &'a [u8]) -> Self {
        Self {
            attachment_id: Some(bytes),
            ..self
        }
    }

    /// Sets the compat `subpost` slug bytes.
    #[must_use]
    pub const fn subpost(self, bytes: &'a [u8]) -> Self {
        Self {
            subpost: Some(bytes),
            ..self
        }
    }

    /// Sets the compat `subpost_id` bytes.
    #[must_use]
    pub const fn subpost_id(self, bytes: &'a [u8]) -> Self {
        Self {
            subpost_id: Some(bytes),
            ..self
        }
    }
}

impl Default for AttachmentInput<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// The parsed attachment query vars (`contracts/query/attachment.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentVar {
    slug: Box<[u8]>,
    id: i64,
}

impl AttachmentVar {
    /// Parse optional raw keys the way `parse_query` reads attachment vars.
    ///
    /// `subpost` / `subpost_id` replace the matching key when they are not
    /// loosely equal to empty. The slug is not trimmed. The id is `absint`.
    #[must_use]
    pub fn parse(raw: AttachmentInput<'_>) -> Self {
        let slug = pick(raw.subpost, raw.attachment);
        let id = raw
            .subpost_id
            .filter(|bytes| loosely_nonempty(bytes))
            .or(raw.attachment_id)
            .map_or(0, absint);
        Self {
            slug: slug.into(),
            id,
        }
    }

    /// The stored slug bytes after remap (not trimmed).
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.slug
    }

    /// The stored non-negative id after remap and `absint`.
    #[must_use]
    pub const fn id(&self) -> i64 {
        self.id
    }

    /// Whether this is an attachment request (non-empty slug or id greater
    /// than zero).
    #[must_use]
    pub fn is_attachment(&self) -> bool {
        !self.slug.is_empty() || self.id > 0
    }

    /// Whether this is a single-post request (the same as [`Self::is_attachment`]).
    #[must_use]
    pub fn is_single(&self) -> bool {
        self.is_attachment()
    }
}

impl AsRef<[u8]> for AttachmentVar {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

fn pick<'a>(compat: Option<&'a [u8]>, primary: Option<&'a [u8]>) -> &'a [u8] {
    compat
        .filter(|bytes| loosely_nonempty(bytes))
        .or(primary)
        .unwrap_or(&[])
}

fn loosely_nonempty(bytes: &[u8]) -> bool {
    let empty = Value::from("");
    let value = Value::from(PhpString::from(bytes));
    let cx = Collect::default();
    !loose_equals(&empty, &value, &cx).unwrap_or(true)
}

fn absint(bytes: &[u8]) -> i64 {
    php_int(bytes).saturating_abs()
}

fn php_int(bytes: &[u8]) -> i64 {
    let value = Value::from(PhpString::from(bytes));
    let mut cx = Collect::default();
    value.to_int(&mut cx).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{AttachmentInput, AttachmentVar};

    #[test]
    fn missing_empty_and_zero_id() {
        let missing = AttachmentVar::parse(AttachmentInput::new());
        assert_eq!(missing.as_bytes(), b"");
        assert_eq!(missing.id(), 0);
        assert!(!missing.is_attachment());
        assert!(!missing.is_single());
        let empty =
            AttachmentVar::parse(AttachmentInput::new().attachment(b"").attachment_id(b"0"));
        assert!(!empty.is_attachment());
    }

    #[test]
    fn slug_id_and_compat() {
        let slug = AttachmentVar::parse(AttachmentInput::new().attachment(b" file"));
        assert_eq!(slug.as_bytes(), b" file");
        assert!(slug.is_attachment());
        assert!(slug.is_single());
        let zero = AttachmentVar::parse(AttachmentInput::new().attachment(b"0"));
        assert_eq!(zero.as_bytes(), b"0");
        assert!(zero.is_attachment());
        let id = AttachmentVar::parse(AttachmentInput::new().attachment_id(b"-4"));
        assert_eq!(id.id(), 4);
        assert!(id.is_attachment());
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
        assert_eq!(
            AttachmentVar::parse(AttachmentInput::new().attachment(b"x")).as_ref(),
            b"x"
        );
        assert_eq!(AttachmentInput::default(), AttachmentInput::new());
    }
}
