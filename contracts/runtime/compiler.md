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
  - `__FUNCTION__` is the namespaced function name, the method name, the closure name (see "Closure
    names"), or empty at the top level. A property hook is named `$property::get` or `$property::set`.
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
- A parameter compiles its default, then its attributes, then its type.
- A class constant compiles its type, then its value, then its attributes. An enum case compiles its
  value, then its attributes.
- A property compiles its type, then its default, then its hooks, then its attributes.

### Not yet implemented

The following stay unfolded, so their values do not take part in later checks:

- Constants other than `true`, `false` and `null`. These need the built-in constant registry. In
  property defaults and class constants the reference substitutes built-in constants, so a mistyped
  default such as `public int $a = PHP_EOL;` is not reported yet. Parameter defaults never substitute
  them.
- Class constants.
- `X::class` for a class name and for `parent`.
- `__FILE__` and `__DIR__`.
- Anonymous class names, and the names of closures declared inside their methods.
- Class constants in ordinary code, which the reference evaluates while compiling when the class is
  known: `(self::B)::f()` with `const B = 1` is "Illegal class name" there.
- Folding in ordinary code other than array literals.

## Class names

A class name in a type or an `extends` clause is resolved like this:

- A fully qualified name is taken as written.
- `namespace\X` gets the current namespace prefixed.
- Otherwise, a `use` import whose alias matches the name's first part, compared case-insensitively,
  replaces that part.
- Anything else gets the current namespace prefixed.

Only class imports count (`use X`, and the class items of a group `use`). Each namespace starts with no
imports. `self`, `parent` and `static` stay as written, and qualifying them is a fatal error:
"'\self' is an invalid class name" or "'namespace\self' is an invalid class name".

## Class references

Here "special" means `self`, `parent` or `static` in any case, and "the scope check" is the check
described under "Constant expressions": where the scope is known, a special class needs an active
class ("Cannot use "self" when no class scope is active") and `parent` needs that class to have a
parent ("Cannot use "parent" when current class scope has no parent").

- **Class operands.** The class of `X::m()`, `X::$p`, `X::C`, `new X` and `instanceof X`:
  - A special name that is not fully qualified (`self`, `namespace\self`) gets the scope check.
  - Any other name is resolved as a class name, so `\self` is a fatal error.
  - A literal number is a fatal error: "Illegal class name".
  - A literal string is a fully qualified name, with or without its leading `\`; a special one is a
    fatal error: "'\self' is an invalid class name". The parser joins `'a' . 'b'` into one literal.
  - Any other operand is compiled first. If it compiles to a constant (literals, `true`, `false`,
    `null`, `X::class` with a known result, and operators and casts over those), a string is checked
    like a written name without a prefix (special ones get the scope check, others resolve as a fully
    qualified string) and any other value is "Illegal class name".
  - `instanceof` does not compile its class at all when the tested value compiles to a constant.
- **`X::class`** in ordinary code:
  - A literal number is "Illegal class name".
  - A special name or string, prefixed or not, gets the scope check; `\self::class` is accepted inside
    a class.
  - Other names and strings resolve as above.
  - Any other operand is compiled; if it compiles to a constant, that is a fatal error: "Cannot use
    "::class" on string" (or `int`, `true`, `null`, …). `self::class` and `parent::class` compile to
    constants where the scope is known, so `self::class::class` fails there.
- **Declarations.** The parent of a class is resolved first (after the name checks), then the class's
  attributes compile, then its interfaces (or an interface's parents). Trait names resolve when their
  `use` compiles. A special name that is not fully qualified is a fatal error there: "Cannot use
  "self" as class name, as it is reserved", with "interface name" or "trait name". A `use` inside an
  interface is a fatal error first: "Cannot use traits inside of interfaces. T is used in I".
- **`catch`.** Each class is checked in order. A special name that is not fully qualified is a fatal
  error: "Bad class name in the catch statement". The others resolve as class names. Errors are
  reported on the line of the catch's first class.

## Namespaces, imports and declarations

### Namespaces

- Braced and unbraced namespace declarations cannot be mixed: "Cannot mix bracketed namespace
  declarations with unbracketed namespace declarations". This is checked first, so an unbraced
  declaration after a braced one, or a braced one after a named unbraced one, reports it.
- A braced namespace inside another one is a fatal error: "Namespace declarations cannot be nested".
- The first namespace declaration of the file may only be preceded by `declare` statements and empty
  statements: "Namespace declaration statement has to be the very first statement or after any declare
  call in the script". Later unbraced declarations may follow any code.
- Once a braced namespace exists, every top-level statement other than a namespace or
  `__halt_compiler()` must be inside one: "No code may exist outside of namespace {}". The check runs
  after the statement compiles.
- Each namespace start and end clears the imports.

### Imports

- The alias of `use A\B` is `B`, the last part of the name. In a group `use`, the name is the prefix
  followed by the item; an item's own kind overrides the group's.
- `use A;` outside any namespace warns: "The use statement with non-compound name 'A' has no effect".
  The import is still recorded.
- A class alias that is `self`, `parent` or `static` in any case is a fatal error: "Cannot use A\self as
  self because 'self' is a special class name".
- Class and function aliases compare case-insensitively, constant aliases case-sensitively. Reusing an
  alias of the same kind in the namespace is a fatal error: "Cannot use A\B as B because the name is
  already in use", with ` function` or ` const` after `use` for those kinds. The same error arises
  when the file already declared a symbol of that kind named `NAMESPACE\ALIAS`, unless the imported
  name is that symbol (compared case-insensitively).

### Declarations

A file records the classes, functions and constants it declares: classes and functions by lowercase
namespaced name, constants by namespaced name as written.

- **Functions.** Before its body compiles, a named function is checked in this order:
  - a function import with the same alias that names a different function: "Cannot redeclare function
    N\f() (previously declared as local import)";
  - a lowercase namespaced name of `__autoload`: "__autoload() is no longer supported, use
    spl_autoload_register() instead";
  - a written name of `assert` in any case, in any namespace: "Defining a custom assert() function is
    not allowed, as the function has special semantics".

  A function declared directly at the top level of the file or of a namespace that was already declared
  there is a fatal error after its body compiles: "Cannot redeclare function f() (previously declared in
  FILE:LINE)", reported on the second declaration's first line. Functions declared inside blocks or
  other functions are bound at run time and are not checked.
- **Classes, interfaces, traits and enums.** A named one is checked before anything else of it compiles:
  - declaring it while another class body is being compiled (in a method, for example) is a fatal error:
    "Class declarations may not be nested";
  - a reserved class name is a fatal error: "Cannot use "self" as a class name as it is reserved", with
    "an interface name", "a trait name" or "an enum name" for the other kinds;
  - the name `_` is deprecated: "Using "_" as a class name is deprecated since 8.4";
  - a class import with the same alias that names a different class: "Cannot redeclare class N\B
    (previously declared as local import)".
- **Constants.** After its value and the `true`/`false`/`null` check, a top-level constant whose name has
  a constant import naming a different constant is a fatal error: "Cannot declare const N\C because
  the name is already in use".

### Closure names

A closure or arrow function is named `{closure:PARENT:LINE}`, where `LINE` is the line it starts on and
`PARENT` is:

- the file path, at the top level of the file or directly in a class body;
- `f()` in a function, with its namespaced name, and `A::m()` in a method;
- the enclosing closure's name, in a closure.

An anonymous class's generated name holds a NUL byte, and messages stop at it. A closure inside a
method of an anonymous class therefore shows as `{closure:class@anonymous` in messages.

## Declared types

Types are compiled for return types, parameters, properties and class constants.

### Single types

- `iterable` stands for `Traversable|array`.
- A builtin name (`null`, `true`, `false`, `int`, `float`, `string`, `bool`, `void`, `never`,
  `iterable`, `object`, `mixed`, `static`) matches in any case. Written qualified, it is a fatal error:
  "Type declaration 'int' must be unqualified". The name is printed in lowercase, without the prefix.
- `static` needs a class when the scope is known: "Cannot use "static" when no class scope is active".
  Scope is known as described for `X::class`.
- `self` and `parent` check the scope the same way as in `X::class`. When the scope is known and the
  class has a name, they become the class name and the parent's name. Otherwise, in traits, closures
  and anonymous classes, they stay as written.
- Any other name is a resolved class name:
  - A name whose last part is reserved (the builtin names, plus `parent`, `self`, `array` and
    `callable`) is a fatal error: "Cannot use "A\int" as a type name as it is reserved".
  - A name that resolves to exactly `_` is deprecated: "Using "_" as a type name is deprecated since
    8.4".
  - The unqualified names `boolean`, `integer`, `double` and `resource` (matched case-sensitively, and
    not imported) raise a compile warning. For the first three it is ""integer" will be interpreted as a
    class name. Did you mean "int"? Write "\integer" to suppress this warning". For `resource` it is
    ""resource" is not a supported builtin type and will be interpreted as a class name. Write
    "\resource" to suppress this warning". Inside a namespace, the written name is the resolved one and
    ` or import the class with "use"` follows it.

### Unions, intersections and nullability

- **Union members** are checked left to right:
  - `mixed` is not allowed: "Type mixed can only be used as a standalone type".
  - A builtin kind already present is a fatal error: "Duplicate type int is redundant". `bool` overlaps
    with `true` and `false`, and `iterable` overlaps with `array`.
  - `true` with `false` is a fatal error: "Type contains both true and false, bool must be used instead".
  - A class already present (compared case-insensitively) is a fatal error: "Duplicate type a is
    redundant", naming the later spelling.
  - A class that is also part of an intersection in the union is a fatal error: "Type A&B is redundant
    as it is more restrictive than type A".
  - Two intersections where one's classes include all of the other's are a fatal error. With equal
    sizes it is "Type B&A is redundant with type A&B" (the later one first). Otherwise it is "Type
    A&B&C is redundant as it is more restrictive than type A&B".
  - After the members, `object` together with `static` or with a class (other than the one `iterable`
    implies) is a fatal error: "Type A|object contains both object and a class type, which is
    redundant".
- **Intersection members** must be plain classes: "Type int cannot be part of an intersection type"
  (also for `iterable`, printed `Traversable|array`). `self` and `parent` are also rejected when they
  stayed as written. A repeated class is "Duplicate type a is redundant".
- **Nullability.**
  - `?mixed` is a fatal error: "Type mixed cannot be marked as nullable since mixed already includes
    null".
  - `?null` is a fatal error: "null cannot be marked as nullable".
  - `void` and `never` must stand alone, including not with `?` or `null`: "Void can only be used as a
    standalone type" and "never can only be used as a standalone type".

### Printing

Messages print a type in this order:

1. Classes in the order written. A union's intersections are in parentheses; a type that is one
   intersection has none.
2. Builtin kinds in a fixed order: `static`, `callable`, `object`, `array`, `string`, `int`, `float`,
   then `bool` (or `false` or `true` alone), `void`, `never`.
3. `null`. A type with a single part is printed `?X`. Otherwise `|null` is appended.

`mixed` prints as `mixed`. For example, `int|string` prints `string|int` and `iterable` prints
`Traversable|array`.

### Values against types

A value (folded, see constant expressions) fits a type when its kind is part of the type. An integer
also fits `float`. Objects are never folded values, so a class type only accepts `null` when nullable.
Messages name a value's kind `null`, `bool`, `int`, `float`, `string` or `array`. Property messages
instead say `true` or `false` for booleans.

## Functions

The return type and parameters compile after the function's attributes and before its body. Messages
that name the function use `name()` for functions, `Class::name()` for methods, and
`class@anonymous()` for methods of anonymous classes.

- **Return type.** A method named `__toString` (any case) without a return type is treated as
  returning `string`. A function returning by reference with a `void` return type is deprecated:
  "f(): Returning by reference from a void function is deprecated".
- **Parameters**, each on its own line, in this order:
  1. The earlier checks: auto-globals, redefinition, `$this`.
  2. A parameter after a variadic one: "Only the last parameter can be variadic".
  3. A variadic parameter with a default: "Variadic parameter cannot have a default value".
  4. The default, then the attributes.
  5. The type. A `null` default on a typed parameter that is not a promoted property makes the type
     nullable. If the type did not already allow `null`, this is deprecated: "f(): Implicitly marking
     parameter $a as nullable is deprecated, the explicit nullable type must be used instead".
  6. `void` and `never` are rejected: "void cannot be used as a parameter type".
  7. Any other folded default must fit: "Cannot use float as default value for parameter $a of type
     int".
  8. A parameter with a default before the last required parameter (neither defaulted nor variadic) is
     deprecated: "f(): Optional parameter $a declared before required parameter $c is implicitly
     treated as a required parameter". Parameters made nullable by step 5 are exempt.
- **Generators.** A function is a generator when its own body contains `yield` or `yield from`, not
  counting nested functions. Its return type must admit generators, which `object`, `mixed`, `iterable`
  and the classes `Traversable`, `Iterator` and `Generator` do. Otherwise it is a fatal error raised
  before the body compiles: "Generator return type must be a supertype of Generator, int given".
  `yield from` in a function that returns by reference is a fatal error: "Cannot use "yield from" inside
  a by-reference generator".
- **`return`** in a function with a return type, unless it is a generator, is checked after its value
  compiles. Messages say `method` instead of `function` inside a class, including in closures there,
  but not in functions declared in methods.
  - With `void`, a value is not allowed: "A void function must not return a value". If the value is the
    constant `null`, `(did you mean "return;" instead of "return null;"?)` is appended.
  - With `never`, any `return` is a fatal error: "A never-returning function must not return".
  - Any other type needs a value: "A function with return type must return a value". When the type
    allows `null`, `(did you mean "return null;" instead of "return;"?)` is appended.
  - An arrow function's body counts as a returned value, unless its return type is `never`.

## Properties and class constants

The type is compiled for each property or constant in a group.

- **Property types.** `void`, `never` and `callable` (also within a union) are rejected: "Property
  A::$a cannot have type ?callable".
- **Property defaults.** A folded default must fit.
  - `null` on a type that is not an intersection gives: "Default value for property of type int may
    not be null. Use the nullable type ?int to allow null default value".
  - Anything else gives: "Cannot use true as default value for property A::$a of type int".
- **Class constant types.** Other than `mixed`, a type with `callable`, `void` or `never` is rejected:
  "Class constant A::B cannot have type callable".
- **Class constant values.** A folded value must fit: "Cannot use bool as value for class constant
  A::B of type int".

Anonymous classes are named `class@anonymous` in these messages.

### Not yet implemented

- Promoted constructor parameters: their placement checks and the `callable` property type check.
