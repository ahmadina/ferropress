//! The PHP compiler.
//!
//! [`check`] runs the compile-time checks of the reference compiler over a parsed program and
//! returns the fatal error, warnings and deprecations PHP reports for it, each on the line PHP
//! reports it on. The cases it is proven against are captured from the reference PHP into
//! `contracts/compiler/cases.jsonl`; the design is recorded in ADR-0007.
//!
//! [`bytecode`] defines what compiling produces for the VM: units, code objects and instructions,
//! with a disassembly for tests. [`compile`] checks a program and, when it has no fatal error,
//! emits its unit.

pub mod bytecode;
mod check;
mod diagnostic;
mod emit;

pub use diagnostic::{Diagnostic, Diagnostics, Severity};
pub use emit::Failure;

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

/// Why [`compile`] could not finish.
#[derive(Debug, thiserror::Error)]
pub enum CompileFailure {
    /// The thread compiling runs on could not be started.
    #[error("cannot start the compiling thread: {0}")]
    Thread(#[source] std::io::Error),
    /// The program passed the checks but uses what the compiler cannot emit yet.
    #[error(transparent)]
    Emit(#[from] Failure),
}

/// What compiling a program produced.
#[derive(Debug)]
pub struct Compiled {
    /// The checks' warnings and fatal error, as [`check`] reports them.
    pub diagnostics: Diagnostics,
    /// The unit, absent when the checks found a fatal error.
    pub unit: Option<bytecode::Unit>,
}

/// Runs `body` on a scoped thread whose stack fits the deepest tree the parser builds, so deeply
/// nested input cannot exhaust the caller's stack.
fn on_big_stack<T: Send>(body: impl FnOnce() -> T + Send) -> std::io::Result<T> {
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("php-compiler".to_owned())
            .stack_size(STACK_SIZE)
            .spawn_scoped(scope, body)?;
        Ok(worker
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload)))
    })
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
    on_big_stack(|| check::Checker::run(program, file)).map_err(CheckFailure::Thread)
}

/// Checks `program`, compiled from the file at path `file`, and emits its unit when the checks
/// find no fatal error. Like [`check`], it runs on a thread with a stack for the deepest tree.
///
/// # Errors
///
/// Returns [`CompileFailure::Thread`] when the compiling thread cannot start, and
/// [`CompileFailure::Emit`] when the program uses a construct not compiled yet or exceeds the
/// bytecode's limits.
pub fn compile(program: &Program, file: &[u8]) -> Result<Compiled, CompileFailure> {
    on_big_stack(|| {
        let diagnostics = check::Checker::run(program, file);
        let unit = match diagnostics.fatal {
            Some(_) => None,
            None => Some(emit::run(program, file)?),
        };
        Ok(Compiled { diagnostics, unit })
    })
    .map_err(CompileFailure::Thread)?
}
