# Compile-time checks

What the reference PHP (version pinned in `contracts/reference-manifest.toml`) reports while compiling a
file, before any of it runs. Implemented by `crates/php-compiler` (`check`); differential evidence in
`contracts/compiler/cases.jsonl`, recorded by `cargo xtask lab compiler`. Cases that do not match yet
are listed in `crates/php-compiler/tests/pending.txt`.

Provenance: `Zend/zend_compile.c` in the pinned PHP source.

Diagnostics are reported on the compiler's current line: the line of the statement or expression it
last started compiling. That line is not restored when a child finishes.

## Constant expressions

Constant expressions are the values of top-level `const` declarations, class constants, property
defaults, parameter defaults, enum case values and attribute arguments. Each is handled in two passes:
folding, then validation.

### Folding

Folding evaluates what is already known and replaces the node with the result. A node whose operands
are not known stays, but its operands are still folded. Folding rules:

- **Literals.** Integers, floats and strings are values.
- **Arithmetic, string, bitwise, comparison and `xor` operators.** These fold when both operands are
  values and evaluating them would neither throw nor warn (`contracts/runtime/operators.md`). That
  excludes:
  - `.` with an array;
  - a numeric operator with an array, except `+` on two arrays;
  - a non-numeric string with a numeric operator, except a bitwise operator on two strings;
  - a shift, bitwise operator or `%` with an operand that does not convert exactly to an integer;
  - `/` or `%` by zero;
  - `0 ** n` with a negative `n`;
  - a negative shift.

  `>` and `>=` are evaluated as `<` and `<=` with the operands swapped.
- **`&&` and `||`.** Both operands are folded first. If the left one is known and decides the result,
  that result is the value. Otherwise both must be known.
- **Unary operators.**
  - `!` folds unless its operand is a float.
  - `~` folds on strings and on integers, and on floats that convert exactly; never on `null` or
    booleans.
  - Unary `+` and `-` fold as multiplication by `1` and `-1`.
  - `@` never folds.
- **`??`.** With a known left side, the node becomes the right side when the left is `null`, and the left
  side otherwise. With an unknown left side, the right side is still folded.
- **`?:` and `? :`.** With a known condition, the node becomes the selected branch, which is then folded.
  A true condition in `?:` selects the condition itself. With an unknown condition, both branches are
  still folded.
- **`[]` reads.**
  - A read without an index is a fatal error: "Cannot use [] for reading".
  - An array with an integer or string key folds to the element when the key exists. A string key is
    normalised to an integer when it is one.
  - A string with an integer offset folds to that byte when it is in range. A string offset may also be
    a string whose leading number is an integer.
- **Array literals.**
  - `list()` syntax is a fatal error: "Cannot use list() as standalone expression".
  - An empty element is a fatal error: "Cannot use empty array elements in arrays". It is reported on
    the line of the last element before it.
  - All elements are folded first. The array folds only when every key and value is known and none is
    taken by reference.
  - Building the array:
    - Spreading a non-array is a fatal error: "Only arrays and Traversables can be unpacked, T given".
      Here `T` is `int`, `float`, `string`, `null`, `true` or `false`.
    - An array key is a fatal error: "Illegal offset type".
    - `null` keys, floats that are not exact integers, and running out of integer keys leave the array
      unfolded.
    - Integer-like floats and booleans become integer keys.
- **Casts.**
  - `(unset)` is a fatal error: "The (unset) cast is no longer supported".
  - A `NAN` operand never folds.
  - `(int)` does not fold a float outside the integer range.
  - `(string)` does not fold an array or a float, because the float output depends on run-time
    `precision`.
  - `(object)` never folds.
- **Constants.** Only `true`, `false` and `null` fold, in any case. This works unqualified (even inside
  a namespace), fully qualified, or as `namespace\…` outside any namespace.
- **`X::class`.**
  - A literal number as `X` is a fatal error: "Illegal class name".
  - A string literal folds to itself.
  - `self`, `parent` and `static` first check the scope, when the scope is known. Outside a class this
    raises "Cannot use "self" when no class scope is active" (or `parent`, `static`). `parent` in a
    class without a parent raises "Cannot use "parent" when current class scope has no parent".
  - `self` folds to the class name when the scope is known.
  - The scope is known inside a class that is not a trait, and inside a named function. It is not known
    in closures, in traits, or at the top level of a file.
