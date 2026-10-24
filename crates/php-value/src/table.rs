//! The insertion-ordered slot table behind arrays and object properties.
//!
//! Entries live in a vector in insertion order; removal leaves a hole so that positions stay stable,
//! and holes are dropped once they outnumber live entries. Small tables are searched linearly; a hash
//! index (DoS-resistant, randomly keyed) is built once a table grows past [`INDEX_THRESHOLD`] entries,
//! since keys can come from request data.

use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;

use crate::{Reference, Slot};

/// Largest table searched without a hash index.
const INDEX_THRESHOLD: usize = 8;

#[derive(Debug)]
pub(crate) struct Entry<K> {
    pub(crate) key: K,
    pub(crate) slot: Slot,
}

#[derive(Debug)]
pub(crate) struct Table<K> {
    entries: Vec<Option<Entry<K>>>,
    index: Option<HashMap<K, usize>>,
    live: usize,
}

impl<K> Default for Table<K> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            index: None,
            live: 0,
        }
    }
}

impl<K: Eq + Hash + Clone> Table<K> {
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            index: None,
            live: 0,
        }
    }

    pub(crate) const fn len(&self) -> usize {
        self.live
    }

    fn position<Q>(&self, key: &Q) -> Option<usize>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        match &self.index {
            Some(index) => index.get(key).copied(),
            None => self.entries.iter().position(|entry| {
                entry
                    .as_ref()
                    .is_some_and(|entry| entry.key.borrow() == key)
            }),
        }
    }

    pub(crate) fn get<Q>(&self, key: &Q) -> Option<&Slot>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        let position = self.position(key)?;
        self.entries[position].as_ref().map(|entry| &entry.slot)
    }

    pub(crate) fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut Slot>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        let position = self.position(key)?;
        self.entries[position].as_mut().map(|entry| &mut entry.slot)
    }

    /// The slot for `key`, appended with `make()` when absent. The flag tells whether it was added.
    pub(crate) fn slot_or_insert_with(
        &mut self,
        key: K,
        make: impl FnOnce() -> Slot,
    ) -> (&mut Slot, bool) {
        let found = self.position(&key);
        let position = found.unwrap_or_else(|| self.reserve(&key));
        let entry = self.entries[position].get_or_insert_with(|| Entry { key, slot: make() });
        (&mut entry.slot, found.is_none())
    }

    /// Appends an empty position for a key known to be absent, to be filled by the caller.
    fn reserve(&mut self, key: &K) -> usize {
        let position = self.entries.len();
        self.entries.push(None);
        self.live += 1;
        if self.index.is_none() && self.live > INDEX_THRESHOLD {
            self.index = Some(self.build_index());
        }
        if let Some(index) = &mut self.index {
            index.insert(key.clone(), position);
        }
        position
    }

    pub(crate) fn remove<Q>(&mut self, key: &Q) -> Option<Slot>
    where
        K: Borrow<Q>,
        Q: Eq + Hash + ?Sized,
    {
        let position = self.position(key)?;
        if let Some(index) = &mut self.index {
            index.remove(key);
        }
        let entry = self.entries[position].take()?;
        self.live -= 1;
        while self.entries.last().is_some_and(Option::is_none) {
            self.entries.pop();
        }
        let holes = self.entries.len() - self.live;
        if holes > self.live && self.entries.len() > INDEX_THRESHOLD {
            self.entries.retain(Option::is_some);
            if self.index.is_some() {
                self.index = Some(self.build_index());
            }
        }
        Some(entry.slot)
    }

    pub(crate) fn iter(&self) -> impl DoubleEndedIterator<Item = (&K, &Slot)> {
        self.entries
            .iter()
            .flatten()
            .map(|entry| (&entry.key, &entry.slot))
    }

    /// The first entry at or after `position`, with its own position, for walking a table that is
    /// not modified meanwhile.
    pub(crate) fn entry_from(&self, position: usize) -> Option<(usize, &K, &Slot)> {
        self.entries
            .get(position..)?
            .iter()
            .enumerate()
            .find_map(|(offset, entry)| {
                entry
                    .as_ref()
                    .map(|entry| (position + offset, &entry.key, &entry.slot))
            })
    }

    fn build_index(&self) -> HashMap<K, usize> {
        let mut index = HashMap::with_capacity(self.live);
        for (position, entry) in self.entries.iter().enumerate() {
            if let Some(entry) = entry {
                index.insert(entry.key.clone(), position);
            }
        }
        index
    }

    /// The table a copy of its container gets, without holes (see [`Slot::duplicate`]).
    pub(crate) fn duplicate(&self, keeps: impl Fn(&Reference) -> bool) -> Self {
        let mut entries = Vec::with_capacity(self.live);
        entries.extend(self.entries.iter().flatten().map(|entry| {
            Some(Entry {
                key: entry.key.clone(),
                slot: entry.slot.duplicate(&keeps),
            })
        }));
        let mut table = Self {
            entries,
            index: None,
            live: self.live,
        };
        if self.live > INDEX_THRESHOLD {
            table.index = Some(table.build_index());
        }
        table
    }
}

