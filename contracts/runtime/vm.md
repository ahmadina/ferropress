# VM core

What the reference PHP CLI (version pinned in `contracts/reference-manifest.toml`) prints while running a
script, beyond what the script echoes itself, with `display_errors=1`, `html_errors=0` and
`error_reporting=-1`. Implemented by `crates/php-vm` (ADR-0008); differential evidence in
`contracts/vm/cases.jsonl`, recorded by `cargo xtask lab vm`.

Provenance: `main/main.c` (error display), `Zend/zend_exceptions.c` (uncaught errors and stack traces),
`Zend/zend_smart_str.c` (trace arguments), `Zend/zend_execute.c` (argument count errors) in the pinned
PHP source.

## Displayed diagnostics

A warning or deprecation raised while running is printed, at the moment it is raised, as a newline, the
label (`Warning`, `Deprecated`), a colon and a space, the message, ` in `, the file path, ` on line `,
the line of the instruction running, and a newline.

Reading a local variable that was never assigned raises the warning `Undefined variable $name` and
gives `null`. It is raised by the instruction that reads it: passing the variable to a parameter taken
by value raises it at the call; passing it by reference creates the variable as `null` silently.

## Uncaught errors

An error thrown and not caught ends the script with exit status 255 after printing:

```
<newline>Fatal error: Uncaught <class>: <message> in <file>:<line>
Stack trace:
#0 <file>(<line of the call>): <function>(<arguments>)
…
#<n> {main}
  thrown in <file> on line <line>
```

The trace lists the active calls innermost first, each with the line it was made on and the function's
name as declared (not as the call wrote it), and ends with `{main}`. The arguments are separated by
`, `:

- the current value of each declared parameter that received an argument (a parameter changed by the
  function shows its new value), then every argument passed beyond the declared parameters, including
  those collected by a variadic parameter;
- `NULL`, `true`, `false`, integers as written, floats in `precision` (14) digits with `.0` added
  when the result has no `.` and the float is finite, `Array`, and `Object(<class>)`;
- strings in single quotes, cut after 15 bytes with `...` appended, with bytes below 32, the backslash
  and bytes above 126 escaped: `\n`, `\r`, `\t`, `\f`, `\v`, `\\`, `\e` for the escape byte, otherwise
  `\x` and two uppercase hex digits.

## Calls

- Calling an undefined function throws `Error` with `Call to undefined function <name>()`, the name as
  the call wrote it (fully qualified, without the leading backslash). The arguments are not evaluated.
- A computed callee (`$f()`, `$name()`) is evaluated first. A string looks up a function by that name
  (case-insensitive; a leading backslash is dropped). A `Closure` object is invoked. Any other value
  throws `Error` with `Value of type <type> is not callable` (`null`, `bool`, `int`, `float`, `string`,
  `array`, or the object's class name).
- Passing fewer arguments than the function declares parameters (not counting a variadic one) throws
  `ArgumentCountError` from inside the called function, so the trace starts with that call: `Too few
  arguments to function <name>(), <passed> passed in <file> on line <call line> and exactly <declared>
  expected`. Its line is the line of the first parameter that received no argument.
- Passing more arguments than declared is allowed; the extra ones appear in stack traces.

## Closures

A `function (…) use (…) { … }` or `fn (…) => expr` expression evaluates to an object of class `Closure`.
Calling it runs the compiled body. `use ($a)` copies `$a` when the closure is created (an undefined
variable warns and copies `null`); `use (&$a)` binds the same reference, creating `$a` as `null` when
it was undefined. An arrow function captures every free variable of its body by value, in the order
they first appear, excluding its parameters. Echoing a `Closure` throws `Error` with `Object of class
Closure could not be converted to string`.

The closure's name in stack traces is `{closure:PARENT:LINE}`, where `PARENT` is the file path at the
top level, `f()` inside a function, or the enclosing closure's name.

## Classes

An unconditional `class` with public instance properties (literal defaults) and public instance
methods compiles. `new Name` and `new Name(args)` create an object of that class. Property defaults
are copied onto each instance. `$object->name` reads a property (`Undefined property: Class::$name`
and `null` when missing; a warning `Attempt to read property "name" on <type>` and `null` when the
receiver is not an object). `$object->name = value` writes it, or throws `Error` with
`Attempt to assign property "name" on <type>` (`true` and `false` named as such). `$object->name(args)`
calls the method; `$this` is the instance. Class and method names are case-insensitive.

`__construct` runs when declared. A class with no constructor still accepts `new Name`; extra
arguments are evaluated and then discarded. Extra arguments to a declared `__construct` follow
the same argument-count rules as a function. An undefined class throws `Error` with
`Class "Name" not found`. An undefined method throws `Error` with
`Call to undefined method Class::name()`. A method call on a non-object throws `Error` with
`Call to a member function name() on <type>`.

Interfaces, traits, enums, anonymous, abstract and readonly classes, inheritance, static, private
and protected members, typed and hooked properties, class constants and computed names are not
compiled yet.

## Exceptions

`throw` raises the value as an error. Only objects of the built-in throwable classes may be thrown
(`Exception`, `Error`, `TypeError`, `ArgumentCountError`, `ArithmeticError`, `DivisionByZeroError`).
Any other object throws `Error` with `Cannot throw objects that do not implement Throwable`. A
non-object throws `Error` with `Can only throw objects`. `new Exception("message")` and `new Error("message")` store
the first argument as the public `message` property.

`try { … } catch (Class $e) { … }` runs the first arm whose class matches the thrown class, including
the built-in hierarchy (`Throwable` matches every throwable, `Error` matches the `Error` family,
`ArithmeticError` matches `DivisionByZeroError`). The optional catch variable receives the thrown
object. A throw inside a called function unwinds to a matching `catch` in a caller. `finally` is not
compiled yet.

Operator errors (`DivisionByZeroError`, `ArithmeticError`, `TypeError`) and the warnings and deprecations
of conversions follow `contracts/runtime/operators.md`.
