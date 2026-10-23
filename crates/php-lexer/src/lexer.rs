//! The scanner state machine.

use crate::diagnostic::{Diagnostic, LexError, LexWarning};
use crate::escape::{self, Quote};
use crate::keyword;
use crate::number;
use crate::token::{Token, TokenKind};

/// Settings that change how source is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LexerOptions {
    /// Whether `<?` opens PHP code (the `short_open_tag` setting, on by default in PHP).
    pub short_open_tag: bool,
    /// Whether a first line starting with `#!` is skipped, as when a file is compiled. PHP's
    /// tokenizer functions do not skip it.
    pub skip_shebang: bool,
}

impl Default for LexerOptions {
    fn default() -> Self {
        Self {
            short_open_tag: true,
            skip_shebang: false,
        }
    }
}

/// The source is too long for 32-bit token offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("source of {0} bytes is longer than the lexer's limit of 4294967294 bytes")]
pub struct SourceTooLarge(pub usize);

/// What the lexer is in the middle of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Outside the PHP tags.
    Initial,
    /// PHP code.
    Code,
    /// Inside `"…"` with interpolation.
    DoubleQuotes,
    /// Inside `` `…` ``.
    Backquote,
    /// Inside a heredoc body.
    Heredoc,
    /// Inside a nowdoc body.
    Nowdoc,
    /// At the closing label of a heredoc or nowdoc.
    EndHeredoc,
    /// After `${`, where a name followed by `[` or `}` is a variable name.
    VarName,
    /// Inside the brackets of `"$a[…]"`.
    Offset,
    /// After `->` or `?->`, where a name is a property.
    Property,
}

/// An open heredoc or nowdoc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Heredoc {
    /// Where the label lies in the source.
    label_start: usize,
    label_len: usize,
    /// Indentation the closing token is expected to span and the body is stripped of.
    indentation: Indentation,
}

/// The indentation of a closing label line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Indentation {
    width: usize,
    /// Whether it consists of spaces only (false for an empty or tab-only indentation).
    uses_spaces: bool,
}

impl Indentation {
    fn new(width: usize, spacing: Spacing) -> Self {
        Self {
            width,
            uses_spaces: spacing.spaces && !spacing.tabs,
        }
    }
}

/// What the parser needs to strip the indentation off the heredoc or nowdoc body being read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeredocBody {
    /// Indentation removed from every body line: measured ahead for a heredoc, read from the
    /// closing line for a nowdoc (0 until that line is found).
    pub indentation: usize,
    /// Whether that indentation consists of spaces only.
    pub uses_spaces: bool,
    /// Whether the last token read ended right before the closing label.
    pub at_closing_label: bool,
}

/// One scanning step.
enum Step {
    /// A token ending at `end`; `newlines` overrides the line breaks counted in its text.
    Token {
        kind: TokenKind,
        end: usize,
        newlines: Option<u32>,
    },
    /// The state changed without consuming input; scan again.
    Restart,
    /// The end of the source.
    End,
}

/// A source offset as stored in tokens; [`Lexer::new`] guarantees every offset fits.
#[expect(
    clippy::cast_possible_truncation,
    reason = "Lexer::new rejects sources longer than Lexer::MAX_SOURCE_LEN"
)]
pub(crate) const fn offset32(offset: usize) -> u32 {
    offset as u32
}

const fn token(kind: TokenKind, end: usize) -> Step {
    Step::Token {
        kind,
        end,
        newlines: None,
    }
}

const fn is_label_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

const fn is_label_successor(byte: u8) -> bool {
    is_label_start(byte) || byte.is_ascii_digit()
}

const fn is_whitespace(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r')
}

/// Line breaks in `source[start..end]`: `\n`, and `\r` not followed by `\n` (looking past `end`).
fn count_newlines(source: &[u8], start: usize, end: usize) -> u32 {
    let mut count = 0;
    for index in start..end {
        match source[index] {
            b'\n' => count += 1,
            b'\r' if source.get(index + 1) != Some(&b'\n') => count += 1,
            _ => {}
        }
    }
    count
}

/// Indentation flags of a heredoc line.
#[derive(Debug, Clone, Copy, Default)]
struct Spacing {
    tabs: bool,
    spaces: bool,
}

impl Spacing {
    const fn mixed(self) -> bool {
        self.tabs && self.spaces
    }
}

/// Reads PHP source into tokens.
///
/// The lexer yields every byte of the source in exactly one token, in order, the way PHP's
/// `token_get_all()` does without flags. Scanner errors do not stop it; they are collected in
/// [`Lexer::diagnostics`] in the order they were raised, together with warnings. The rules are
/// described in `contracts/runtime/lexer.md`.
///
/// ```
/// use php_lexer::{Lexer, LexerOptions, TokenKind};
///
/// let source = b"<?php echo $a;";
/// let lexer = Lexer::new(source, LexerOptions::default())?;
/// let kinds: Vec<_> = lexer.map(|token| token.kind).collect();
/// assert_eq!(kinds[1], TokenKind::Echo);
/// assert_eq!(kinds[3], TokenKind::Variable);
/// assert_eq!(kinds[4], TokenKind::Char(b';'));
/// # Ok::<(), php_lexer::SourceTooLarge>(())
/// ```
#[derive(Debug, Clone)]
pub struct Lexer<'s> {
    source: &'s [u8],
    options: LexerOptions,
    position: usize,
    line: u32,
    state: State,
    stack: Vec<State>,
    nesting: Vec<(u8, u32)>,
    heredocs: Vec<Heredoc>,
    diagnostics: Vec<Diagnostic>,
    token_start: usize,
    finished: bool,
    /// Set on the throwaway copy that measures a heredoc's closing indentation.
    scan_ahead: bool,
    /// Indentation of the last closing label the throwaway copy found.
    measured_indentation: Indentation,
}

