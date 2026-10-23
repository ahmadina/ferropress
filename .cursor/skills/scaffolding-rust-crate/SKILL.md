---
name: scaffolding-rust-crate
description: Creates the Rust workspace root or a new member crate with the project's canonical Cargo settings, lint block, error type, docs and test layout. Use when initializing the Cargo workspace, adding a crate under crates/ or apps/, or when the user asks to create a new module crate.
---

# Scaffolding a Rust Crate

## Before creating anything
1. Confirm the crate exists in master plan §3 or an approved ADR. New crates outside that list need an ADR first
   (`rust-architecture-boundaries.mdc`).
2. Decide its layer (foundation / domain / orchestration / PHP runtime / PHP bindings / infrastructure /
   platform / binary / test support) and its prefix (`wp-`, `php-`, `infra-`, `platform-`); list allowed
   dependencies from master plan §2.6.
3. Confirm every external dependency is already approved in `[workspace.dependencies]`; otherwise ask.

## Workspace root (first time only)
Copy the root `Cargo.toml`, `rust-toolchain.toml` and `rustfmt.toml` from [templates.md](templates.md).
Pin `channel` to the exact output of `rustc --version`.

## New member crate
```
Task Progress:
- [ ] 1. Create crates/<name>/ (or apps/<name>/) with Cargo.toml from templates.md
- [ ] 2. Add src/lib.rs with crate docs stating owned behavior and contract IDs
- [ ] 3. Add src/error.rs with a crate-specific #[non_exhaustive] error type
- [ ] 4. Add tests/ directory with at least one behavior test sourced from an oracle/contract
- [ ] 5. Register path dependency in root [workspace.dependencies] if other crates will use it
- [ ] 6. Run the quality gate and fix everything
```

Rules while scaffolding:
- Never add a crate-local `[lints]` table other than `workspace = true`.
- Never add placeholder functions or `todo!()` to "fill in later" — scaffold only what is implemented.
- `apps/*` crates are composition roots: they wire adapters into domain crates and contain no business logic.
- Add `test-util` feature for fakes; never expose fakes in default features.

## Verify
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
```
Only report success after these commands pass; include their output summary.
