---
task_uid: "01m7v0kajns6cw8bz643ee8kf9"
task: "T704"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "coverage_tests::a_report_through_a_symlinked_ancestor_of_the_root_resolves_inside: with the project opened through t/r/link -\u003e t/r/real and the file named via t/alias -\u003e t/r, an lcov SF record and a Cobertura class filename for t/alias/real/src/c.rs mark canonical c.rs; a path under t/alias that resolves outside the root is kept as spelled."
    required: true
    manual: false
  - id: "C2"
    text: "coverage_tests::paths_resolve_under_symlinked_roots and the rest of coverage_tests pass unchanged."
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench passes the vet stage (clippy -D warnings)."
    required: true
    manual: false
---

# Test coverage I fix: a report naming files through a symlinked ancestor of the root (macOS /var) resolves them inside the project (Mac T1982 gate)

## Problem

The Mac's macOS gate on helium-batch-209 (T700 = Mac T1982, picked onto mac/main 29a8a7ef4)
fails `coverage_tests::paths_resolve_under_symlinked_roots` (crates/fe-workbench/src/coverage_tests.rs:573):
`report.mark(&canonical("c.rs"), 3)` is `None`, expected `Some(Covered)`.

The cause is in `Resolver` (crates/fe-workbench/src/coverage.rs, `inside`, `resolve`, `canonical`).
An absolute report path is normalized and passed to `canonical`. If it doesn't start with the
root as opened (`root`) or the canonical root (`canonical_root`), `canonical` treats it as
outside the project and returns it unresolved. On macOS the tempdir is `/var/folders/...`, and
`/var` is a symlink to `/private/var`. The record `SF:{real}/src/c.rs` therefore spells the
project through a symlinked ancestor. It matches neither prefix and is never matched to the open
file's canonical path. Linux's `/tmp` is canonical, so the test passes there.

This is a real loader bug, not just a test bug: a report that names files through any symlinked
ancestor of the root (a symlinked home, `/var` on macOS) loses those records. It affects lcov
`SF:` lines and Cobertura absolute `filename`/`<source>` paths alike, since both go through
`Resolver::resolve`.

## Fix

In the resolver, when an absolute path is not inside by prefix and it exists, canonicalize it
(`dunce::canonicalize`). If the canonical path is inside `canonical_root`, use it. Otherwise keep
the original normalized path: the existing rule "outside the root: kept, normalized, never
canonicalized" still holds for paths that stay outside. Relative candidates and the existing
prefix checks keep working as they do.

Keep `paths_resolve_under_symlinked_roots` unchanged; it covers this case on macOS.

## Regression test

Add a test to crates/fe-workbench/src/coverage_tests.rs that reproduces the bug on Linux. In a
tempdir `t`:
- create `t/r/real/src/c.rs`;
- symlink `t/alias -> t/r` (a symlinked ancestor of the real tree);
- symlink `t/r/link -> t/r/real`, and open the project through `link`.

Then load a report naming the file as `t/alias/real/src/c.rs`: an lcov `SF:` record and a
Cobertura `<class filename=...>`. Mark `canonical(c.rs)` and expect the covered line. Add a path
under `t/alias/` that resolves outside the root (e.g. `t/alias/elsewhere.rs`, created so it
exists), and expect it kept as spelled. At base, the `alias` spelling matches neither prefix, so
the mark is `None` and the test fails.

This fixes Mac T1982 (helium T700), which is held at the Mac's gate. Once it lands, helium cuts
helium-batch-210 holding T700 and this fix together on current mac/main.
