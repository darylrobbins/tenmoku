---
task_uid: "01m59j01154qqkymj80ev4yf09"
task: "T225"
rev: 6
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "A test in the vim_remaps tests passes (vim_remaps::tests::a_project_file_changing_between_load_and_construction_keeps_its_warning_lines, or the name the worker used): a valid project settings file made invalid between SettingsStore::load and VimRemaps::new still has its remap and leader warnings at the lines of the accepted text, not the user file's. The test lives inline in vim_remaps.rs, which the regression overlay cannot carry to the base, so this card is a chore with its test as evidence, not a bugfix with a regression test"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fastedit -p kiln-core passes with no existing test edited; cargo clippy -p fastedit -p kiln-core --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Vim remap warnings use the settings store's accepted text, so a file changing between load and construction cannot misplace them (T219 review F2)

## Problem

T219 (Mac T875, the vimrc loaded as remaps) shipped with a known limit (review finding F2, codex, medium, `crates/fastedit/src/vim_remaps.rs:~100`): `VimRemaps` rereads the settings files to place diagnostics, rather than retaining the settings store's accepted text. If a valid project settings file becomes syntactically invalid between `SettingsStore::load` and `VimRemaps::new`, `accept()` caches the invalid text using the store's earlier diagnostics, so project remap and leader warnings fall back to the user file and stay misplaced when the first watcher batch rejects the project file. Constructing the source from a store that already retains a rejected layer also loses that layer's accepted locations. The separate reread during reload has the same snapshot mismatch.

## Change

Retain each settings layer's accepted text or spans alongside its last-good table in the settings store (`kiln/crates/kiln-core/src/settings.rs`) and expose that snapshot to `VimRemaps`, so positions are computed from what the store accepted, not from a fresh read. Remove the limit from `vim_remaps.rs`'s module docs. New tests in a new file declared from a test file: a project file changing between store load and source construction, construction after a rejected reload, and retained locations for both remaps and leader.
