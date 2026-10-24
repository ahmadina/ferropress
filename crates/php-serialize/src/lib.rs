//! Byte-exact reader and writer for PHP's serialized value format.
//!
//! This crate owns the format that PHP's `serialize()` writes and `unserialize()` reads, as
//! described in `contracts/runtime/serialized-format.md`:
//!
//! - [`Writer`] emits each token exactly as PHP does, including PHP's shortest round-trip float
//!   notation.
//! - [`decode`] accepts exactly the inputs PHP accepts, reports the error offset PHP reports and
//!   the diagnostics PHP emits, for every rule that depends only on the bytes.
//! - [`Document`] holds a decoded value flat in pre-order and writes it back in canonical form.
//!
//! It does not own PHP value semantics: class lookup and autoloading, `allowed_classes`,
//! `__serialize`/`__unserialize`/`__sleep`/`__wakeup`/`Serializable`, enum case resolution,
//! array key normalization, and turning `R:`/`r:` numbers into a reference graph belong to the
//! runtime, which drives this crate. Decoding treats classes as PHP does when no class may be
//! instantiated.

mod decode;
mod document;
mod error;
mod write;

pub use decode::{DecodeOptions, decode, decode_with};
pub use document::{Document, Entries, Key, Value};
pub use error::{DecodeError, Diagnostic, Severity};
pub use write::Writer;
