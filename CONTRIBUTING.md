# Contributing to Ferropress

Thank you for helping. Read [AGENTS.md](AGENTS.md) first: it contains the engineering rules every change
must follow, whether written by a person or a coding agent.

## Ground rules

1. **Never copy WordPress code.** Study upstream behavior in the git-ignored `lab/` reference, describe it
   in `contracts/` in your own words, and implement it independently in Rust. Do not commit upstream code,
   comments, assets, translations or test files.
2. **Every change is tested.** New behavior needs unit tests and, where it is WordPress-observable, a
   differential test against the reference oracle. Coverage must not drop.
3. **No stubs.** No placeholder functions, `todo!()`, silent fallbacks or fabricated results.
4. **Performance matters.** Follow the memory/CPU guidance in `.cursor/rules/rust-performance.mdc`.
5. **Ask before adding dependencies.** Each new crate must pass the license and maintenance checklist.

## Workflow

1. Pick or open an issue that references a master-plan work package or contract ID.
2. Create a branch, implement one logical change, and run the quality gate:

   ```bash
   cargo fmt --all --check
   cargo clippy --workspace --all-targets --all-features -- -D warnings
   cargo test --workspace --all-features
   cargo llvm-cov --workspace --all-features
   ```

3. Commit using the single-line convention `<type>(<scope>): <summary>`
   (see `.cursor/rules/commit-conventions.mdc`).
4. Open a pull request using the template and attach test and differential evidence.

## Reporting compatibility problems

Use the "Plugin/theme compatibility" issue template and include the package name, exact version,
the workflow that fails, and what the reference WordPress installation does instead.
