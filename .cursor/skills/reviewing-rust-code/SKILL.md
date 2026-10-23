---
name: reviewing-rust-code
description: Reviews Rust changes in this repository against SOLID-in-Rust, architecture boundaries, API guidelines, error/panic policy, unsafe policy, concurrency rules, testing evidence and WordPress compatibility mandates. Use when reviewing a diff, PR, branch or file, or when the user asks whether code follows SOLID or best practices.
---

# Reviewing Rust Code

## Process
1. Get the diff (`git diff`, `git diff main...HEAD`, or the named files).
2. Run the quality gate first; any failure is a 🔴 finding:
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```
3. Walk the checklist below. Cite `file:line` for every finding.
4. Report findings grouped by severity; do not rewrite code unless asked.

## Checklist

**Compatibility mandate (AGENTS.md)**
- [ ] No stubs, placeholder returns, silent fallbacks, `todo!`/`unimplemented!`
- [ ] Behavior traced to a contract ID / upstream source; not invented
- [ ] Hook order, SQL side effects, serialization bytes, globals preserved
- [ ] No path to stock PHP, Zend, libphp, or PHP-FPM

**SOLID (`rust-solid.mdc`)**
- [ ] S: module/crate has one reason to change; pure logic separated from I/O
- [ ] O: open sets via traits, closed sets via exhaustive enums; no string/`Any` type dispatch
- [ ] L: every trait impl honors the documented contract; contract test suite covers it
- [ ] I: traits narrow; functions take the narrowest bound
- [ ] D: domain code depends on ports; clock/RNG/fs/net/DB/VM injected; no global mutable state
- [ ] No speculative trait with a single impl and no test fake

**Boundaries (`rust-architecture-boundaries.mdc`)**
- [ ] Dependencies follow the §2.6 layer table; no cycles; binaries are the only composition roots
- [ ] Domain crates do not import the VM, bridge, `infra-*` or `platform-*` crates
- [ ] VM types do not leak outside `php-vm-adapter`/`php-abi-bridge`
- [ ] `test-harness` is only a dev-dependency; no grab-bag crate or module names
- [ ] No duplicate owner for the same WordPress behavior

**API (`rust-api-design.mdc`)**
- [ ] Newtypes for IDs/keys; enums instead of meaningful `bool` params
- [ ] Private fields, `#[non_exhaustive]` where growth is expected, `Debug` on public types
- [ ] No wrappers (`Arc<Mutex<_>>`, `Box<dyn _>`) in public signatures

**Errors and panics (`rust-errors-and-panics.mdc`)**
- [ ] PHP-visible outcomes modeled as data, not Rust `Err`
- [ ] Crate-specific error types; sources preserved; no swallowed errors
- [ ] No `unwrap`; `expect` only with proven invariant + `#[expect(..., reason)]`

**Unsafe, concurrency, observability**
- [ ] `unsafe` only in ADR-approved crates, each block with `// SAFETY:`; Miri considered
- [ ] Request-affine state stays on one execution context; no parallel hook dispatch; no lock across `.await`
- [ ] Every external call has a timeout; tasks have owners
- [ ] `tracing` with structured fields; no secrets in logs/`Debug`

**Tests (`rust-testing.mdc`)**
- [ ] Expected values from oracle/contract, not from the implementation
- [ ] Deterministic (injected clock/RNG); fakes behind `test-util`
- [ ] Regression test for every bug fix

## Output format
- 🔴 **Blocker** — violates mandate, soundness, boundaries, or gate fails
- 🟡 **Should fix** — SOLID/API/test-quality issue with a concrete fix
- 🟢 **Nit** — optional improvement

End with: gate commands run + results, and an overall verdict (approve / request changes).
