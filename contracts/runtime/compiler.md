# Compile-time checks

What the reference PHP (version pinned in `contracts/reference-manifest.toml`) reports while compiling a
file, before any of it runs. Implemented by `crates/php-compiler` (`check`); differential evidence in
`contracts/compiler/cases.jsonl`, recorded by `cargo xtask lab compiler`. Cases that do not match yet
are listed in `crates/php-compiler/tests/pending.txt`.

Provenance: `Zend/zend_compile.c` in the pinned PHP source.

Diagnostics are reported on the compiler's current line: the line of the statement or expression it
last started compiling. That line is not restored when a child finishes.

A node's line is usually its first token's line. A few nodes take their first child's line instead:
`break`, `continue` and `return` take the line of their operand, or of the `;` when there is none;
`goto` takes its label's line; a method takes the line of `function`, not of its modifiers.

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
- **Trait adaptations.** After the trait list, each adaptation of the `use` block is checked in
  order. `T::f insteadof U, V` resolves `T`, then `U` and `V`, as trait names. `[T::]f as …` is first
  checked for its modifier: `static` is "Cannot use "static" as method modifier in trait alias",
  `abstract` is the same with "abstract". Then `T` resolves as a trait name.
- **`catch`.** Each class is checked in order. A special name that is not fully qualified is a fatal
  error: "Bad class name in the catch statement". The others resolve as class names. After each class,
  a variable named `$this` is a fatal error: "Cannot re-assign $this". So it is reported once the
  first class has passed. Errors are reported on the line of the catch's first class.

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

### Declare directives

Source: `zend_compile_declare` and `zend_is_first_statement` in `Zend/zend_compile.c`.

`declare(...)` reports its diagnostics on the line of its first directive name. Each directive, in
order:

1. The value must be an integer, float or string literal, with no interpolation. A sign, a constant,
   a magic constant or an array is not one: "declare(NAME) value must be a literal", with the name as
   written.
2. Then by name, compared case-insensitively:
   - `ticks`: no further check.
   - `encoding`: every statement before it in the file must be a `declare`: "Encoding declaration
     pragma must be the very first statement in the script". The parser already warned that the
     directive is ignored.
   - `strict_types`: every statement before it in the file must be a `declare` or an empty statement
     (`;` or a closing tag): "strict_types declaration must be the very first statement in the
     script". A `declare` inside a block, a function or a braced namespace body never qualifies. Then
     "strict_types declaration must not use block mode" if it has a body, and "strict_types
     declaration must have 0 or 1 as its value" unless the literal is the integer 0 or 1.
   - Any other name: the warning "Unsupported declare 'NAME'", and compilation continues.

The body, if any, compiles after all directives.

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

## Methods

Messages name a method `Class::name` with the class's namespaced name (`class@anonymous` for an
anonymous class) and the method name as written. All checks report on the line of `function`.

### Declaration

Before a method's attributes and signature compile, these checks run in order:

- A private final method other than the constructor (compared case-insensitively) warns: "Private
  methods cannot be final as they are never overridden by other classes".
- An abstract method in a class that is neither declared abstract nor a trait is a fatal error:
  "Anonymous class method f() must not be abstract" in an anonymous class, "Enum method E::f() must
  not be abstract" or "Interface method I::f() must not be abstract" in those, and otherwise "Class A
  declares abstract method f() and must therefore be declared abstract".
