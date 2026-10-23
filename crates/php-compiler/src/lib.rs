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

/// Runs the compile-time checks over `program`.
#[must_use]
pub fn check(program: &Program) -> Diagnostics {
    check::Checker::run(program)
}
