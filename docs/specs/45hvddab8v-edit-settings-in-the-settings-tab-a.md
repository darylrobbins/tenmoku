---
task_uid: "01m5n96xqd7zpxrj45hvddab8v"
task: "T296"
rev: 6
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "Bool rows draw a switch (selector setting-switch-\u003ckey\u003e) that toggles on click or Space and writes through settings_edit::set_setting for the selected scope; Enum rows draw a dropdown (setting-select-\u003ckey\u003e) listing the schema variants, Enter or click picks one; Int and Float rows draw a number input that commits on Enter or blur, clamps nothing silently but shows the schema range message under the input and does not write an invalid value; Text rows commit on Enter or blur; TextList rows show one input per item with add and remove buttons and write the whole list. Tests bool_switch_writes, enum_pick_writes, number_commits_on_enter, number_out_of_range_shows_message_and_does_not_write, text_commit, list_add_remove_writes Limit (Known limits, review round 4): (a) pending edits and uncommitted row or list inputs are keyed per setting and scope but are not isolated across target files or a workspace-root change while a write is delayed (follow-ups T307, T308); (b) a dirty input is protected from a re-render only while it has focus, so text typed between a failed write or an external reload and the input's blur may be replaced by the reloaded value (the committed file is never corrupted; follow-up card F17 keeps typed text through the blur); (c) 'Edit in settings.toml' computes the table row from the file on disk, so with the file open and unsaved edits in its editor the cursor may land on a stale row (follow-up card F18 reads the open editor's text). A committed write always goes to the file of the scope and root current when it was accepted, and a failed write never overwrites a newer value. (d) a blur commits only when the final text differs from the value the input first showed, so retyping an unset setting's default (or an inherited list) and blurring writes nothing while Enter writes it (review round 4 F19; follow-up card F19 tracks an edited flag per input)."
    required: true
    manual: false
  - id: "C2"
    text: "A modified row (set in the selected scope) shows a Reset to Default gear/button (setting-reset-\u003ckey\u003e); clicking it calls reset_setting, the marker disappears and the row shows the default; a row at default shows no button. Setting a value equal to its default still writes it when committed by Enter, a switch, a dropdown pick or an edit whose text differs from the shown value (the row stays modified, as VS Code does); committing it by blur alone after retyping the shown value is the documented limit C1(d). Tests reset_removes_the_key, reset_updates_the_row, setting_the_default_keeps_it_modified"
    required: true
    manual: false
  - id: "C3"
    text: "Writes go to the file for the selected scope, user_settings_path() or project_settings_path(root); the tab then shows the written value without waiting for the watcher, and the hot reload that follows changes nothing visible. A write error (read-only file, invalid TOML in the file) shows a toast and the row keeps its old value. Comments and key order in the file are untouched (assert on the file text). Tests user_scope_writes_user_file, workspace_scope_writes_project_file, write_error_toasts_and_keeps_value, file_comments_survive"
    required: true
    manual: false
  - id: "C4"
    text: "Other-kind rows (tables, arrays of tables such as [[vim.remap]]) show 'Edit in settings.toml' which opens the file (workbench.open_settings) and moves the caret to the key's table when present"
    required: true
    manual: false
  - id: "C5"
    text: "Headless GPUI tests run on Linux; typing into inputs goes through the same key path other view tests use (see find_replace_tests.rs); no golden"
    required: true
    manual: false
---

# Edit settings in the Settings tab: a switch for booleans, a dropdown for enums, number and text inputs, a list editor for string lists, Reset to Default, and a link to settings.toml for tables, writing the selected scope's file with comments kept (Mac T1097)

## Problem
Card 3's Settings tab only shows values. VS Code edits in place: a checkbox/switch for booleans, a select for enums, number and text inputs, a list editor for string arrays, a gear with Reset Setting, and Edit in settings.json for complex values. Card 2 provides the TOML-preserving writes.

## Change
- New `crates/fe-workbench/src/settings_editor_controls.rs` (mod declared from settings_editor.rs with `#[path]`): one small function per `SettingKind` returning the control element and the commit callback; `settings_editor.rs` calls it from the row renderer and holds the dropdown/input state keyed by setting key so a re-render keeps focus and a half-typed value.
- The commit path: parse the typed text for the kind, check it with `SettingsModel`/schema range (same message text as the diagnostics show), then `fe_config::settings_edit::set_setting(scope, root, key, value)` on the background executor, then update the in-memory model from the returned text.
- Reset uses `reset_setting`. The Reset button is a small icon from kiln-ui's icon set (`kiln/crates/kiln-ui/src/file_icons.rs` + an svg under kiln-ui/assets/icons if `rotate-ccw` is absent; check with rg).
- The dropdown reuses the workbench's existing popup/picker element if there is one that is not modal-global (see `picker.rs`); otherwise an anchored list in the tab.
- No new settings keys, no change to settings.rs or the schema.

