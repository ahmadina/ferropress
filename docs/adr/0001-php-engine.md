# ADR-0001: PHP engine — independent, clean-room Rust implementation

- **Status:** Accepted
- **Date:** 2026-10-23

## Context

Ferropress must run existing PHP plugins and themes unchanged, and production must never invoke or bundle
a PHP interpreter, the Zend engine, `libphp`, PHP-FPM or a `php` subprocess. Plugins therefore need a PHP
engine written in Rust.

The options considered:

1. **Embed or link the official PHP runtime.** Rejected: violates the no-PHP mandate outright.
2. **Adopt an existing Rust PHP engine as a dependency.** The candidates we know of are experimental, and
   at least one is published under the PHP License 3.01, whose combination with GPL code we do not assume
   is permitted (product rule 10). Adopting one would also tie the hardest part of the product to code we
   do not control.
3. **Write our own engine**, from documented and observed PHP behavior.

## Decision

Ferropress implements its own PHP engine in Rust, clean-room:

- Behavior comes from the PHP language documentation, the observable output of the pinned reference PHP
  version running in the isolated lab, and the official PHP test suite run there (never vendored).
- No source code from php-src or from other PHP engines is copied or translated. Other engines may be
  studied as prior art for architecture ideas only, with the same clean-room rules as upstream sources.
- The engine targets one pinned PHP behavior profile (version, enabled extensions, reported constants) and
  reports that profile truthfully to plugins. Supporting more profiles needs its own ADR and tests.
- Engine crates live in the PHP runtime layer (`php-*`) and depend only on foundation crates. The CMS core
  never depends on engine internals; it talks to the engine through VM-neutral handles (ADR-0003).
- Native PHP extensions compiled for the Zend ABI are not loadable. Extension functionality that the
  certified plugin set needs is reimplemented in Rust, recorded in an extension capability registry, and
  unimplemented extensions are reported as absent rather than faked.

## Consequences

- The engine is the largest and riskiest work stream; it runs in parallel with the contract inventory
  (roadmap Phase 3) and is measured by PHP conformance results, not by claims.
- We own the engine's memory model, so request-scoped arenas, interning and the bridge design can be
  optimized for this product.
- Plugins that need a native-only extension with no Rust implementation are documented as unsupported
  until one exists.

## Verification

- The official PHP test suite, run in the lab, produces a per-feature pass rate for the engine (roadmap
  step 17 onward).
- The CI "forbidden PHP runtime" scan and the `cargo deny` bans on crates that link PHP keep the runtime
  out of the repository and the dependency graph.
- Release qualification includes an independent scan of production artifacts for any PHP interpreter or
  library.
