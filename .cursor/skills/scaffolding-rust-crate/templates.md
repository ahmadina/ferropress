# Crate Templates

## Root `Cargo.toml` (workspace) — canonical settings and lint block

```toml
[workspace]
resolver = "3"
members = ["crates/*", "apps/*", "xtask"]
exclude = ["fuzz"]

[workspace.package]
edition = "2024"
rust-version = "1.94"
publish = false
version = "0.0.0"

[workspace.dependencies]
# Internal crates (path deps) and approved external crates are declared here only.
# wp-hooks = { path = "crates/wp-hooks" }

[workspace.lints.rust]
unsafe_code = "deny"
unsafe_op_in_unsafe_fn = "deny"
missing_docs = "warn"
missing_debug_implementations = "warn"
unreachable_pub = "warn"
unused_lifetimes = "warn"
redundant_lifetimes = "warn"
trivial_numeric_casts = "warn"
ambiguous_negative_literals = "warn"
rust_2018_idioms = { level = "warn", priority = -1 }

[workspace.lints.clippy]
correctness = { level = "deny", priority = -1 }
suspicious = { level = "deny", priority = -1 }
complexity = { level = "warn", priority = -1 }
perf = { level = "warn", priority = -1 }
style = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
cargo = { level = "warn", priority = -1 }
# restriction lints: no stubs, no hidden panics, no stray output
unwrap_used = "deny"
expect_used = "warn"
panic = "deny"
todo = "deny"
unimplemented = "deny"
dbg_macro = "deny"
print_stdout = "deny"
print_stderr = "deny"
exit = "deny"
mem_forget = "deny"
allow_attributes_without_reason = "deny"
undocumented_unsafe_blocks = "deny"
clone_on_ref_ptr = "warn"
map_err_ignore = "warn"
unused_result_ok = "warn"
if_then_some_else_none = "warn"
empty_structs_with_brackets = "warn"
semicolon_outside_block = "warn"
# noisy opt-outs
module_name_repetitions = "allow"
must_use_candidate = "allow"
multiple_crate_versions = "allow"
cargo_common_metadata = "allow"   # license/repository metadata pending LEGAL-001

[workspace.lints.rustdoc]
broken_intra_doc_links = "deny"
private_intra_doc_links = "warn"
```

Binaries that legitimately print or exit (CLI output in `apps/site-operator`, `xtask`) use item-scoped
`#[expect(clippy::print_stdout, reason = "…")]`, never a crate-wide override.

## `rust-toolchain.toml`

```toml
[toolchain]
channel = "1.94.1"          # exact version from `rustc --version`; bump via chore(toolchain) commit
components = ["rustfmt", "clippy"]
profile = "minimal"
```

## `rustfmt.toml`

```toml
edition = "2024"
newline_style = "Unix"
use_field_init_shorthand = true
use_try_shorthand = true
```

## Member crate `crates/<name>/Cargo.toml`

```toml
[package]
name = "<name>"
description = "<one line: what this crate owns>"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
publish.workspace = true

[features]
test-util = []

[dependencies]
thiserror = { workspace = true }

[dev-dependencies]
proptest = { workspace = true }

[lints]
workspace = true
```

## `crates/<name>/src/lib.rs`

```rust
//! <One-line summary of what this crate owns.>
//!
//! <What WordPress/PHP behavior it implements, the contract IDs it covers, and what it does NOT own.>

mod error;

pub use error::<Name>Error;
```

## `crates/<name>/src/error.rs`

```rust
//! Errors returned by `<name>`.

/// Failure to <operation> in `<name>`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum <Name>Error {
    /// <When this occurs.>
    #[error("<lowercase message without trailing period>")]
    Variant,
}
```

## Integration test `crates/<name>/tests/<behavior>.rs`

```rust
//! Integration tests for <behavior>; expected values come from <oracle/contract reference>.

#[test]
fn <behavior_under_condition>() {
    // arrange / act / assert
}
```
