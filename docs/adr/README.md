# Architecture Decision Records

An ADR records one significant decision: the context, the choice, and its consequences. Any change to a
crate boundary, a layer, an `unsafe`-permitted crate, the license policy or a frozen interface needs an ADR
before code lands.

## Lifecycle

- **Proposed** — the direction is chosen but depends on evidence still to be produced; the ADR names that
  evidence and the roadmap step that produces it.
- **Accepted** — in force. Code and rules must follow it.
- **Superseded by ADR-NNNN** — kept for history; never deleted or rewritten.

Accepted ADRs change only through a new ADR that supersedes them.

## Format

Copy [`0000-template.md`](0000-template.md) to `NNNN-short-title.md` with the next free number.

## Index

| ADR | Title | Status |
|---|---|---|
| [0001](0001-php-engine.md) | PHP engine: independent, clean-room Rust implementation | Accepted |
| [0002](0002-license-and-dependencies.md) | License and dependency policy (GPL-2.0-or-later) | Accepted |
| [0003](0003-bridge-semantics.md) | Bridge semantics: PHP values and callables across the Rust boundary | Proposed |
| [0004](0004-database-parity.md) | Database parity | Accepted |
| [0005](0005-worker-and-tenant-isolation.md) | Worker and tenant isolation | Proposed |
