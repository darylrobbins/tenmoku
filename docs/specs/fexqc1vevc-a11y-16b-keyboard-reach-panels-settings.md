---
task_uid: "01m7595awxan3ctefexqc1vevc"
task: "T584"
rev: 6
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Each panel gap in section 7's table is reachable by keyboard as it specifies: the settings and shortcuts editors, git log, git panel, commit diff, AI changes and search panel. In the search panel a file's identity for keyboard selection, Dismiss and Replace is its path (the PathBuf, or rel_path(file).to_owned()), never FileMatches.rel or another lossy display label, so files whose lossy labels collide stay distinct (review F6); a keyboard-dismissal regression test with distinct non-UTF-8 names whose lossy labels collide (b'\\x80.txt', b'\\x80a.txt', b'\\x81.txt': dismissing the middle one selects the next, not the first) covers it, gated #[cfg(target_os = \"linux\")] because APFS refuses non-UTF-8 names"
    required: true
    manual: false
  - id: "C2"
    text: "Tests in crates/fe-workbench/tests/a11y_reach_panels.rs and crates/fe-keymap/tests/shortcuts_model.rs drive each by keys and pass"
    required: true
    manual: false
  - id: "C3"
    text: "The finish notes list this card's section 12 human-check lines (H-numbers and text), for the A11Y-11 VoiceOver pass"
    required: true
    manual: false
---

# A11Y-16b Keyboard reach, panels: settings and shortcuts editors, git log and panel, commit diff, AI changes and search (T294 design) (Mac T1679)

# A11Y-16b Keyboard reach: panels

Spec: `docs/decisions/qenw9xxdzk-accessibility-design.md`, the **A11Y-16b Keyboard reach: panels** row of section 11 "Proposed card breakdown", and section 7 "Keyboard reachability" (the panel rows).
The user approved the design's platform path (section 3, option (a): gpui-pre 0.3.9's
AccessKit, no vendoring) and its first slice (section 10) as written on 2026-11-09.

Build what that row and those sections specify, with the tests section 9 "Testing strategy"
assigns (shape 1 for text elements, shape 2 for chrome). If the design's text differs from
this card's criteria, the design wins.

Platform: **Linux-OK**.

## Implementation notes

Each panel gap of section 7's decision table, as built. New commands are
typed (`#[kiln::command]` on the panel, in its key context) or, for the
search panel, in its registry, so the palette lists them while the panel
has focus; none has a key of its own but next and previous change in a
commit diff, where a sibling surface (the diff view) binds them.

