# ADR-0003: Bridge semantics — PHP values and callables across the Rust boundary

- **Status:** Proposed — the dependency direction is accepted; the handle semantics below are confirmed or
  revised by the bridge proof in roadmap step 20.
- **Date:** 2026-10-23

## Context

The native core and plugin PHP call each other constantly and recursively: a plugin registers a callback
with the core, the core later invokes it while dispatching an event, the callback calls back into the core,
which may dispatch again. Plugins observe the results through object identity, by-reference arguments,
static and global state, exceptions and output buffering. A bridge that copies values (for example through
JSON or a message protocol) breaks identity and references and is rejected.

The core cannot depend on the engine and the engine cannot depend on the core without a crate cycle.

## Decision

**Dependency direction (accepted).**

1. `php-value` (foundation) defines VM-neutral handle types — values, arrays, objects, references and
   callables — and the ports the core uses to invoke callables and convert values. Domain crates store and
   pass these handles without knowing which engine is behind them.
2. `php-abi-bridge` (PHP runtime) implements those ports on top of the engine and owns identity,
   references and reentrancy.
3. `php-bindings` registers the PHP-visible API by delegating each function or class to the single native
   crate that owns the behavior. No behavior is implemented twice.
4. Binaries wire the concrete bridge into the domain ports in one composition root.

**Handle semantics (proposed).**

- **Identity.** A callable or object handle refers to the engine's live value, never a copy. Two handles to
  the same object compare equal, and the object's identity seen by PHP is unchanged after a round trip
  through the core.
- **References.** By-reference parameters are passed as reference handles; writes through them are
  visible to the caller exactly as in the reference implementation.
- **Reachability.** A handle held by the core is a garbage-collection root in the engine until the core
  releases it (for example when a callback is removed) or the request ends. Request teardown releases all
  request-scoped handles in bulk.
- **Reentrancy.** Nested core → PHP → core → PHP calls run on the same execution context and the same call
  stack, with no locks held across a call into PHP.
- **Affinity.** Handles are request-affine and not `Send`; the type system prevents moving them to another
  thread or request.
- **Errors.** PHP exceptions, warnings and fatal errors cross the boundary as values the core can propagate
  unchanged; Rust errors never surface to plugins as invented PHP errors.
- **Inheritance.** Core-owned classes are exposed through native method tables so that PHP classes can
  extend them, override methods, call the parent implementation and be reflected on as in the reference.

## Consequences

- The foundation layer carries the cross-cutting value model; it must stay small and stable, because every
  domain crate depends on it.
- Engine design (Phase 3) is constrained by these semantics from the start, for example GC roots for
  external handles and native method tables.
- Performance work must keep identity semantics; batching or copying values to save calls is not allowed.

## Verification

The bridge proof (roadmap step 20) runs differential scenarios against the reference: a closure capturing
an object by reference, invoked twice by the core during recursive dispatch; by-reference mutation;
callback removal during dispatch; exceptions thrown through nested calls; PHP classes extending core
classes with reflection and serialization. Object ids, values, output and errors must match. The ADR moves
to Accepted only when these pass.