## Tests
New `crates/fe-workbench/src/settings_editor_controls_tests.rs` with the tests named in the criteria, on tempfile config dirs. Linux-OK. Depends on cards 2 and 3.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y54), not truncated.

## Implementation notes
- The dropdown is an anchored list in the tab (`anchored` in a `deferred` under its button): `picker.rs`'s pickers are modal and window-wide. A backdrop over the tab while it's open makes a click anywhere else, its button included, close it.
- Rows grew from 96 to 126 px (`ROW_HEIGHT`): a text input is 30 px tall, and the line under it holds the range message. Rows stay one height, as the list is a `uniform_list`; a list editor's items sit side by side in one scrolling line.
- A change is pending, by key and scope, with a generation and its files, from when it's queued until its write is read back or fails; its row shows it meanwhile, so renders, reloads and further typing don't roll it back, and toggles count from it. A completion only clears its own generation.
- Every read of the files takes a number as it starts (an `AtomicU64` shared with the background reads); a read older than the one shown is dropped, so a write's read finishing after a newer watcher read can't replace it. The watcher's reload after a write reads the same text, so nothing it shows changes.
- The write queue is an app global (`SettingsWrites`), not the tab's: accepted writes finish after the tab closes, and a reopened tab's writes queue behind them. A failed write is logged and toasted from the queue, so it's reported with the tab gone. The global also lists the open tabs: once a write lands, every other tab reads the files again, and the writing tab does too if its root changed mid-write.
- Switching scope drops the row inputs (and their blur subscriptions) rather than marking them stale, so a blur arriving after the switch can't commit old text into the new scope's file.
- An input keeps typed text across renders; the file's value replaces it only when it has no focus or holds nothing typed. Reset rewrites the row's inputs to the reset value at once, so a blur after it commits nothing.
- A settings file that exists but can't be opened for writing is refused before `set_setting`, whose temporary-file rename would otherwise replace a read-only file.
- Table navigation uses `fe_config::parse_layer`'s key spans, so quoted keys work and strings aren't mistaken for headers.
- The list and its rows take the tab's width (`uniform_list` otherwise sizes to its measured row); a list's items scroll sideways beside a fixed Add Item.
- `set_setting` puts the user's file where the environment says (`FASTEDIT_CONFIG_DIR`), while the tab reads it through the tests' `ConfigDirOverride`. So the tests write the workspace file, but for `user_scope_writes_user_file`, which runs itself again in a child process with the variable set, rather than setting it in a multi-threaded test process. Under test, a user-scope write asserts that the two paths agree, so a test can never write the real settings file.
- Scope: added `kiln/crates/kiln-ui/assets/icons/rotate-ccw.svg` (Lucide's, unmodified) for Reset to Default.

## Known limits
Review rounds left medium findings that C1 now records as limits, with follow-up cards that remove them:
- Pending edits are keyed by setting and scope, not by target file (F15, follow-up T307). While a write is delayed, an edit to the same key in a second workspace root after a root change replaces the first root's pending entry. Returning to the first root then shows its file's value until the write is read back, and a toggle there counts from that value. A pending User edit also stops showing across a root change, because the display filter compares both file paths.
- A root change doesn't drop row inputs or their blur subscriptions (F16, follow-up T308). An uncommitted input, or a list item, typed for the old root can survive until the row is next rebuilt; committing it then writes to the new root's file.
- A dirty input is protected from a re-render only while it has focus (F17, follow-up T310). GPUI re-renders with the new focus state before the input's blur callback runs. So text typed between a failed write, or an outside reload, and the input's blur can be replaced by the file's value before the blur reads it. This applies to list items too. The file itself is never corrupted.
- Edit in settings.toml works out the table's row from the file on disk (F18, follow-up T311). With the file already open and unsaved edits in its editor, the caret can land on a stale row, and a table that exists only in the unsaved buffer gets no row.
- A blur commits only when the input's final text differs from the value it first showed (F19, follow-up T312). Retyping an unset setting's default, or an inherited list, then leaving the input writes nothing, while Enter writes it. T312 tracks an edited flag per input so a blur commits any explicit edit.

What holds regardless: a committed write goes to the file of the scope and root current when it was accepted, writes stay in order across tabs, and a failed write never overwrites a newer value.