| Gap | Built | Tests |
|---|---|---|
| Settings: text-list items after the first; Add Item; Remove item | In a list's item, Up and Down focus the item before or after it. `settings.add_list_item` ("Settings: Add List Item") adds an item, focused, to the list whose item has focus, else to the selected row's list; `settings.remove_list_item` removes the focused item and focuses the item now in its place, else the one before, else the list. Neither acts where the scope's file can't set the setting | `a11y_reach_panels.rs` `settings::*` |
| Settings: Reset to Default; User/Workspace scope tabs | `settings.reset_to_default` resets the selected row where its Reset shows (the scope's file sets it, and may); `settings.switch_scope` switches You ↔ This project (a folder open) | `a11y_reach_panels.rs` `settings::*` |
| Shortcuts editor menu items | With a line's menu open, Up and Down move its highlight over the items it doesn't refuse, wrapping, from none to the first or the last, and Enter chooses it; the pointer moves it too. `shortcuts.copy_command_id`, `shortcuts.reset_binding` and `shortcuts.show_same_keybindings` do the item on the selected line, where its menu offers it | `a11y_reach_panels.rs` `shortcuts::*` |
| Git log right-click menu | Up and Down move the menu's highlight (kiln's `MenuPanel::highlighted`), wrapping; Enter chooses it, Escape still closes | `a11y_reach_panels.rs` `git_log::the_menu_is_chosen_from_by_keys` |
| Git log filter field; filter clear | `git_log.focus_filter` ("Git: Focus Log Filter") focuses the box, its text selected, as `/` does with vim on. Escape in the box empties it and lists every commit, the keys staying there; Escape in the empty box gives them to the list | `a11y_reach_panels.rs` `git_log::the_filter_is_reached_and_left_by_keys`; `git_log_filter_tests.rs` updated |
| Git log commit card web link | `git_log.open_commit_link` ("Git: Open Commit on Remote") opens the selected row's commit's page as the card's link does, or says in a toast that its remote has none | `a11y_reach_panels.rs` `git_log::the_commit_page_opens_by_keys` |
| Git panel per-row Stage/Unstage/Discard; header buttons | `git_panel.stage_row`, `git_panel.unstage_row` and `git_panel.discard_row` ("Git: Stage/Unstage/Discard Selected Row") do on the selected row what its checkbox and Discard do (vim's `s`, `u`, `X`): a Merge row stages after asking about markers, and isn't unstaged or discarded. The header buttons' are `git.*` commands already | `a11y_reach_panels.rs` `git_panel::*` |
| Commit diff: previous/next change, collapse/expand all | The diff view's `diff.next_change` / `diff.previous_change` are bound in `CommitDiffView` with its keys (F7, Shift-F7, Alt-F5, Shift-Alt-F5), and the view runs them as its toolbar buttons. For the palette (the diff commands' registry is the diff view's context): `commit_diff.next_change`, `commit_diff.previous_change`, `commit_diff.collapse_all`, `commit_diff.expand_all` | `a11y_reach_panels.rs` `commit_diff::*`; keys in `shortcuts_model.rs`, `git_keys.rs` |
| Commit diff: section toggle; "N hidden lines" | The view has no caret, so "the caret's section" is the one navigation (F7, a reveal) last went to while it shows, else the top row's. `commit_diff.toggle_section` toggles it, keeping its header in view; `commit_diff.expand_hidden` ("Commit Diff: Show Hidden Lines") shows its nearest `N hidden lines` fold below that row, else above | `a11y_reach_panels.rs` `commit_diff::*` |
| AI changes: refactor-preview Apply/Cancel | `ai_changes.apply_preview` / `ai_changes.cancel_preview`, while a preview shows | `a11y_reach_panels.rs` `ai_changes::*` |
| AI changes: per-file Accept/Reject | `ai_changes.accept_file` / `ai_changes.reject_file` on the selected file row, where it has the buttons (Accept only while something's left to decide) | `a11y_reach_panels.rs` `ai_changes::*` |
| Search panel per-file Replace; dismiss × | `search.replace_in_file` ("Replace in Selected File", while the replace field shows) and `search.dismiss_file` ("Dismiss Selected File") on the selected row's file, a line's file, or folder, as its buttons; after a dismissal the row in its place is selected | `a11y_reach_panels.rs` `search_panel::*` |

Test hooks: `Workbench::show_ai_changeset` and `Workbench::preview_file_edits`
(`#[doc(hidden)]`, in `ai_changes_panel.rs`) and fe-lsp's
`FileEdits::closed` (test-support only) let the integration test show a
turn's changes and a refactor's preview with no chat or language server;
Graph rows have a `git-log-row:<n>` debug selector.

Carried findings fixed:

- T560 F1: the shortcuts search's and the recorder's When placeholders are
  keyed `Text`, resolved as drawn, so they and the inputs' accessible names
  follow a change of language with the inputs' state and focus kept.
  Regression: `a11y_reach_panels.rs`
  `shortcuts::the_menu_is_chosen_from_by_keys` (a fake-locale switch with
  the recorder open).
- T569 F7: the Settings tab observes `ActiveLocale` and reads its model
  again in the new language, keeping what's typed; the rows' Common
  heading and group titles, the help, and an open dropdown's descriptions
  (refreshed by value as each read lands) follow. A refused input keeps
  the text it refused, and its message is found again from it as each read
  lands (a change of language reads the files again), so it follows too. Regression: `settings_editor_locale_tests.rs`.

Scope widened by respec for `git_log_filter_tests.rs` (the two-step
Escape), `shortcuts_recorder.rs` (T560 F1), `settings_editor_locale_tests.rs`
(T569 F7's test), `fe-lsp/src/workspace_edit.rs` (`FileEdits::closed`) and
`fe-keymap/tests/git_keys.rs` (its count of `diff.*` bindings).

### Review round 1 (fvcq56)

- F1: switching the scope while a row's input has focus (by key or the switch) gives focus to the list of rows, as the inputs are made again; a reset that drops the focused list item (its default is shorter) focuses an item left, else the list, by key or Reset's click. Regression: `settings::focus_survives_a_scope_switch_and_a_reset_from_an_item`.
- F2: each list's items scroller has a tracked `ScrollHandle`; Up and Down between items, Add Item and Remove List Item scroll the focused item into view. Regression: `settings::the_focused_item_scrolls_into_view` (twelve items).
- F3: Dismiss Selected File from a match row takes its file's row as the place to select, so the file after it is selected however many matches the dismissed one had. Regression: `search_panel::dismissing_from_a_match_selects_the_next_file`.

### Review round 2 (8wkmzt)

- F4: Enter on a list's row focuses its first item through the same focus-and-reveal helper as Remove List Item and Reset (`focus_item_or_list`), so it scrolls back into view. Regression: `settings::the_focused_item_scrolls_into_view` (down to the end, Escape, Enter).
- F5: Dismiss Selected File notes the node after the dismissed one and its subtree by what it is (a file's or folder's path from the root) and selects it again once the rows are rebuilt, so a folder left empty and removed doesn't shift the selection; else the old place. Regression: `search_panel::dismissing_a_folders_last_file_selects_the_next_folder`.

### Review round 3 (ax2ayt)

- F6: a search result file's identity, which a dismissal finds the next node by, is its path (`FileMatches::path`), not its lossy label `rel`, so files whose names aren't UTF-8 and share a label stay apart. Regression (Linux only, as APFS refuses such names): `search_panel::files_whose_labels_collide_stay_apart` (`\x80.txt`, `\x80a.txt`, `\x81.txt`: dismissing the middle one selects the last).

### Section 12 human checks for A11Y-11

- **H11′. Command reach, keyboard only, VoiceOver off.** For every row of section 7's decision table owned by A11Y-16a or A11Y-16b, run its command from the palette (or its key) and confirm it does what the mouse control does. Deferred rows are skipped. Escape closes, and Return confirms, in every in-window modal in section 7's modal table, including the git log and shortcuts menus. (This card's rows: Settings: Add List Item / Remove List Item / Reset to Default / Switch Scope and Up/Down between a list's items; Keyboard Shortcuts: Copy Command ID / Reset Keybinding / Show Same Keybindings and the line menu's arrows and Return; the Graph menu's arrows and Return, Git: Focus Log Filter with Escape twice, Git: Open Commit on Remote; Git: Stage/Unstage/Discard Selected Row; F7/Shift-F7 and Commit Diff: Collapse All / Expand All / Toggle Section / Show Hidden Lines; AI Changes: Apply/Cancel Preview, Accept/Reject Selected File; Replace in Selected File, Dismiss Selected File.)
- **H11a′. Focus and focus ring, keyboard only.** Only the surfaces that take focus are checked: a toast after "Toast: Focus" (Tab between its buttons, Space and Return press), list and tree rows, text fields, and the view-stack headers. Each shows the focus ring. Other buttons take no focus in the first slice (Q3), so no ring is expected on them. Repeat with Full Keyboard Access on (as H11a): Space presses a focused toast button, and Space still types in the editor and text fields. (This card adds no focusable control; its panels' rows and text fields — the settings list items, the Graph's filter — are checked here.)
