---
task_uid: "01m5412db5fpntf2ppd4m5c4ny"
task: "T737"
rev: 2
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-workbench test lsp_format::save_tests::overlap_tests::a_manual_format_replying_during_an_auto_save_hook_applies passes: the manual format's edits are applied and no toast shows, and it fails at the base. The overlap_tests module is declared by a '#[path = \"lsp_format_overlap_tests.rs\"] mod overlap_tests;' line in lsp_format_save_tests.rs (not in lsp_format.rs), so Regatta's base overlay, which copies *_tests.rs files, compiles it at base"
    required: true
    manual: false
  - id: "C2"
    text: "The same module's a_save_format_replying_after_insert_is_still_dropped passes: a format-on-save reply landing after vim entered insert leaves the text unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited (lsp_format_tests.rs, lsp_format_save_tests.rs and auto_save_tests.rs included; adding the one mod line to lsp_format_save_tests.rs is allowed); cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Apply a manual lsp.format_document reply that lands while an auto-save of the same editor runs its hook in a vim insert, instead of dropping it (T542 review F2)

## Problem

`lsp.format_document` and `lsp.format_selection` (`Workbench::format_active`) and format on save
(`FormatOnSave::before_save` -> `Workbench::format_buffer`) share `format_range` in
`crates/fe-workbench/src/lsp_format.rs`. Since T542, its reply handler drops the edits when
`!view.save_edits_allowed(cx)` (lsp_format.rs:128). That check reads the save *currently running a hook*
on the editor (`HOOK_REASONS` in `crates/fe-gpui/src/pane_item.rs`), not the request's own reason, so it
also applies to a manual format that has nothing to do with the save.

Failing case, by the code at Mac main c5e93ed6: vim on, `editor.format_on_save` on, the fake server's
formatting reply delayed on the fake clock. The user runs `lsp.format_document` in normal mode; before
the reply, a `SaveReason::AutoFocus` save of the same editor starts (normal mode, so its hook runs and
`HOOK_REASONS` holds `AutoFocus`) and the user presses `i` without typing. The manual reply then lands:
the buffer version is unchanged, but `save_edits_allowed` is false (`AutoFocus` in a vim insert), so the
format is silently dropped (`FormatOutcome::Unchanged`, no toast). It should apply: nothing changed the
text, and the save's own rules are for the save's own format. Codex raised this as T542 review F2
(medium, not carried to a card). It matters once T543's focus-change auto-save lands.

## Change

- `format_range` takes whether the request is for a save. `format_active` passes false and
  `format_buffer` (format on save) passes true; only a save's request checks
  `view.save_edits_allowed(cx)` before applying. The version and path checks stay for both.
- Nothing changes in fe-gpui, in `FormatOnSave`, or in `lsp_format_save_tests.rs` / `auto_save_tests.rs`
  expectations: a save's own format reply arriving after vim entered insert is still dropped.
- New test file `crates/fe-workbench/src/lsp_format_overlap_tests.rs`, declared in `lsp_format.rs` as
  `#[cfg(test)] #[path = "lsp_format_overlap_tests.rs"] mod overlap_tests;` beside `tests` and
  `save_tests`, using `crate::tests::lsp::Lsp::with_fake_clock` as `lsp_format_tests.rs` does.
  `a_manual_format_replying_during_an_auto_save_hook_applies` runs the failing case above and checks the
  manual format's edits are in the buffer and that no toast shows; it fails at the base.
  `a_save_format_replying_after_insert_is_still_dropped` checks the same timeline with only the save's
  own format request (no manual one) leaves the text as it was.
