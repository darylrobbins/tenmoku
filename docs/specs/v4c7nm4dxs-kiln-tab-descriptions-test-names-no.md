---
task_uid: "01m57r4fmwqtr37nv4c7nm4dxs"
task: "T200"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "python3 scripts/check_kiln_boundary.py exits 0"
    required: true
    manual: false
  - id: "C2"
    text: "The kiln-shell test same_titled_tabs_show_their_shortest_distinguishing_folders passes, checking the same descriptions as before"
    required: true
    manual: false
---

# kiln: tab_descriptions test names no fastedit crate, so check_kiln_boundary.py passes again (T195 follow-up)

## Problem

`python3 scripts/check_kiln_boundary.py` fails on helium main since T195 (bdb3c26e):

    error: kiln/crates/kiln-shell/tests/tab_descriptions.rs:100: names fastedit: Some("crates/fe-vim/src/ex.rs"),
    error: kiln/crates/kiln-shell/tests/tab_descriptions.rs:101: names fastedit: Some("crates/fe-vim/src/tests/ex.rs"),

kiln must name nothing of fastedit (the script's `FASTEDIT` pattern matches `fe-<letter>`). The two paths are
only sample data for `same_titled_tabs_show_their_shortest_distinguishing_folders`.

## Change

In `kiln/crates/kiln-shell/tests/tab_descriptions.rs`, use `crates/vim/src/ex.rs` and
`crates/vim/src/tests/ex.rs` instead. The distinguishing folders the test expects stay the same; adjust any
expected description that spells `fe-vim`. No production change.