#[cfg(test)]
mod tests {
    use super::{INDEX_THRESHOLD, Table};
    use crate::{Slot, Value};
    use proptest::prelude::*;

    fn int(slot: Option<&Slot>) -> Option<i64> {
        slot.and_then(|slot| match slot.value() {
            Value::Int(number) => Some(number),
            _ => None,
        })
    }

    fn insert(table: &mut Table<u16>, key: u16, value: i64) -> bool {
        let (slot, added) = table.slot_or_insert_with(key, || Slot::from(Value::Null));
        slot.assign(Value::Int(value));
        added
    }

    #[test]
    fn entries_keep_insertion_order_across_overwrites_and_removals() {
        let mut table = Table::with_capacity(4);
        assert!(insert(&mut table, 3, 30));
        assert!(insert(&mut table, 1, 10));
        assert!(!insert(&mut table, 3, 31));
        assert!(insert(&mut table, 2, 20));
        assert_eq!(int(table.remove(&1).as_ref()), Some(10));
        assert!(table.remove(&1).is_none());
        assert!(insert(&mut table, 1, 11));
        let keys: Vec<u16> = table.iter().map(|(key, _)| *key).collect();
        assert_eq!(keys, [3, 2, 1]);
        assert_eq!(table.len(), 3);
        assert_eq!(int(table.get(&3)), Some(31));
        if let Some(slot) = table.get_mut(&2) {
            slot.assign(Value::Int(21));
        }
        assert_eq!(int(table.get(&2)), Some(21));
        assert!(table.get_mut(&9).is_none());
    }

    #[test]
    fn large_tables_are_indexed_and_compacted() {
        let mut table = Table::default();
        let count = u16::try_from(INDEX_THRESHOLD * 4).unwrap();
        for key in 0..count {
            insert(&mut table, key, i64::from(key));
        }
        assert!(table.index.is_some());
        for key in (0..count).filter(|key| key % 4 != 0) {
            assert!(table.remove(&key).is_some());
        }
        assert_eq!(table.len(), INDEX_THRESHOLD);
        let holes = table.entries.len() - table.len();
        assert!(
            holes <= table.len(),
            "holes never outnumber entries: {holes}"
        );
        assert!(
            table.entries.len() < usize::from(count),
            "holes were dropped"
        );
        for key in (0..count).step_by(4) {
            assert_eq!(int(table.get(&key)), Some(i64::from(key)));
        }
        let copy = table.duplicate(|_| false);
        assert!(copy.index.is_none());
        assert_eq!(copy.iter().count(), INDEX_THRESHOLD);
        insert(&mut table, 1000, 1);
        assert_eq!(
            table
                .duplicate(|_| false)
                .iter()
                .map(|(k, _)| *k)
                .next_back(),
            Some(1000)
        );
    }

    #[test]
    fn trailing_holes_are_dropped() {
        let mut table = Table::default();
        insert(&mut table, 1, 1);
        insert(&mut table, 2, 2);
        table.remove(&2);
        assert_eq!(table.entries.len(), 1);
    }

    #[derive(Debug, Clone)]
    enum Op {
        Insert(u16, i64),
        Remove(u16),
    }

    fn op() -> impl Strategy<Value = Op> {
        prop_oneof![
            (0_u16..40, any::<i64>()).prop_map(|(k, v)| Op::Insert(k, v)),
            (0_u16..40).prop_map(Op::Remove),
        ]
    }

    proptest! {
        #[test]
        fn behaves_like_an_ordered_association_list(ops in proptest::collection::vec(op(), 0..300)) {
            let mut table = Table::default();
            let mut model: Vec<(u16, i64)> = Vec::new();
            for op in ops {
                match op {
                    Op::Insert(key, value) => {
                        insert(&mut table, key, value);
                        match model.iter_mut().find(|(k, _)| *k == key) {
                            Some(entry) => entry.1 = value,
                            None => model.push((key, value)),
                        }
                    }
                    Op::Remove(key) => {
                        let removed = int(table.remove(&key).as_ref());
                        let expected = model.iter().position(|(k, _)| *k == key).map(|at| model.remove(at).1);
                        prop_assert_eq!(removed, expected);
                    }
                }
                let seen: Vec<(u16, i64)> = table.iter().map(|(k, s)| (*k, int(Some(s)).unwrap_or_default())).collect();
                prop_assert_eq!(&seen, &model);
                prop_assert_eq!(table.len(), model.len());
            }
            for (key, value) in &model {
                prop_assert_eq!(int(table.get(key)), Some(*value));
            }
            let copy: Vec<(u16, i64)> = table.duplicate(|_| false).iter().map(|(k, s)| (*k, int(Some(s)).unwrap_or_default())).collect();
            prop_assert_eq!(copy, model);
        }
    }
}
