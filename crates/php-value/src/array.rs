//! PHP arrays: ordered, copy-on-write maps from integer or string keys to slots.

use std::fmt;
use std::rc::Rc;

use crate::table::Table;
use crate::{ArrayKey, Reference, Slot, Value};

/// The key the next append (`$array[] = …`) will use.
///
/// It starts unset (append uses 0), becomes one past each integer key inserted at or above it
/// (negative keys included), and is never lowered by removals. After `i64::MAX` is inserted there is no
/// next key and appending fails.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NextIndex {
    /// No integer key has been inserted yet.
    #[default]
    Unset,
    /// The key the next append uses.
    At(i64),
    /// `i64::MAX` has been inserted; appending fails.
    Exhausted,
}

impl NextIndex {
    /// The key an append would use, if any.
    pub const fn key(self) -> Option<i64> {
        match self {
            Self::Unset => Some(0),
            Self::At(key) => Some(key),
            Self::Exhausted => None,
        }
    }

    const fn after_insert(self, key: i64) -> Self {
        match self {
            Self::At(next) if key < next => self,
            Self::Exhausted => self,
            Self::Unset | Self::At(_) => match key.checked_add(1) {
                Some(next) => Self::At(next),
                None => Self::Exhausted,
            },
        }
    }
}

/// Appending failed because the next integer key would exceed `i64::MAX`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("Cannot add element to the array as the next element is already occupied")]
pub struct NextIndexOccupied;

/// Consecutive integer keys `0..len` live in `packed` until a hole, a string key, or a
/// non-sequential integer key forces `mixed`.
#[derive(Debug, Default)]
struct ArrayData {
    packed: Vec<Slot>,
    mixed: Option<Table<ArrayKey>>,
    next: NextIndex,
}

impl ArrayData {
    fn len(&self) -> usize {
        self.mixed.as_ref().map_or(self.packed.len(), Table::len)
    }

    fn packed_home(&self, key: &ArrayKey) -> Option<usize> {
        if self.mixed.is_some() {
            return None;
        }
        let ArrayKey::Int(number) = key else {
            return None;
        };
        let index = usize::try_from(*number).ok()?;
        if index < self.packed.len() {
            return Some(index);
        }
        (index == self.packed.len() && self.next.key() == Some(*number)).then_some(index)
    }

    fn promote(&mut self) {
        if self.mixed.is_some() {
            return;
        }
        let mut table = Table::with_capacity(self.packed.len());
        for (index, slot) in self.packed.drain(..).enumerate() {
            let Ok(number) = i64::try_from(index) else {
                continue;
            };
            let (dst, _) =
                table.slot_or_insert_with(ArrayKey::Int(number), || Slot::Value(Value::Null));
            *dst = slot;
        }
        self.mixed = Some(table);
    }

    /// The slot for `key`, appended holding `null` when absent.
    fn slot(&mut self, key: ArrayKey) -> &mut Slot {
        if let Some(index) = self.packed_home(&key) {
            if index == self.packed.len() {
                self.packed.push(Slot::Value(Value::Null));
                if let ArrayKey::Int(number) = key {
                    self.next = self.next.after_insert(number);
                }
            }
            return &mut self.packed[index];
        }
        let added_int = match &key {
            ArrayKey::Int(number)
                if self
                    .mixed
                    .as_ref()
                    .is_none_or(|table| table.get(&key).is_none()) =>
            {
                Some(*number)
            }
            _ => None,
        };
        if let Some(number) = added_int {
            self.next = self.next.after_insert(number);
        }
        self.promote();
        let table = self.mixed.get_or_insert_with(Table::default);
        table
            .slot_or_insert_with(key, || Slot::Value(Value::Null))
            .0
    }

    fn next_slot(&mut self) -> Result<(i64, &mut Slot), NextIndexOccupied> {
        let key = self.next.key().ok_or(NextIndexOccupied)?;
        Ok((key, self.slot(ArrayKey::Int(key))))
    }

    fn get(&self, key: &ArrayKey) -> Option<&Slot> {
        if let Some(table) = &self.mixed {
            return table.get(key);
        }
        let index = match key {
            ArrayKey::Int(number) => usize::try_from(*number).ok()?,
            ArrayKey::String(_) => return None,
        };
        self.packed.get(index)
    }

