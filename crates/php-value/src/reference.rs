//! PHP references and the slots that hold either a value or a reference.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::Value;

/// A PHP reference: one value cell shared by every variable, element and property bound to it.
///
/// Writing through any binding is seen by all of them. The number of bindings is observable in PHP
/// (a reference left with a single binding is copied as a plain value when its array is copied), so
/// [`Reference::holders`] reports it.
///
/// # Panics
///
/// The cell is borrowed only for the duration of [`Reference::with`] and [`Reference::with_mut`];
/// re-entering the same reference from inside `with_mut` (including through [`Slot::assign`] or
/// [`Slot::with_value_mut`] on a slot bound to it) panics. Engine code walking a path takes a handle
/// of each reference it meets and continues inside it after releasing the current borrow.
#[derive(Clone)]
pub struct Reference(Rc<RefCell<Value>>);

impl Reference {
    /// A new reference holding `value`, with one binding.
    pub fn new(value: Value) -> Self {
        Self(Rc::new(RefCell::new(value)))
    }

    /// A copy of the referenced value.
    pub fn get(&self) -> Value {
        self.0.borrow().clone()
    }

    /// Replaces the referenced value, returning the previous one.
    pub fn set(&self, value: Value) -> Value {
        self.0.replace(value)
    }

    /// Runs `f` on the referenced value.
    pub fn with<R>(&self, f: impl FnOnce(&Value) -> R) -> R {
        f(&self.0.borrow())
    }

    /// Runs `f` on the referenced value, in place.
    pub fn with_mut<R>(&self, f: impl FnOnce(&mut Value) -> R) -> R {
        f(&mut self.0.borrow_mut())
    }

    /// Runs `f` on the referenced value unless it is being changed right now.
    pub(crate) fn try_with<R>(&self, f: impl FnOnce(&Value) -> R) -> Option<R> {
        self.0.try_borrow().ok().map(|value| f(&value))
    }

    /// How many variables, elements and properties are bound to this reference.
    pub fn holders(&self) -> usize {
        Rc::strong_count(&self.0)
    }

    /// Whether both handles are bindings of the same reference.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Reference")
            .field("holders", &self.holders())
            .finish_non_exhaustive()
    }
}

/// What a variable, array element or property holds: its own value, or a binding to a reference.
#[derive(Debug)]
pub enum Slot {
    /// A value owned by this slot alone.
    Value(Value),
    /// A binding to a shared reference.
    Reference(Reference),
}

impl Slot {
    /// A copy of the value seen through this slot.
    pub fn value(&self) -> Value {
        match self {
            Self::Value(value) => value.clone(),
            Self::Reference(reference) => reference.get(),
        }
    }

    /// Runs `f` on the value seen through this slot.
    pub fn with_value<R>(&self, f: impl FnOnce(&Value) -> R) -> R {
        match self {
            Self::Value(value) => f(value),
            Self::Reference(reference) => reference.with(f),
        }
    }

    /// Runs `f` on the value seen through this slot, in place; a reference is written through.
    pub fn with_value_mut<R>(&mut self, f: impl FnOnce(&mut Value) -> R) -> R {
        match self {
            Self::Value(value) => f(value),
            Self::Reference(reference) => reference.with_mut(f),
        }
    }

    /// Assigns `value` as PHP's `=` does: through the reference when bound to one. Returns the value
    /// it replaced.
    pub fn assign(&mut self, value: Value) -> Value {
        match self {
            Self::Value(current) => std::mem::replace(current, value),
            Self::Reference(reference) => reference.set(value),
        }
    }

    /// Turns this slot into a binding of a reference (creating one around its value if needed) and
    /// returns a further binding of it.
    pub fn make_reference(&mut self) -> Reference {
        let reference = match self {
            Self::Reference(reference) => return reference.clone(),
            Self::Value(value) => Reference::new(std::mem::take(value)),
        };
        *self = Self::Reference(reference.clone());
        reference
    }

