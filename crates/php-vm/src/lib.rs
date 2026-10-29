//! The PHP VM core (ADR-0008).
//!
//! [`run`] executes a compiled [`Unit`] and writes what the script prints, including the warnings
//! and uncaught errors the reference CLI displays with `display_errors=1` and `html_errors=0`, to an
//! [`Output`]. Calls between PHP functions use an explicit frame stack, never Rust recursion.
//!
//! Every behavior is proven against the reference: `contracts/vm/cases.jsonl` holds scripts with the
//! output and exit status the reference produced, recorded by `cargo xtask lab vm`.

mod arrays;
mod autoload;
mod builtins;
mod calls;
mod classes;
mod closures;
mod datetime;
mod error;
mod exceptions;
mod fibers;
mod foreach;
mod generators;
mod includes;
mod isset;
mod json;
mod machine;
mod math;
mod mbstring;
mod paths;
mod replace;
mod sprintf;
mod streams;
mod strings;
mod thrown;

pub use error::VmError;

use php_compiler::bytecode::Unit;

/// Where a script's output goes.
pub trait Output {
    /// Receives the next bytes the script prints.
    fn write(&mut self, bytes: &[u8]);
}

impl Output for Vec<u8> {
    fn write(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }
}

/// Runs `unit`, compiled from the file at path `file`, writing its output to `output`, and returns
/// the exit status: 0 when the script ends normally, 255 after an uncaught error.
///
/// # Errors
///
/// Returns [`VmError`] when the bytecode is malformed or needs what this VM cannot run; what the
/// script itself gets wrong is reported in its output, as the reference reports it.
pub fn run(unit: &Unit, file: &[u8], output: &mut impl Output) -> Result<u8, VmError> {
    machine::Machine::new(unit, file, output).run()
}