    fn get_mut(&mut self, key: &ArrayKey) -> Option<&mut Slot> {
        if let Some(table) = &mut self.mixed {
            return table.get_mut(key);
        }
        let index = match key {
            ArrayKey::Int(number) => usize::try_from(*number).ok()?,
            ArrayKey::String(_) => return None,
        };
        self.packed.get_mut(index)
    }

    fn remove(&mut self, key: &ArrayKey) -> Option<Slot> {
        if self.mixed.is_none() {
            if let ArrayKey::Int(number) = key
                && let Ok(index) = usize::try_from(*number)
                && index + 1 == self.packed.len()
            {
                return self.packed.pop();
            }
            self.promote();
        }
        self.mixed.as_mut()?.remove(key)
    }

    fn entry_from(&self, position: usize) -> Option<(usize, ArrayKey, &Slot)> {
        if let Some(table) = &self.mixed {
            return table
                .entry_from(position)
                .map(|(at, key, slot)| (at, key.clone(), slot));
        }
        let slot = self.packed.get(position)?;
        let key = i64::try_from(position).ok()?;
        Some((position, ArrayKey::Int(key), slot))
    }

    fn iter(&self) -> ArrayIter<'_> {
        if let Some(table) = &self.mixed {
            let entries = table.raw_entries();
            return ArrayIter::Mixed {
                entries,
                front: 0,
                back: entries.len(),
            };
        }
        ArrayIter::Packed {
            slots: &self.packed,
            front: 0,
            back: self.packed.len(),
        }
    }

    /// Whether `reference` holds this very storage, the one case where a reference bound only once
    /// still acts as a reference. A reference being changed right now is only reachable while a
    /// write walks through it, which happens when it holds this storage, so it counts as holding it.
    fn is_held_by(&self, reference: &Reference) -> bool {
        reference
            .try_with(|value| matches!(value, Value::Array(array) if array.is_storage(self)))
            .unwrap_or(true)
    }
}

/// Copying an array's storage is PHP's separation (see [`Slot::duplicate`]); only
/// [`Rc::make_mut`] calls it, when a shared array is about to change.
impl Clone for ArrayData {
    fn clone(&self) -> Self {
        let keeps = |reference: &Reference| self.is_held_by(reference);
        Self {
            packed: self
                .packed
                .iter()
                .map(|slot| slot.duplicate(keeps))
                .collect(),
            mixed: self.mixed.as_ref().map(|table| table.duplicate(keeps)),
            next: self.next,
        }
    }
}

/// Walks packed slots or a mixed table in insertion order.
enum ArrayIter<'a> {
    Empty,
    Packed {
        slots: &'a [Slot],
        front: usize,
        back: usize,
    },
    Mixed {
        entries: &'a [Option<crate::table::Entry<ArrayKey>>],
        front: usize,
        back: usize,
    },
}

impl<'a> Iterator for ArrayIter<'a> {
    type Item = (ArrayKey, &'a Slot);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Empty => None,
            Self::Packed { slots, front, back } => {
                if *front >= *back {
                    return None;
                }
                let index = *front;
                *front += 1;
                let key = i64::try_from(index).ok()?;
                Some((ArrayKey::Int(key), &slots[index]))
            }
            Self::Mixed {
                entries,
                front,
                back,
            } => {
                while *front < *back {
                    let index = *front;
                    *front += 1;
                    if let Some(entry) = &entries[index] {
                        return Some((entry.key.clone(), &entry.slot));
                    }
                }
                None
            }
        }
    }
}

impl DoubleEndedIterator for ArrayIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            Self::Empty => None,
            Self::Packed { slots, front, back } => {
                if *front >= *back {
                    return None;
                }
                *back -= 1;
                let index = *back;
                let key = i64::try_from(index).ok()?;
                Some((ArrayKey::Int(key), &slots[index]))
            }
            Self::Mixed {
                entries,
                front,
                back,
            } => {
                while *front < *back {
                    *back -= 1;
                    if let Some(entry) = &entries[*back] {
                        return Some((entry.key.clone(), &entry.slot));
                    }
                }
                None
            }
        }
    }
}

