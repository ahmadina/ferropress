---
name: implementing-work-package
description: Executes one master-plan work package or ticket (e.g. ABI-001, DB-001, CORE-002) end to end - contract lookup, behavior specification, SOLID-compliant Rust implementation, tests, oracle evidence and completion report. Use when the user asks to implement a ticket, work package, phase task, feature or WordPress behavior in this repository.
---

# Implementing a Work Package

Implements master plan §12.4 with the project's Rust standards. Do one work package only.

```
Task Progress:
- [ ] 1. Read context
- [ ] 2. Specify behavior (before coding)
- [ ] 3. Design (SOLID + boundaries)
- [ ] 4. Implement
- [ ] 5. Test and gather evidence
- [ ] 6. Report
```

## 1. Read context
- `AGENTS.md`, relevant ADRs in `docs/adr/`, contract entries in `contracts/`, the master plan phase and gate.
- The pinned upstream WordPress/PHP source locations and reference tests for the behavior.
- Existing crate boundaries the change touches.
If the contract, oracle capture or ADR the work depends on does not exist, stop and report it as a blocker.

## 2. Specify behavior (write this down before coding)
A. Expected observable behavior and difficult edge cases.
B. Exact oracle reproduction steps (or "oracle not yet available" — then the package cannot be completed).
C. Data, hook, global, reference and SQL side effects, including ordering.
D. Acceptance tests and rollback risk.
E. Whether a frozen interface changes → request ADR review and wait.

## 3. Design
Check against `rust-solid.mdc` and `rust-architecture-boundaries.mdc`:
- Which crate owns the behavior? Exactly one owner.
- Pure logic separated from I/O; I/O, clock, RNG, VM reached through injected ports.
- Narrowest traits; abstraction level chosen via concrete → enum → generic → dyn.
- PHP-visible outcomes modeled as data, Rust `Err` reserved for platform failure (`rust-errors-and-panics.mdc`).
- Request-affine, deterministic ordering preserved (`rust-async-concurrency.mdc`).

## 4. Implement
- Smallest **complete** behavior. No stubs, `todo!()`, placeholder returns or silent fallbacks.
- Docs on every public item with contract ID and upstream reference.
- No new dependencies without approval.

## 5. Test and gather evidence
- Unit + property tests; trait contract suite for every new impl; differential fixture against the oracle.
- Run the quality gate:
```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo llvm-cov --workspace --all-features
```
- Hot-path changes: run the crate's benchmarks and report before/after numbers.
- Fix failures at their root; never weaken a lint, skip a test, or normalize a diff to go green.

## 6. Report
Use this template:
```markdown
## <ID>: <title> — <complete | blocked | partial (not complete)>
**Behavior implemented:** …
**Contracts covered:** …
**Commands run and results:** (exact commands, pass/fail counts)
**Differential evidence:** (scenario IDs, artifacts) or why missing
**Changed files:** …
**Remaining risks / known differences:** …
**Compatibility manifest update:** …
```
Classify every claim as: verified by execution, sourced from upstream, engineering hypothesis, or known limitation.
After the gate passes, commit and push the step per `commit-conventions.mdc`.
