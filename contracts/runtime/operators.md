# Operators and conversions

How the reference PHP (version pinned in `contracts/reference-manifest.toml`) converts values between
types and applies its operators, including every warning, deprecation and thrown error. Implemented by
`crates/php-value` (`ops`); differential evidence in `contracts/operators/`, recorded by
`cargo xtask lab operators` over a fixed grid of operands (every unary operation on each operand, every
binary operator on each ordered pair), with `precision` 14 and without strict types.

Provenance: `Zend/zend_operators.c` and `Zend/zend_operators.h` (conversions, operators, numeric
strings), `Zend/zend_strtod.c` (decimal to float, float to decimal), `Zend/zend_multiply.h` (overflow),
`Zend/zend_compile.c` (unary minus and plus) and `Zend/zend_vm_def.h` (casts) in the pinned PHP source.

Objects are out of scope: they convert, combine and compare through their class's handlers, which the
engine supplies. Every operation below rejects an object operand before raising anything.

## Numeric strings

A string is read as a number like this:

1. Leading whitespace (space, `\t`, `\n`, `\r`, `\v`, `\f`) is skipped.
2. An optional `+` or `-` follows.
3. Then either digits (optionally followed by `.` and more digits, possibly none), or `.` followed by at
   least one digit.
4. Then an optional exponent: `e` or `E`, an optional sign and at least one digit. Without a digit the
   `e` is not part of the number.

If no number starts the string, it is **non-numeric**. If only whitespace follows the number, the string
is **numeric**. Otherwise it is **leading-numeric** (`"12 apples"`, `"1\0"`, `"0x1A"` reads as 0
followed by `x1A`). Hexadecimal, octal and binary forms, `INF` and `NAN` are not numbers.

Digits with no fraction or exponent give an integer if they fit; leading zeros are ignored. Otherwise the
number is a float, correctly rounded, and possibly infinite (`"1e1000"`). An integer string with 20 or
more digits after its leading zeros, or a 19-digit one beyond the integer range, is read as a float and
remembered as **overflowing** to its side. That flag matters to string comparison. One quirk:
`"-9223372036854775808"` is an integer only when nothing but a NUL byte follows the digits; with a
trailing space it is a float.

`(float)` reads a string differently: it takes the float at the start of the string after whitespace
(the same grammar, never an integer), ignores whatever follows, and gives 0 when there is none
(`"-abc"` gives `0.0`, not `-0.0`; `"-0"` gives `-0.0`).

## Casts

| From | `(bool)` | `(int)` | `(float)` | `(string)` | `(array)` |
|---|---|---|---|---|---|
| `null` | false | 0 | 0.0 | `""` | `[]` |
| `false` / `true` | itself | 0 / 1 | 0.0 / 1.0 | `""` / `"1"` | `[value]` |
| integer | not 0 | itself | nearest float | decimal | `[value]` |
| float | not ±0.0 | see below | itself | see below | `[value]` |
| string | not `""` or `"0"` | see below | see above | itself | `[value]` |
| array | not empty | 1 if not empty | 1.0 if not empty | `"Array"` + warning | itself |

- **`NAN`** is true as a bool, and becoming a bool, a string or an array warns
  "unexpected NAN value was coerced to bool" (or `string`, `array`). The same warning comes from `!`
  and from conditions, which use the bool conversion.
- **Float to `(int)`**: truncates toward zero when the float is in the integer range. Out of range, the
  result wraps modulo 2^64 into the signed range and the warning "The float X is not representable as an
  int, cast occurred" is raised. For an infinity or `NAN` the result is 0 with the same warning.
- **String to `(int)`**: the number at the start of the string, 0 if there is none, with no warnings. A
  float that is out of range saturates at `PHP_INT_MAX` / `PHP_INT_MIN`; an infinity gives 0.
- **Float to string**: rounded to `precision` significant digits (14 by default; exact ties go to the
  even digit), with trailing zeros dropped. It is written in plain notation while the decimal exponent
  (the position of the decimal point relative to the first digit) is within -3..=`precision`, and as
  `d.dddE+x` otherwise (`1.0E+15`, `1.0E-5`, `-0`). `NAN`, `INF` and `-INF` are written as such.
  A `precision` of -1 uses the shortest digits that read back exactly, with plain notation up to an
  exponent of 17; a `precision` of 0 means 1.
- Diagnostic messages always print floats with the shortest digits (`1.0E+100`, `9.223372036854776E+18`).

## Arithmetic: `+ - * / % **`

Unary `-x` and `+x` compile to `x * -1` and `x * 1`. Their results and errors are those of `*`, so
`-"abc"` throws "Unsupported operand types: string * int".

Operands are converted to numbers, left first:

- `null` and `false` become 0 and `true` becomes 1;
- a numeric string becomes its number;
- a leading-numeric string becomes its number with the warning "A non-numeric value encountered";
- a non-numeric string or an array is **rejected**.

Once an operand is rejected, `TypeError` "Unsupported operand types: L op R" is thrown at once, naming
the original types (`null`, `bool`, `int`, `float`, `string`, `array`), and the right operand is not
converted. Warnings from an operand converted before that stay raised.

- `+` on two arrays is their union: every element of the left, then each element of the right whose key
  the left lacks. Elements bound to references stay bound.
- `+`, `-` and `*` on two integers give an integer, or the float result of the operands as floats when
  the result overflows. Any float operand gives a float.
- `/` throws `DivisionByZeroError` "Division by zero" when the divisor is 0 or ±0.0, after both operands
  are converted. Two integers that divide exactly give an integer (`PHP_INT_MIN / -1` is a float);
  everything else gives a float.
- `**` with two integers and a non-negative exponent works by repeated squaring. If a step overflows,
  the rest of the power is computed in floats from the values reached so far. Any other combination is
  a float power. A zero base with a negative exponent raises "Power of base 0 and negative exponent is
  deprecated" (and gives `INF`).
- `%` converts both operands to integers as described below. It throws `DivisionByZeroError` "Modulo by
  zero" for a zero divisor. The result takes the sign of the left operand, and `x % -1` is 0.

## Integer operands: `% << >> | & ^ ~`

These operators convert operands to integers:

- like arithmetic, booleans, `null` and strings are accepted, and arrays and non-numeric strings are
  rejected;
- a float converts like `(int)` (with the out-of-range warning). If it is in range but not an integer
  (or is `NAN`), it also raises the deprecation "Implicit conversion from float X to int loses
  precision";
- a numeric string holding a float saturates like `(int)`. If the result does not equal the float, the
  deprecation "Implicit conversion from float-string "S" to int loses precision" is raised, printing the
  string up to its first NUL byte.

The operators behave as follows:

- `<<` and `>>` throw `ArithmeticError` "Bit shift by negative number" for a negative count. A count of
  64 or more gives 0, or for `>>` of a negative number, -1. `<<` discards bits shifted out; `>>` keeps
  the sign.
- `|`, `&` and `^` on two strings combine them byte by byte: `|` over the longer length (the rest copied
  from the longer string), `&` and `^` over the shorter. Otherwise both operands are integers.
- `~` complements an integer, a float converted as above, or every byte of a string. For `null`, a
  boolean or an array it throws `TypeError` "Cannot perform bitwise not on null" (or `false`, `true`,
  `array`).

## Concatenation: `.`

Both operands are converted as `(string)` converts them, left first (so each array warns "Array to
string conversion" and each `NAN` warns), and joined.
