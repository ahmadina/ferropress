//! Properties every tokenization must have, whatever the input.

use php_lexer::{Lexer, LexerOptions, TokenKind, tokenize};
use proptest::prelude::*;

/// Pieces that drive the lexer through every state when joined at random.
const PIECES: &[&str] = &[
    "<?php ",
    "<?",
    "<?=",
    "?>",
    "\n",
    "\r",
    " ",
    "\t",
    "$a",
    "$",
    "${",
    "{$",
    "{",
    "}",
    "(",
    ")",
    "[",
    "]",
    "#[",
    "\"",
    "'",
    "`",
    "\\",
    "->",
    "?->",
    "&",
    "<<<A\n",
    "<<<'A'\n",
    "A",
    "\nA",
    "\n A",
    "\n\tA",
    "0",
    "09",
    "0x1",
    ".5",
    "1e",
    "/*",
    "*/",
    "//",
    "#",
    "yield",
    "from",
    "enum",
    "Foo",
    "(int)",
    "\\u{",
    "\\400",
    "__halt_compiler",
    "\0",
    "\u{e9}",
    "b'",
    "b\"",
    "$a[",
    "x",
];

fn line_breaks(bytes: &[u8]) -> u32 {
    let mut count = 0;
    for (index, &byte) in bytes.iter().enumerate() {
        if byte == b'\n' || (byte == b'\r' && bytes.get(index + 1) != Some(&b'\n')) {
            count += 1;
        }
    }
    count
}

fn check(source: &[u8], short_open_tag: bool) -> Result<(), TestCaseError> {
    let options = LexerOptions {
        short_open_tag,
        ..LexerOptions::default()
    };
    let lexer =
        Lexer::new(source, options).map_err(|error| TestCaseError::fail(error.to_string()))?;
    let tokens: Vec<_> = lexer.collect();
    let mut offset = 0;
    let mut line = 1;
    for token in &tokens {
        prop_assert_eq!(token.start as usize, offset, "tokens must be contiguous");
        prop_assert!(token.end >= token.start);
        prop_assert!(
            token.end > token.start || token.kind == TokenKind::StringFragment,
            "only a string fragment may be empty: {:?}",
            token
        );
        prop_assert!(token.line >= line, "lines never decrease");
        prop_assert!(
            token.line <= 1 + line_breaks(&source[..offset]),
            "a line never runs ahead of the text"
        );
        line = token.line;
        offset = token.end as usize;
    }
    prop_assert_eq!(offset, source.len(), "every byte belongs to a token");
    let whole =
        tokenize(source, options).map_err(|error| TestCaseError::fail(error.to_string()))?;
    let covered: usize = whole
        .tokens
        .iter()
        .map(|token| token.text(source).len())
        .sum();
    prop_assert_eq!(covered, source.len());
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn arbitrary_bytes_are_covered_exactly_once(source in proptest::collection::vec(any::<u8>(), 0..200), short in any::<bool>()) {
        let mut code = b"<?php ".to_vec();
        code.extend_from_slice(&source);
        check(&source, short)?;
        check(&code, short)?;
    }

    #[test]
    fn joined_pieces_are_covered_exactly_once(pieces in proptest::collection::vec(proptest::sample::select(PIECES), 0..30), short in any::<bool>()) {
        let source = pieces.concat();
        check(source.as_bytes(), short)?;
    }
}
