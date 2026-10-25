# ADR-0008: VM core — execution model, frames, output and errors

- **Status:** Accepted
- **Date:** 2026-10-24

## Context

Roadmap step 17 ends with the VM core: running the bytecode of ADR-0007 with the reference's observable
behavior. What a script shows is its output, the warnings and errors PHP displays in that output, and
its exit status. ADR-0001 requires engine crates in the PHP runtime layer to be added by ADR, and the
bridge (ADR-0003) and worker isolation (ADR-0005) constrain how the VM is driven later: requests are
request-affine and single-threaded, while compiled code should be shareable between workers.

Owner mandates apply: memory- and CPU-efficient code, no `unsafe` outside ADR-approved crates, no
copying of the reference implementation's structure, and evidence from the reference for every
behavior.

## Decision

1. **Crate.** `php-vm` (PHP runtime layer) depends on `php-compiler` and `php-value`. It owns execution
   only; compiling stays in `php-compiler`. No `unsafe`.
2. **Sharing.** A compiled `Unit` holds no shared-ownership values (ADR-0007 literals are owned data), so
   it is `Send + Sync` and can be cached and shared. Runtime values (`php-value`) are single-threaded
   (`Rc`): one VM instance runs one request on one thread, and builds its values from literals.
3. **Interpreter loop.** One loop dispatches on the opcode of the current instruction. Calls between PHP
   functions push a frame on an explicit frame stack instead of recursing in Rust, so PHP recursion depth
   is bounded by memory, not by the Rust stack, and unwinding an exception walks the frame stack.
4. **Frames.** A frame holds its code object, the program counter, one entry per local slot (undefined,
   a value, or a reference, so references keep identity across frames) and a register file of
   temporaries sized from the code object. Arguments being prepared for a call live on a pending-call
   stack until the call starts.
5. **Functions.** User functions are bound by lowercase name in a per-request function table. A file's
   unconditional top-level functions are bound before its main code runs.
6. **Output.** Output goes to a sink owned by the embedder. Displayed diagnostics are written to the same
   output as the reference CLI writes them with `display_errors=1` and `html_errors=0`: a warning is
   `"\nWarning: <message> in <file> on line <line>\n"`.
7. **Errors.** Operator errors from `php-value` become thrown errors with the reference's class and
   message. Until classes exist (roadmap step 18) a thrown error is a VM value carrying its class name,
   message, file, line and stack trace. An uncaught error prints the reference's
   `"\nFatal error: Uncaught <class>: <message> in <file>:<line>\nStack trace:\n…\n  thrown in <file> on line <line>\n"`
   and ends the script with exit status 255.
8. **Scope.** Instructions the VM does not run yet cannot occur: the compiler rejects every construct it
   does not compile (ADR-0007), so the VM never guesses.

## Consequences

- PHP code runs without Rust recursion between PHP functions, which keeps deep recursion and exception
  unwinding predictable.
- Error objects become real objects of the built-in exception classes in step 18; the uncaught-error
  output and the stack-trace format stay as proven here.
- `error_reporting`, `display_errors` and error handlers are fixed to the oracle's settings until the
  ini and error-handler work lands; that work changes only where diagnostics go, not what they say.

## Verification

- `cargo xtask lab vm` records the output and exit status of hand-written scripts from the reference
  CLI into `contracts/vm/`; `crates/php-vm` replays every case.
- The PHP test suite in `lab/upstream/php` is run against the VM with `cargo xtask lab vm-phpt`, which
  reports how many tests pass, fail or use constructs not compiled yet.
