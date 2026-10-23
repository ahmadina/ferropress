//! Object handles, their identities and their property tables.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::table::Table;
use crate::{PhpString, Reference, Slot, Value};

/// An object's identity number (`spl_object_id`), unique among live objects of one request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjectId(u32);

impl ObjectId {
    /// The number PHP reports for this object.
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Every object identity number is in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("no object identity number is free")]
pub struct ObjectIdsExhausted;

#[derive(Debug, Default)]
struct IdPool {
    issued: u32,
    free: Vec<u32>,
}

/// Creates the objects of one request and numbers them.
///
/// Numbers start at 1. The number of a destroyed object is reused by the next object created, the most
/// recently freed number first. An object frees its properties, in order, before its own number.
#[derive(Debug, Default)]
pub struct ObjectStore {
    ids: Rc<RefCell<IdPool>>,
}

impl ObjectStore {
    /// A store with no objects.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an object of class `class` with no properties.
    ///
    /// # Errors
    ///
    /// [`ObjectIdsExhausted`] when every identity number (up to `u32::MAX`) belongs to a live object.
    pub fn create(&self, class: PhpString) -> Result<Object, ObjectIdsExhausted> {
        let id = {
            let mut pool = self.ids.borrow_mut();
            if let Some(id) = pool.free.pop() {
                id
            } else {
                pool.issued = pool.issued.checked_add(1).ok_or(ObjectIdsExhausted)?;
                pool.issued
            }
        };
        Ok(Object(Rc::new(ObjectData {
            id: ObjectId(id),
            class,
            properties: RefCell::default(),
            ids: Rc::clone(&self.ids),
        })))
    }

    /// Number of objects alive.
    pub fn live_objects(&self) -> usize {
        let pool = self.ids.borrow();
        pool.issued as usize - pool.free.len()
    }
}

struct ObjectData {
    id: ObjectId,
    class: PhpString,
    properties: RefCell<Properties>,
    ids: Rc<RefCell<IdPool>>,
}

impl Drop for ObjectData {
    fn drop(&mut self) {
        drop(std::mem::take(self.properties.get_mut()));
        self.ids.borrow_mut().free.push(self.id.0);
    }
}

/// A handle to a PHP object. Clones are further handles to the same object; equality is identity.
/// Maps keyed by object use [`Object::id`].
///
/// # Panics
///
/// Properties are borrowed only for the duration of [`Object::with_properties`] and
/// [`Object::with_properties_mut`]; re-entering the same object from inside `with_properties_mut`
/// panics. Object handles are cheap to clone out of a property before working on them.
#[derive(Clone)]
pub struct Object(Rc<ObjectData>);

impl Object {
    /// The object's identity number.
    pub fn id(&self) -> ObjectId {
        self.0.id
    }

    /// The name of the object's class as given at creation.
    pub fn class_name(&self) -> &PhpString {
        &self.0.class
    }

    /// Runs `f` on the object's properties.
    pub fn with_properties<R>(&self, f: impl FnOnce(&Properties) -> R) -> R {
        f(&self.0.properties.borrow())
    }

    /// Runs `f` on the object's properties, in place.
    ///
    /// # Panics
    ///
    /// When called again for the same object from inside `f`; take an [`Object`] handle out of a
    /// property and continue after `f` returns instead.
    pub fn with_properties_mut<R>(&self, f: impl FnOnce(&mut Properties) -> R) -> R {
        f(&mut self.0.properties.borrow_mut())
    }
}

impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Object {}

impl fmt::Debug for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Object")
            .field("id", &self.0.id.0)
            .field("class", &self.0.class)
            .finish_non_exhaustive()
    }
}

/// An object's dynamic properties: an insertion-ordered map from names to slots.
///
/// Names are kept exactly as given; unlike array keys, a numeric name stays a string.
#[derive(Debug, Default)]
pub struct Properties(Table<PhpString>);

impl Properties {
    /// Number of properties.
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether there are no properties.
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The slot of property `name`.
    pub fn get(&self, name: &[u8]) -> Option<&Slot> {
        self.0.get(name)
    }

    /// The properties in order.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&PhpString, &Slot)> {
        self.0.iter()
    }

    /// `$object->name = $value`: assigns through a reference bound there, otherwise stores the value
    /// (appending the property when new). Returns the value it replaced.
    pub fn assign(&mut self, name: PhpString, value: Value) -> Value {
        self.slot(name).assign(value)
    }

    /// `$object->name = &$other`: binds the property to `reference`, keeping its position when present.
    /// Returns the slot it replaced.
    pub fn bind(&mut self, name: PhpString, reference: Reference) -> Slot {
        std::mem::replace(self.slot(name), Slot::Reference(reference))
    }

    /// `&$object->name`: makes the property a reference (creating it as `null` when absent) and returns
    /// a binding of it.
    pub fn reference_at(&mut self, name: PhpString) -> Reference {
        self.slot(name).make_reference()
    }

    /// The property's slot for a write fetch, creating it as `null` when absent.
    pub fn slot_mut(&mut self, name: PhpString) -> &mut Slot {
        self.slot(name)
    }

    /// The property's slot if it exists.
    pub fn existing_slot_mut(&mut self, name: &[u8]) -> Option<&mut Slot> {
        self.0.get_mut(name)
    }

    /// `unset($object->name)`: removes the property and returns its slot.
    pub fn remove(&mut self, name: &[u8]) -> Option<Slot> {
        self.0.remove(name)
    }

    fn slot(&mut self, name: PhpString) -> &mut Slot {
        self.0
            .slot_or_insert_with(name, || Slot::Value(Value::Null))
            .0
    }
}