- In an interface, a protected or private method is a fatal error ("Access type for interface method
  I::f() must be public"), and so is a final one ("Interface method I::f() must not be final").
- Interface methods are abstract. An abstract method may not be private outside a trait ("Abstract
  function A::f() cannot be declared private") and may not have a body ("Abstract function A::f()
  cannot contain body", with "Interface function" in an interface). Any other method needs a body:
  "Non-abstract method A::f() must contain body".
- A method whose lowercase name was already declared in the class is a fatal error: "Cannot redeclare
  A::f()".

### Magic methods

After the body compiles, a method whose lowercase name is one of the following is checked. For each,
the table gives the number of parameters it must declare (variadic ones are not counted), whether it
must be static, the types its first parameters must admit when typed, and the return type it must fit
when one is declared.

| Method | Parameters | Static | Parameter types | Return type |
|---|---|---|---|---|
| `__construct` | any | no | | none allowed |
| `__destruct` | 0 | no | | none allowed |
| `__clone` | 0 | no | | `void` |
| `__get` | 1 | no | `string` | |
| `__set` | 2 | no | `string` | `void` |
| `__unset` | 1 | no | `string` | `void` |
| `__isset` | 1 | no | `string` | `bool` |
| `__call` | 2 | no | `string`, `array` | |
| `__callStatic` | 2 | yes | `string`, `array` | |
| `__toString` | 0 | no | | `string` |
| `__debugInfo` | 0 | no | | `?array` |
| `__serialize` | 0 | no | | `array` |
| `__unserialize` | 1 | no | `array` | `void` |
| `__set_state` | 1 | yes | `array` | `object` |
| `__invoke` | any | no | | |
| `__sleep` | 0 | no | | `array` |
| `__wakeup` | 0 | no | | `void` |

The checks run in this order; each failure is fatal unless noted:

1. The parameter count: "Method A::__get() must take exactly 1 argument" ("cannot take arguments"
   for none, "must take exactly 2 arguments" for two). Then none of the counted parameters may be by
   reference: "Method A::__set() cannot take arguments by reference".
2. "Method A::__call() cannot be static" or "Method A::__callStatic() must be static".
3. For `__construct` and `__destruct`, any return type: "Method A::__construct() cannot declare a
   return type".
4. A protected or private method other than `__construct`, `__destruct` and `__clone` warns: "The
   magic method A::__get() must have public visibility". Compilation continues.
5. A typed parameter whose type admits none of the required kinds: "A::__get(): Parameter #1 ($a)
   must be of type string when declared". Class types admit no builtin kind.
6. A declared return type with kinds outside the allowed ones, or with classes or `static` unless
   `object` is allowed: "A::__isset(): Return type must be bool when declared". A type including
   `never` always fits.

## Properties, class constants and enum cases

Messages name a member `Class::$a` or `Class::A`. The class is named by its namespaced name, or
`class@anonymous` for an anonymous class. Member names are compared case-sensitively.

### Lines

- A property group reports on the line of its type, or on the line of its first property if it is
  untyped. The modifiers' line does not count.
- A constant group reports on the line of its first constant's name.
- An enum case reports on the line of its name.
- Promoted properties report on the constructor's line.

### Enum backing types

An enum's backing type is checked before any member. Only `int` or `string` are accepted
(case-insensitively). Anything else is fatal: "Enum backing type must be int or string, ?string
given". The type is printed as for declared types, so a class written `\Foo` prints as `Foo`.

### Property groups

These checks run once per group, before its properties:

1. In an enum: "Enum E cannot include properties".
2. Final together with private: "Property cannot be both final and private".
3. In an interface:
   - final is rejected: "Property in interface cannot be final";
   - protected or private gives "Property in interface cannot be protected or private";
   - abstract gives "Property in interface cannot be explicitly abstract. All interface members are
     implicitly abstract".

### Properties

A property without hooks is checked first:

- in an interface: "Interfaces may only include hooked properties";
- when abstract: "Only hooked properties may be declared abstract".

The remaining checks run in this order:

1. **Type.** `void`, `never` and `callable` (also within a union) are rejected: "Property A::$a
   cannot have type ?callable".
2. **Redeclaration.** A name already declared in the class gives "Cannot redeclare A::$a". Promoted
   properties count too.
3. **Default.** A folded default must fit the type.
   - `null` on a type that is not an intersection gives: "Default value for property of type int may
     not be null. Use the nullable type ?int to allow null default value".
   - Anything else gives: "Cannot use true as default value for property A::$a of type int".
4. **Readonly.** Every property of a readonly class is readonly. A readonly property:
   - must be typed ("Readonly property A::$a must have type");
   - may not have a default ("Readonly property A::$a cannot have default value");
   - may not be static ("Static property A::$a cannot be readonly").
5. **Asymmetric visibility.** With a set visibility, the property must be typed: "Property with
   asymmetric visibility A::$a must have type". Its get visibility must not be weaker than its set
   visibility: "Visibility of property A::$a must not be weaker than set visibility". Public is
   weakest and private is strongest.

The hooks and attributes compile after these checks.

### Promoted properties

A parameter with a visibility, set visibility, readonly or final modifier, or with hooks, promotes a
property. Its checks run in this order:

1. Outside a constructor: "Cannot declare promoted property outside a constructor".
2. In an abstract constructor, or any interface constructor: "Cannot declare promoted property in an
   abstract constructor".
3. A variadic parameter: "Cannot declare variadic promoted property".
4. "Cannot redeclare A::$a".
5. A `callable` type: "Property A::$a cannot have type callable".
6. Readonly without a type and without hooks: "Readonly property A::$a must have type".
7. The asymmetric visibility checks for properties.

### Class constants

For each constant in a group, the constant is declared first. Declaring checks, in order:

1. In an interface, a protected or private constant: "Access type for interface constant I::A must be
   public".
2. A constant named `class` (case-insensitively): "A class constant must not be called 'class'; it is
   reserved for class name fetching".
3. A name already declared as a constant or a case: "Cannot redefine class constant A::B".

The type and value checks follow:

- **Type.** Other than `mixed`, a type with `callable`, `void` or `never` is rejected: "Class constant
  A::B cannot have type callable".
- **Private final.** "Private constant A::B cannot be final as it is not visible to other classes".
- **Value.** A folded value must fit: "Cannot use bool as value for class constant A::B of type int".

### Enum cases

Checks run in this order:

1. Outside an enum: "Case can only be used in enums".
2. A backed enum's case without a value: "Case B of backed enum E must have a value". A non-backed
   enum's case with one: "Case B of non-backed enum E must not have a value".
3. The value compiles.
4. The case is declared like a constant.
5. The attributes compile.

### Not yet implemented

- The class-level checks that run when a class is linked. One example is the abstract-member count:
  "Class A contains 1 abstract method and must therefore be declared abstract or implement the
  remaining method (A::$a::get)".

## Property hooks

A property's hooks compile right after the property is declared, and before its attributes. This
covers declared and promoted properties alike. Hook names are compared case-insensitively, but
messages show them as written.

### The hook list

1. A readonly property, including any property of a readonly class: "Hooked properties cannot be
   readonly".
2. An empty list: "Property hook list must not be empty".

### Each hook

Each hook reports on the line of its name. Its checks run in this order:

1. A static property: "Cannot declare hooks for static property".
2. A final hook of a private property: "Property hook cannot be both final and private".
3. **Abstract hooks.** A hook is abstract when it is in an interface, or when the property is
   abstract and the hook has no body.
   - An abstract hook may not have a body: "Abstract property hook cannot have body".
   - The property may not be private: "Property hook cannot be both abstract and private".
   - The hook may not be final: "Property hook cannot be both abstract and final".
   - Any other hook needs a body: "Non-abstract property hook must have a body".
4. A name other than `get` or `set`: "Unknown hook "foo" for property A::$a, expected "get" or
   "set"".
5. **Get parameters.** A get hook may not have a parameter list, even an empty one: "get hook of
   property A::$a must not have a parameter list".
6. **Set parameters.** A set hook's parameter list, if written, must hold exactly one parameter: "set
   hook of property A::$a must accept exactly one parameters". That parameter:
   - may not be by reference: "Parameter $x of set hook A::$a must not be pass-by-reference";
   - may not be variadic ("must not be variadic");
   - may not have a default ("must not have a default value").

   It must be typed exactly when the property is typed. Otherwise: "Type of parameter $x of hook
   A::$a::set must be compatible with property type".
7. **Compilation.** The hook compiles as a method named `$a::get` (the hook name as written). Its
   attributes come first.
   - A get hook returns the property's type.
   - A set hook returns `void`, so `&set` gets the by-reference void deprecation. Without a
     parameter list it takes `$value` of the property's type.

   The property's type is compiled again each time it is used, so its warnings repeat. A
   short-bodied get hook returns its expression. The usual return and generator checks apply.

The remaining checks run after the body and report on the line where the hook's last token ends:

8. A second hook of the same kind: "Cannot redeclare property hook "get"".
9. **Set parameter type.** A declared parameter type must admit every value of the property's
   type. It fails with the message from step 6 when it is known to fall short:
   - when the property allows builtin kinds the parameter does not;
   - when one of the property's classes is not named in the parameter's type, and the parameter
     has neither other classes nor `object` or `callable`.

   `mixed` admits everything. A class counts as named when a class of the parameter has the same
   name (case-insensitively), and an intersection when one of its classes is. Every other case
   depends on how classes are related, so the reference decides it when classes are linked.

### Virtual properties

A property is virtual when it is in an interface, or when no hook body uses it. A body uses it when
it fetches `$this->name` with the exact name, outside the closures, arrow functions and classes
declared inside it. A short-bodied set hook always uses it. `${'this'}` counts as `$this`.

### Verification

When the class has no parent, these checks follow the hooks. With a parent they happen when the
class is linked. They report on the line where the last hook ends.

1. A virtual property with an explicit default: "Cannot specify default value for virtual hooked
   property A::$a". Promoted properties have no explicit default.
2. A by-reference get hook on a property that is not virtual and also has a set hook: "Get hook of
   backed property A::a with set hook may not return by reference".
3. An abstract property without an abstract hook: "Abstract property A::$a must specify at least
   one abstract hook".
4. A virtual property with a set visibility and only one hook: "Read-only virtual property A::$a
   must not specify asymmetric visibility" ("Write-only" when the get hook is missing).

### Parent hook calls

A static call is a call of the parent class's hook when all of these hold:

- its method is a literal `get` or `set` (any case);
- its class part is an unparenthesized `parent::$name`;
- the property name is literal;
- `parent` is written as a name in any form or as a string.

Otherwise it compiles as an ordinary static call. Its checks:

1. Outside any class: "Cannot use "parent" when no class scope is active".
2. With `(...)` arguments: "Cannot create Closure for parent property hook call".
3. Outside a hook, including closures and classes declared inside one: "Must not use
   parent::$a::get() outside a property hook".
4. A different property: "Must not use parent::$b::get() in a different property ($a)".
5. A different hook kind: "Must not use parent::$a::set() in a different property hook (get)".

Whether the class has a parent is not checked here.

### Not yet implemented

- `#[\Override]` on a hook. The check runs when the class is linked.
- Set parameter types that depend on class relationships.

## Built-in attributes

Source: `Zend/zend_attributes.c` and `zend_compile_attributes` in `Zend/zend_compile.c`.

Each attribute name resolves like a class name, so `#[Override]` inside a namespace without an import
names `N\Override`, not the built-in. Lookup is case-insensitive, and messages print the name as
written in the source. The built-ins, and where each may appear:

| Attribute | Targets | Extra check |
|---|---|---|
| `Attribute` | class | yes |
| `ReturnTypeWillChange` | method | |
| `AllowDynamicProperties` | class | yes |
| `SensitiveParameter` | parameter | |
| `Override` | method, property | |
| `Deprecated` | class, function, method, class constant, constant | yes |
| `NoDiscard` | function, method | yes |
| `DelayedTargetValidation` | everything | |

Targets: closures and arrow functions count as functions inside a function or at the top level, and as
methods inside a class. Property hooks count as methods. Enum cases count as class constants. Top-level
`const` is a constant.

None of the built-ins may be repeated. User attributes may be.

### Order

1. All arguments of every attribute on the declaration are checked first (see "Attributes" under
   constant expressions).
2. Then each attribute, in source order:
   1. A built-in whose targets do not include this one: "Attribute "X" cannot target T (allowed
      targets: A, B)". The allowed targets are listed in the order of the table header above (class,
      function, method, property, class constant, parameter, constant).
   2. A built-in seen before on the same declaration: "Attribute "X" must not be repeated".
   3. The extra check.

