## Summary

<!-- What behavior does this change implement or fix? Reference the work package / contract IDs. -->

Work package / contract IDs:

## Evidence

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test --workspace --all-features` (paste pass/fail counts)
- [ ] Coverage did not drop (`cargo llvm-cov`)
- [ ] Differential/oracle evidence attached, or explained why not applicable
- [ ] Benchmarks run for hot-path changes (before/after)

## Compliance

- [ ] No upstream WordPress/PHP code, comments, assets or test files added
- [ ] No new dependency, or dependency approved with license checked
- [ ] No new `unsafe`, or it is in an ADR-approved crate with `// SAFETY:` comments

## Risks and known differences

<!-- Anything that differs from reference WordPress behavior, and why it is acceptable or tracked. -->
