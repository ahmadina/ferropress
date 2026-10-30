# Ferropress examples

PHP scripts that run on the Rust VM (no PHP binary). From the repository root:

```bash
cargo xtask example
cargo xtask example hello.php
cargo xtask example example/shop.php
cargo run --release -p xtask -- example-bench
```

`example-bench` times the loop scripts in `example/bench/` on the Rust VM and on
`php` (CLI opcache off). Use a release build; a debug `cargo xtask` time is not
comparable. `pipeline.php` and `core-node.php` are Rust-host demos and are not
part of the PHP comparison.

These stay inside what the engine compiles today: functions, closures, classes and
`extends`, arrays, `foreach`, `include`/`require`, exceptions, JSON, PCRE, and the
Rust-owned `CoreNode` class.