#[cfg(test)]
mod tests {
    use super::{ObjectId, ObjectStore};
    use crate::{PhpString, Reference, Slot, Value};
    use std::collections::HashSet;

    fn ids(objects: &[super::Object]) -> Vec<u32> {
        objects.iter().map(|object| object.id().get()).collect()
    }

    #[test]
    fn ids_start_at_one_and_reuse_the_last_freed_first() {
        let store = ObjectStore::new();
        let mut objects: Vec<_> = (0..3)
            .map(|_| store.create("stdClass".into()).unwrap())
            .collect();
        assert_eq!(ids(&objects), [1, 2, 3]);
        assert_eq!(store.live_objects(), 3);
        let second = objects.remove(1);
        let first = objects.remove(0);
        drop(second);
        drop(first);
        assert_eq!(store.live_objects(), 1);
        let reused: Vec<_> = (0..3)
            .map(|_| store.create("stdClass".into()).unwrap())
            .collect();
        assert_eq!(ids(&reused), [1, 2, 4]);
    }

    #[test]
    fn properties_are_freed_before_their_owner() {
        let store = ObjectStore::new();
        let parent = store.create("stdClass".into()).unwrap();
        for name in ["c", "d"] {
            let child = store.create("stdClass".into()).unwrap();
            parent.with_properties_mut(|properties| {
                properties.assign(name.into(), Value::Object(child))
            });
        }
        drop(parent);
        let next: Vec<_> = (0..3)
            .map(|_| store.create("stdClass".into()).unwrap())
            .collect();
        assert_eq!(ids(&next), [1, 3, 2]);
    }

    #[test]
    fn handles_compare_by_identity() {
        let store = ObjectStore::new();
        let object = store.create("Shop\\Item".into()).unwrap();
        let other = store.create("Shop\\Item".into()).unwrap();
        assert_eq!(object, object.clone());
        assert_ne!(object, other);
        assert_eq!(object.class_name(), &PhpString::from("Shop\\Item"));
        let ids: HashSet<_> = [&object, &object, &other].iter().map(|o| o.id()).collect();
        assert_eq!(ids.len(), 2);
        assert_eq!(
            format!("{object:?}"),
            r#"Object { id: 1, class: b"Shop\\Item", .. }"#
        );
        assert!(ObjectId(1) < ObjectId(2));
    }

    #[test]
    fn exhausted_ids_are_reported() {
        let store = ObjectStore::new();
        store.ids.borrow_mut().issued = u32::MAX;
        assert_eq!(store.create("A".into()), Err(super::ObjectIdsExhausted));
        assert_eq!(
            super::ObjectIdsExhausted.to_string(),
            "no object identity number is free"
        );
    }

    #[test]
    fn properties_keep_order_and_numeric_names() {
        let store = ObjectStore::new();
        let object = store.create("stdClass".into()).unwrap();
        object.with_properties_mut(|properties| {
            properties.assign("5".into(), Value::Int(1));
            properties.assign("b".into(), Value::Int(2));
            properties.assign("a".into(), Value::Int(3));
            assert!(properties.remove(b"b").is_some());
            assert!(properties.remove(b"b").is_none());
            properties.assign("b".into(), Value::Int(4));
            properties.slot_mut("c".into()).assign(Value::Int(5));
            assert!(properties.existing_slot_mut(b"zz").is_none());
            assert!(properties.existing_slot_mut(b"c").is_some());
        });
        object.with_properties(|properties| {
            let names: Vec<&[u8]> = properties.iter().map(|(name, _)| name.as_bytes()).collect();
            assert_eq!(names, [&b"5"[..], b"a", b"b", b"c"]);
            assert_eq!(properties.len(), 4);
            assert!(!properties.is_empty());
            assert!(
                properties
                    .get(b"a")
                    .is_some_and(|slot| matches!(slot.value(), Value::Int(3)))
            );
        });
    }

    #[test]
    fn properties_can_be_references() {
        let store = ObjectStore::new();
        let object = store.create("stdClass".into()).unwrap();
        let reference = Reference::new(Value::Int(1));
        object.with_properties_mut(|properties| {
            assert!(matches!(
                properties.bind("r".into(), reference.clone()),
                Slot::Value(Value::Null)
            ));
            properties.assign("r".into(), Value::Int(2));
            let created = properties.reference_at("n".into());
            created.set(Value::Int(3));
            assert!(
                properties
                    .get(b"n")
                    .is_some_and(|slot| matches!(slot.value(), Value::Int(3)))
            );
        });
        assert!(matches!(reference.get(), Value::Int(2)));
    }
}
