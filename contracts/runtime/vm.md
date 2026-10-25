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

`$object instanceof Name` is true when the object's class is `Name` (case-insensitive), when that
class implements `Name` or an interface that extends it, or when a built-in throwable matches the
hierarchy (`Throwable`, `Error`, `ArithmeticError`). A non-object is never an instance; a name that
is not a declared class is false.

An `interface` compiles. `class C implements I` records the interface (and `interface B extends A`
records the parent). `new I` throws `Error` with `Cannot instantiate interface I`. Interface methods
without a body are not callable.

A `trait` compiles its methods. `class C { use T; }` and `trait T { use U; }` copy methods onto the
user, with a method declared on the class winning. `new T` throws `Error` with
`Cannot instantiate trait T`. Adaptations (`insteadof`, `as`) are not compiled yet.

A unit `enum` compiles its cases. `E::A` is a singleton object of class `E` with a public `name`
property. `E::A === E::A` is true. `new E` throws `Error` with `Cannot instantiate enum E`. An
unknown case throws `Error` with `Undefined constant E::A`. Backed enums are not compiled yet.

Anonymous, abstract and readonly classes, class inheritance (`extends`), static, private and
protected members, typed and hooked properties, class constants and computed names are not compiled
yet.

## Exceptions

`throw` raises the value as an error. Only objects of the built-in throwable classes may be thrown
(`Exception`, `Error`, `TypeError`, `ArgumentCountError`, `ArithmeticError`, `DivisionByZeroError`,
`FiberError`).
Any other object throws `Error` with `Cannot throw objects that do not implement Throwable`. A
non-object throws `Error` with `Can only throw objects`. `new Exception("message")` and `new Error("message")` store
the first argument as the public `message` property.

`try { … } catch (Class $e) { … }` runs the first arm whose class matches the thrown class, including
the built-in hierarchy (`Throwable` matches every throwable, `Error` matches the `Error` family,
`ArithmeticError` matches `DivisionByZeroError`). The optional catch variable receives the thrown
object. A throw inside a called function unwinds to a matching `catch` in a caller.

`finally` always runs when leaving the `try` (and its `catch` arms): after a normal completion, after
a caught throw, before an uncaught throw is displayed, and before a `return` from the `try` or a
`catch`. A throw or return from the `finally` body itself replaces the pending leave.

## Generators

A function that contains `yield` is a generator. Calling it returns a `Generator` object without
running the body. `yield`, `yield value` and `yield key => value` suspend the body; `current()`,
`key()` and `valid()` start it if needed and report the current yield. `next()` and `send($v)`
resume; `send` makes `$v` the value of the `yield` expression. `getReturn()` is the function's
`return` after the generator has finished. `rewind()` starts an unstarted generator and throws
`Exception` if it has already run. `yield from` is not compiled yet.

## Fibers

`new Fiber($callable)` stores a `Closure` or a string naming a function. A non-callable throws
`TypeError`. `$f->start(...$args)` runs the callback; `Fiber::suspend($v)` leaves the fiber and
makes `$v` the value of `start` (or the later `resume`). `$f->resume($v)` continues and makes `$v`
the value of the `suspend` expression. `getReturn()` is the callback's `return` after the fiber has
finished. `isStarted`, `isSuspended`, `isRunning` and `isTerminated` report the state. `throw($e)`
raises `$e` at the suspend point. `FiberError` extends `Error` and cannot be constructed (`The
"FiberError" class is reserved for internal use and cannot be manually instantiated`). Suspend
outside a fiber, resume of a fiber that is not suspended, a second `start`, and `getReturn` before
the fiber has returned each throw `FiberError` (`Cannot get fiber return value: The fiber has not
returned`). A non-callable constructor argument throws `TypeError` (`no array or string given`
unless the value is a string naming a missing function).

## Arrays

`[1, "a" => 2]` and `array(1, "a" => 2)` compile to an array value. Items are appended in order;
a key is converted as an array offset (`null` is the empty string, with a deprecation). `list()`,
holes, unpacking and items by reference are not compiled yet. Echoing an array warns
`Array to string conversion` and prints `Array`.

## Autoloading

`spl_autoload_register($callback)` stores a `Closure` or a string naming a function and returns
`true`. A missing class on `new Name` or `class_exists($name)` (the second argument defaults to
true) runs the registered callbacks in order, passing the name as written, until the class is
declared. `$object instanceof Name` does not load. A class statement inside a function or closure is registered
when that statement runs. A loader that `include`s a file with a top-level class makes that class
visible to the caller. A class already being loaded is not asked for again.

## Built-in functions

These names are bound by the VM: `strlen`, `strtolower`, `strtoupper`, `count`, `gettype`,
`is_null`, `is_bool`, `is_int` / `is_integer`, `is_float` / `is_double` / `is_real`, `is_string`,
`is_array`, `is_object`, `is_numeric`, `function_exists`, `class_exists`, `spl_autoload_register`,
`abs`, `min`, `max`, `floor`, `ceil`, `round`, `intval`, `floatval` / `doubleval`, `strval`,
`intdiv`, `sqrt`, `implode` / `join`, `explode`. Too few arguments throw
`ArgumentCountError` from the function (`name() expects exactly N argument(s), M given`). `strlen`
and the case mappers take a string (other scalars are converted; an array or object throws
`TypeError`). `count` of a non-array throws `TypeError`. `function_exists` sees user functions and
these built-ins;
`class_exists` sees declared and built-in classes. Other built-in families are not bound yet.

## Include

`include`, `include_once`, `require` and `require_once` take a string path. A relative path is
resolved against the directory of the file that is running. A missing path: `include` raises two
warnings (`Failed to open stream` and `Failed opening … for inclusion`) and yields `false`;
`require` raises the first warning and throws `Error` with `Failed opening required '<path>'
(include_path='…')`. A file already recorded by `include` or `require` is skipped by the `_once`
forms, which then yield `true`. A loaded file is compiled and run; its `return` value (or `1` when
it does not return) is the value of the include expression. `eval` is not compiled yet.

Operator errors (`DivisionByZeroError`, `ArithmeticError`, `TypeError`) and the warnings and deprecations
of conversions follow `contracts/runtime/operators.md`.
