# ADR-0002: License and dependency policy

- **Status:** Accepted
- **Date:** 2026-10-23

## Context

Ferropress hosts third-party PHP plugins and themes that are themselves overwhelmingly licensed
GPL-2.0-or-later, and it implements behavior compatible with a GPL-licensed platform. The project must
avoid copyright and trademark problems, and every dependency it ships becomes part of the distributed
product.

## Decision

**Project license.** All Ferropress code is licensed GPL-2.0-or-later, declared once in the workspace
manifest and inherited by every crate. The full license text is in `LICENSE`.

**Dependency admission.** A new dependency needs a concrete need that the standard library or an existing
dependency cannot meet, active maintenance, no unresolved security advisory, and a license from the
pre-cleared list:

| Pre-cleared | Needs review | Blocked |
|---|---|---|
| MIT, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0, LGPL-2.1-or-later, GPL-2.0-or-later | Apache-2.0-only (compatible with GPLv3, not GPLv2) and anything else | PHP License, proprietary, unknown or GPL-incompatible licenses |

Crates dual-licensed "MIT OR Apache-2.0" are used under MIT.

**Banned crates.** Crates that link or embed the PHP runtime are banned regardless of license.

**Sources.** Dependencies come from crates.io only; git and alternative registries are denied unless a
later ADR allows a specific source.

**Upstream material.** Upstream platform and PHP sources, tests, assets and translations are never
committed; they are fetched into git-ignored lab directories for study and oracle runs
(`clean-room-copyright.mdc`).

**Names.** The product, repository, crates, modules, types, functions and files never use the upstream
platform's name or its abbreviation. Native core crates use the `cms-` prefix. Identifiers that unchanged
plugins call exist only as data in contracts, fixtures and PHP-visible registries.

## Consequences

- `deny.toml` encodes the license list, the bans and the source rules; `cargo deny check` runs in CI and
  fails the build on any violation.
- Some well-known Apache-2.0-only crates are unavailable without review; we accept that cost.
- Legal review of the overall distribution model is tracked separately and is not assumed by this ADR.

## Verification

- CI job `supply-chain` (`cargo deny check advisories licenses bans sources`) on every push and pull
  request.
- Reviewer checklist item: no new dependency without the admission checklist and no upstream material in
  the diff.
