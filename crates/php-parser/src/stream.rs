//! The tokens the parser sees.
//!
//! The lexer produces the tokenizer's view of the source. Compiling reads it differently:
//! whitespace, comments and open tags are skipped, `?>` ends a statement like `;`, `<?=` reads as
//! `echo`, a few cast spellings raise diagnostics, string tokens carry their decoded value, and a
//! scanner error stops parsing. Tokens are read one at a time, only when the parser needs them, so
//! that errors are raised in the same order and on the same lines as in the reference.

use crate::describe::END;
use crate::error::{ErrorClass, ParseError, Severity, Warning};
use crate::literal::{self, Indentation};
use php_lexer::{
    Diagnostic, LexError, LexWarning, Lexer, LexerOptions, Quote, SourceTooLarge, Token, TokenKind,
    decode_double_quoted, decode_single_quoted,
};
use std::collections::VecDeque;

/// The result of a parsing step.
pub(crate) type PResult<T> = Result<T, Box<ParseError>>;

/// A token as the parser sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Tok {
    /// The kind, after `?>` became `;` and `<?=` became `echo`.
    pub(crate) kind: TokenKind,
    /// Offset of the first byte.
    pub(crate) start: u32,
    /// Offset just past the last byte.
    pub(crate) end: u32,
    /// Line the token starts on.
    pub(crate) line: u32,
    /// Line the scanner is on once the token is read; a syntax error at this token reports it.
    pub(crate) end_line: u32,
    /// The decoded value of a string token; empty for other tokens.
    pub(crate) value: Vec<u8>,
}

/// What kind of text the scanner is in, as far as decoding string fragments is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Context {
    /// Code, with the number of `{` opened since the code started.
    Code(u32),
    /// A `"…"` string.
    Double,
    /// A `` `…` `` command.
    Backtick,
    /// A heredoc body.
    Heredoc,
    /// A nowdoc body.
    Nowdoc,
}

/// Reads parser tokens from source.
#[derive(Debug)]
pub(crate) struct Stream<'s> {
    source: &'s [u8],
    lexer: Lexer<'s>,
    reported: usize,
    ahead: VecDeque<Tok>,
    /// The line the scanner was on once the last consumed token was read.
    consumed_line: u32,
    contexts: Vec<Context>,
    stopped: bool,
    halted: bool,
    warnings: Vec<Warning>,
}

impl<'s> Stream<'s> {
    pub(crate) fn new(source: &'s [u8], options: LexerOptions) -> Result<Self, SourceTooLarge> {
        Ok(Self {
            source,
            lexer: Lexer::new(source, options)?,
            reported: 0,
            ahead: VecDeque::with_capacity(2),
            consumed_line: 1,
            contexts: vec![Context::Code(0)],
            stopped: false,
            halted: false,
            warnings: Vec::new(),
        })
    }

