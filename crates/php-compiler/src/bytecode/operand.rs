//! Instruction operands, packed into one `u32` each.

use std::fmt;

/// What an [`Operand`] refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperandKind {
    /// No operand.
    Unused,
    /// A named local variable, by slot index.
    Slot,
    /// A temporary register, by index.
    Temp,
    /// An entry of the code object's literal pool, by index.
    Literal,
    /// An instruction of the same code object, by index (a jump target).
    Target,
}

/// One operand of an instruction: a kind and a 29-bit index, packed so instructions stay small and
/// `Copy`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Operand(u32);

impl Operand {
    const KIND_SHIFT: u32 = 29;
    const INDEX_MASK: u32 = (1 << Self::KIND_SHIFT) - 1;

    /// The largest index an operand holds.
    pub const MAX_INDEX: u32 = Self::INDEX_MASK;

    /// No operand.
    pub const UNUSED: Self = Self(0);

    const fn pack(kind: u32, index: u32) -> Option<Self> {
        if index > Self::INDEX_MASK {
            None
        } else {
            Some(Self((kind << Self::KIND_SHIFT) | index))
        }
    }

    /// The local variable at slot `index`, unless the index exceeds [`Self::MAX_INDEX`].
    #[must_use]
    pub const fn slot(index: u32) -> Option<Self> {
        Self::pack(1, index)
    }

    /// The temporary `index`, unless the index exceeds [`Self::MAX_INDEX`].
    #[must_use]
    pub const fn temp(index: u32) -> Option<Self> {
        Self::pack(2, index)
    }

    /// The literal `index`, unless the index exceeds [`Self::MAX_INDEX`].
    #[must_use]
    pub const fn literal(index: u32) -> Option<Self> {
        Self::pack(3, index)
    }

    /// The instruction `index` as a jump target, unless the index exceeds [`Self::MAX_INDEX`].
    #[must_use]
    pub const fn target(index: u32) -> Option<Self> {
        Self::pack(4, index)
    }

    /// What the operand refers to.
    #[must_use]
    pub const fn kind(self) -> OperandKind {
        match self.0 >> Self::KIND_SHIFT {
            1 => OperandKind::Slot,
            2 => OperandKind::Temp,
            3 => OperandKind::Literal,
            4 => OperandKind::Target,
            _ => OperandKind::Unused,
        }
    }

    /// The index within its kind; 0 for [`Self::UNUSED`].
    #[must_use]
    pub const fn index(self) -> u32 {
        self.0 & Self::INDEX_MASK
    }

    /// Whether the operand is [`Self::UNUSED`].
    #[must_use]
    pub const fn is_unused(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Debug for Operand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind() {
            OperandKind::Unused => f.write_str("_"),
            OperandKind::Slot => write!(f, "S{}", self.index()),
            OperandKind::Temp => write!(f, "T{}", self.index()),
            OperandKind::Literal => write!(f, "L{}", self.index()),
            OperandKind::Target => write!(f, "@{}", self.index()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Operand, OperandKind};

    #[test]
    fn operands_pack_their_kind_and_index() {
        let cases = [
            (Operand::slot(3), OperandKind::Slot, "S3"),
            (Operand::temp(0), OperandKind::Temp, "T0"),
            (Operand::literal(7), OperandKind::Literal, "L7"),
            (
                Operand::target(Operand::MAX_INDEX),
                OperandKind::Target,
                "@536870911",
            ),
        ];
        for (operand, kind, shown) in cases {
            let operand = operand.unwrap();
            assert_eq!(operand.kind(), kind);
            assert!(!operand.is_unused());
            assert_eq!(format!("{operand:?}"), shown);
        }
        assert_eq!(Operand::slot(3).unwrap().index(), 3);
        assert_eq!(Operand::UNUSED.kind(), OperandKind::Unused);
        assert_eq!(Operand::UNUSED.index(), 0);
        assert!(Operand::default().is_unused());
        assert_eq!(format!("{:?}", Operand::UNUSED), "_");
    }

    #[test]
    fn indexes_past_the_limit_do_not_pack() {
        assert_eq!(Operand::slot(Operand::MAX_INDEX + 1), None);
        assert_eq!(Operand::temp(u32::MAX), None);
        assert_eq!(Operand::literal(Operand::MAX_INDEX + 1), None);
        assert_eq!(Operand::target(Operand::MAX_INDEX + 1), None);
    }
}
