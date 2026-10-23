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

/// Parses PHP source.
///
/// # Errors
///
/// Returns [`SourceTooLarge`] when the source does not fit the 32-bit offsets tokens use; errors in
/// the source itself are reported in [`Parse::result`].
pub fn parse(source: &[u8], options: ParseOptions) -> Result<Parse, SourceTooLarge> {
    let lexer_options = LexerOptions {
        short_open_tag: options.short_open_tag,
        skip_shebang: options.skip_shebang,
    };
    let stream = stream::Stream::new(source, lexer_options)?;
    let mut parser = parser::Parser::new(stream);
    let result = parser.program().map_err(|error| *error);
    let warnings = parser.into_stream().into_warnings();
    Ok(Parse { result, warnings })
}

#[cfg(test)]
mod tests {
    use super::{ErrorClass, ParseOptions, parse};

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