    pub(crate) const fn source(&self) -> &'s [u8] {
        self.source
    }

    /// Warnings raised so far, in order.
    pub(crate) fn into_warnings(self) -> Vec<Warning> {
        self.warnings
    }

    pub(crate) fn warn(&mut self, warning: Warning) {
        self.warnings.push(warning);
    }

    /// The line the scanner is on after the last token read.
    pub(crate) const fn line(&self) -> u32 {
        self.lexer.line()
    }

    /// The next token, read if needed.
    pub(crate) fn peek(&mut self) -> PResult<&Tok> {
        if self.ahead.is_empty() {
            let token = self.fetch()?;
            self.ahead.push_back(token);
        }
        self.ahead
            .front()
            .ok_or_else(|| internal("token buffer emptied while peeking"))
    }

    /// The kind of the next token.
    pub(crate) fn peek_kind(&mut self) -> PResult<TokenKind> {
        self.peek().map(|token| token.kind)
    }

    /// The kind of the token after the next one. Only used where the reference parser certainly
    /// consumes the next token and then reads the one after it.
    pub(crate) fn peek_second_kind(&mut self) -> PResult<TokenKind> {
        while self.ahead.len() < 2 {
            let token = self.fetch()?;
            self.ahead.push_back(token);
        }
        self.ahead
            .get(1)
            .map(|token| token.kind)
            .ok_or_else(|| internal("token buffer emptied while peeking"))
    }

    /// Consumes and returns the next token.
    pub(crate) fn bump(&mut self) -> PResult<Tok> {
        let token = match self.ahead.pop_front() {
            Some(token) => token,
            None => self.fetch()?,
        };
        self.consumed_line = token.end_line;
        Ok(token)
    }

    /// The line the scanner was on once the last consumed token was read.
    pub(crate) const fn consumed_line(&self) -> u32 {
        self.consumed_line
    }

    /// Stops reading after `__halt_compiler();`: every later token is the end of input, which
    /// fails like a real end of input while a bracket is still open.
    pub(crate) fn halt(&mut self) {
        self.stopped = true;
        self.halted = true;
        self.ahead.clear();
    }

    fn end_token(&self) -> Tok {
        let offset = literal_offset(self.lexer.offset());
        Tok {
            kind: END,
            start: offset,
            end: offset,
            line: self.lexer.line(),
            end_line: self.lexer.line(),
            value: Vec::new(),
        }
    }

    fn fetch(&mut self) -> PResult<Tok> {
        loop {
            if self.stopped {
                if let Some(error) = self.lexer.unclosed().filter(|_| self.halted) {
                    return Err(Box::new(ParseError::new(
                        ErrorClass::Parse,
                        error.to_string(),
                        Some(self.lexer.line()),
                    )));
                }
                return Ok(self.end_token());
            }
            let next = self.lexer.next();
            self.report_diagnostics(next.as_ref())?;
            let Some(token) = next else {
                self.stopped = true;
                return Ok(self.end_token());
            };
            if matches!(
                token.kind,
                TokenKind::Whitespace
                    | TokenKind::Comment
                    | TokenKind::DocComment
                    | TokenKind::OpenTag
            ) {
                continue;
            }
            let mut tok = Tok {
                kind: token.kind,
                start: token.start,
                end: token.end,
                line: token.line,
                end_line: self.lexer.line(),
                value: Vec::new(),
            };
            self.translate(&mut tok)?;
            let nowdoc = tok.kind == TokenKind::StartHeredoc && self.text(&tok).contains(&b'\'');
            self.track_context(tok.kind, nowdoc);
            return Ok(tok);
        }
    }

    /// Turns the scanner diagnostics raised while reading `token` into warnings, or into the
    /// error that stops parsing.
    fn report_diagnostics(&mut self, token: Option<&Token>) -> PResult<()> {
        while let Some(diagnostic) = self.lexer.diagnostics().get(self.reported).cloned() {
            self.reported += 1;
            match diagnostic {
                Diagnostic::Warning { warning, line, .. } => {
                    self.warnings
                        .push(Warning::new(Severity::Warning, warning.to_string(), line));
                }
                Diagnostic::Error { error, .. } => {
                    let line = scanner_error_line(&error, token, self.lexer.line());
                    return Err(Box::new(ParseError::new(
                        ErrorClass::Parse,
                        error.to_string(),
                        line,
                    )));
                }
            }
        }
        Ok(())
    }

    fn text(&self, tok: &Tok) -> &'s [u8] {
        &self.source[tok.start as usize..tok.end as usize]
    }

    fn translate(&mut self, tok: &mut Tok) -> PResult<()> {
        match tok.kind {
            TokenKind::CloseTag => {
                tok.kind = TokenKind::Char(b';');
                tok.end_line = tok.line;
            }
            TokenKind::OpenTagWithEcho => tok.kind = TokenKind::Echo,
            TokenKind::IntCast
            | TokenKind::FloatCast
            | TokenKind::StringCast
            | TokenKind::BoolCast => self.check_cast(tok)?,
            TokenKind::ConstantString => tok.value = self.quoted_string(tok)?,
            TokenKind::StringFragment => tok.value = self.fragment(tok)?,
            _ => {}
        }
        Ok(())
    }

    fn check_cast(&mut self, tok: &Tok) -> PResult<()> {
        let text = self.text(tok);
        let word: Vec<u8> = text[1..text.len() - 1]
            .iter()
            .filter(|&&byte| byte != b' ' && byte != b'\t')
            .map(u8::to_ascii_lowercase)
            .collect();
        let canonical = match word.as_slice() {
            b"real" => {
                return Err(Box::new(ParseError::new(
                    ErrorClass::Parse,
                    "The (real) cast has been removed, use (float) instead",
                    Some(tok.line),
                )));
            }
            b"integer" => "int",
            b"double" => "float",
            b"binary" => "string",
            b"boolean" => "bool",
            _ => return Ok(()),
        };
        let written = String::from_utf8_lossy(&word);
        self.warnings.push(Warning::new(
            Severity::Deprecated,
            format!(
                "Non-canonical cast ({written}) is deprecated, use the ({canonical}) cast instead"
            ),
            tok.line,
        ));
        Ok(())
    }

    fn quoted_string(&self, tok: &Tok) -> PResult<Vec<u8>> {
        let text = self.text(tok);
        let text = match text.first() {
            Some(b'b' | b'B') => &text[1..],
            _ => text,
        };
        let body = &text[1..text.len() - 1];
        if text[0] == b'\'' {
            return Ok(decode_single_quoted(body));
        }
        decode_double_quoted(body, Quote::Double)
            .map(|decoded| decoded.bytes)
            .map_err(|failure| escape_error(&failure.error, tok.end_line))
    }

    fn fragment(&mut self, tok: &Tok) -> PResult<Vec<u8>> {
        let text = self.text(tok);
        let quote = match self.contexts.last() {
            Some(Context::Backtick) => Quote::Backtick,
            Some(Context::Heredoc) => return self.heredoc_fragment(tok, text, true),
            Some(Context::Nowdoc) => return self.heredoc_fragment(tok, text, false),
            Some(Context::Double) => Quote::Double,
            Some(Context::Code(_)) | None => return Ok(text.to_vec()),
        };
        decode_double_quoted(text, quote)
            .map(|decoded| decoded.bytes)
            .map_err(|failure| escape_error(&failure.error, tok.end_line))
    }

    /// Reports the octal overflows found in the decoded heredoc segment `body` of `tok`.
    fn escape_warnings(&mut self, tok: &Tok, body: &[u8], warnings: Vec<(LexWarning, usize)>) {
        for (warning, at) in warnings {
            let line = tok.line + literal::count_lines(&body[..at]);
            self.warnings
                .push(Warning::new(Severity::Warning, warning.to_string(), line));
        }
    }

    /// The value of a heredoc or nowdoc segment: the closing line break dropped, the indentation
    /// stripped and, for a heredoc, the escapes decoded.
    fn heredoc_fragment(&mut self, tok: &Tok, text: &[u8], heredoc: bool) -> PResult<Vec<u8>> {
        let Some(body) = self.lexer.open_heredoc() else {
            return Ok(text.to_vec());
        };
        let unterminated = !body.at_closing_label && tok.end as usize == self.source.len();
        let last_line = literal::last_line_indentation(text);
        if unterminated && last_line.is_some_and(|(_, rest_is_blank)| rest_is_blank) {
            return Ok(text.to_vec());
        }
        let text = if body.at_closing_label {
            literal::without_final_newline(text)
        } else {
            text
        };
        let (width, uses_spaces) = match (heredoc, unterminated, last_line) {
            (false, true, Some((indentation, _))) => indentation,
            (false, true, None) => return Ok(text.to_vec()),
            _ => (body.indentation, body.uses_spaces),
        };
        let at_line_start = tok
            .start
            .checked_sub(1)
            .and_then(|index| self.source.get(index as usize))
            .is_some_and(|&byte| byte == b'\n' || byte == b'\r');
        let layout = Indentation {
            width,
            uses_spaces,
            at_line_start,
            at_closing_label: body.at_closing_label,
        };
        let stripped = literal::strip_indentation(text, layout).map_err(|failure| {
            Box::new(ParseError::new(
                ErrorClass::Parse,
                failure.error.to_string(),
                Some(tok.line + failure.lines_before),
            ))
        })?;
        if !heredoc {
            return Ok(stripped);
        }
        match decode_double_quoted(&stripped, Quote::Heredoc) {
            Ok(decoded) => {
                self.escape_warnings(tok, &stripped, decoded.warnings);
                Ok(decoded.bytes)
            }
            Err(failure) => {
                let before = &stripped[..failure.at];
                if let Ok(decoded) = decode_double_quoted(before, Quote::Heredoc) {
                    self.escape_warnings(tok, before, decoded.warnings);
                }
                let lines = literal::count_lines(before);
                Err(escape_error(&failure.error, tok.line + lines))
            }
        }
    }

    fn track_context(&mut self, kind: TokenKind, nowdoc: bool) {
        let nested = self.contexts.len() > 1;
        let Some(top) = self.contexts.last_mut() else {
            return;
        };
        match (*top, kind) {
            (Context::Code(_), TokenKind::Char(b'"')) => self.contexts.push(Context::Double),
            (Context::Code(_), TokenKind::Char(b'`')) => self.contexts.push(Context::Backtick),
            (Context::Code(_), TokenKind::StartHeredoc) => {
                self.contexts.push(if nowdoc {
                    Context::Nowdoc
                } else {
                    Context::Heredoc
                });
            }
            (Context::Double, TokenKind::Char(b'"'))
            | (Context::Backtick, TokenKind::Char(b'`'))
            | (Context::Heredoc | Context::Nowdoc, TokenKind::EndHeredoc) => {
                self.contexts.pop();
            }
            (
                Context::Double | Context::Backtick | Context::Heredoc,
                TokenKind::CurlyOpen | TokenKind::DollarOpenCurlyBraces,
            ) => self.contexts.push(Context::Code(0)),
            (Context::Code(depth), TokenKind::Char(b'{')) => *top = Context::Code(depth + 1),
            (Context::Code(0), TokenKind::Char(b'}')) if nested => {
                self.contexts.pop();
            }
            (Context::Code(depth), TokenKind::Char(b'}')) => {
                *top = Context::Code(depth.saturating_sub(1));
            }
            _ => {}
        }
    }
}