/// A PHP array: an insertion-ordered map from [`ArrayKey`]s to [`Slot`]s.
///
/// Sequential integer keys `0..n` stay in a packed list so `$a[] =` is a push. A hole, a string
/// key, or a non-sequential integer key promotes that storage to a mixed table. Clones share
/// storage until one of them is changed; the changed one then gets its own copy, in which
/// element references still bound elsewhere stay shared and references bound only in the array become
/// plain values, unless such a reference holds the array itself (`$a[] = &$a; unset($a)`). A new,
/// never-written array allocates nothing.
#[derive(Clone, Default)]
pub struct PhpArray(Option<Rc<ArrayData>>);

impl PhpArray {
    /// An empty array.
    pub const fn new() -> Self {
        Self(None)
    }

    /// An empty array with room for `capacity` elements.
    pub fn with_capacity(capacity: usize) -> Self {
        if capacity == 0 {
            return Self::new();
        }
        Self(Some(Rc::new(ArrayData {
            packed: Vec::with_capacity(capacity),
            mixed: None,
            next: NextIndex::Unset,
        })))
    }

    /// Number of elements.
    pub fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |data| data.len())
    }

    /// Whether the array has no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The key the next append will use.
    pub fn next_index(&self) -> NextIndex {
        self.0.as_ref().map_or(NextIndex::Unset, |data| data.next)
    }

    /// The slot stored under `key`.
    pub fn get(&self, key: &ArrayKey) -> Option<&Slot> {
        self.0.as_ref()?.get(key)
    }

    /// Whether an element is stored under `key`.
    pub fn contains_key(&self, key: &ArrayKey) -> bool {
        self.get(key).is_some()
    }

    /// The elements in order.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (ArrayKey, &Slot)> {
        self.0.as_ref().map_or(ArrayIter::Empty, |data| data.iter())
    }

    /// The first live element at or after `position`, with that element's position.
    pub fn entry_from(&self, position: usize) -> Option<(usize, ArrayKey, &Slot)> {
        self.0.as_ref()?.entry_from(position)
    }

    /// The address of the storage, 0 for an array that never had any; equal for arrays that
    /// [share storage](Self::ptr_eq).
    pub(crate) fn storage_address(&self) -> usize {
        self.0.as_ref().map_or(0, |data| Rc::as_ptr(data).addr())
    }

    /// Whether both arrays currently share storage.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
    }

    /// Whether `reference`, bound in this array, acts as a reference: it stays shared when the array
    /// is copied and PHP reports it as one. That holds while it is bound anywhere else too, or when
    /// it holds this array itself.
    pub fn keeps_reference(&self, reference: &Reference) -> bool {
        reference.holders() > 1
            || self
                .0
                .as_deref()
                .is_some_and(|data| data.is_held_by(reference))
    }

    fn is_storage(&self, data: &ArrayData) -> bool {
        self.0.as_deref().is_some_and(|own| std::ptr::eq(own, data))
    }

    fn data_mut(&mut self) -> &mut ArrayData {
        Rc::make_mut(self.0.get_or_insert_with(Rc::default))
    }

    /// `$array[$key] = $value`: assigns through a reference bound there, otherwise stores the value
    /// (appending the key when new). Returns the value it replaced.
    pub fn assign(&mut self, key: ArrayKey, value: Value) -> Value {
        self.data_mut().slot(key).assign(value)
    }

    /// `$array[] = $value`: stores the value under the next key and returns that key.
    ///
    /// # Errors
    ///
    /// [`NextIndexOccupied`] when there is no next key; the array is unchanged.
    pub fn append(&mut self, value: Value) -> Result<i64, NextIndexOccupied> {
        let (key, slot) = self.data_mut().next_slot()?;
        slot.assign(value);
        Ok(key)
    }

    /// `$array[$key] = &$other`: binds the element to `reference`, keeping its position when present.
    /// Returns the slot it replaced.
    pub fn bind(&mut self, key: ArrayKey, reference: Reference) -> Slot {
        std::mem::replace(self.data_mut().slot(key), Slot::Reference(reference))
    }

    /// `$array[] = &$other`: binds a new element under the next key and returns that key.
    ///
    /// # Errors
    ///
    /// [`NextIndexOccupied`] when there is no next key; the array is unchanged.
    pub fn append_reference(&mut self, reference: Reference) -> Result<i64, NextIndexOccupied> {
        let (key, slot) = self.data_mut().next_slot()?;
        *slot = Slot::Reference(reference);
        Ok(key)
    }

    /// `&$array[$key]`: makes the element a reference (creating it as `null` when absent) and returns
    /// a binding of it.
    pub fn reference_at(&mut self, key: ArrayKey) -> Reference {
        self.data_mut().slot(key).make_reference()
    }

    /// `&$array[]`: creates a `null` element under the next key, makes it a reference and returns the
    /// key and a binding.
    ///
    /// # Errors
    ///
    /// [`NextIndexOccupied`] when there is no next key; the array is unchanged.
    pub fn reference_at_next(&mut self) -> Result<(i64, Reference), NextIndexOccupied> {
        let (key, slot) = self.data_mut().next_slot()?;
        Ok((key, slot.make_reference()))
    }

    /// The element's slot for a write fetch (as for `$array[$key][…] = …`), creating it as `null` when
    /// absent and separating shared storage first.
    ///
    /// Engines walking a path step from slot to slot: on a reference they take a handle with
    /// [`Slot::as_reference`] and continue inside it after releasing this borrow, which keeps paths
    /// that pass through the same reference twice (`$a[0] = &$a; $a[0][1] = …`) from re-entering it.
    pub fn slot_mut(&mut self, key: ArrayKey) -> &mut Slot {
        self.data_mut().slot(key)
    }

    /// A new `null` element under the next key, for a write fetch (as for `$array[][…] = …`).
    ///
    /// # Errors
    ///
    /// [`NextIndexOccupied`] when there is no next key; the array is unchanged.
    pub fn next_slot_mut(&mut self) -> Result<(i64, &mut Slot), NextIndexOccupied> {
        self.data_mut().next_slot()
    }

    /// The element's slot if it exists (as for `unset($array[$key][…])`). A missing element leaves
    /// the array untouched and shared storage intact.
    pub fn existing_slot_mut(&mut self, key: &ArrayKey) -> Option<&mut Slot> {
        if !self.contains_key(key) {
            return None;
        }
        self.data_mut().get_mut(key)
    }

    /// `unset($array[$key])`: removes the element and returns its slot. Removing a missing key leaves
    /// the array untouched.
    pub fn remove(&mut self, key: &ArrayKey) -> Option<Slot> {
        if !self.contains_key(key) {
            return None;
        }
        self.data_mut().remove(key)
    }

    #[cfg(test)]
    fn is_packed(&self) -> bool {
        self.0.as_ref().is_none_or(|data| data.mixed.is_none())
    }
}