If `DelayedTargetValidation` appears anywhere on the declaration, the target errors and the extra
checks are skipped. The repetition error and the `NoDiscard` return check below still apply.

On a promoted constructor parameter, a built-in that targets properties but not parameters is skipped
while checking the parameter, and the other way round.

### Extra checks

- `Attribute`: "Cannot apply #[\Attribute] to trait T", then interface, enum, and "abstract class A".
- `AllowDynamicProperties`: "Cannot apply #[\AllowDynamicProperties] to trait T", then interface,
  "readonly class A", then enum. Anonymous classes print as `class@anonymous`.
- `Deprecated` on a class: "Cannot apply #[\Deprecated] to interface I", enum or "class A". Only
  traits accept it.
- `NoDiscard` on a property hook: "#[\NoDiscard] is not supported for property hooks".

### NoDiscard and return types

After a function's `use` checks and before its body, a `NoDiscard` function or method returning `void`
or `never` is a fatal error: "A void function does not return a value, but #[\NoDiscard] requires a
return value". The message reads "never returning" for `never`, and "method" for methods and closures
inside classes.

Magic methods that return nothing (`__construct`, `__destruct`, `__clone`, and `__set`, `__unset`,
`__unserialize`, `__wakeup` and the other `void` ones) reject it: "Method A::__construct cannot be
#[\NoDiscard]". `__get`, `__toString` and the other magic methods that return a value accept it.