/// An offset of the source, which [`Lexer::new`] guarantees fits 32 bits.
fn literal_offset(offset: usize) -> u32 {
    u32::try_from(offset).unwrap_or(u32::MAX)
}

#[expect(
    clippy::unnecessary_box_returns,
    reason = "parse results carry boxed errors so the success path stays small"
)]
fn escape_error(error: &LexError, line: u32) -> Box<ParseError> {
    Box::new(ParseError::new(
        ErrorClass::Parse,
        error.to_string(),
        Some(line),
    ))
}

/// A parser invariant was broken; reported as an error rather than a panic.
#[expect(
    clippy::unnecessary_box_returns,
    reason = "parse results carry boxed errors so the success path stays small"
)]
pub(crate) fn internal(what: &str) -> Box<ParseError> {
    Box::new(ParseError::new(
        ErrorClass::Parse,
        format!("internal parser error: {what}"),
        None,
    ))
}

/// The line the reference reports a scanner error on.
fn scanner_error_line(error: &LexError, token: Option<&Token>, line_after: u32) -> Option<u32> {
    let token_line = token.map_or(line_after, |token| token.line);
    match error {
        LexError::MixedIndentation
            if token.is_some_and(|token| token.kind == TokenKind::StartHeredoc) =>
        {
            Some(token_line + 1)
        }
        LexError::Unclosed { .. }
        | LexError::InvalidCodepointEscape
        | LexError::CodepointTooLarge => Some(line_after),
        LexError::BodyIndentation(_) => None,
        LexError::Unmatched(_)
        | LexError::Mismatched { .. }
        | LexError::UnterminatedComment(_)
        | LexError::InvalidNumericLiteral
        | LexError::MixedIndentation => Some(token_line),
    }
}

