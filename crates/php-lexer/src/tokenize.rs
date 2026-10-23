//! Whole-source tokenizing with PHP's `__halt_compiler` handling.

use crate::diagnostic::Diagnostic;
use crate::lexer::{Lexer, LexerOptions, SourceTooLarge, offset32};
use crate::token::{Token, TokenKind};

/// Every token of a source and the diagnostics raised while reading it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tokens {
    /// The tokens, in source order.
    pub tokens: Vec<Token>,
    /// Errors and warnings, in the order they were raised.
    pub diagnostics: Vec<Diagnostic>,
}

/// Reads `source` the way PHP's `token_get_all()` does without flags.
///
/// After `__halt_compiler`, three more tokens other than whitespace, comments and open tags are
/// read; whatever follows them becomes one inline HTML token on the line of the third, and reading
/// stops there (so brackets left open are not reported).
///
/// # Errors
///
/// [`SourceTooLarge`] when `source` is longer than [`Lexer::MAX_SOURCE_LEN`].
pub fn tokenize(source: &[u8], options: LexerOptions) -> Result<Tokens, SourceTooLarge> {
    let mut lexer = Lexer::new(source, options)?;
    let mut tokens = Vec::new();
    let mut remaining: Option<u8> = None;
    while let Some(token) = lexer.next() {
        tokens.push(token);
        match remaining {
            None if token.kind == TokenKind::HaltCompiler => remaining = Some(3),
            None => {}
            Some(_)
                if matches!(
                    token.kind,
                    TokenKind::Whitespace
                        | TokenKind::OpenTag
                        | TokenKind::Comment
                        | TokenKind::DocComment
                ) => {}
            Some(1) => {
                if lexer.offset() < source.len() {
                    tokens.push(Token {
                        kind: TokenKind::InlineHtml,
                        start: token.end,
                        end: offset32(source.len()),
                        line: token.line,
                    });
                }
                break;
            }
            Some(count) => remaining = Some(count - 1),
        }
    }
    Ok(Tokens {
        tokens,
        diagnostics: lexer.into_diagnostics(),
    })
}

#[cfg(test)]
mod tests {
    use super::tokenize;
    use crate::lexer::LexerOptions;
    use crate::token::TokenKind;

    #[test]
    fn halt_compiler_keeps_the_rest_as_inline_html() {
        let source = b"<?php __halt_compiler ( ) ; rest\n(";
        let Ok(result) = tokenize(source, LexerOptions::default()) else {
            panic!("source fits");
        };
        let last = result.tokens.last().copied();
        assert_eq!(last.map(|token| token.kind), Some(TokenKind::InlineHtml));
        assert_eq!(last.map(|token| token.text(source)), Some(&b" rest\n("[..]));
        assert_eq!(last.map(|token| token.line), Some(1));
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn halt_compiler_at_the_end_adds_nothing() {
        let source = b"<?php __halt_compiler();";
        let Ok(result) = tokenize(source, LexerOptions::default()) else {
            panic!("source fits");
        };
        assert_eq!(result.tokens.len(), 5);
        let short = b"<?php __halt_compiler(";
        let Ok(result) = tokenize(short, LexerOptions::default()) else {
            panic!("source fits");
        };
        assert_eq!(result.tokens.len(), 3);
        assert_eq!(result.diagnostics.len(), 1);
    }
}
