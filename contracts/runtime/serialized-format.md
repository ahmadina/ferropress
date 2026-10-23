# Serialized value format

The byte format that PHP's `serialize()` writes and `unserialize()` reads, as observed on the reference
PHP (version pinned in `contracts/reference-manifest.toml`). Implemented by `crates/php-serialize`;
differential evidence in `contracts/serialize/`, recorded by `cargo xtask lab serialize` from seed inputs,
their truncations and seeded random edits, read with no class instantiable.

Provenance: `ext/standard/var.c` (writer), `ext/standard/var_unserializer.re` (reader),
`Zend/zend_strtod.c` (float text), `Zend/zend_hash.c` (numeric keys), `Zend/zend_execute_API.c` (class
name characters) in the pinned PHP source.

## Tokens

| Value | Written as |
|---|---|
| null | `N;` |
| bool | `b:0;` / `b:1;` |
| integer | `i:<decimal>;` |
| float | `d:<float text>;` |
| string | `s:<byte length>:"<bytes>";` |
| array | `a:<count>:{` then `count` key/value pairs, then `}` |
| object | `O:<name length>:"<class>":<count>:{` then `count` name/value pairs, then `}` |
| class-defined payload | `C:<name length>:"<class>":<payload length>:{<payload>}` |
| enum case | `E:<length>:"<enum>:<case>";` |
| PHP reference | `R:<n>;` |
| repeated object | `r:<n>;` |

Keys are integer or string tokens. Lengths count bytes, so string contents are written raw (quotes, NUL
and non-UTF-8 bytes included). Object property names carry PHP's visibility mangling (`\0*\0name` for
protected, `\0Class\0name` for private) unchanged.

**Numbering.** Every value read outside a key position gets the next number starting at 1 — the
top-level value first, then each nested value in reading order — except `R:` values, which get none.
`R:<n>` and `r:<n>` name an earlier number.

## Float text

The writer uses the shortest decimal digits that read back as the same 64-bit float. When several digit
strings of that length read back, it takes the one nearest the float's exact binary value, and on an exact
tie the one with the even last digit (`2059555018444622.25` is written `2059555018444622.2`). With `p` the
position of the decimal point relative to those digits (`0.5` has digits `5` and `p = 0`; `250` has
digits `25` and `p = 3`):

- `p > 17` or `p < -3`: one digit, `.`, the remaining digits or `0`, then `E`, a sign and the exponent
  `p - 1` without padding — `1.0E+25`, `1.2345678901234568E+17`, `1.0E-5`, `5.0E-324`.
- `-3 <= p <= 0`: `0.`, `-p` zeros, then the digits — `0.5`, `0.0001`.
- otherwise the digits, padded with zeros up to `p` places or split by `.` after `p` digits — `1`, `250`,
  `10000000000000000`, `3.14`.
- Negative values get a leading `-`, including negative zero (`-0`). Non-finite values are `NAN`, `INF`,
  `-INF`.

The reader accepts `NAN`, `INF`, `-INF`, or an optionally signed decimal with digits on at least one side
of an optional `.` and an optional `e`/`E` exponent; it rounds to the nearest float. `+`, a missing integer
or fraction part, and leading zeros are all accepted (`d:+.5e3;` is 500).

## Reading rules

The reader takes one value from the start of the input; bytes after it are reported as extra data and
the value is still returned. Empty input is refused without any message. When the reader refuses an
input it reports a byte offset; each rule below says which offset.

Positions past the end read as a NUL byte, which matches no token character.

- **Token shape.** A byte sequence that does not match any token shape is refused at the token's first
  byte. Integers and lengths in `i:`, `s:`, `S:`, `a:`, `R:` and `r:` tokens must be present (`i:` allows a
  sign; the others are digits only).
- **Integers** (`i:`, and the counts and lengths parsed as integers below) skip leading zeros. More than 19
  significant digits, or a value outside the signed 64-bit range, emits *Numerical result out of range*
  and clamps to the nearest limit.
