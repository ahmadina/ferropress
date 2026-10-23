//! Lexer options and reporting details the reference captures do not exercise.

use php_lexer::{Diagnostic, LexError, Lexer, LexerOptions, SourceTooLarge, TokenKind, tokenize};

fn kinds(source: &[u8], options: LexerOptions) -> Vec<TokenKind> {
    Lexer::new(source, options)
        .map(|lexer| lexer.map(|token| token.kind).collect())
        .unwrap_or_default()
}

#[test]
fn defaults_follow_php() {
    let options = LexerOptions::default();
    assert!(options.short_open_tag);
    assert!(!options.skip_shebang);
}

#[test]
fn shebang_is_skipped_only_when_asked() {
    let source = b"#!/usr/bin/env php\n<?php $a;";
    let compiled = LexerOptions {
        skip_shebang: true,
        ..LexerOptions::default()
    };
    let lexer = Lexer::new(source, compiled).expect("source fits");
    assert_eq!((lexer.offset(), lexer.line()), (19, 2));
    let tokens: Vec<_> = lexer.collect();
    assert_eq!(tokens[0].kind, TokenKind::OpenTag);
    assert_eq!(tokens[0].line, 2);
    assert_eq!(
        kinds(source, LexerOptions::default())[0],
        TokenKind::InlineHtml
    );
}

#[test]
fn shebang_without_newline_ends_at_the_last_carriage_return() {
    let options = LexerOptions {
        skip_shebang: true,
        ..LexerOptions::default()
    };
    let lexer = Lexer::new(b"#!a\rb\rc", options).expect("source fits");
    assert_eq!(lexer.offset(), 6);
    let lexer = Lexer::new(b"#!php", options).expect("source fits");
    assert_eq!((lexer.offset(), lexer.line()), (0, 1));
    let lexer = Lexer::new(b"<?php", options).expect("source fits");
    assert_eq!(lexer.offset(), 0);
}

#[test]
fn diagnostics_carry_the_offset_of_their_token() {
    let mut lexer = Lexer::new(b"<?php 1; ) 09;", LexerOptions::default()).expect("source fits");
    assert!(lexer.by_ref().count() > 0);
    assert_eq!(
        lexer.diagnostics(),
        [
            Diagnostic::Error {
                error: LexError::Unmatched(b')'),
                offset: 9,
            },
            Diagnostic::Error {
                error: LexError::InvalidNumericLiteral,
                offset: 11,
            },
        ]
    );
    assert_eq!(lexer.next(), None, "a finished lexer stays finished");
    assert_eq!(lexer.into_diagnostics().len(), 2);
}

#[test]
fn scan_ahead_reports_nothing_itself() {
    let source = b"<?php <<<A\n  {$a[\"\\400\"]}\n  A;\n";
    let result = tokenize(source, LexerOptions::default()).expect("source fits");
    let warnings = result
        .diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic, Diagnostic::Warning { .. }))
        .count();
    assert_eq!(warnings, 1, "{:?}", result.diagnostics);
}

#[test]
fn closing_label_spans_the_indentation_measured_ahead() {
    let source = b"<?php <<<A\n{$a[}\n    A;\n";
    let result = tokenize(source, LexerOptions::default()).expect("source fits");
    let end = result
        .tokens
        .iter()
        .find(|token| token.kind == TokenKind::EndHeredoc)
        .map(|token| token.text(source));
    assert_eq!(
        end,
        Some(&b" "[..]),
        "the scan ahead stopped at the error before measuring, so the token spans 0 + 1 bytes"
    );
}

#[test]
fn oversized_sources_are_refused() {
    assert_eq!(Lexer::MAX_SOURCE_LEN, u32::MAX as usize - 1);
    assert_eq!(
        SourceTooLarge(5_000_000_000).to_string(),
        "source of 5000000000 bytes is longer than the lexer's limit of 4294967294 bytes"
    );
}
