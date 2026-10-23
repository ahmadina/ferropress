//! VM-neutral PHP value model: strings, ordered arrays, references and object handles.
//!
//! The rules implemented here are described in `contracts/runtime/value-model.md` and checked against
//! the reference PHP by the captures in `contracts/value/`.
//!
//! Values have PHP's sharing semantics: [`PhpString`] and [`PhpArray`] are shared until one side
//! changes, [`Object`] is a handle whose clones name the same object, and [`Reference`] is the cell
//! behind PHP's `&`, held by [`Slot`]s. Everything is reference-counted with `Rc`, so values belong to
//! one request on one thread and cannot be sent elsewhere.
//!
//! Conversions between types, comparison, offsets of other types than integers and strings, string
//! offsets, cycle collection and destructors belong to the engine and are not modeled here.
//!
//! ```
//! use php_value::{ArrayKey, PhpArray, Value};
//!
//! let mut original = PhpArray::new();
//! original.assign(ArrayKey::from("5"), Value::from("five"));
//! let mut copy = original.clone();
//! assert_eq!(copy.append(Value::Null), Ok(6));
//! assert_eq!((original.len(), copy.len()), (1, 2));
//! ```

mod array;
mod key;
mod object;
mod reference;
mod string;
mod table;
mod value;

pub use array::{NextIndex, NextIndexOccupied, PhpArray};
pub use key::ArrayKey;
pub use object::{Object, ObjectId, ObjectIdsExhausted, ObjectStore, Properties};
pub use reference::{Reference, Slot};
pub use string::PhpString;
pub use value::Value;
