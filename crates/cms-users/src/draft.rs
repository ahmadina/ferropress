//! Compact writable cells for one users-table row.

use std::fmt;

/// A writable users-table column from the insert compact list.
///
/// `ID` and `user_status` are not here. Contract: `contracts/users/write.md`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Field {
    /// `user_pass`.
    Password,
    /// `user_nicename`.
    Nicename,
    /// `user_email`.
    Email,
    /// `user_url`.
    Url,
    /// `user_registered`.
    Registered,
    /// `user_activation_key`.
    ActivationKey,
    /// `display_name`.
    DisplayName,
    /// `user_login` (insert only).
    Login,
}

impl Field {
    /// Compact insert order used by the reference `INSERT`.
    pub const INSERT: [Self; 8] = [
        Self::Password,
        Self::Nicename,
        Self::Email,
        Self::Url,
        Self::Registered,
        Self::ActivationKey,
        Self::DisplayName,
        Self::Login,
    ];

    /// Compact update order used by the reference `UPDATE` (no login).
    pub const UPDATE: [Self; 7] = [
        Self::Password,
        Self::Nicename,
        Self::Email,
        Self::Url,
        Self::Registered,
        Self::ActivationKey,
        Self::DisplayName,
    ];

    /// Every writable cell. Same as [`Self::INSERT`].
    pub const ALL: [Self; 8] = Self::INSERT;

    /// Inventory column name.
    #[must_use]
    pub const fn column(self) -> &'static [u8] {
        match self {
            Self::Password => b"user_pass",
            Self::Nicename => b"user_nicename",
            Self::Email => b"user_email",
            Self::Url => b"user_url",
            Self::Registered => b"user_registered",
            Self::ActivationKey => b"user_activation_key",
            Self::DisplayName => b"display_name",
            Self::Login => b"user_login",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

/// Caller-supplied cells for one insert or update.
///
/// Empty by default. Password, email and activation key are redacted in [`Debug`].
#[derive(Clone, PartialEq, Eq)]
pub struct Draft {
    cells: [Box<[u8]>; 8],
}

impl Draft {
    /// Every cell empty.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cells: [(); 8].map(|()| Box::from([])),
        }
    }

    /// Replace one cell. Bytes are stored as given.
    pub fn set(&mut self, field: Field, value: impl AsRef<[u8]>) -> &mut Self {
        self.cells[field.index()] = Box::from(value.as_ref());
        self
    }

    /// Stored bytes for `field`.
    #[must_use]
    pub fn get(&self, field: Field) -> &[u8] {
        &self.cells[field.index()]
    }
}

impl Default for Draft {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Draft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Draft")
            .field("login", &String::from_utf8_lossy(self.get(Field::Login)))
            .field(
                "display_name",
                &String::from_utf8_lossy(self.get(Field::DisplayName)),
            )
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::{Draft, Field};

    #[test]
    fn empty_draft_and_setters_round_trip() {
        let mut draft = Draft::new();
        assert!(draft.get(Field::Login).is_empty());
        draft.set(Field::Login, b"ada");
        assert_eq!(draft.get(Field::Login), b"ada");
        assert_eq!(Draft::default().get(Field::Email), b"");
        assert_eq!(Field::ALL.len(), 8);
        assert_eq!(Field::INSERT.len(), 8);
        assert_eq!(Field::UPDATE.len(), 7);
        assert_eq!(Field::INSERT[7], Field::Login);
        assert_eq!(Field::Password.column(), b"user_pass");
        assert_eq!(Field::Login.column(), b"user_login");
        let shown = format!(
            "{:?}",
            draft
                .set(Field::Password, b"$P$secret")
                .set(Field::Email, b"ada@example.test")
                .set(Field::ActivationKey, b"reset-token")
        );
        assert!(shown.contains("Draft"));
        assert!(!shown.contains("$P$secret"));
        assert!(!shown.contains("ada@example.test"));
        assert!(!shown.contains("reset-token"));
    }

    #[test]
    fn every_compact_column_is_named() {
        for field in Field::ALL {
            assert!(!field.column().is_empty());
        }
        for field in Field::UPDATE {
            assert!(!field.column().is_empty());
        }
    }
}