#[cfg(test)]
mod tests {
    use super::{Stream, Tok};
    use crate::describe::END;
    use php_lexer::{LexerOptions, TokenKind};

    fn tokens(source: &[u8]) -> Vec<Tok> {
        let Ok(mut stream) = Stream::new(source, LexerOptions::default()) else {
            panic!("source fits");
        };
        let mut out = Vec::new();
        loop {
            match stream.bump() {
                Ok(token) if token.kind == END => return out,
                Ok(token) => out.push(token),
                Err(error) => panic!("{error}"),
            }
        }
    }

    #[test]
    fn trivia_is_skipped_and_tags_are_translated() {
        let kinds: Vec<_> = tokens(b"<?php /* c */ $a ?>x<?= 1;")
            .iter()
            .map(|token| token.kind)
            .collect();
        assert_eq!(
            kinds,
            [
                TokenKind::Variable,
                TokenKind::Char(b';'),
                TokenKind::InlineHtml,
                TokenKind::Echo,
                TokenKind::Integer,
                TokenKind::Char(b';'),
            ]
        );
    }

    #[test]
    fn string_tokens_carry_their_value() {
        let values: Vec<_> = tokens(b"<?php 'a\\'b' b\"\\x41\" \"$v\\n\" `\\`$v`")
            .into_iter()
            .filter(|token| {
                matches!(
                    token.kind,
                    TokenKind::ConstantString | TokenKind::StringFragment
                )
            })
            .map(|token| token.value)
            .collect();
        assert_eq!(
            values,
            [
                b"a'b".to_vec(),
                b"A".to_vec(),
                b"\n".to_vec(),
                b"`".to_vec()
            ]
        );
    }

    #[test]
    fn heredoc_values_are_stripped_and_decoded() {
        let source = b"<?php <<<A\n    x\\t$v\n      y\n    A;\n<<<'B'\n  \\t\n  B;";
        let values: Vec<_> = tokens(source)
            .into_iter()
            .filter(|token| token.kind == TokenKind::StringFragment)
            .map(|token| token.value)
            .collect();
        assert_eq!(
            values,
            [b"x\t".to_vec(), b"\n  y".to_vec(), b"\\t".to_vec()]
        );
    }

    #[test]
    fn halting_ends_the_input() {
        let Ok(mut stream) = Stream::new(b"<?php a; b", LexerOptions::default()) else {
            panic!("source fits");
        };
        assert!(stream.peek_second_kind().is_ok());
        stream.halt();
        assert_eq!(stream.peek_kind().ok(), Some(END));
        assert_eq!(stream.line(), 1);
    }
}
