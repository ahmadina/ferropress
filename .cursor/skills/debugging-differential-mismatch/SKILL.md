---
name: debugging-differential-mismatch
description: Diagnoses and fixes a mismatch between the native Rust platform and the reference WordPress/PHP oracle by reproducing, attributing it to the owning boundary, minimizing it to a regression fixture and fixing the true semantic cause. Use when a differential, oracle, certification or plugin compatibility test fails, or output differs from stock WordPress.
---

# Debugging a Differential Mismatch

Implements master plan §12.5.

```
Task Progress:
- [ ] 1. Reproduce on both environments
- [ ] 2. Capture full evidence
- [ ] 3. Attribute to one owning boundary
- [ ] 4. Minimize to a deterministic fixture
- [ ] 5. Fix the semantic cause
- [ ] 6. Prove no regressions
```

## 1. Reproduce
Run the failing scenario against the pinned reference and the native platform with identical fixtures,
clock, timezone, locale and seeds. If it does not reproduce deterministically, fix determinism first.

## 2. Capture
Input, both outputs, PHP warnings/notices, ordered hook trace, SQL log and DB diff, callback state,
headers/cookies, scheduled events, emails, runtime and profile versions.

## 3. Attribute (pick exactly one owner)
| Boundary | Typical signal |
|---|---|
| PHP language semantics | Same diff reproduces in a plain `.php` script without WordPress |
| PHP extension | Diff isolated to one built-in function family (mbstring, PCRE, mysqli…) |
| Bridge | Values/identity/references change when crossing Rust↔PHP; reentrancy issues |
| Native Core | Hook order, SQL, cache invalidation or return values differ with no plugin involved |
| Theme / plugin interaction | Only appears with a specific package or combination |
| Database / environment | Collation, version, timezone, filesystem, network differences |

## 4. Minimize
Shrink to the smallest scenario that still mismatches; commit it as a differential fixture under
`scenarios/differential/` (or a unit/contract test in the owning crate) with the oracle-captured expectation.

## 5. Fix
- Fix the owning crate's semantics. Never special-case a plugin name, slug or path.
- Never normalize, skip or loosen the assertion to make the diff disappear.
- If the fix changes a frozen interface or contract, update the contract/ADR and request review.

## 6. Verify
Run the minimized fixture, the original failing scenario, and the full quality gate:
```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```
Not fixed until: both environments match on the original scenario and the regression test passes.
A screenshot, HTTP 200, or a different runtime running the original PHP WordPress Core is not evidence.
