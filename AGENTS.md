# Engineering Rules: Ferropress

Ferropress is a Rust-native platform compatible with WordPress: it runs existing WordPress plugins and
themes without any PHP installation. It is not affiliated with or endorsed by WordPress or the WordPress
Foundation.

When present locally, `WordPress_Rust_Full_Product_Engineering_Master_Plan.md` is the owner's private
scope document ("master plan"); read the relevant sections before changing anything. It is git-ignored
and must never be committed, quoted at length, or linked from public files.

## Owner mandates (standing instructions — always apply)

1. **High test coverage, tests for everything.** Every function, impl, error path and bug fix is tested;
   coverage gates in `rust-testing.mdc` are blocking.
2. **High standard, memory- and CPU-efficient code.** SOLID, idiomatic Rust, and `rust-performance.mdc`.
3. **Study WordPress, never copy it.** Clone upstream into the git-ignored `lab/` reference only; implement
   behavior independently, the Rust way, proven by tests (`clean-room-copyright.mdc`).
4. **Existing PHP plugins and themes must work unchanged with no PHP runtime at all.** That is the goal for
   every plugin; public claims are limited to what the certification suite has proven (master plan §1).
5. **Respect WordPress copyright and trademarks** at every step (`clean-room-copyright.mdc`). Never use
   "WP"/`wp` in crate, module, type, function or file names, or in roadmap/docs prose; native core crates
   use the `cms-` prefix. Plugin-facing identifiers exist only as data in contracts and registries.
6. **Step by step, committed and pushed.** Each verified step is its own commit, pushed to GitHub
   (`commit-conventions.mdc`). The repository is always GitHub-ready.
7. **Own admin, full plugin admin support.** Do not clone the WordPress admin UI. Ferropress has its own
   admin application that fully hosts what plugins and themes add to the admin (menus, settings pages,
   meta boxes, notices, AJAX, list tables, editor blocks) — master plan ADR-R8.

## Product mandate (from master plan §12.2)

1. This is a full WordPress 7.1.3 compatibility product, NOT an MVP.
2. Production must never invoke Zend/PHP-FPM/php CLI or depend on a bundled PHP engine.
3. Existing third-party PHP plugin/theme source must run unmodified in the Rust VM.
4. Never claim full compatibility without oracle and native-core evidence.
5. Before writing code: locate related contract IDs, WordPress source and reference tests.
6. Do not invent WordPress/PHP behavior. Reproduce and measure it.
7. No placeholder functions, silent fallbacks, fabricated test output, or catch-all stubs.
8. Separate language-runtime conformance from native WordPress Core compatibility.
9. Preserve callback identity, reference semantics, hook order, globals and SQL side effects.
10. No legal assumption that the PHP License 3.01 and GPL code may be linked/distributed.
11. Do not commit proprietary plugins or their source without rights.
12. Every behavioral change needs unit + differential/regression tests and trace artifacts.
13. No broad refactors outside the assigned module without an approved ADR.
14. No dependency updates, network installers or binary execution without explicit review.
15. When blocked, report the smallest reproducer and evidence, not optimistic guesses.
16. Include exact commands and test logs in completion reports; never say a test passed if not run.
17. Avoid replacing upstream WordPress behavior with a cleaner design unless parity is proven.
18. Never release or deploy an unfinished feature as production-compatible.

## Engineering standards

Rust code follows SOLID adapted to Rust, the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/checklist.html)
and the [Microsoft Pragmatic Rust Guidelines](https://microsoft.github.io/rust-guidelines/).
The detailed, enforceable versions live in `.cursor/rules/`:

| Rule | Covers |
|---|---|
| `rust-solid.mdc` | SRP, OCP, LSP, ISP, DIP expressed with modules, traits, enums and generics |
| `rust-architecture-boundaries.mdc` | Crate layering, dependency direction, composition roots, VM isolation |
| `rust-api-design.mdc` | Naming, newtypes, visibility, conversions, builders, sealed traits |
| `rust-errors-and-panics.mdc` | Error types, PHP-visible errors vs Rust errors, panic policy |
| `rust-unsafe-ffi.mdc` | When `unsafe` is allowed and how it is justified and verified |
| `rust-async-concurrency.mdc` | Request-affine execution, ordering, blocking, cancellation |
| `rust-testing.mdc` | Unit, property, contract, differential and oracle tests |
| `rust-docs-observability.mdc` | Rustdoc sections, ADRs, structured tracing, secret redaction |
| `rust-dependencies-tooling.mdc` | Workspace lints, dependency admission, licensing, toolchain |
| `rust-performance.mdc` | Allocation strategy, hashing, streaming, benchmarks, release profile |
| `clean-room-copyright.mdc` | Studying vs copying WordPress, what may be committed, trademarks |
| `commit-conventions.mdc` | Commit format, commit-and-push per verified step |

Project skills in `.cursor/skills/`: `implementing-work-package`, `scaffolding-rust-crate`,
`reviewing-rust-code`, `debugging-differential-mismatch`.

## Quality gate (must pass before any work is reported complete)

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --no-deps --all-features   # with RUSTDOCFLAGS="-D warnings"
cargo llvm-cov --workspace --all-features        # coverage gates from rust-testing.mdc
```

Once installed and approved, `cargo deny check` is also part of the gate.
Report the commands you actually ran and their results.