### Not yet implemented

- `#[\Override]` on methods and properties. The reference checks it when the class is linked: "A::$a has
  #[\Override] attribute, but no matching parent property exists".
- User attribute classes and their own `Attribute::TARGET_*` flags. The reference checks them at run
  time, when the attribute is instantiated.

## Jumps

Source: `zend_compile_label`, `zend_compile_goto` and `zend_resolve_goto_label` in `Zend/zend_compile.c`;
`pass_two` and `zend_check_finally_breakout` in `Zend/zend_opcode.c`.

Each function, method, closure and property hook has its own labels; the file's top level is one more,
shared by all of its namespace blocks. Label names are case-sensitive.

While compiling:

- A label defined twice in the same function: "Label 'a' already defined", on the second label's line.

After the function has compiled, and after the checks that follow its body (the magic-method checks
of a method, the redeclaration check of a top-level function), its jumps are checked in source order.
The top level's jumps are checked after the whole file has compiled, so any other compile error in
the file wins. Each error is reported on the line of the jump.

- `goto a` with no label `a` in the same function: "'goto' to undefined label 'a'".
- A label inside a loop or `switch` that does not also enclose the `goto`: "'goto' into loop or switch
  statement is disallowed". `try`, `catch`, `if` and `declare` blocks do not count.
