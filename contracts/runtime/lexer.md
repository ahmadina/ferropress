# Lexer

How the reference PHP (version pinned in `contracts/reference-manifest.toml`) splits source bytes into
tokens: their kinds, extents and lines, the warnings raised while reading and the errors compiling the
same source reports. Implemented by `crates/php-lexer`. Differential evidence:

- `contracts/lexer/cases.jsonl`, recorded by `cargo xtask lab lexer` from hand-written cases and
  seeded random joins of fragments, each also run with `short_open_tag` off when it contains `<?`;
- `cargo xtask lab lexer-corpus`, which compares token digests for every `.php` file of the platform
  checkout and the FILE section of every `.phpt` test of the PHP checkout (nothing is committed).

Provenance: `Zend/zend_language_scanner.l` (rules and states) and `ext/tokenizer/tokenizer.c`
(`token_get_all`, `PhpToken::tokenize`, `__halt_compiler` handling) in the pinned PHP source.

## Two views

The tokenizer view (what `token_get_all()` returns without flags) keeps every byte: whitespace,
comments and the PHP tags are tokens, and the scanner's errors are thrown away. Compiling reads the same
tokens but skips whitespace and comments, turns `?>` into `;` and `<?=` into `echo`, strips heredoc
indentation, decodes heredoc escapes and stops at the first scanner error, which becomes a `ParseError`
with the message given below. This crate produces the tokenizer view and records every scanner error in
order; the compile view belongs to the parser.

## Tokens

- **Coverage.** Every byte belongs to exactly one token, in order. A token is a kind, a start and end
  offset and a line. Only one token may be empty: the string fragment described under "Offsets".
- **Kinds.** Single characters (`;`, `(`, `"`, …) are tokens named by the character; with a `b` prefix
  the opening `"` token is two bytes long (`b"`). Every other kind has PHP's name and number
  (`T_LNUMBER` = 260 … `T_BAD_CHARACTER` = 411); number 402 is reported as `T_DOUBLE_COLON`.
- **Lines.** A token's line is 1 plus the line breaks before it, where a break is `\n`, or `\r` not
  directly followed by `\n`. Two exceptions are kept on purpose because the reference has them:
  - a string body whose escape check fails at a bad `\u{…}` counts only the breaks before that
    escape, so later tokens report a line too small;
  - an unterminated single-quoted string counts no breaks at all.

## Outside PHP code

- `<?=` opens code. `<?php` (any case) opens code when followed by a space, tab or line break (the
  token includes that one character, or both of `\r\n`) or by the end of the source. Followed by
  anything else it is `<?` plus a name when short tags are on, and text otherwise.
- `<?` opens code when short tags are on.
- Everything else is inline HTML up to just before the next `<` that starts one of those openings.
- A first line starting with `#!` is ordinary inline HTML in this view. Running a file skips it
  through the first `\n` (or the last `\r` when there is no `\n`) and starts counting at line 2; the
  `skip_shebang` option does the same.

## Names and words

- A name is `[A-Za-z_\x80-\xff][A-Za-z0-9_\x80-\xff]*`. `name(\name)+` is qualified,
  `\name(\name)*` fully qualified, `namespace\name…` (any case) relative, and a lone `\` is the
  namespace separator.
- Reserved words are matched without regard to ASCII case and only as whole names (`classes` is a
  name). `die` and `exit` are the same kind; `__halt_compiler` and the magic constants are reserved.
- `yield`, then at least one run of whitespace or complete comments, then `from` not followed by a
  name character (the end counts) is one `T_YIELD_FROM` token covering all of it.
- `enum` is `T_ENUM` when whitespace or comments lead to a name, and a plain name when they lead to
  `extends` or `implements` (or to nothing). In both cases the token is the four bytes `enum`.
- `public(set)`, `protected(set)`, `private(set)` (any case, no spaces) are single tokens.
- In these lookaheads a comment must be terminated and free of NUL bytes; a `#` comment includes its
  line break. When a comment can be read more than one way, the reading giving the longest match wins.

## Numbers

Digit runs may contain single underscores between digits. The longest of these wins: decimal
`1_000`, float `1.5`, `1.`, `.5` and exponent forms `1e5`, `1.5E-3`, hexadecimal `0x…`, binary `0b…` and
octal `0o…` (prefix letters in any case).

| Literal | Integer when |
|---|---|
| decimal without leading zero | fewer than 19 digits, or the value is at most `PHP_INT_MAX` |
| decimal with leading zero (octal) | fewer than 19 digits up to the first `8`/`9`, or the octal value of the digits before it fits |
| `0x…` | after leading zeros, fewer than 16 digits, or exactly 16 starting with `0`–`7` |
| `0b…` | after leading zeros, fewer than 64 digits |
| `0o…` | the value fits |

Otherwise it is `T_DNUMBER`. A leading-zero decimal containing `8` or `9` is still one token but raises
"Invalid numeric literal".

## Strings

- **Single quotes** (`'…'`, `b'…'`, `B'…'`): `\'` and `\\` are escapes. Unterminated, the rest of the
  source is one `T_ENCAPSED_AND_WHITESPACE`.