impl fmt::Debug for PhpArray {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::{NextIndex, NextIndexOccupied, PhpArray};
    use crate::{ArrayKey, Reference, Slot, Value};

    fn int(value: &Value) -> Option<i64> {
        match value {
            Value::Int(number) => Some(*number),
            _ => None,
        }
    }

    fn element(array: &PhpArray, key: impl Into<ArrayKey>) -> Option<i64> {
        array.get(&key.into()).and_then(|slot| slot.with_value(int))
    }

    fn keys(array: &PhpArray) -> Vec<ArrayKey> {
        array.iter().map(|(key, _)| key).collect()
    }

    #[test]
    fn new_arrays_allocate_nothing() {
        let array = PhpArray::new();
        assert!(array.is_empty());
        assert!(array.ptr_eq(&PhpArray::default()));
        assert!(array.ptr_eq(&PhpArray::with_capacity(0)));
        assert!(!array.ptr_eq(&PhpArray::with_capacity(4)));
        assert!(PhpArray::with_capacity(4).is_empty());
        assert_eq!(array.next_index(), NextIndex::Unset);
        assert!(array.get(&ArrayKey::Int(0)).is_none());
        assert_eq!(format!("{array:?}"), "{}");
    }

    #[test]
    fn next_index_follows_the_highest_integer_key() {
        let mut array = PhpArray::new();
        array.assign("x".into(), Value::Int(1));
        assert_eq!(array.append(Value::Int(2)), Ok(0));
        array.assign((-5).into(), Value::Null);
        assert_eq!(array.next_index(), NextIndex::At(1));

        let mut negative = PhpArray::new();
        negative.assign((-5).into(), Value::Null);
        negative.assign((-10).into(), Value::Null);
        assert_eq!(negative.append(Value::Null), Ok(-4));

        let mut string_int = PhpArray::new();
        string_int.assign("5".into(), Value::Null);
        assert_eq!(string_int.append(Value::Null), Ok(6));
        string_int.remove(&ArrayKey::Int(6));
        assert_eq!(
            string_int.append(Value::Null),
            Ok(7),
            "removal never lowers it"
        );

        let mut full = PhpArray::new();
        full.assign(i64::MAX.into(), Value::Int(1));
        assert_eq!(full.next_index(), NextIndex::Exhausted);
        assert_eq!(full.append(Value::Int(2)), Err(NextIndexOccupied));
        assert_eq!(
            full.append_reference(Reference::new(Value::Null)),
            Err(NextIndexOccupied)
        );
        assert!(full.reference_at_next().is_err());
        assert!(full.next_slot_mut().is_err());
        full.assign(0.into(), Value::Null);
        assert_eq!(full.next_index(), NextIndex::Exhausted);
        assert_eq!(full.len(), 2);
        assert_eq!(NextIndex::Exhausted.key(), None);
        assert_eq!(
            NextIndexOccupied.to_string(),
            "Cannot add element to the array as the next element is already occupied"
        );
    }

