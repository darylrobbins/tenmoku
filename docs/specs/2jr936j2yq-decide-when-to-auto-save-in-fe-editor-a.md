---
task_uid: "01m4rwgeyb99jjmj2jr936j2yq"
task: "T56"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-editor test auto_save::after_delay: with AfterDelay(1s), edits at t=0 and t=0.5s make the buffer due at t=1.5s and not before (next_deadline says 1.5s), once; an edit with vim_insert true is not due at any time until insert_left at t=5s, then due at t=6s"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test auto_save::focus_and_window: OnFocusChange makes the buffer due on its focus_lost and every dirty buffer due on window_blurred; OnWindowChange only on window_blurred; OnInsertLeave due on insert_left and on a vim_insert-false edit when vim was seen, and acts as OnFocusChange when it wasn't; Off never"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor test auto_save::exclusions: a buffer without a path or read-only is never due; after save_failed(conflict=true) a dirty buffer is never due until saved or reloaded; after save_failed(false) not until the next edit; set_mode(Off) drops pending deadlines"
    required: true
    manual: false
  - id: "C4"
    text: "fe-editor test auto_save::edits_allowed: SaveReason::edits_allowed is false for AutoDelay always and for every auto reason with vim_insert true, true for Explicit, AutoFocus, AutoWindow and AutoInsertLeave otherwise"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-editor passes; the policy reads no clock (no Instant::now in auto_save.rs); git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Decide when to auto-save in fe-editor: a GPUI-free AutoSavePolicy over edits, focus, window, insert-mode and save events, with VS Code's rules and vim insert sessions kept whole (Mac T541)

Filed on argon from Mac T541 (card ~/coordination/argon-cards/T541.json rev 1). Mac task uid 01m4rhrgamrh32nqb5skeaj9wz. Depends on argon T55: both change crates/fe-editor/src/lib.rs. Title the host commit "... (Mac T541)".

## Problem

The user (2026-10-12) asked for auto-save with VS Code's semantics, and for its timing and policy to be testable
without the GUI. The vim-mode requirement (coordinator, same day): auto-save must not save mid-insert in a way that
breaks `.` repeat or insert mode's single undo step. Nothing decides when a buffer should be auto-saved.

## Change

New GPUI-free file `crates/fe-editor/src/auto_save.rs` (`pub mod auto_save` from `crates/fe-editor/src/lib.rs`): a
policy driven by events and a caller-supplied clock (`std::time::Instant` passed in, never read inside), keyed by a
caller's `BufferKey` (`Copy + Eq + Hash`).

- `AutoSaveMode { Off, AfterDelay(Duration), OnFocusChange, OnWindowChange, OnInsertLeave }` (its own enum; the wiring
  card maps fe-config's).
- `AutoSavePolicy::new(mode)`, `set_mode(mode)` (re-evaluates pending deadlines; `Off` drops them).
- Events: `edited(key, now, Facts)` where `Facts { has_path: bool, read_only: bool, vim_insert: bool }`;
  `insert_left(key, now)`; `focus_lost(key)`; `window_blurred()`; `saved(key)` (explicit or auto, clears the hold
  below); `save_failed(key, conflict: bool)`; `reloaded(key)`; `closed(key)`.
- Queries: `due(now) -> Vec<BufferKey>` (buffers to save now, each returned once until edited again) and
  `next_deadline() -> Option<Instant>` for the caller's timer.
- Rules, as VS Code's EditorAutoSave unless noted:
  - Never a buffer without a path (untitled) or read-only.
  - `AfterDelay(d)`: due `d` after the last edit; each edit restarts it. While `vim_insert`, not due until
    `insert_left`, then due `d` after it (Neovim auto-save plugins' InsertLeave trigger), so an insert session is
    never saved midway.
  - `OnFocusChange`: due on `focus_lost` for that buffer and on `window_blurred` for every dirty one (VS Code saves
    on window change in this mode too).
  - `OnWindowChange`: due on `window_blurred` for every dirty one.
  - `OnInsertLeave`: due on `insert_left`, and on an edit with `vim_insert` false (a normal-mode change such as `dd`,
    `p`, `u`); with vim off (no `vim_insert` ever seen for the buffer, the caller says so through `Facts`), as
    `OnFocusChange`.
  - A dirty buffer whose save failed with `conflict` (changed on disk) is held: never due again until `saved` or
    `reloaded` (VS Code doesn't auto-overwrite a newer file). Another failure is held until the next edit.
- `SaveReason { Explicit, AutoDelay, AutoFocus, AutoWindow, AutoInsertLeave }` and `SaveReason::edits_allowed(self,
  vim_insert: bool) -> bool`: `false` for `AutoDelay` (VS Code runs no format-on-save or trim participants for
  `afterDelay`) and for any auto reason while `vim_insert`; `true` otherwise.

## Tests

New file `crates/fe-editor/tests/auto_save.rs`, driving the policy with `Instant`s offset from one base.