- **Double quotes**: when the body holds no `$` before a name start or `{`, no `{$`, and the quote is
  closed (a backslash protects the next byte), the whole literal is one constant string. Otherwise the
  quote is a token and the lexer reads the body piece by piece: `$name` (and `$name->prop`,
  `$name?->prop` when a name starts after the arrow, or `$name[` for an offset), `${`, `{$` (a one-byte
  token), literal text up to the next of those or the closing quote, and the closing quote.
- **Backticks** behave like double quotes with `` ` `` as delimiter, always piecewise.
- **Escapes** in double-quoted and backtick text (constant or piecewise) are checked while reading:
  `\u{}` with no digits, a non-hex digit or no closing brace raises "Invalid UTF-8 codepoint escape
  sequence"; above U+10FFFF it raises "Invalid UTF-8 codepoint escape sequence: Codepoint too large".
  Three octal digits starting above `3` warn "Octal escape sequence overflow \NNN is greater than \377"
  and keep the low byte. `\u` without `{` is left as it is. Values decode `\n \r \t \v \e \f \\ \$`,
  the delimiter's own quote, `\x` with one or two hex digits, one to three octal digits and `\u{…}` as
  UTF-8 (surrogates included); any other backslash is kept.
- **`${name`** followed by `[` or `}` makes `name` a `T_STRING_VARNAME`; anything else is read as code.
- **Offsets** (`"$a[…]"`): integers in any base are `T_NUM_STRING`, `$name` a variable, a name
  `T_STRING`, `]` closes, operator and bracket characters are one-character tokens, and a space, line
  break, tab, `\`, `'` or `#` ends the offset with an empty `T_ENCAPSED_AND_WHITESPACE`. Any other byte
  is a bad character.

## Heredoc and nowdoc

- `<<<` (optionally `b`/`B` before it), spaces or tabs, then `LABEL`, `"LABEL"` or `'LABEL'` (nowdoc),
  then a line break, is the start token, line break included.
- The closing label is the label at the start of a line after optional spaces and tabs, followed by at
  least one more byte that cannot continue a name. Indentation mixing tabs and spaces raises "Invalid
  indentation - tabs and spaces cannot be mixed". The body token ends before the indentation (its last
  line break included); the end token is the indentation plus the label.
- A body ends at the end of the source when no closing label appears; a label right at the end is text.
- Heredoc bodies stop at `$name`, `${` and `{$` like double-quoted strings; a backslash protects the
  next byte unless it is a line break. Nowdoc bodies are plain text.
- After a heredoc start (not nowdoc, not an empty body), the reference reads ahead to measure the
  closing label's indentation: it stops at the matching end, the end of the source or the first scanner
  error, and keeps the indentation of the last closing label it met (a nested heredoc closing later in
  the read overwrites it). If the first body token interpolates and that indentation is not zero, it
  raises "Invalid body indentation level (expecting an indentation level of at least N)". The end token
  spans exactly that measured indentation plus the label, even when the read stopped early and the
  real indentation differs. Nothing the read-ahead meets is reported itself.

## Comments, operators and casts

- `#` and `//` comments run up to, not including, a line break or `?>`. `#[` is an attribute token.
- `/*` runs to the first `*/`; `/**` followed by whitespace is a doc comment (`/**/` is not).
  Unterminated, it runs to the end and raises "Unterminated comment starting line N".
- Operators take the longest match, including `|>`, `<>` (not equal), `<=>`, `**=`, `??=`, `?->`, `...`.
- `&` is "followed by var or vararg" when optional whitespace or comments lead to `$` or `...`, and
  "not followed" otherwise; either way the token is one byte.
- `(`, optional spaces or tabs, a type word, optional spaces or tabs, `)` is a cast: `int`, `integer`;
  `float`, `double`, `real`; `string`, `binary`; `array`; `object`; `bool`, `boolean`; `unset`; `void`
  (any case). Compiling additionally rejects `(real)` and deprecates the non-canonical names.
- After `->` or `?->`, whitespace, comments and further arrows keep the lexer looking for a property;
  a name is then `T_STRING` (even a reserved word), and anything else ends the search without a token.
- Bytes 0x00–0x08, 0x0B, 0x0C, 0x0E–0x1F and 0x7F are bad characters.
- `?>` with one directly following line break is the close tag; it returns to inline HTML.

## Brackets

`(`, `[`, `{`, `#[`, `${` and `{$` open a bracket (recorded with their line); `)`, `]`, `}` close one.

- closing with nothing open: "Unmatched 'X'";
- closing the wrong kind: "Unclosed 'O' does not match 'X'", with " on line N" after `'O'` when it was
  opened on another line; the open bracket stays open;
- at the end of the source with brackets open: "Unclosed 'O'" (plus " on line N" likewise) for the
  innermost one. Reading stops after `__halt_compiler` before this check.

`{` in code also enters a nested code state that `}` leaves, which is how `{$…}` and `${…}` return to
the string around them.

## `__halt_compiler`

After `T_HALT_COMPILER` the tokenizer view collects three more tokens that are not whitespace,
comments or open tags. Whatever follows the third becomes one inline HTML token on the third token's
line, and reading stops.

## Known differences

None. All 4999 recorded cases and all 25286 corpus files match.