impl<'s> Lexer<'s> {
    /// The longest source the lexer accepts.
    pub const MAX_SOURCE_LEN: usize = u32::MAX as usize - 1;

    /// Starts reading `source`.
    ///
    /// # Errors
    ///
    /// [`SourceTooLarge`] when `source` is longer than [`Lexer::MAX_SOURCE_LEN`].
    pub fn new(source: &'s [u8], options: LexerOptions) -> Result<Self, SourceTooLarge> {
        if source.len() > Self::MAX_SOURCE_LEN {
            return Err(SourceTooLarge(source.len()));
        }
        let mut lexer = Self {
            source,
            options,
            position: 0,
            line: 1,
            state: State::Initial,
            stack: Vec::new(),
            nesting: Vec::new(),
            heredocs: Vec::new(),
            diagnostics: Vec::new(),
            token_start: 0,
            finished: false,
            scan_ahead: false,
            measured_indentation: Indentation::default(),
        };
        if options.skip_shebang {
            lexer.skip_shebang();
        }
        Ok(lexer)
    }

    /// Errors and warnings raised so far, in order.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Consumes the lexer, returning its diagnostics.
    pub fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics
    }

    /// Offset where the next token starts.
    pub const fn offset(&self) -> usize {
        self.position
    }

    /// The error for the innermost bracket still open, as raised if the input ended here.
    pub fn unclosed(&self) -> Option<LexError> {
        self.nesting
            .last()
            .map(|&(opening, opening_line)| LexError::Unclosed {
                opening,
                opening_line,
                line: self.line,
            })
    }

    /// The innermost heredoc or nowdoc still open, if any.
    pub fn open_heredoc(&self) -> Option<HeredocBody> {
        self.current_heredoc().map(|heredoc| HeredocBody {
            indentation: heredoc.indentation.width,
            uses_spaces: heredoc.indentation.uses_spaces,
            at_closing_label: self.state == State::EndHeredoc,
        })
    }

    /// Line the next token starts on.
    pub const fn line(&self) -> u32 {
        self.line
    }

    fn byte(&self, index: usize) -> Option<u8> {
        self.source.get(index).copied()
    }

    fn byte_is(&self, index: usize, predicate: impl FnOnce(u8) -> bool) -> bool {
        self.byte(index).is_some_and(predicate)
    }

    fn starts_with(&self, index: usize, text: &[u8]) -> bool {
        self.source
            .get(index..)
            .is_some_and(|rest| rest.starts_with(text))
    }

    fn starts_with_ignore_case(&self, index: usize, text: &[u8]) -> bool {
        self.source
            .get(index..index + text.len())
            .is_some_and(|rest| rest.eq_ignore_ascii_case(text))
    }

    fn label_end(&self, start: usize) -> usize {
        self.source[start..]
            .iter()
            .position(|&byte| !is_label_successor(byte))
            .map_or(self.source.len(), |offset| start + offset)
    }

    fn whitespace_end(&self, start: usize) -> usize {
        self.source[start..]
            .iter()
            .position(|&byte| !is_whitespace(byte))
            .map_or(self.source.len(), |offset| start + offset)
    }

    fn error(&mut self, error: LexError) {
        self.diagnostics.push(Diagnostic::Error {
            error,
            offset: offset32(self.token_start),
        });
    }

    fn warning(&mut self, warning: LexWarning) {
        if !self.scan_ahead {
            self.diagnostics.push(Diagnostic::Warning {
                warning,
                offset: offset32(self.token_start),
            });
        }
    }

    fn push_state(&mut self, state: State) {
        self.stack.push(self.state);
        self.state = state;
    }

    fn pop_state(&mut self) {
        if let Some(state) = self.stack.pop() {
            self.state = state;
        }
    }

    fn enter_nesting(&mut self, opening: u8) {
        self.nesting.push((opening, self.line));
    }

    fn exit_nesting(&mut self, closing: u8) {
        let Some(&(opening, opening_line)) = self.nesting.last() else {
            self.error(LexError::Unmatched(closing));
            return;
        };
        let expected = match opening {
            b'(' => b')',
            b'[' => b']',
            _ => b'}',
        };
        if closing == expected {
            self.nesting.pop();
        } else {
            self.error(LexError::Mismatched {
                opening,
                opening_line,
                closing,
                line: self.line,
            });
        }
    }

    fn skip_shebang(&mut self) {
        if !self.starts_with(0, b"#!") {
            return;
        }
        let rest = &self.source[2..];
        let end = rest
            .iter()
            .position(|&byte| byte == b'\n')
            .or_else(|| rest.iter().rposition(|&byte| byte == b'\r'));
        if let Some(offset) = end {
            self.position = 2 + offset + 1;
            self.line = 2;
        }
    }

    fn finish(&mut self) {
        self.finished = true;
        if let Some(error) = self.unclosed() {
            self.error(error);
        }
    }

    fn step(&mut self) -> Step {
        let start = self.position;
        match self.state {
            State::Initial => self.scan_initial(start),
            State::Code => self.scan_code(start),
            State::DoubleQuotes => self.scan_interpolated(start, Quote::Double),
            State::Backquote => self.scan_interpolated(start, Quote::Backtick),
            State::Heredoc => self.scan_interpolated(start, Quote::Heredoc),
            State::Nowdoc => self.scan_nowdoc(start),
            State::EndHeredoc => self.scan_end_heredoc(start),
            State::VarName => self.scan_var_name(start),
            State::Offset => self.scan_offset(start),
            State::Property => self.scan_property(start),
        }
    }

    fn scan_initial(&mut self, start: usize) -> Step {
        if start >= self.source.len() {
            return Step::End;
        }
        if self.starts_with(start, b"<?=") {
            self.state = State::Code;
            return token(TokenKind::OpenTagWithEcho, start + 3);
        }
        if self.starts_with_ignore_case(start, b"<?php") {
            let after = start + 5;
            let end = match self.byte(after) {
                None => Some(after),
                Some(b' ' | b'\t' | b'\n') => Some(after + 1),
                Some(b'\r') => Some(after + 1 + usize::from(self.byte(after + 1) == Some(b'\n'))),
                Some(_) if self.options.short_open_tag => Some(start + 2),
                Some(_) => None,
            };
            return match end {
                Some(end) => {
                    self.state = State::Code;
                    token(TokenKind::OpenTag, end)
                }
                None => token(TokenKind::InlineHtml, self.inline_html_end(after)),
            };
        }
        if self.starts_with(start, b"<?") {
            if self.options.short_open_tag {
                self.state = State::Code;
                return token(TokenKind::OpenTag, start + 2);
            }
            return token(TokenKind::InlineHtml, self.inline_html_end(start + 2));
        }
        token(TokenKind::InlineHtml, self.inline_html_end(start + 1))
    }

    /// End of inline HTML searched from `from`: just before the next `<?` that opens PHP code.
    fn inline_html_end(&self, mut from: usize) -> usize {
        let length = self.source.len();
        loop {
            let Some(offset) = self
                .source
                .get(from..)
                .and_then(|rest| rest.iter().position(|&byte| byte == b'<'))
            else {
                return length;
            };
            let after = from + offset + 1;
            if after >= length {
                return length;
            }
            if self.byte(after) == Some(b'?')
                && (self.options.short_open_tag
                    || self.byte(after + 1) == Some(b'=')
                    || (self.starts_with_ignore_case(after + 1, b"php")
                        && (after + 4 == length || self.byte_is(after + 4, is_whitespace))))
            {
                return after - 1;
            }
            from = after;
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "one arm per leading byte keeps the dispatch flat"
    )]
    fn scan_code(&mut self, start: usize) -> Step {
        use TokenKind as K;
        let Some(byte) = self.byte(start) else {
            return Step::End;
        };
        let next = self.byte(start + 1);
        let next_is = |expected: u8| next == Some(expected);
        match byte {
            b' ' | b'\t' | b'\n' | b'\r' => token(K::Whitespace, self.whitespace_end(start)),
            b'b' | b'B' if next_is(b'\'') => self.single_quoted(start + 1),
            b'b' | b'B' if next_is(b'"') => self.double_quoted(start, start + 1),
            b'b' | b'B' if self.starts_with(start + 1, b"<<<") => {
                match self.heredoc_start(start + 1) {
                    Some(found) => self.start_heredoc(start, found),
                    None => self.scan_label(start),
                }
            }
            _ if is_label_start(byte) => self.scan_label(start),
            b'\\' if self.byte_is(start + 1, is_label_start) => token(
                K::FullyQualifiedName,
                self.qualified_end(self.label_end(start + 1)),
            ),
            b'\\' => token(K::NsSeparator, start + 1),
            b'$' if self.byte_is(start + 1, is_label_start) => {
                token(K::Variable, self.label_end(start + 1))
            }
            b'0'..=b'9' => self.scan_number(start),
            b'.' if next.is_some_and(|byte| byte.is_ascii_digit()) => self.scan_number(start),
            b'.' if self.starts_with(start, b"...") => token(K::Ellipsis, start + 3),
            b'.' if next_is(b'=') => token(K::ConcatAssign, start + 2),
            b'\'' => self.single_quoted(start),
            b'"' => self.double_quoted(start, start),
            b'`' => {
                self.state = State::Backquote;
                token(K::Char(b'`'), start + 1)
            }
            b'<' => match self.heredoc_start(start) {
                Some(found) => self.start_heredoc(start, found),
                None => self.operator(
                    start,
                    &[
                        (b"<<=", K::ShiftLeftAssign),
                        (b"<=>", K::Spaceship),
                        (b"<<", K::ShiftLeft),
                        (b"<=", K::LessOrEqual),
                        (b"<>", K::NotEqual),
                    ],
                ),
            },
            b'#' if next_is(b'[') => {
                self.enter_nesting(b'[');
                token(K::Attribute, start + 2)
            }
            b'#' => self.line_comment(start, 1),
            b'/' if next_is(b'/') => self.line_comment(start, 2),
            b'/' if next_is(b'*') => self.block_comment(start),
            b'?' if self.starts_with(start, b"?->") => {
                self.push_state(State::Property);
                token(K::NullsafeObjectOperator, start + 3)
            }
            b'?' if next_is(b'>') => {
                let end = match self.byte(start + 2) {
                    Some(b'\n') => start + 3,
                    Some(b'\r') => start + 3 + usize::from(self.byte(start + 3) == Some(b'\n')),
                    _ => start + 2,
                };
                self.state = State::Initial;
                token(K::CloseTag, end)
            }
            b'-' if next_is(b'>') => {
                self.push_state(State::Property);
                token(K::ObjectOperator, start + 2)
            }
            b'(' => {
                if let Some((kind, end)) = self.cast(start) {
                    return token(kind, end);
                }
                self.enter_nesting(b'(');
                token(K::Char(b'('), start + 1)
            }
            b'[' => {
                self.enter_nesting(b'[');
                token(K::Char(b'['), start + 1)
            }
            b')' | b']' => {
                self.exit_nesting(byte);
                token(K::Char(byte), start + 1)
            }
            b'{' => {
                self.push_state(State::Code);
                self.enter_nesting(b'{');
                token(K::Char(b'{'), start + 1)
            }
            b'}' => {
                self.pop_state();
                self.exit_nesting(b'}');
                token(K::Char(b'}'), start + 1)
            }
            b'&' if next_is(b'&') => token(K::BooleanAnd, start + 2),
            b'&' if next_is(b'=') => token(K::AndAssign, start + 2),
            b'&' => {
                let followed = std::iter::once(start + 1)
                    .chain(self.whitespace_or_comment_ends(start + 1))
                    .any(|at| self.byte(at) == Some(b'$') || self.starts_with(at, b"..."));
                let kind = if followed {
                    K::AmpersandFollowedByVarOrVararg
                } else {
                    K::AmpersandNotFollowedByVarOrVararg
                };
                token(kind, start + 1)
            }
            b'?' => self.operator(start, &[(b"??=", K::CoalesceAssign), (b"??", K::Coalesce)]),
            b'>' => self.operator(
                start,
                &[
                    (b">>=", K::ShiftRightAssign),
                    (b">>", K::ShiftRight),
                    (b">=", K::GreaterOrEqual),
                ],
            ),
            b'=' => self.operator(
                start,
                &[
                    (b"===", K::Identical),
                    (b"==", K::Equal),
                    (b"=>", K::DoubleArrow),
                ],
            ),
            b'!' => self.operator(start, &[(b"!==", K::NotIdentical), (b"!=", K::NotEqual)]),
            b'+' => self.operator(start, &[(b"++", K::Increment), (b"+=", K::PlusAssign)]),
            b'-' => self.operator(start, &[(b"--", K::Decrement), (b"-=", K::MinusAssign)]),
            b'*' => self.operator(
                start,
                &[
                    (b"**=", K::PowAssign),
                    (b"**", K::Pow),
                    (b"*=", K::MulAssign),
                ],
            ),
            b'/' => self.operator(start, &[(b"/=", K::DivAssign)]),
            b'%' => self.operator(start, &[(b"%=", K::ModAssign)]),
            b'|' => self.operator(
                start,
                &[
                    (b"||", K::BooleanOr),
                    (b"|=", K::OrAssign),
                    (b"|>", K::Pipe),
                ],
            ),
            b'^' => self.operator(start, &[(b"^=", K::XorAssign)]),
            b':' => self.operator(start, &[(b"::", K::DoubleColon)]),
            b';' | b',' | b'.' | b'~' | b'@' | b'$' => token(K::Char(byte), start + 1),
            _ => token(K::BadCharacter, start + 1),
        }
    }

    /// The first operator of `candidates` (longest first) present at `start`, else the byte itself.
    fn operator(&self, start: usize, candidates: &[(&[u8], TokenKind)]) -> Step {
        candidates
            .iter()
            .find(|(text, _)| self.starts_with(start, text))
            .map_or_else(
                || token(TokenKind::Char(self.source[start]), start + 1),
                |&(text, kind)| token(kind, start + text.len()),
            )
    }

    /// End of `\label` repetitions after a name ending at `end`.
    fn qualified_end(&self, mut end: usize) -> usize {
        while self.byte(end) == Some(b'\\') && self.byte_is(end + 1, is_label_start) {
            end = self.label_end(end + 1);
        }
        end
    }

    fn scan_label(&mut self, start: usize) -> Step {
        let end = self.label_end(start);
        let qualified = self.qualified_end(end);
        if qualified > end {
            let kind = if self.source[start..end].eq_ignore_ascii_case(b"namespace") {
                TokenKind::RelativeName
            } else {
                TokenKind::QualifiedName
            };
            return token(kind, qualified);
        }
        let word = &self.source[start..end];
        if word.eq_ignore_ascii_case(b"enum") {
            return token(self.enum_kind(end), end);
        }
        match keyword::reserved(word) {
            Some(TokenKind::Yield) => match self.yield_from_end(end) {
                Some(from_end) => token(TokenKind::YieldFrom, from_end),
                None => token(TokenKind::Yield, end),
            },
            Some(visibility @ (TokenKind::Public | TokenKind::Protected | TokenKind::Private))
                if self.starts_with_ignore_case(end, b"(set)") =>
            {
                let kind = keyword::with_set(visibility).unwrap_or(visibility);
                token(kind, end + 5)
            }
            Some(kind) => token(kind, end),
            None => token(TokenKind::Identifier, end),
        }
    }

    /// Every offset reachable from `start` through one or more runs of whitespace and complete
    /// comments, as used by the lookaheads after `yield`, `enum` and `&`. A comment may be split in
    /// more than one way (`#` followed by a newline), so all ends are kept.
    fn whitespace_or_comment_ends(&self, start: usize) -> Vec<usize> {
        let mut ends = Vec::new();
        let mut pending = vec![start];
        while let Some(at) = pending.pop() {
            for end in self.whitespace_or_comment_element(at).into_iter().flatten() {
                if !ends.contains(&end) {
                    ends.push(end);
                    pending.push(end);
                }
            }
        }
        ends
    }

    /// Ends of the whitespace run or comment starting at `at`. Such comments must be terminated
    /// and contain no NUL byte.
    fn whitespace_or_comment_element(&self, at: usize) -> [Option<usize>; 2] {
        let source = self.source;
        let newline_after = |from: usize| {
            let rest = source.get(from..)?;
            let offset = rest
                .iter()
                .position(|&byte| matches!(byte, b'\n' | b'\r' | 0))?;
            (rest[offset] != 0).then_some(from + offset + 1)
        };
        match (self.byte(at), self.byte(at + 1)) {
            (Some(byte), _) if is_whitespace(byte) => [Some(self.whitespace_end(at)), None],
            (Some(b'/'), Some(b'*')) => {
                let rest = &source[at + 2..];
                let close = rest
                    .windows(2)
                    .position(|pair| pair == b"*/")
                    .filter(|&offset| !rest[..offset].contains(&0));
                [close.map(|offset| at + 2 + offset + 2), None]
            }
            (Some(b'/'), Some(b'/')) => [newline_after(at + 2), None],
            (Some(b'#'), Some(next)) => {
                let bare = matches!(next, b'\n' | b'\r').then_some(at + 2);
                let text = (next != b'[' && next != 0)
                    .then(|| newline_after(at + 2))
                    .flatten();
                [bare, text]
            }
            _ => [None, None],
        }
    }

    fn yield_from_end(&self, end: usize) -> Option<usize> {
        self.whitespace_or_comment_ends(end)
            .into_iter()
            .filter(|&at| {
                self.starts_with_ignore_case(at, b"from")
                    && !self.byte_is(at + 4, is_label_successor)
            })
            .max()
            .map(|at| at + 4)
    }

    /// `enum` is a keyword when a name follows it, unless that name is `extends` or `implements`;
    /// the longest lookahead decides.
    fn enum_kind(&self, end: usize) -> TokenKind {
        let mut identifier = None;
        let mut keyword = None;
        for at in self.whitespace_or_comment_ends(end) {
            if self.starts_with_ignore_case(at, b"extends") {
                identifier = identifier.max(Some(at + 7));
            }
            if self.starts_with_ignore_case(at, b"implements") {
                identifier = identifier.max(Some(at + 10));
            }
            if self.byte_is(at, is_label_start) {
                keyword = keyword.max(Some(at + 1));
            }
        }
        match (identifier, keyword) {
            (identifier, Some(keyword)) if identifier < Some(keyword) => TokenKind::Enum,
            _ => TokenKind::Identifier,
        }
    }

    fn cast(&self, start: usize) -> Option<(TokenKind, usize)> {
        let skip_blanks = |from: usize| {
            self.source[from..]
                .iter()
                .position(|&byte| byte != b' ' && byte != b'\t')
                .map_or(self.source.len(), |offset| from + offset)
        };
        let name_start = skip_blanks(start + 1);
        let name_end = self.source[name_start..]
            .iter()
            .position(|byte| !byte.is_ascii_alphabetic())
            .map_or(self.source.len(), |offset| name_start + offset);
        let close = skip_blanks(name_end);
        if self.byte(close) != Some(b')') {
            return None;
        }
        keyword::cast(&self.source[name_start..name_end]).map(|kind| (kind, close + 1))
    }

    fn scan_number(&mut self, start: usize) -> Step {
        let Some(found) = number::scan(self.source, start) else {
            return token(TokenKind::Char(self.source[start]), start + 1);
        };
        if found.invalid_octal {
            self.error(LexError::InvalidNumericLiteral);
        }
        token(found.kind, found.end)
    }

    fn line_comment(&self, start: usize, prefix: usize) -> Step {
        let mut end = start + prefix;
        while let Some(byte) = self.byte(end) {
            if byte == b'\n' || byte == b'\r' || (byte == b'?' && self.byte(end + 1) == Some(b'>'))
            {
                break;
            }
            end += 1;
        }
        token(TokenKind::Comment, end)
    }

    fn block_comment(&mut self, start: usize) -> Step {
        let doc = self.byte(start + 2) == Some(b'*') && self.byte_is(start + 3, is_whitespace);
        let kind = if doc {
            TokenKind::DocComment
        } else {
            TokenKind::Comment
        };
        let close = self.source[start + 2..]
            .windows(2)
            .position(|pair| pair == b"*/")
            .map(|offset| start + 2 + offset + 2);
        let end = close.unwrap_or_else(|| {
            self.error(LexError::UnterminatedComment(self.line));
            self.source.len()
        });
        token(kind, end)
    }

    /// A single-quoted literal whose quote is at `quote`.
    fn single_quoted(&self, quote: usize) -> Step {
        let mut at = quote + 1;
        while let Some(byte) = self.byte(at) {
            match byte {
                b'\'' => return token(TokenKind::ConstantString, at + 1),
                b'\\' => at += 2,
                _ => at += 1,
            }
        }
        Step::Token {
            kind: TokenKind::StringFragment,
            end: self.source.len(),
            newlines: Some(0),
        }
    }

    /// End of an interpolated segment starting at `from`: before `$name`, `${`, `{$`, the closing
    /// `delimiter` or the end. A backslash protects the next byte.
    fn segment_end(&self, from: usize, delimiter: u8) -> usize {
        let mut at = from;
        while let Some(byte) = self.byte(at) {
            match byte {
                b'$' if self.byte_is(at + 1, |next| is_label_start(next) || next == b'{') => {
                    return at;
                }
                b'{' if self.byte(at + 1) == Some(b'$') => return at,
                b'\\' => at = (at + 2).min(self.source.len()),
                _ if byte == delimiter => return at,
                _ => at += 1,
            }
        }
        at
    }

    /// A double-quoted literal whose quote is at `quote`: one token when it has no interpolation,
    /// else the opening `"` token and the double-quotes state.
    fn double_quoted(&mut self, start: usize, quote: usize) -> Step {
        let body_start = quote + 1;
        let end = self.segment_end(body_start, b'"');
        if self.byte(end) == Some(b'"') {
            return self.escaped(
                TokenKind::ConstantString,
                start,
                body_start,
                end,
                end + 1,
                Quote::Double,
            );
        }
        self.state = State::DoubleQuotes;
        token(TokenKind::Char(b'"'), body_start)
    }

    /// A token whose body `body_start..body_end` holds escape sequences that are validated as it is
    /// read; a bad `\u{…}` stops the line count at the sequence.
    fn escaped(
        &mut self,
        kind: TokenKind,
        start: usize,
        body_start: usize,
        body_end: usize,
        end: usize,
        quote: Quote,
    ) -> Step {
        let mut warnings = Vec::new();
        let source = self.source;
        let body = &source[body_start..body_end];
        let outcome = escape::scan(body, quote, None, &mut |warning| warnings.push(warning));
        for warning in warnings {
            self.warning(warning);
        }
        let newlines = outcome.err().map(|failure| {
            self.error(failure.error);
            count_newlines(self.source, start, body_start + failure.at)
        });
        Step::Token {
            kind,
            end,
            newlines,
        }
    }

    /// The parts of `<<<LABEL` and its newline starting at `start` (the first `<`).
    fn heredoc_start(&self, start: usize) -> Option<HeredocStart> {
        if !self.starts_with(start, b"<<<") {
            return None;
        }
        let mut at = start + 3;
        while matches!(self.byte(at), Some(b' ' | b'\t')) {
            at += 1;
        }
        let quote = match self.byte(at)? {
            quote @ (b'\'' | b'"') => {
                at += 1;
                Some(quote)
            }
            _ => None,
        };
        if !self.byte_is(at, is_label_start) {
            return None;
        }
        let label_start = at;
        let label_end = self.label_end(at);
        at = label_end;
        if let Some(quote) = quote {
            if self.byte(at) != Some(quote) {
                return None;
            }
            at += 1;
        }
        let end = match self.byte(at)? {
            b'\n' => at + 1,
            b'\r' => at + 1 + usize::from(self.byte(at + 1) == Some(b'\n')),
            _ => return None,
        };
        Some(HeredocStart {
            label_start,
            label_len: label_end - label_start,
            nowdoc: quote == Some(b'\''),
            end,
        })
    }

    /// Whether the closing label of `heredoc` starts at `at` (strictly followed by another byte
    /// that cannot continue a name).
    fn closes(&self, heredoc: Heredoc, at: usize) -> bool {
        let label = &self.source[heredoc.label_start..heredoc.label_start + heredoc.label_len];
        heredoc.label_len < self.source.len() - at
            && self.source[at..].starts_with(label)
            && !is_label_successor(self.source[at + heredoc.label_len])
    }

    /// End of the spaces and tabs at `from`, and which of them occurred.
    fn indentation_end(&self, from: usize) -> (usize, Spacing) {
        let mut spacing = Spacing::default();
        let mut at = from;
        loop {
            match self.byte(at) {
                Some(b' ') => spacing.spaces = true,
                Some(b'\t') => spacing.tabs = true,
                _ => return (at, spacing),
            }
            at += 1;
        }
    }

    fn start_heredoc(&mut self, start: usize, found: HeredocStart) -> Step {
        let end = found.end;
        self.heredocs.push(Heredoc {
            label_start: found.label_start,
            label_len: found.label_len,
            indentation: Indentation::default(),
        });
        self.state = if found.nowdoc {
            State::Nowdoc
        } else {
            State::Heredoc
        };
        let (body, spacing) = self.indentation_end(end);
        if body == self.source.len() {
            return token(TokenKind::StartHeredoc, end);
        }
        let heredoc = Heredoc {
            label_start: found.label_start,
            label_len: found.label_len,
            indentation: Indentation::new(body - end, spacing),
        };
        if self.byte_is(body, is_label_start) && self.closes(heredoc, body) {
            if spacing.mixed() {
                self.error(LexError::MixedIndentation);
            }
            self.replace_heredoc(heredoc);
            self.state = State::EndHeredoc;
            return token(TokenKind::StartHeredoc, end);
        }
        if !found.nowdoc && !self.scan_ahead {
            let line = self.line + count_newlines(self.source, start, end);
            let (indentation, first) = self.read_ahead(end, line);
            let interpolates = matches!(
                first,
                Some(TokenKind::Variable | TokenKind::DollarOpenCurlyBraces | TokenKind::CurlyOpen)
            );
            if interpolates && indentation.width > 0 {
                self.error(LexError::BodyIndentation(indentation.width));
            }
            self.replace_heredoc(Heredoc {
                indentation,
                ..heredoc
            });
        }
        token(TokenKind::StartHeredoc, end)
    }

    fn replace_heredoc(&mut self, heredoc: Heredoc) {
        if let Some(top) = self.heredocs.last_mut() {
            *top = heredoc;
        }
    }

    /// A throwaway copy positioned at `position` that reads ahead without reporting anything.
    fn probe(&self, position: usize, line: u32) -> Self {
        Self {
            source: self.source,
            options: self.options,
            position,
            line,
            state: self.state,
            stack: self.stack.clone(),
            nesting: self.nesting.clone(),
            heredocs: self.heredocs.clone(),
            diagnostics: Vec::new(),
            token_start: position,
            finished: false,
            scan_ahead: true,
            measured_indentation: Indentation::default(),
        }
    }

    /// Reads ahead from the body at `body` until the heredoc just opened is closed, the source ends
    /// or an error is raised. Returns the indentation of the last closing label seen (nested
    /// heredocs overwrite it, as in PHP; 0 when none was seen) and the kind of the first token read
    /// without error.
    fn read_ahead(&self, body: usize, line: u32) -> (Indentation, Option<TokenKind>) {
        let mut probe = self.probe(body, line);
        let mut depth = 1_usize;
        let mut first = None;
        while depth > 0 {
            let Some(token) = probe.next() else { break };
            if !probe.diagnostics.is_empty() {
                break;
            }
            first.get_or_insert(token.kind);
            match token.kind {
                TokenKind::StartHeredoc => depth += 1,
                TokenKind::EndHeredoc => depth -= 1,
                _ => {}
            }
        }
        (probe.measured_indentation, first)
    }

    /// The heredoc whose body is being read.
    fn current_heredoc(&self) -> Option<Heredoc> {
        self.heredocs.last().copied()
    }

    fn scan_interpolated(&mut self, start: usize, quote: Quote) -> Step {
        use TokenKind as K;
        let Some(byte) = self.byte(start) else {
            return Step::End;
        };
        match byte {
            b'$' if self.byte(start + 1) == Some(b'{') => {
                self.push_state(State::VarName);
                self.enter_nesting(b'{');
                return token(K::DollarOpenCurlyBraces, start + 2);
            }
            b'$' if self.byte_is(start + 1, is_label_start) => {
                let end = self.label_end(start + 1);
                if (self.starts_with(end, b"->") && self.byte_is(end + 2, is_label_start))
                    || (self.starts_with(end, b"?->") && self.byte_is(end + 3, is_label_start))
                {
                    self.push_state(State::Property);
                } else if self.byte(end) == Some(b'[') {
                    self.push_state(State::Offset);
                }
                return token(K::Variable, end);
            }
            b'{' if self.byte(start + 1) == Some(b'$') => {
                self.push_state(State::Code);
                self.enter_nesting(b'{');
                return token(K::CurlyOpen, start + 1);
            }
            _ => {}
        }
        match quote {
            Quote::Double | Quote::Backtick => {
                let delimiter = if quote == Quote::Double { b'"' } else { b'`' };
                if byte == delimiter {
                    self.state = State::Code;
                    return token(K::Char(delimiter), start + 1);
                }
                let first = if byte == b'\\' { start + 2 } else { start + 1 };
                let end = self.segment_end(first.min(self.source.len()), delimiter);
                self.escaped(K::StringFragment, start, start, end, end, quote)
            }
            Quote::Heredoc => self.heredoc_body(start),
        }
    }

    fn heredoc_body(&mut self, start: usize) -> Step {
        let Some(heredoc) = self.current_heredoc() else {
            return Step::End;
        };
        let length = self.source.len();
        let mut at = start;
        while let Some(byte) = self.byte(at) {
            at += 1;
            match byte {
                b'\r' | b'\n' => {
                    if byte == b'\r' && self.byte(at) == Some(b'\n') {
                        at += 1;
                    }
                    let line_start = at;
                    let (label, spacing) = self.indentation_end(at);
                    at = label;
                    if at == length {
                        return token(TokenKind::StringFragment, length);
                    }
                    if self.byte_is(at, is_label_start) && self.closes(heredoc, at) {
                        if spacing.mixed() {
                            self.error(LexError::MixedIndentation);
                        }
                        self.state = State::EndHeredoc;
                        if self.scan_ahead {
                            self.measured_indentation = Indentation::new(at - line_start, spacing);
                            return token(TokenKind::StringFragment, at);
                        }
                        return token(TokenKind::StringFragment, line_start);
                    }
                }
                b'$' if self.byte_is(at, |next| is_label_start(next) || next == b'{') => {
                    return token(TokenKind::StringFragment, at - 1);
                }
                b'{' if self.byte(at) == Some(b'$') => {
                    return token(TokenKind::StringFragment, at - 1);
                }
                b'\\' if self.byte_is(at, |next| next != b'\n' && next != b'\r') => at += 1,
                _ => {}
            }
        }
        token(TokenKind::StringFragment, length)
    }

    fn scan_nowdoc(&mut self, start: usize) -> Step {
        let Some(heredoc) = self.current_heredoc() else {
            return Step::End;
        };
        let length = self.source.len();
        if start >= length {
            return Step::End;
        }
        let mut at = start;
        while let Some(byte) = self.byte(at) {
            at += 1;
            if byte != b'\r' && byte != b'\n' {
                continue;
            }
            if byte == b'\r' && self.byte(at) == Some(b'\n') {
                at += 1;
            }
            let line_start = at;
            let (label, spacing) = self.indentation_end(at);
            at = label;
            if at == length {
                return token(TokenKind::StringFragment, length);
            }
            if self.byte_is(at, is_label_start) && self.closes(heredoc, at) {
                if spacing.mixed() {
                    self.error(LexError::MixedIndentation);
                }
                self.replace_heredoc(Heredoc {
                    indentation: Indentation::new(at - line_start, spacing),
                    ..heredoc
                });
                self.state = State::EndHeredoc;
                return token(TokenKind::StringFragment, line_start);
            }
        }
        token(TokenKind::StringFragment, length)
    }

    fn scan_end_heredoc(&mut self, start: usize) -> Step {
        let Some(heredoc) = self.heredocs.pop() else {
            self.state = State::Code;
            return Step::Restart;
        };
        self.state = State::Code;
        let end = (start + heredoc.indentation.width + heredoc.label_len).min(self.source.len());
        token(TokenKind::EndHeredoc, end)
    }

    fn scan_var_name(&mut self, start: usize) -> Step {
        self.state = State::Code;
        if self.byte_is(start, is_label_start) {
            let end = self.label_end(start);
            if matches!(self.byte(end), Some(b'[' | b'}')) {
                return token(TokenKind::VariableName, end);
            }
        }
        Step::Restart
    }

    fn scan_offset(&mut self, start: usize) -> Step {
        let Some(byte) = self.byte(start) else {
            return Step::End;
        };
        match byte {
            b'0'..=b'9' => {
                let plain = if byte == b'0' {
                    start + 1
                } else {
                    self.source[start..]
                        .iter()
                        .position(|byte| !byte.is_ascii_digit())
                        .map_or(self.source.len(), |offset| start + offset)
                };
                let end =
                    number::scan_offset(self.source, start).map_or(plain, |end| end.max(plain));
                token(TokenKind::OffsetNumber, end)
            }
            b'$' if self.byte_is(start + 1, is_label_start) => {
                token(TokenKind::Variable, self.label_end(start + 1))
            }
            b']' => {
                self.pop_state();
                token(TokenKind::Char(b']'), start + 1)
            }
            b';' | b':' | b',' | b'.' | b'|' | b'^' | b'&' | b'+' | b'-' | b'/' | b'*' | b'='
            | b'%' | b'!' | b'~' | b'$' | b'<' | b'>' | b'?' | b'@' | b'[' | b'(' | b')' | b'{'
            | b'}' | b'"' | b'`' => token(TokenKind::Char(byte), start + 1),
            b' ' | b'\n' | b'\r' | b'\t' | b'\\' | b'\'' | b'#' => {
                self.pop_state();
                token(TokenKind::StringFragment, start)
            }
            _ if is_label_start(byte) => token(TokenKind::Identifier, self.label_end(start)),
            _ => token(TokenKind::BadCharacter, start + 1),
        }
    }

    fn scan_property(&mut self, start: usize) -> Step {
        let Some(byte) = self.byte(start) else {
            return Step::End;
        };
        match byte {
            _ if is_whitespace(byte) => token(TokenKind::Whitespace, self.whitespace_end(start)),
            b'-' if self.byte(start + 1) == Some(b'>') => {
                token(TokenKind::ObjectOperator, start + 2)
            }
            b'?' if self.starts_with(start, b"?->") => {
                token(TokenKind::NullsafeObjectOperator, start + 3)
            }
            b'#' => self.line_comment(start, 1),
            b'/' if self.byte(start + 1) == Some(b'/') => self.line_comment(start, 2),
            b'/' if self.byte(start + 1) == Some(b'*') => self.block_comment(start),
            _ if is_label_start(byte) => {
                self.pop_state();
                token(TokenKind::Identifier, self.label_end(start))
            }
            _ => {
                self.pop_state();
                Step::Restart
            }
        }
    }
}

/// The pieces of a heredoc opener.
#[derive(Debug, Clone, Copy)]
struct HeredocStart {
    label_start: usize,
    label_len: usize,
    nowdoc: bool,
    end: usize,
}

impl Iterator for Lexer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        if self.finished {
            return None;
        }
        loop {
            let start = self.position;
            self.token_start = start;
            match self.step() {
                Step::Restart => {}
                Step::End => {
                    self.finish();
                    return None;
                }
                Step::Token {
                    kind,
                    end,
                    newlines,
                } => {
                    let line = self.line;
                    self.position = end;
                    self.line +=
                        newlines.unwrap_or_else(|| count_newlines(self.source, start, end));
                    return Some(Token {
                        kind,
                        start: offset32(start),
                        end: offset32(end),
                        line,
                    });
                }
            }
        }
    }
}