    #[test]
    fn overwrites_keep_position_and_reinserts_go_last() {
        let mut array = PhpArray::new();
        for key in ["a", "b", "c"] {
            array.assign(key.into(), Value::Int(1));
        }
        assert_eq!(int(&array.assign("a".into(), Value::Int(9))), Some(1));
        assert!(array.remove(&"b".into()).is_some());
        assert!(array.remove(&"b".into()).is_none());
        array.assign("b".into(), Value::Int(5));
        assert_eq!(keys(&array), ["a".into(), "c".into(), "b".into()]);
        assert_eq!(element(&array, "a"), Some(9));
        assert!(array.contains_key(&"c".into()));
        assert_eq!(
            format!("{array:?}"),
            r#"{String(b"a"): Value(Int(9)), String(b"c"): Value(Int(1)), String(b"b"): Value(Int(5))}"#
        );
    }

    #[test]
    fn clones_share_storage_until_written() {
        let mut original = PhpArray::new();
        original.assign(0.into(), Value::Int(1));
        let mut copy = original.clone();
        assert!(copy.ptr_eq(&original));
        copy.assign(0.into(), Value::Int(2));
        assert!(!copy.ptr_eq(&original));
        assert_eq!(element(&original, 0), Some(1));
        assert_eq!(element(&copy, 0), Some(2));

        let mut untouched = original.clone();
        assert!(untouched.remove(&9.into()).is_none());
        assert!(untouched.existing_slot_mut(&9.into()).is_none());
        assert!(untouched.ptr_eq(&original), "misses do not separate");
    }

    #[test]
    fn copies_share_references_bound_elsewhere_only() {
        let mut array = PhpArray::new();
        array.assign(0.into(), Value::Int(1));
        let outside = array.reference_at(0.into());
        let mut copy = array.clone();
        copy.assign(0.into(), Value::Int(2));
        assert_eq!(element(&array, 0), Some(2), "the reference is shared");
        assert_eq!(int(&outside.get()), Some(2));

        drop(outside);
        let mut lone = PhpArray::new();
        lone.assign(0.into(), Value::Int(1));
        drop(lone.reference_at(0.into()));
        let mut copy = lone.clone();
        copy.assign(0.into(), Value::Int(2));
        assert_eq!(
            element(&lone, 0),
            Some(1),
            "a lone reference is copied as a value"
        );
    }

    #[test]
    fn a_lone_reference_holding_its_own_array_stays_shared() {
        let mut slot = Slot::from(Value::Array(PhpArray::new()));
        let reference = slot.make_reference();
        reference.with_mut(|value| {
            if let Value::Array(array) = value {
                array.assign(0.into(), Value::Int(1));
                array.append_reference(reference.clone()).unwrap();
            }
        });
        let Value::Array(mut copy) = reference.get() else {
            panic!("an array was stored");
        };
        drop(slot);
        drop(reference);
        let bound = copy.get(&1.into()).and_then(Slot::as_reference).unwrap();
        assert_eq!(bound.holders(), 1, "only the element is bound");
        assert!(copy.keeps_reference(bound));
        assert!(!PhpArray::new().keeps_reference(bound));

        copy.assign(0.into(), Value::Int(5));
        let bound = copy
            .get(&1.into())
            .and_then(Slot::as_reference)
            .unwrap()
            .clone();
        assert_eq!(bound.holders(), 3, "separation kept both elements bound");
        bound.with_mut(|value| {
            if let Value::Array(array) = value {
                array.assign(0.into(), Value::Int(7));
            }
        });
        let Value::Array(through) = bound.get() else {
            panic!("an array was stored");
        };
        assert_eq!(element(&through, 0), Some(7));
        assert_eq!(element(&copy, 0), Some(5));
    }

