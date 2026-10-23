# Parser

How the reference PHP (version pinned in `contracts/reference-manifest.toml`) turns the compile view
of the tokens (see `lexer.md`) into a syntax tree: which sources it accepts, the error that stops it
(class, message, line), the warnings it raises on the way, and how the engine prints an expression
back as text. Implemented by `crates/php-parser`. Differential evidence:

- `contracts/parser/cases.jsonl`, recorded by `cargo xtask lab parser` from hand-written cases, the
  lexer oracle's cases, seeded random expressions, seeded one-token edits of valid cases and
  hand-written cases grouped by grammar area;
- `cargo xtask lab parser-corpus`, which parses every `.php` file of the platform checkout and the
  FILE section of every `.phpt` test of the PHP checkout and compares the error and warnings
  (nothing is committed).

Provenance: `Zend/zend_language_parser.y` (grammar, `zend_yytnamerr`), `Zend/zend_language_scanner.l`
(compile view of tokens, `zend_scan_escape_string`, heredoc indentation stripping) and
`Zend/zend_compile.c` (errors raised while building the tree) in the pinned PHP source.

## Outcome

Parsing yields either a tree or exactly one error, plus the warnings raised before the error or the
end. The error is a `ParseError` for syntax and scanner errors and a `CompileError` for the few
checks done while the tree is built (repeated or conflicting modifiers, `__HALT_COMPILER()` outside
the outermost scope, a non-literal `declare(encoding=…)`). Its line is the line of the token being
read when it was detected, except where noted below.

Warnings, in source order:

- the scanner's warnings ("Octal escape sequence overflow …"), including those raised in a string
  that later fails on a bad `\u{` escape;
- `Deprecated`: "Non-canonical cast (integer) is deprecated, use the (int) cast instead", and the same
  for `double`→`float`, `binary`→`string`, `boolean`→`bool`;
- `Warning`: "declare(encoding=...) ignored because Zend multibyte feature is turned off by settings"
  for each literal `encoding` directive.

## Syntax error messages

`syntax error, unexpected X` names the token the parser stopped at:

| Token | Named as |
|---|---|
| end of input | `end of file` |
| `"` | `double-quote mark` |
| single character | `token "c"` |
| a byte the scanner rejects | `character 0xNN` |
| reserved word or operator | `token "spelling"` (e.g. `token "namespace"`, `token "?->"`) |
| anything else | its label and its text in quotes: `integer "1"`, `variable "$a"`, `identifier "f"`, `double-quoted string "a"` … |

The quoted text stops at the first line break and loses one surrounding quote on each side. Text longer
than 33 bytes is cut to 30 bytes plus `...`. When the shown text contains a NUL byte the message ends at
the closing quote after the bytes before the NUL, and nothing more is added.

`, expecting A or B …` follows when the parser state at the error accepts between one and four token
kinds. They are listed in the grammar's token order, end of input first, quoted spellings or labels
as above. With more than four, or none, the suffix is absent. Because the reference parser reduces by
default in states that have a reduction, the expected set is the one of the first state that only
shifts, which is often narrower than what the language would allow at that point. Notable sets:

- **Argument start.** At the start of a positional argument (after `(` or `,` of a call or attribute,
  not after `...`), a reserved word that could name a named argument, followed by a token that cannot
  continue the expression it would start, is read as that name: the message is at the following token,
  `expecting ":"`. This covers `new` not followed by a class reference, `class`, `#[` or `static`;
  `static` not followed by `::`, `fn` or `function`; `match`, `list`, `array`, `isset`, `empty`, `eval`
  not followed by `(`; and `clone`, `print`, `include`, `include_once`, `require`, `require_once`,
  `throw` not followed by an expression start. `function` and `fn` still expect `"("`.
- **`use` imports.** First name after `use`: no list. After `use function` / `use const`, or after a
  comma in a plain list: identifier, fully qualified name or namespaced name. First item of a group:
  identifier, namespaced name, `function` or `const` (only the two names in a group declared
  `function` or `const`). After a comma in a group: `"}"`. After `function`/`const` inside a group:
  identifier or namespaced name. After `as`: identifier. Group items never start with `\`.
- **`for`.** A `(void)` cast in the condition clause must be followed by `,`; elsewhere in `for` it
  may end the clause.
- **`clone(`.** After the first argument of a parenthesized `clone`, a comma followed by anything but
  `)` or an argument start gives no list.
- **Trait adaptations.** A bare method name not followed by `as` expects `"::"`.

When the parser's stack would grow past 10000 entries the message is `memory exhausted` (a
`ParseError`). Each level of nested expression operand or statement counts as one entry.

## Compile view details

- `?>` acts as `;`. A directly following line break belongs to it, and the token's line is the line of
  `?>` itself.
- An unterminated single-quoted string in code is one string fragment whose value is its raw text, with
  no escape decoding.
- Heredoc and nowdoc bodies lose the closing label's indentation from every line. A line that ends,
  or a segment that ends before an interpolation, before the full indentation has been seen raises
  "Invalid body indentation level (expecting an indentation level of at least N)", on the line where
  the shortfall is. Only the segment right before the closing label may end inside the indentation.
  Whitespace that mixes tabs and spaces raises "Invalid indentation - tabs and spaces cannot be mixed";
  for the indentation of the start token's following line it is reported one line after the start.
- A heredoc without a closing label keeps indentation 0 and still decodes escapes. A nowdoc without one
  is stripped by the indentation of its last line. A body whose last line break, followed only by
  spaces or tabs, reaches the end of the source is kept as written: not stripped, not decoded.
- `__halt_compiler();` ends the input. The end of input then fails like a real one when a bracket is
  still open: "Unclosed '{' on line N", on the scanner's line after the halt.

## Rendering

The engine prints the tree of an expression when an assertion fails; the oracle captures that text for
a closure whose body is the case's code. The printer uses four-space indentation, one statement per
line, parentheses only where operator priority or associativity needs them, canonical spellings of
literals, and closures called directly wrapped in parentheses. Integers are decimal. Floats have 14
significant digits, plain unless the exponent is below -4 or at least 14 (`9.2233720368548E+18`), with
`.0` added when no point is written. Strings are single-quoted with `\'` and `\\` escaped. A string
holding a control character is double-quoted instead: `\n \r \t \f \v \e` escapes, other control
bytes as three-digit octal, and `"`, `$` and `\` escaped. Code that
cannot appear in a function body (namespaces, imports, top-level constants) has no rendering.

## Implementation limits

The parser runs on its own thread with a large stack (`ParseFailure::Thread` when it cannot start),
so the nesting allowed by the reference fits. Printing or dropping a tree recurses on the caller's
thread: printing a tree nested close to the 10000 limit needs more than 16 MiB of stack in debug builds
(about 2 MiB in release builds).

## Known differences

None. All 7188 recorded cases match (error, warnings and rendering), and all 25286 corpus files match
(158 of them are rejected by the reference with the same error).
