//! Byte-exact PHP source lexer: tokens, positions, lines and scanner diagnostics.
//!
//! [`Lexer`] turns source bytes into [`Token`]s the way the reference engine's scanner does, so that
//! every byte belongs to exactly one token, tokens carry the line they start on, and the errors and
//! warnings the scanner raises are reported in order. [`tokenize`] adds the `__halt_compiler`
//! handling of PHP's `token_get_all()`. The rules are described in `contracts/runtime/lexer.md`
//! and checked against the reference PHP by the captures in `contracts/lexer/`.
//!
//! Only the tokenizer view is produced here: whitespace, comments and open and close tags are
//! tokens. Dropping them, stripping heredoc indentation and turning scanner errors into parse
//! errors belong to the parser.
//!
//! ```
//! use php_lexer::{LexerOptions, TokenKind, tokenize};
//!
//! let source = b"<?php $greeting = \"Hi $name\";";
//! let tokens = tokenize(source, LexerOptions::default())?.tokens;
//! let names: Vec<_> = tokens.iter().map(|token| token.kind.name().unwrap_or("char")).collect();
//! assert_eq!(names[..6], ["T_OPEN_TAG", "T_VARIABLE", "T_WHITESPACE", "char", "T_WHITESPACE", "char"]);
//! assert_eq!(tokens[6].kind, TokenKind::StringFragment);
//! # Ok::<(), php_lexer::SourceTooLarge>(())
//! ```

mod diagnostic;
mod escape;
mod keyword;
mod lexer;
mod number;
mod token;
mod tokenize;

pub use diagnostic::{Diagnostic, LexError, LexWarning};
pub use escape::{Decoded, Quote, decode_double_quoted, decode_single_quoted};
pub use lexer::{Lexer, LexerOptions, SourceTooLarge};
pub use token::{Token, TokenKind};
pub use tokenize::{Tokens, tokenize};