- A `goto` that crosses the edge of a `finally` block. The `try` statements with a `finally` block are
  considered in the order of their `try` keywords. The first one whose `finally` block holds exactly
  one of the two ends decides the message: "jump into a finally block is disallowed" when it holds
  the label, "jump out of a finally block is disallowed" when it holds the `goto`.
- A `break` or `continue` inside a `finally` block whose target loop or `switch` lies outside that
  block: "jump out of a finally block is disallowed". The compile-time `break` and `continue` checks
  and warnings come first.

Jumps out of `try` and `catch` blocks, and into `catch` blocks, are allowed.

## Calls and write contexts

Source: `zend_compile_method_call`, `zend_compile_static_call`, `zend_compile_new`,
`zend_compile_call_common`, `zend_separate_if_call_and_write`, `zend_compile_assign_ref` and
`zend_compile_assign_coalesce` in `Zend/zend_compile.c`.

### Closure creation

A call written `f(...)` creates a closure.

- `new X(...)`, including anonymous classes and `new (expr)(...)`: "Cannot create Closure for new
  expression". It is reported after the class (or the anonymous class body) compiles. The `new`
  expression's line is its class's line, so `new` and `A` on different lines report `A`'s line.
- A method call whose chain a `?->` can skip: "Cannot combine nullsafe operator with Closure
  creation". The chain is the method call itself and, through its object, the properties, elements
  and method calls it is built on. A `?->` applied directly to `$this` does not count, because it
  never skips. A `?->` inside a function argument, a static class reference or a parenthesized call
  target belongs to a different chain. It is reported on the line of the method name, after the name
  compiles.

### Method names

A method name given as an expression (`$a->{expr}()`, `A::{expr}()`) compiles after the object or
class. If it folds to a constant that is not a string (an integer, float, boolean, null or array):
"Method name must be a string". Runtime constants such as `PHP_EOL` do not fold.

### Writing through a created closure

A closure created by `f(...)` is a temporary value. Using it as the container of an element or
property that is written, incremented, unset, assigned by reference, or used as a `foreach` or
`list()` target is a fatal error: "Cannot use result of built-in function in write context". The
check follows right after the container compiles. `$x = &f(...)` fails the same way after both
sides compile. Reads, `isset()` and function arguments are not writes.

### Coalescing assignment

`target ??= value` first checks that the target is writable, as other assignments do. Then it
compiles the target for an `isset()`-style read, so `[]` without an index fails as "Cannot use [] for
reading". Then it compiles the value. Finally it checks the write: a container that is a temporary
expression fails as "Cannot use temporary expression in write context", and a created closure fails
as above.