    #[test]
    fn references_can_be_bound_and_created() {
        let mut array = PhpArray::new();
        let reference = Reference::new(Value::Int(3));
        assert!(matches!(
            array.bind("r".into(), reference.clone()),
            crate::Slot::Value(Value::Null)
        ));
        assert_eq!(array.append_reference(reference.clone()), Ok(0));
        reference.set(Value::Int(4));
        assert_eq!(element(&array, "r"), Some(4));
        assert_eq!(element(&array, 0), Some(4));
        assert_eq!(int(&array.assign("r".into(), Value::Int(5))), Some(4));
        assert_eq!(int(&reference.get()), Some(5));

        let created = array.reference_at("new".into());
        assert!(matches!(created.get(), Value::Null));
        let (key, appended) = array.reference_at_next().unwrap();
        assert_eq!(key, 1);
        appended.set(Value::Int(6));
        assert_eq!(element(&array, 1), Some(6));
    }

    #[test]
    fn write_fetches_create_null_elements() {
        let mut array = PhpArray::new();
        assert!(matches!(
            array.slot_mut("a".into()),
            Slot::Value(Value::Null)
        ));
        array.slot_mut("a".into()).assign(Value::Int(1));
        let (key, slot) = array.next_slot_mut().unwrap();
        assert_eq!(key, 0);
        slot.assign(Value::Int(2));
        assert_eq!(
            array
                .existing_slot_mut(&"a".into())
                .map(|slot| int(&slot.value())),
            Some(Some(1))
        );
        assert_eq!(element(&array, 0), Some(2));

        let shared = array.clone();
        array.slot_mut("a".into()).assign(Value::Int(3));
        assert_eq!(element(&shared, "a"), Some(1), "write fetches separate");
        assert_eq!(element(&array, "a"), Some(3));
    }

    #[test]
    fn sequential_appends_stay_packed() {
        let mut array = PhpArray::new();
        assert!(array.is_packed());
        for i in 0..64 {
            assert_eq!(array.append(Value::Int(i)), Ok(i));
        }
        assert!(array.is_packed());
        assert_eq!(array.len(), 64);
        assert_eq!(element(&array, 0), Some(0));
        assert_eq!(element(&array, 63), Some(63));
        array.assign("name".into(), Value::Int(1));
        assert!(!array.is_packed());
        assert_eq!(element(&array, 10), Some(10));
        assert_eq!(element(&array, "name"), Some(1));
    }

    #[test]
    fn unique_appends_do_not_copy_prior_elements() {
        let mut array = PhpArray::new();
        let n = 20_000_i64;
        for i in 0..n {
            array.append(Value::Int(i)).unwrap();
        }
        assert_eq!(array.len(), usize::try_from(n).unwrap());
        assert!(array.is_packed());
        assert_eq!(element(&array, 0), Some(0));
        assert_eq!(element(&array, n - 1), Some(n - 1));
        let copy = array.clone();
        array.append(Value::Int(n)).unwrap();
        assert_eq!(copy.len(), usize::try_from(n).unwrap());
        assert_eq!(array.len(), usize::try_from(n).unwrap() + 1);
    }

    #[test]
    fn trailing_removal_keeps_next_index() {
        let mut array = PhpArray::new();
        array.append(Value::Int(1)).unwrap();
        array.append(Value::Int(2)).unwrap();
        assert!(array.remove(&ArrayKey::Int(1)).is_some());
        assert!(array.is_packed());
        assert_eq!(array.append(Value::Int(9)), Ok(2));
        assert!(
            !array.is_packed(),
            "a hole at 1 promotes on the next append"
        );
        assert_eq!(element(&array, 0), Some(1));
        assert_eq!(element(&array, 2), Some(9));
    }
}
