# ADR-0005: Worker and tenant isolation

- **Status:** Proposed — the execution model is accepted; the in-process vs out-of-process default for
  multi-tenant hosting is decided from the measurements listed under Verification.
- **Date:** 2026-10-23

## Context

PHP software assumes every request starts with fresh globals and statics and that nothing survives the
request except what it stores explicitly. A resident Rust server breaks that assumption unless state is
separated deliberately. Plugins are also third-party code: Rust memory safety does not stop a plugin from
reading files, opening sockets or looping forever, and in multi-tenant hosting one site must never see
another site's data or secrets.

## Decision

**Execution model (accepted).**

- **Shared, immutable:** compiled PHP code (an opcode cache keyed by file content), interned symbols and
  read-only configuration may be shared across requests and workers.
- **Request-local, mutable:** globals, statics, the PHP heap, registered callbacks, the object cache's
  request layer, output buffers and the current user live in a per-request context and are released in
  bulk at teardown. Nothing request-local outlives the request.
- **Request-affine:** a request runs start to finish on one execution context. Event dispatch is sequential
  and deterministic; there is no parallel dispatch of plugin callbacks. Async is used only at I/O edges
  (HTTP ingress, database and outbound network), never in a way that reorders plugin-observable effects.
- **Bounded:** every request has a wall-clock timeout, a memory limit and a cancellation path; a panic or
  fatal error in one request is contained and does not take down the process.
- **Per-site workers:** a worker serves one site at a time; switching sites requires a full context reset.

**Isolation profiles (proposed).**

- **Trusted profile:** plugin code runs in-process in the request worker, with the full plugin
  compatibility surface. Intended for single-tenant deployments.
- **Isolated profile:** each site's workers run in separate OS processes with least-privilege filesystem
  and network policy, so a compromised or runaway site cannot read another site's memory, files or
  credentials. Restrictions may break plugins that need the restricted capability; the profile documents
  which capabilities are restricted and certification is reported per profile.

## Consequences

- The request context type is the owner of all mutable per-request state, which makes leaks testable:
  after teardown, nothing from the request is reachable.
- Process isolation costs memory and startup latency; the worker pool must pre-start and reuse processes
  per site.
- Compatibility claims are always qualified by profile.

## Verification

- Soak tests run hundreds of thousands of mixed requests across users and sites, asserting no cross-request
  or cross-site state, stable memory and correct destructor and shutdown-function behavior.
- Fault tests cover infinite loops, deep recursion, memory exhaustion and panics in a request.
- The in-process vs out-of-process default is fixed by measuring latency, memory per site and
  compatibility under both profiles once the server exists (roadmap Phase 6 onward), and this ADR moves to
  Accepted with those numbers.
