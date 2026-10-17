---
task_uid: "01m4rhrhs9zss2hs2kknarg603"
task: "T543"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test auto_save_tests::after_delay_saves: with after_delay 1000, typing into a file's editor then advancing the clock 999 ms leaves it dirty and the file unchanged; at 1000 ms the file on disk has the typed text and the tab is clean; a second split of the buffer causes one save"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test auto_save_tests::focus_and_window: on_focus_change saves a dirty editor when focus moves to another tab and when the window deactivates; on_window_change saves only on deactivation; off never saves; an untitled (path-less) dirty buffer is never saved"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test auto_save_tests::vim_insert: with vim on and after_delay, typing in insert mode and advancing 5 s saves nothing; Esc then 1000 ms saves; '.' then repeats the insert and 'u' undoes it in one step. on_insert_leave saves on Esc and after a normal-mode dd"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test auto_save_tests::conflict_held: a dirty buffer whose file was rewritten on disk isn't overwritten by an auto-save, one warning toast says 'Auto save skipped: \u003cname\u003e changed on disk', and no further auto-save is tried until cmd-s or a reload"
    required: true
    manual: false
  - id: "C5"
    text: "Changing editor.auto_save in settings.toml applies without restart (test auto_save_tests::setting_reload); cargo test -p fe-workbench passes; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Auto-save editors after a delay, on focus change, on window change or on leaving vim insert mode, as editor.auto_save says

## Problem

The user (2026-10-12): "card … auto-save too". With the setting (auto-save setting card), the policy (auto-save policy
card) and save reasons (save-reason card) in place, nothing feeds the policy or saves what it says is due.

## Change

New file `crates/fe-workbench/src/auto_save.rs` (declared from `crates/fe-workbench/src/lib.rs`), owned by the
workbench (`crates/fe-workbench/src/workbench.rs`: one field, created in `Workbench::new`, and the calls below):

- Builds an `AutoSavePolicy` from `editor.auto_save`/`auto_save_delay`, and `set_mode` on settings reload.
- Feeds it: each open editor's edits (`edited` with `has_path`, read-only, and vim insert from the editor's vim
  state), vim insert mode ending (`insert_left`), an editor losing focus (`focus_lost`; switching tabs or panes, or
  focusing a panel or the vim line counts, as VS Code's active-editor change), the window deactivating
  (`window.observe_window_activation` → `window_blurred`), saves (`saved`), T433's reloads (`reloaded`), and tabs
  closing (`closed`).
- One `Timer` on the foreground executor for `next_deadline()`; on firing, and after every event, saves each `due`
  buffer through `EditorView::save_with(<its auto reason>)` and reports the result (`saved` / `save_failed`). A
  changed-on-disk failure shows one warning toast (T517's `toast::show`) `Auto save skipped: <name> changed on disk` (cmd-s
  on it then asks, as T432 does), nothing modal; another failure an error toast
  with the error. VS Code holds the file dirty for the user's explicit save the same way.
- A buffer shown in several editors (splits) is one `BufferKey`, saved once.
- Hot exit (T509) stays independent, as VS Code keeps both: an auto-save makes the buffer clean, so T509 removes its
  backup; this card changes nothing about closing or quitting.

## Tests

New file `crates/fe-workbench/src/auto_save_tests.rs` (declared `#[cfg(test)]` from auto_save.rs), gpui
`TestAppContext` with a temp dir, advancing the test executor's clock.
