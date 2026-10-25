# Evaluation order

When the reference PHP (version pinned in `contracts/reference-manifest.toml`) reads operands and runs
side effects inside one expression. The bytecode compiler (`crates/php-compiler`, `emit`) produces
code with this order, and the VM runs it.

Provenance: `zend_compile_expr_inner`, `zend_compile_binary_op`, `zend_compile_greater`,
`zend_compile_encaps_list` and `zend_compile_rope_finalize` in `Zend/zend_compile.c`.

## Local variables are read late

An operand that is a plain local variable is read when its operator runs, not when the compiler
reaches it. Operands that are expressions are evaluated left to right first. So a later operand
that changes the variable is seen by the earlier operand:

- `$a = 1; echo $a + ($a = 5);` prints `10`.
- `$a = 1; echo $a . ($a = 2);` prints `22`.
- `$i = 1; echo $i + $i++;` prints `3`: the increment yields `1` and leaves `$i` at `2` before the
  addition reads `$i`.

Every other operand (a compound assignment, an increment, a call, a constant) yields its value when
it is evaluated:

- `$a = 1; echo ($a += 1) + ($a = 5);` prints `7`.

`>` and `>=` evaluate their left operand first, then compare as `<` and `<=` with the operands
swapped.

## Expression statements

A statement that is only a local variable, such as `$a;`, does nothing: it does not read the
variable, so an undefined variable raises no warning.

## Interpolated strings

The parts of a string with embedded expressions are joined like this:

- Adjacent literal parts merge, and empty literal parts drop out.
- One remaining part is converted to a string.
- Two remaining parts are concatenated. Both are read when they are joined, so a local variable in
  the first part sees changes made while the second part is evaluated.
- With three or more parts, each part is converted to a string right after it is evaluated, before
  the next part is evaluated.

With `function f() { $GLOBALS["a"] = 2; return 0; }`, `$b = ["x"]` and `$c = "c"`, and `$a` reset to
`1` before each line:

| Expression | Prints |
|---|---|
| `"$a{$b[f()]}"` | `2x` |
| `"x$a{$b[f()]}"` | `x1x` |
| `"$c$a{$b[f()]}"` | `c1x` |
| `"$a-{$b[f()]}"` | `1-x` |
| `"{$b[f()]}$a"` | `x2` |
