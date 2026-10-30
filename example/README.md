# Ferropress examples

PHP scripts that run on the Rust VM (no PHP binary). From the repository root:

```bash
cargo xtask example
cargo xtask example hello.php
cargo xtask example example/shop.php
```

These stay inside what the engine compiles today: functions, closures, classes and
`extends`, arrays, `foreach`, `include`/`require`, exceptions, JSON, PCRE, and the
Rust-owned `CoreNode` class.
