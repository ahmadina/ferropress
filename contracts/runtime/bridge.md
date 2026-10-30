# Bridge

How the native core talks to plugin PHP on one request: live values, not copies. Proven by
`crates/php-abi-bridge` against the VM (ADR-0003). The full differential suite against the
reference (recursive dispatch, callback removal, inheritance) is still open.

## Invoke

The core holds a PHP callable as the same value the engine holds — a function name or a `Closure`
object — and asks the engine to run it. Arguments that are references stay references; a write the
callable makes through one is visible to the caller. An object that the core passes in and PHP
returns is the same object (`spl_object_id` is unchanged). Nested core → PHP → core calls share
one request and one call stack; that path is not bound yet.

An uncaught throw from the callable is the thrown object, not a newly invented Rust error and not
the CLI "Fatal error: Uncaught" banner.

## Inheritance

PHP classes extending Rust-owned classes, calling the parent implementation and reflecting on them
are not bound yet.
