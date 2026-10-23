# Ferropress Roadmap

Each step is one self-contained unit of work: it ships with its own tests, passes the full quality gate
(format, lint, tests, coverage), and is committed and pushed before the next step starts. Later phases are
broken into numbered steps when they begin, using what earlier phases proved.

Status legend: `[x]` done · `[ ]` not started

## Phase 0 — Foundations

- [x] 1. Cargo workspace: root manifest with workspace lints, pinned toolchain, `rustfmt.toml`,
      `clippy.toml`, `.cargo/config.toml`
- [x] 2. `xtask check-layers`: fail when a crate dependency breaks the layer rules
- [x] 3. `deny.toml`: GPL-compatible license allow-list, advisories, banned crates
- [x] 4. CI workflow: fmt, clippy, tests, docs, coverage gates, `cargo deny`, forbidden-PHP-binary scan
- [x] 5. Architecture decision records (one commit each):
  - [x] ADR-0001 PHP engine: independent, clean-room Rust implementation
  - [x] ADR-0002 License and dependency policy (GPL-2.0-or-later)
  - [x] ADR-0003 Bridge semantics: PHP values and callables across the Rust boundary
  - [x] ADR-0004 Database parity
  - [x] ADR-0005 Worker and tenant isolation
  - [x] ADR-0006 Admin application and block-editor hosting
- [x] 6. `contracts/reference-manifest.toml` with pinned versions and hashes; `xtask verify-manifest`

## Phase 1 — Reference lab (oracle)

- [x] 7. `lab/` Docker stack: pinned upstream reference platform + PHP 8.5 + MySQL; `xtask lab up|down|reset`
- [x] 8. `xtask lab fetch`: clone upstream sources at pinned tags into git-ignored directories
- [x] 9. Capture tooling: HTTP responses, ordered hook trace, SQL log, DB diff, emails, cron events
- [x] 10. `test-harness` crate: scenario schema, comparators, reviewed normalizations, fakes
- [x] 11. `xtask lab check`: deterministic lab (fixed clock and secret keys); every scenario must give
      identical results in two runs from a fresh install

## Phase 2 — Contract inventory

- [x] 12. Symbol inventory into `contracts/inventory/`: functions, classes, hooks, constants, globals,
      REST routes, options, tables (recorded facts only; per-item descriptions in our own words follow
      with step 14)
- [x] 13. Lifecycle trace: boot, plugin load and hook order for single site and multisite
      (`contracts/lifecycle/`, recorded by `xtask lab lifecycle` from both lab profiles)
- [ ] 14. Completeness dashboard: documented / implemented / tested / certified per item

## Phase 3 — PHP engine in Rust (runs in parallel with Phase 2)

- [ ] 15. `php-serialize`: byte-exact serialize/unserialize with property tests and fuzzing
- [ ] 16. `php-value`: value model (ordered arrays, references, copy-on-write, object handles)
- [ ] 17. Lexer → parser → bytecode compiler → VM core (one step each), checked against the PHP test suite
- [ ] 18. Language features: functions and closures; classes, interfaces, traits, enums; exceptions;
      generators and fibers; include/require and autoloading
- [ ] 19. Built-in function families: strings, arrays, math, JSON, PCRE, mbstring, date/time, streams,
      mysqli/PDO, and the rest required by the certified plugin set
- [ ] 20. Bridge proof: Rust calls PHP closures with correct references, identity and reentrancy; PHP
      classes extend Rust-owned classes

## Phase 4 — Native CMS kernel

- [ ] 21. `cms-types`, `cms-hooks`, `cms-request`, `cms-bootstrap`, `php-bindings`
- [ ] 22. `apps/test-orchestrator` runs scenarios against the lab and Ferropress; milestone: a real
      plugin loads and produces the same hook trace as the reference

## Phase 5 — Data, users and queries

- [ ] 23. `cms-db` + `infra-mysql`, `cms-options`, `cms-cache`, `cms-content`, `cms-taxonomy`, `cms-comments`,
      `cms-users`, `cms-auth`, `cms-query` against existing site databases

## Phase 6 — Rendering

- [ ] 24. `cms-rewrite`, `cms-formatting`, `cms-assets`, `cms-themes`, `cms-blocks`, `cms-media`, `platform-server`
- [ ] 25. Milestone: a classic theme and a block theme render byte-identical HTML to the reference

## Phases 7–12

- Phase 7: REST API, Ferropress admin application and plugin admin extension host, cron, CLI, updates
- Phase 8: real plugin and theme certification (WooCommerce, Elementor, SEO, fields, forms, multilingual)
- Phase 9: multisite, outbound HTTP, mail, i18n, import/export
- Phase 10: performance and security hardening
- Phase 11: site migration, deployment, observability
- Phase 12: stable release qualification
