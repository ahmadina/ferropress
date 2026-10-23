# Ferropress Roadmap

Each step is one self-contained unit of work: it ships with its own tests, passes the full quality gate
(format, lint, tests, coverage), and is committed and pushed before the next step starts. Later phases are
broken into numbered steps when they begin, using what earlier phases proved.

Status legend: `[x]` done · `[ ]` not started

## Phase 0 — Foundations

- [ ] 1. Cargo workspace: root manifest with workspace lints, pinned toolchain, `rustfmt.toml`,
      `clippy.toml`, `.cargo/config.toml`
- [ ] 2. `xtask check-layers`: fail when a crate dependency breaks the layer rules
- [ ] 3. `deny.toml`: GPL-compatible license allow-list, advisories, banned crates
- [ ] 4. CI workflow: fmt, clippy, tests, docs, coverage gates, `cargo deny`, forbidden-PHP-binary scan
- [ ] 5. Architecture decision records (one commit each):
  - [ ] ADR-0001 PHP engine: independent, clean-room Rust implementation
  - [ ] ADR-0002 License and dependency policy (GPL-2.0-or-later)
  - [ ] ADR-0003 Bridge semantics: PHP values and callables across the Rust boundary
  - [ ] ADR-0004 Database parity
  - [ ] ADR-0005 Worker and tenant isolation
  - [ ] ADR-0006 Admin application and block-editor hosting
- [ ] 6. `contracts/reference-manifest.yaml` with pinned versions and hashes; `xtask verify-manifest`

## Phase 1 — Reference lab (oracle)

- [ ] 7. `lab/` Docker stack: stock WordPress 7.1.3 + PHP 8.5 + MySQL; `xtask lab up|down|reset`
- [ ] 8. `xtask lab fetch`: clone upstream sources at pinned tags into git-ignored directories
- [ ] 9. Capture tooling: HTTP responses, ordered hook trace, SQL log, DB diff, emails, cron events
- [ ] 10. `test-harness` crate: scenario schema, comparators, reviewed normalizations, fakes
- [ ] 11. `apps/test-orchestrator`: run one scenario against the lab and Ferropress, report diffs

## Phase 2 — Contract inventory

- [ ] 12. Symbol inventory into `contracts/`: functions, classes, hooks, constants, globals, REST routes,
      options, tables (facts only, our own descriptions)
- [ ] 13. Lifecycle trace: boot, plugin load and hook order for single site and multisite
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

## Phase 4 — Native WordPress kernel

- [ ] 21. `wp-types`, `wp-hooks`, `wp-request`, `wp-bootstrap`, `wp-php-bindings`
- [ ] 22. Milestone: a real plugin loads and produces the same hook trace as the reference

## Phase 5 — Data, users and queries

- [ ] 23. `wp-db` + `infra-mysql`, `wp-options`, `wp-cache`, `wp-content`, `wp-taxonomy`, `wp-comments`,
      `wp-users`, `wp-auth`, `wp-query` against existing WordPress databases

## Phase 6 — Rendering

- [ ] 24. `wp-rewrite`, `wp-formatting`, `wp-assets`, `wp-themes`, `wp-blocks`, `wp-media`, `platform-server`
- [ ] 25. Milestone: a classic theme and a block theme render byte-identical HTML to the reference

## Phases 7–12

- Phase 7: REST API, Ferropress admin application and plugin admin extension host, cron, CLI, updates
- Phase 8: real plugin and theme certification (WooCommerce, Elementor, SEO, fields, forms, multilingual)
- Phase 9: multisite, outbound HTTP, mail, i18n, import/export
- Phase 10: performance and security hardening
- Phase 11: site migration, deployment, observability
- Phase 12: stable release qualification
