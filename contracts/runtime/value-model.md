# Value model

How the reference PHP (version pinned in `contracts/reference-manifest.toml`) stores values: ordered
arrays, copy-on-write sharing, references and object handles. Implemented by `crates/php-value`;
differential evidence in `contracts/value/`, recorded by `cargo xtask lab value` from hand-written and
seeded random sequences of assignments, reference bindings and unsets, each run in a fresh process.

Provenance: `Zend/zend_hash.c` (ordered tables, next free index, numeric string keys),
`Zend/zend_variables.c` and `Zend/zend_operators.h` (separation), `Zend/zend_objects_API.c` (object
handle store), `Zend/zend_execute.c` (write fetches and their diagnostics), `Zend/zend_compile.c`
(evaluation order) and `ext/reflection/php_reflection.c` (`ReflectionReference`) in the pinned PHP source.

## Arrays

An array maps keys to values and remembers insertion order.

- **Keys** are integers or byte strings. A string that is the canonical decimal form of a 64-bit integer
  becomes that integer: `"0"`, `"-1"`, `"9223372036854775807"`, `"-9223372036854775808"`. Anything else
  stays a string: `"-0"`, `"00"`, `"01"`, `" 1"`, `"1 "`, `"+1"`, `"1e3"`, `"1.0"`, `""`,
  `"9223372036854775808"`. Converting float, bool and null offsets is the engine's job, not the model's.
- **Order.** Overwriting a key keeps its position; removing a key and inserting it again puts it last.
- **Next index** (used by `$a[] = …`). It starts unset, so the first append uses 0. Inserting an integer
  key `k` at or above it (or while it is unset) moves it to `k + 1`, negative keys included: after
  `[-5 => …]` an append uses -4, after `[-5 => …, -10 => …]` still -4, after `[3 => …, -7 => …]` 4.
  Removing elements never lowers it, and copies keep it. Once `PHP_INT_MAX` has been used as a key,
  appending throws `Error` with "Cannot add element to the array as the next element is already occupied"
  and leaves the array unchanged.

## Copies and separation

Assigning an array copies it by value, but the copies share storage until one of them is written; the
written one then gets its own storage ("separation"). Reading, and removing a key that is not there,
never separates.

When storage is separated, each element that is bound to a reference is treated by how many bindings
the reference has:

- bound somewhere else too: the copy binds the same reference, so writes through either array are seen
  by both (`$r = &$a[0]; $b = $a; $b[0] = 5;` changes `$a[0]` and `$r` as well);
- bound only in this array: the copy gets a plain value, unless the reference holds this very array
  (`$a[] = &$a; $b = $a; unset($a);`), in which case it stays shared.

The same rule decides whether PHP reports an element as a reference at all (`ReflectionReference`
returns `null` otherwise), so a reference with a single binding is not observable except in that
self-holding case.

## References

A reference is one value cell bound to any number of variables, elements and properties. Assigning to a
binding writes the shared cell. `$x = &$a['k']` creates the element as `null` when missing, and
`$x = &$a[]` appends a `null` element. Binding a variable or element to a reference replaces its previous
value without writing through the old binding.

Order of evaluation, as compiled by the reference PHP:

- `$t[…] = <expr>`: the right-hand side is evaluated before the target's dimensions are fetched, so
  `$e[0] = $e` stores the old `$e`.
- `$t[…] = &$s[…]`: the source's dimensions are fetched (and created) first, then the target's
  (`$a['x'] = &$a['y']` creates `y` before `x`). A plain source variable, with no dimensions, is only
  created once the target fetch succeeded.

## Objects

Objects are handles: copying a variable copies the handle, never the object. Each object has an id
(`spl_object_id`):

- ids start at 1 in a new process (scripts usually hold some objects already);
- a freed id is reused before a new one is issued, the most recently freed first;
- an object's properties are released in order before its own id is freed, and so are an array's
  elements when the array is freed.

Thrown errors are objects too and take ids like any other.

Dynamic property names stay strings even when numeric (`$o->{'5'}` keeps the name `"5"`), and
properties keep insertion order like array keys.

## Write fetches and diagnostics

Writing through a path (`$v[k]…[k] = …`, `$v[k]->p = …`) creates what is missing: an undefined
variable, `null` or a missing element becomes an empty array. On other containers:

| Container | Write `[k]` / `[]` | Read `[k]` | Unset `[k]` |
|---|---|---|---|
| `false` | becomes an array, with the deprecation "Automatic conversion of false to array is deprecated" | warning "Trying to access array offset on false", gives `null` | becomes an empty array, with the same deprecation |
| `true`, int, float | `Error` "Cannot use a scalar value as an array" | warning "Trying to access array offset on <type>", gives `null` | `Error` "Cannot unset offset in a non-array variable" |
| `null` or undefined | becomes an array | warning "Trying to access array offset on null", gives `null` | nothing happens |
| object without array access | `Error` "Cannot use object of type <class> as array" | same `Error` | same `Error` |

Other diagnostics:

- a missing element on read: warning `Undefined array key 5` or `Undefined array key "k"`, gives `null`;
- an undefined variable on read, or as the base of a dimension unset: warning `Undefined variable $name`;
  `unset($v->p)` on an undefined variable is silent;
- reading `->p` from a non-object: warning `Attempt to read property "p" on <type>`; a missing property:
  warning `Undefined property: <class>::$p`;
- assigning `->p` on a non-object: `Error` `Attempt to assign property "p" on <type>`. An undefined
  variable stays undefined, but dimensions fetched before the property remain created
  (`$z[0]->x = 1` leaves `$z = [0 => null]`).

`<type>` is `null`, `true`, `false`, `int`, `float`, `string` or `array`.

## Known differences

- **Arrays containing themselves without a reference.** When `$b` and `$c` are bound to the same
  reference, `$c[] = $b` makes PHP store the array inside itself by value (`var_dump` prints
  `*RECURSION*`). Copy-on-write values cannot contain themselves, so the model stores a copy of the old
  array instead. Recorded as the only known difference in the differential test.
- **Cycle collection.** Values that reach themselves through references or objects are freed by the
  engine's cycle collector, which the model does not include; until then they keep their object ids.
  The reference PHP only collects cycles once enough candidates accumulate, so short scenarios agree.

## Callables

A callable is a live engine value (a function name, a `Closure`, or later a callable object). The
native core does not copy it: it holds the same value and asks the engine to run it through the
invoke port. Arguments that are references stay references; writes the callable makes through them
are visible to the caller. Nested core → PHP → core calls share one request and one call stack. The
engine, not this model, decides what is callable.

## Owned by the engine

Float/bool/null offsets, string offsets, `ArrayAccess`, destructors, the cycle collector, and
visibility of declared properties are outside the value model. Conversions and operators are described
in `operators.md`.