- **`X::NAME`.** A non-string value as `X` is a fatal error: "Illegal class name".
- **Magic constants.**
  - `__LINE__` is the line it is written on.
  - `__NAMESPACE__` is the current namespace, or empty.
  - `__CLASS__` is the namespaced class name. It is empty outside a class and left unfolded inside a
    trait.
  - `__TRAIT__` is the trait name inside a trait, and empty elsewhere.
  - `__FUNCTION__` is the namespaced function name, the method name, or empty at the top level. A
    property hook is named `$property::get` or `$property::set`.
  - `__METHOD__` is `Class::method` in a method. It is the function name in a function. It is empty at
    the top level and directly inside a class body, even when that class is declared inside a function.
  - `__PROPERTY__` is the property name while the property's default, attributes and hooks compile, and
    empty elsewhere (including in closures).
- **`NAN` warnings.** Converting `NAN` to a boolean or string while folding raises "unexpected NAN value
  was coerced to bool" (or "to string") as a compile-time warning. This happens for `?:`, `&&`, `||`,
  `xor` and `.`.

Array literals in ordinary code are folded the same way before they are compiled. So their folding
errors and `NAN` warnings also occur outside constant expressions, and the compiler then compiles the
replaced nodes. For example, `[1 ? $a : $this = 1]` compiles only `$a`.

### Validation

After folding, each remaining node must be allowed. Anything not listed is a fatal error: "Constant
expression contains invalid operations". That covers variables, assignments, `isset`, `empty`,
interpolation, `print`, `include`, `match`, `throw`, `instanceof`, `@`, `|>`, arrow functions, `yield`,
static properties and method calls.

Allowed nodes, with their checks:

- **Operators.** Operators, casts, ternaries, `??`, `[]` reads, array literals, spreads and magic
  constants are allowed.
- **Constants.** A constant reference is allowed and moves the current line to it.
- **`X::NAME`.**
  - `X` must be a name: "Dynamic class names are not allowed in compile-time class constant
    references".
  - `X` must not be `static`: ""static::" is not allowed in compile-time constants".
- **`X::class`.**
  - `X` must be a name: "(expression)::class cannot be used in constant expressions".
  - `X` must not be `static`: "static::class cannot be used for compile-time class name resolution".
- **`new`.**
  - Only where dynamic values are allowed: "New expressions are not supported in this context".
  - Not an anonymous class: "Cannot use anonymous class in constant expression".
  - The class must be a name: "Cannot use dynamic class name in constant expression".
  - The class must not be `static`: ""static" is not allowed in compile-time constants".
  - Not `new X(...)`: "Cannot create Closure for new expression".
  - Its arguments must not be spread: "Argument unpacking in constant expressions is not supported".
  - Named arguments come last: "Cannot use positional argument after named argument".
- **`(object)` casts.** Only where dynamic values are allowed: "Object casts are not supported in this
  context".
- **Property reads.** `->` and `?->` reads are allowed.
- **Closures.**
  - They must be `static`: "Closures in constant expressions must be static".
  - They must not capture variables: "Cannot use(...) variables in constant expression".
  - The closure is then compiled as a function.
- **First-class callables.**
  - Only `f(...)` and `X::m(...)` are allowed; other calls are invalid operations.
  - The function must be a name: "Cannot use dynamic function name in constant expression".
  - The class follows the `new` rules.
  - The method must be a name or a string: "Cannot use dynamic method name in constant expression".

Dynamic values are allowed in top-level constants, parameter defaults and attribute arguments. They are
not allowed in class constants, property defaults or enum case values.

### Attributes

- Arguments are checked in order: no `X(...)` ("Cannot create Closure as attribute argument"), no spread
  ("Cannot use unpacking in attribute argument list"), no repeated name ("Duplicate named parameter
  $name"), and no positional argument after a named one.
- Each argument is then compiled as a constant expression.
- A top-level `const` with attributes must declare one constant: "Cannot apply attributes to multiple
  constants at once".
- A constant named `true`, `false` or `null` in any case is a fatal error: "Cannot redeclare constant
  'NAME'".

Compile order:

- A class compiles its attributes first.
- A function compiles its attributes after its `use` checks and before its parameters.
- A parameter compiles its default, then its attributes.
- A class constant or enum case compiles its value, then its attributes.
- A property compiles its default, then its hooks, then its attributes.

### Not yet implemented

The following stay unfolded, so their values do not take part in later checks:

- Constants other than `true`, `false` and `null`. These need the built-in constant registry.
- Class constants.
- `X::class` for a class name, which needs import resolution, and for `parent`.
- `__FILE__` and `__DIR__`.
- Names that hold the file path: closures and anonymous classes.
- Folding in ordinary code other than array literals.
