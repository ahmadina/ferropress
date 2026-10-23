# ADR-0004: Database parity

- **Status:** Accepted
- **Date:** 2026-10-23

## Context

Existing sites must run on Ferropress against their existing MySQL or MariaDB databases, and existing
plugins read and write that database directly: they issue raw SQL through the PHP-visible database object,
create custom tables, filter the SQL the core generates, and depend on autocommit behavior, insert ids,
affected-row counts, error state and result ordering. A cleaner storage design (an ORM, a different schema,
a different engine) can return correct-looking API results while changing these side effects, and plugins
would break silently.

## Decision

- **The schema is the reference schema.** Ferropress reads and writes the existing table layout, column
  types, charsets, collations, indexes and serialized value formats unchanged. Schema upgrades follow the
  reference upgrade path. No Ferropress-only tables are added to a site database without an ADR.
- **SQL behavior is part of the contract.** For each core operation, the SQL statements issued, their
  order, the filters that may rewrite them and the resulting side effects match the reference. Where the
  exact statement text is plugin-observable (through SQL filters or query logging), it matches too.
- **The PHP-visible database object is fully supported**: query execution, prepared-statement placeholder
  semantics and escaping, result shapes, insert id, row counts, last error, table-name properties and the
  table prefix, including use by plugins for their own custom tables.
- **Transactions are passed through, not invented.** Ferropress uses the server's autocommit default and
  issues transaction statements only where the reference does or where plugin SQL does.
- **MySQL first.** `infra-mysql` is the only storage adapter at first. Other engines need their own ADR and
  their own parity evidence.
- **Storage stays behind a port.** Domain crates (`cms-db` and its users) depend on a database port, never
  on a driver; the driver lives in `infra-mysql`. Abstractions above raw SQL are introduced only when
  differential tests prove they issue the same statements.

## Consequences

- Performance improvements come from connection handling, statement caching, caching layers and
  allocation, not from changing the queries plugins can observe.
- Database differential testing is a first-class oracle output: every scenario records the SQL log and a
  before/after database diff (roadmap step 9).
- Concurrency-sensitive flows (orders, stock, payments, scheduled actions) need dedicated tests under
  parallel load before any plugin that uses them is certified.

## Verification

- Differential scenarios compare SQL logs and database diffs between the reference and Ferropress.
- The PHP-visible database object has contract tests for placeholders, escaping, error state and result
  shapes, generated from oracle captures.
- Certification of commerce plugins includes concurrent checkout, refund and stock tests.
