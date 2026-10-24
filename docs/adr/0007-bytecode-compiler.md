# ADR-0007: Bytecode compiler — compiled units, instruction format and the linking boundary

- **Status:** Accepted (amended 2026-10-23: constant folding, decisions 1 and 6)
- **Date:** 2026-10-23

## Context

Roadmap step 17 builds the engine as lexer → parser → bytecode compiler → VM core. The lexer and parser
match the reference exactly (`contracts/runtime/lexer.md`, `parser.md`). The compiler turns one file's
syntax tree into code the VM runs. Its observable behavior is twofold:

- **Diagnostics.** Compiling a file can fail with a fatal compile error, or raise compile warnings and
  deprecations, before any of it runs. PHP 8.5 reports about 260 distinct compile-time diagnostics. Lint
  mode (`php -l`) compiles without running, so it shows them all, and it accepts many files per process
  without classes leaking from one file into the next.
- **Semantics.** The VM must run the compiled code exactly as the reference runs the same source. That
  is proven by the VM core step and later steps, not by inspecting the bytecode.

The reference also *links* some classes while compiling ("early binding"). A top-level class with no
interfaces, no traits and no hooked properties needing variance checks is linked immediately when it has
no parent, or when its parent is already known: declared earlier in the same file, or a built-in class.
Linking runs the inheritance checks (method compatibility, `final`, visibility, property and constant
inheritance, `#[\Override]`), so lint reports those errors too. Every other class is linked when its
declaration runs. Linking needs class tables and the built-in classes (roadmap steps 18 and 19), and it
must behave identically whether it happens at compile time or at run time.

Owner mandates apply: memory- and CPU-efficient code, no copying of the reference implementation's
structure, and evidence from the reference for every behavior.

## Decision

1. **Crate.** `php-compiler` (PHP runtime layer) depends on `php-parser` and `php-value`. It owns the
   bytecode types, the compiler and a disassembler for tests. The VM depends on it, because `include` and
   `eval` compile at run time.
2. **Compiled unit.** Compiling one file gives either a `Unit` or one fatal error (message and line, as the
   reference prints them), plus the warnings raised in order. Errors visible to PHP are data, never Rust
   `Err`; Rust errors are kept for platform failures. A `Unit` holds the file's main code, its function
   declarations and its class declarations. Each declaration is a description the linker or VM binds when
   the declaration runs. Each class is marked with whether the reference links it at compile time.
3. **Code objects.** Every function body (main code, functions, methods, closures, arrow functions, property
   hooks) compiles to one code object:
   - a flat instruction list, with a parallel line table (lines are cold data, kept out of the hot list);
   - a literal pool;
   - named local slots, each variable resolved to an index at compile time, with names kept for variable
     variables, `compact`, `extract` and similar;
   - a count of temporary registers;
   - parameter and return type descriptions;
   - static variables;
   - a table of `try`/`catch`/`finally` regions;
   - the ranges where each temporary is live, so an exception releases exactly what is held.
4. **Instruction format.** Register-style three-address code. An instruction is a fixed-size `Copy` value:
   an opcode, a few flag bits, and up to three operands (local slot, temporary, literal or unused) plus a
   result. Jumps carry absolute instruction indices. The instruction set is our own, designed for the
   Ferropress VM. It is internal: it can change in any step without an ADR, as long as diagnostics and
   VM behavior stay identical. It is never a public or cached format until an opcode-cache ADR says so.
5. **Names.** Namespaces, `use` imports and the special class names are resolved at compile time.
   Unqualified function and constant names inside a namespace carry both the namespaced and the global
   name, for the run-time fallback. Lowercased lookup keys are computed once at compile time.
6. **Constant expressions.** Initializers for constants, parameter defaults, properties, static variables,
   enum cases and attribute arguments are checked at compile time for operations the reference rejects. They
   are stored as constant-expression trees that the VM evaluates when the reference does.
   The reference folds constant operands while compiling, and folding is observable: `true or yield` at
   the top level compiles without error because the right side is never compiled, and a folded
   constant expression is validated after folding. The compiler therefore folds exactly where the
   reference folds, with the reference's results. The operator semantics folding needs (conversions,
   arithmetic, comparison, string conversion of numbers) are VM-neutral operations in `php-value`,
   proven against the reference there, so compile-time folding and the VM compute every operator
   the same way.
7. **Linking boundary.** Class linking is one component, owned by the class-model work in roadmap step 18,
   and is used both for compile-time links and for declarations run later. The compiler does every check
   that needs only the file itself, including abstract-method checks of a single class. It leaves the
   inheritance checks to the linker. Until the linker exists, the compiler's differential results list
   files whose reference error comes from linking in a separate category: never hidden, never counted as a
   match.

## Consequences

- Diagnostics are proven against the reference now. Instruction semantics wait for the VM core, so the
  compiler's own tests check the instructions emitted for small programs through the disassembler.
- The VM sees only resolved names, slot indices and literal indices: no name lookups for locals and no
  re-parsing at run time.
- Compile-time links are faithful only once the linker and built-in class descriptions exist (steps 18
  and 19). The separate category in the compiler's differential results keeps that gap visible.
- An opcode cache needs a later ADR for a stable, versioned serialized form.

## Verification

- `cargo xtask lab compiler` records lint diagnostics (fatal error, warnings, deprecations, each with
  message and line) for hand-written cases and every valid parser oracle case.
  `crates/php-compiler` replays all of them.
- `cargo xtask lab compiler-corpus` compares the diagnostics for every upstream source file. A file
  either matches, or it differs and is listed. Files differing because of linking are counted separately
  until step 18.
- The VM core step proves instruction semantics by running the compiled code against reference output.
