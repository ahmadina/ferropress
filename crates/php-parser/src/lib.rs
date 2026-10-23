//! PHP source parser for the Ferropress runtime.
//!
//! [`parse`] turns PHP source into a syntax tree ([`ast`]). It accepts exactly the programs the
//! reference PHP 8.5 parser accepts and rejects the others with the same error class, message
//! and line, raising the same warnings along the way. [`export`] prints an expression back the way
//! the reference prints the message of a failed `assert()`.
//!
//! The observable behavior is described in `contracts/runtime/parser.md` and checked against cases
//! captured from the reference (`contracts/parser/cases.jsonl`).
//!
//! # Examples
//!
//! ```
//! use php_parser::{ParseOptions, ast::{StmtKind, TopStatement}, export, parse};
//!
//! let parsed = parse(b"<?php echo 1 + 2 * 3;", ParseOptions::default()).expect("small source");
//! let program = parsed.result.expect("valid source");
//! let TopStatement::Statement(statement) = &program.statements[0] else { unreachable!() };
//! let StmtKind::Echo(value) = &statement.kind else { unreachable!() };
//! assert_eq!(export(value), b"1 + 2 * 3");
//! ```

pub mod ast;
mod describe;
mod error;
mod export;
mod float;
mod literal;
mod parser;
mod stream;

pub use error::{ErrorClass, ParseError, Severity, Warning};
pub use export::export;
pub use php_lexer::SourceTooLarge;

use php_lexer::LexerOptions;

/// Settings that change how source is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseOptions {
    /// Whether `<?` opens PHP code (the `short_open_tag` setting, on by default).
    pub short_open_tag: bool,
    /// Whether a first line starting with `#!` is skipped, as when a file is run.
    pub skip_shebang: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            short_open_tag: true,
            skip_shebang: true,
        }
    }
}

/// The outcome of parsing a file.
#[derive(Debug, Clone, PartialEq)]
pub struct Parse {
    /// The tree, or the error that stopped parsing.
    pub result: Result<ast::Program, ParseError>,
    /// Warnings and deprecations raised before parsing finished or stopped, in order.
    pub warnings: Vec<Warning>,
}

/// Why source could not be parsed at all.
#[derive(Debug, thiserror::Error)]
pub enum ParseFailure {
    /// The source does not fit the 32-bit offsets tokens use.
    #[error(transparent)]
    SourceTooLarge(#[from] SourceTooLarge),
    /// The thread parsing runs on could not be started.
    #[error("cannot start the parsing thread: {0}")]
    Thread(#[source] std::io::Error),
}

/// Stack of the thread parsing runs on: room for [`MAX_NESTING`] levels with a margin. It is
/// reserved address space; memory is only committed as deeply nested input uses it.
const STACK_SIZE: usize = if cfg!(debug_assertions) {
    512 << 20
} else {
    64 << 20
};

/// Deepest nesting of statements and operands [`parse`] accepts; deeper input fails with the
/// reference's `memory exhausted` error. A tree's depth is bounded by this nesting and by the length
/// of its operator chains; walking or dropping a tree recurses that deep.
pub use parser::MAX_NESTING;

/// Parses PHP source.
///
/// Parsing runs on a scoped thread whose stack fits the deepest nesting accepted, so hostile input
/// cannot exhaust the caller's stack.
///
/// # Errors
///
/// Returns [`ParseFailure`] when the source is too large or the parsing thread cannot start; errors
/// in the source itself are reported in [`Parse::result`].
pub fn parse(source: &[u8], options: ParseOptions) -> Result<Parse, ParseFailure> {
    let lexer_options = LexerOptions {
        short_open_tag: options.short_open_tag,
        skip_shebang: options.skip_shebang,
    };
    let stream = stream::Stream::new(source, lexer_options)?;
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .name("php-parser".to_owned())
            .stack_size(STACK_SIZE)
            .spawn_scoped(scope, move || {
                let mut parser = parser::Parser::new(stream);
                let result = parser.program().map_err(|error| *error);
                let warnings = parser.into_stream().into_warnings();
                Parse { result, warnings }
            })
            .map_err(ParseFailure::Thread)?;
        worker
            .join()
            .map_err(|payload| std::panic::resume_unwind(payload))
    })
}

#[cfg(test)]
mod tests {
    use super::{ErrorClass, MAX_NESTING, ParseFailure, ParseOptions, parse};

    fn nested_arrays(depth: usize) -> Vec<u8> {
        let mut source = b"<?php\n".to_vec();
        source.extend(std::iter::repeat_n(b'[', depth));
        source.push(b'1');
        source.extend(std::iter::repeat_n(b']', depth));
        source.push(b';');
        source
    }

    #[test]
    fn nesting_beyond_the_reference_stack_is_exhausted() {
        let Ok(parsed) = parse(&nested_arrays(MAX_NESTING + 1), ParseOptions::default()) else {
            panic!("source fits");
        };
        let Err(error) = parsed.result else {
            panic!("nesting this deep fails");
        };
        assert_eq!(error.class, ErrorClass::Parse);
        assert_eq!(error.message, b"memory exhausted");
        assert_eq!(error.line, Some(2));
        let Ok(parsed) = parse(&nested_arrays(1000), ParseOptions::default()) else {
            panic!("source fits");
        };
        assert!(parsed.result.is_ok());
    }

    #[test]
    fn failures_describe_their_cause() {
        let thread = ParseFailure::Thread(std::io::Error::other("no threads"));
        assert_eq!(
            thread.to_string(),
            "cannot start the parsing thread: no threads"
        );
    }

    #[test]
    fn options_default_to_running_a_file() {
        let options = ParseOptions::default();
        assert!(options.short_open_tag);
        assert!(options.skip_shebang);
    }

    #[test]
    fn errors_stop_parsing() {
        let Ok(parsed) = parse(b"<?php 1 +;", ParseOptions::default()) else {
            panic!("source fits");
        };
        let Err(error) = parsed.result else {
            panic!("syntax error expected");
        };
        assert_eq!(error.class, ErrorClass::Parse);
        assert_eq!(error.message, b"syntax error, unexpected token \";\"");
        assert_eq!(error.line, Some(1));
    }
}