    /// The reference this slot is bound to, if any.
    pub fn as_reference(&self) -> Option<&Reference> {
        match self {
            Self::Reference(reference) => Some(reference),
            Self::Value(_) => None,
        }
    }

    /// The slot a copy of its container gets: values are shared copy-on-write, a reference stays
    /// shared while something else is bound to it or `keeps` says so, and any other reference bound
    /// only here is copied as a value.
    pub(crate) fn duplicate(&self, keeps: impl FnOnce(&Reference) -> bool) -> Self {
        match self {
            Self::Value(value) => Self::Value(value.clone()),
            Self::Reference(reference) if reference.holders() > 1 || keeps(reference) => {
                Self::Reference(reference.clone())
            }
            Self::Reference(reference) => Self::Value(reference.get()),
        }
    }
}

impl From<Value> for Slot {
    fn from(value: Value) -> Self {
        Self::Value(value)
    }
}

impl From<Reference> for Slot {
    fn from(reference: Reference) -> Self {
        Self::Reference(reference)
    }
}

#[cfg(test)]
mod tests {
    use super::{Reference, Slot};
    use crate::Value;

    fn int(value: &Value) -> Option<i64> {
        match value {
            Value::Int(number) => Some(*number),
            _ => None,
        }
    }

    #[test]
    fn writes_through_any_binding_are_shared() {
        let reference = Reference::new(Value::Int(1));
        let other = reference.clone();
        assert_eq!(reference.holders(), 2);
        assert!(reference.ptr_eq(&other));
        assert!(!reference.ptr_eq(&Reference::new(Value::Null)));
        assert_eq!(int(&other.set(Value::Int(2))), Some(1));
        assert_eq!(int(&reference.get()), Some(2));
        other.with_mut(|value| *value = Value::Int(3));
        assert_eq!(reference.with(int), Some(3));
        assert_eq!(format!("{reference:?}"), "Reference { holders: 2, .. }");
        drop(other);
        assert_eq!(reference.holders(), 1);
    }

    #[test]
    fn slots_read_and_assign_through_references() {
        let mut owned = Slot::from(Value::Int(1));
        assert!(owned.as_reference().is_none());
        assert_eq!(int(&owned.assign(Value::Int(2))), Some(1));
        assert_eq!(owned.with_value(int), Some(2));
        owned.with_value_mut(|value| *value = Value::Int(5));
        assert_eq!(int(&owned.value()), Some(5));

        let reference = Reference::new(Value::Int(1));
        let mut bound = Slot::from(reference.clone());
        assert_eq!(int(&bound.assign(Value::Int(7))), Some(1));
        assert_eq!(int(&reference.get()), Some(7));
        bound.with_value_mut(|value| *value = Value::Int(8));
        assert_eq!(bound.with_value(int), Some(8));
        assert_eq!(int(&bound.value()), Some(8));
        assert!(bound.as_reference().is_some_and(|r| r.ptr_eq(&reference)));
    }

    #[test]
    fn make_reference_wraps_the_value_once() {
        let mut slot = Slot::from(Value::Int(4));
        let first = slot.make_reference();
        let second = slot.make_reference();
        assert!(first.ptr_eq(&second));
        assert_eq!(first.holders(), 3);
        assert_eq!(int(&first.get()), Some(4));
    }

    #[test]
    fn duplicates_unwrap_references_bound_only_here() {
        let mut slot = Slot::from(Value::Int(1));
        let outside = slot.make_reference();
        let shared = slot.duplicate(|_| false);
        assert!(shared.as_reference().is_some_and(|r| r.ptr_eq(&outside)));
        drop(shared);
        drop(outside);
        assert!(slot.duplicate(|_| true).as_reference().is_some());
        let copied = slot.duplicate(|_| false);
        assert!(copied.as_reference().is_none());
        assert_eq!(int(&copied.value()), Some(1));
        assert!(
            Slot::from(Value::Int(2))
                .duplicate(|_| true)
                .as_reference()
                .is_none()
        );
    }
}
