---
task_uid: "01m6c6nd7va6prfpka1m6xqwcv"
task: "T1280"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Cargo.lock has gix 0.90.x (or later) and no gix 0.89; rg -n '^name = \"gix\"' -A1 Cargo.lock shows it"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-git -p fe-workbench passes with no existing test edited"
    required: true
    manual: false
  - id: "C3"
    text: "cargo deny check bans licenses sources passes, and cargo deny check advisories reports nothing for gix or any gix-* crate (other advisories present at the base are out of scope: a separate card covers them)"
    required: true
    manual: false
---

# Bump gix from 0.89 to 0.90 for GHSA-q54r-jf2p-rfr2 (Dependabot alert 1: untrusted .git/config could turn off .git path protection); fe-git adapts with no behaviour change

Dependabot alert 1 (high, GHSA-q54r-jf2p-rfr2): gix >= 0.88, <= 0.89 reads core.protectHFS/protectNTFS/
gitoxide.core.protectWindows from an untrusted repo-local .git/config without the trust filter, so a
reduced-trust repo can turn off the .git path-confusion checks gix runs during checkout, index
creation, merge and tree edits. Fixed in 0.90.0.

Tenmoku uses gix read-only (features status, blame, sha1, parallel in the workspace Cargo.toml);
checkout and staging go through the git CLI (fe-git stage.rs, branch.rs), so the vulnerable paths are
not reached today. Bump anyway: it clears the alert and keeps a future gix write path safe.

Do: workspace Cargo.toml `gix = "0.90"` (same features), `cargo update -p gix` and any gix-* crates it
pulls, adapt fe-git to API changes if any (no behaviour change), keep cargo deny green. If 0.90 forces
a large fe-git rewrite, stop and report instead.

## Implementation notes

- gix 0.89 -> 0.90.0 (plus its gix-* crates); fe-git needed no source change.
- C2: `cargo nextest run -p fe-git -p fe-workbench` on the default profile (no ci filter): 1999 run, 1999 passed, 3 skipped.
  `tests::m3_signoff::a_100k_file_project_end_to_end`, which the ci profile skips, run alone: passed.
- C3: `cargo deny check bans licenses sources` passes. `cargo deny check advisories` reports 6 errors
  (ctutils RUSTSEC-2026-0385, pulldown-cmark -0414, wasmtime -0422/-0423/-0424, a wasip3 UDP one);
  none names gix or a gix-* crate (`rg -c gix` on its output is 0). The GHSA-q54r-jf2p-rfr2 alert is cleared.