- **Lengths** in `s:`, `S:`, `O:`, `C:` and `E:` read as unsigned 64-bit and wrap on overflow
  (`s:18446744073709551617:"a";` reads one byte). A length greater than the bytes left after the opening
  quote refuses at the length's first digit; then the byte after the text must be `"` (else refused
  there) and the next byte `;` — or `:` for class names — (else refused there).
- **Legacy escaped strings** `S:` count decoded characters; `\` must be followed by two hex digits. Running
  out of input or a bad escape refuses at the token start. Success emits the deprecation *Unserializing the
  'S' format is deprecated*.
- **Arrays.** The count may not exceed half the bytes after `{` (each entry needs at least two bytes) or
  the 64-bit table limit; otherwise refused after `{`. An empty array must be followed directly by `}`.
- **Keys.** Only `i:` and `s:`/`S:` tokens are keys. Other complete scalar, `R:`/`r:` and `a:` tokens are
  refused just after the token (for `a:`, after `{`); `O:`, `C:` and `E:` are refused at their start.
- **Wrong entry counts.** A `}` where a key or value should be emits *Unexpected end of serialized data*
  and refuses at the `}`. A missing closing `}` refuses where it was expected.
- **Class names** (`O:`, `C:`, `E:`) may not be empty or start with `\`, and may only contain ASCII letters,
  digits, `_`, `\` and bytes 0x80–0xFF; otherwise refused at the token start (empty: at the length).
- **Objects.** When nothing follows the class name's closing `":`, the reader emits *Bad unserialize data*
  and refuses at the closing quote. The property count is parsed as an integer with optional sign
  (no digits reads as 0); a negative count, or one exceeding half the bytes from the closing quote,
  refuses after the count. Then `:{` must follow (refused at the offending byte).
- **Class-defined payloads.** After the class name the payload length is parsed like an object count;
  `:{` must follow. A negative length, or one that leaves no room for the closing brace, emits
  *Insufficient data for unserializing - N required, M present* (M: bytes after `{`) and refuses after `{`.
  The byte after the payload must be `}` (else refused there). The payload is not interpreted.
- **Enum cases.** The text must contain `:`; without it the reader emits *Invalid enum name '…' (missing
  colon)* (text shown up to its first NUL) and refuses at the token start. The enum name must be a valid
  class name (empty is allowed at this stage).
- **References.** `R:<n>` and `r:<n>` refuse just after the token when `n` is 0, when `n` names a number not
  yet given, or when `n` names a value previously stored under the same key of the same array or object
  (a duplicate key overwrites in place, so the reference would point at itself). Keys compare after PHP's
  numeric-key rule: a string that is a canonical decimal integer in the signed 64-bit range (optional
  `-`, no leading zeros, not `-0`) equals that integer. `r:<n>` also refuses when `n` is its own number or
  names a value that is not an object (`O:`, `C:`, `E:` or `r:`).
- **Depth.** Entering a non-empty array, or any object, at the depth limit (default 4096, set by the
  `max_depth` option; 0 disables it) emits *Maximum depth of N exceeded. The depth limit can be changed
  using the max_depth unserialize() option or the unserialize_max_depth ini setting* and refuses after `{`.

Messages above are emitted with the calling function prefix (`unserialize(): …`), except *Bad unserialize
data* and *Insufficient data …*, which have none. The refusal itself is reported as *Error at offset X of Y
bytes*, and extra data as *Extra data starting at offset X of Y bytes*.

## Owned by the runtime, not the format

These depend on loaded classes or on PHP values and are checked by the runtime that drives the reader;
the rules above describe the reader when no class may be instantiated (`allowed_classes => false`):

- class lookup, autoloading and the unserialize callback; `allowed_classes`; classes that forbid
  serialization; enum and case resolution (a missing enum or case refuses at the token start);
- `__serialize`/`__unserialize`, `__sleep`/`__wakeup`, `Serializable` payloads and their messages
  (for example *Class X has no unserializer* for `C:` without a loaded class);
- array key normalization (`"5"` becomes `5` in arrays; integer property names become strings in
  objects), which changes what a value serializes back to;
- resolving `R:`/`r:` numbers into shared values, including what a number refers to after a duplicate key
  replaced the value it was given to;
- declared and typed properties, visibility changes and dynamic-property rules.
