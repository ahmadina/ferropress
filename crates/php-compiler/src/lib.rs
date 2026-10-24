//! The PHP compiler.
//!
//! [`check`] runs the compile-time checks of the reference compiler over a parsed program and
//! returns the fatal error, warnings and deprecations PHP reports for it, each on the line PHP
//! reports it on. The cases it is proven against are captured from the reference PHP into
//! `contracts/compiler/cases.jsonl`; the design is recorded in ADR-0007.

mod check;
mod diagnostic;

pub use diagnostic::{Diagnostic, Diagnostics, Severity};

use php_parser::ast::Program;

/// Stack of the thread checking runs on: room for a tree as deep as [`php_parser::MAX_NESTING`]
/// with a margin. It is reserved address space; memory is only committed as deep trees use it.
const STACK_SIZE: usize = if cfg!(debug_assertions) {
    512 << 20
} else {
    64 << 20
};

/// Why [`check`] could not run.
#[derive(Debug, thiserror::Error)]
pub enum CheckFailure {
    /// The thread checking runs on could not be started.
    #[error("cannot start the checking thread: {0}")]
    Thread(#[source] std::io::Error),
}

/// Runs the compile-time checks over `program`, compiled from the file at path `file`, which
/// messages and generated closure names include.
///
/// Checking runs on a scoped thread whose stack fits the deepest tree the parser builds, so deeply
/// nested input cannot exhaust the caller's stack.
///
/// # Errors
///
/// Returns [`CheckFailure`] when the checking thread cannot start; what the source itself gets
/// wrong is reported in the returned [`Diagnostics`].
pub fn check(program: &Program, file: &[u8]) -> Result<Diagnostics, CheckFailure> {
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("php-compiler".to_owned())
            .stack_size(STACK_SIZE)
            .spawn_scoped(scope, || check::Checker::run(program, file))
            .map_err(CheckFailure::Thread)?;
        worker
            .join()
            .map_err(|payload| std::panic::resume_unwind(payload))
    })
}
