# Accessibility audit: what screen readers see today

Spike T389. No production code changed; `crates/fe-workbench/tests/a11y_tree_dump.rs`
makes the baseline reproducible. Mac follow-up: T1266.

## Summary

- fastedit exposes **nothing** of its own to assistive technology.
  `rg -n 'a11y|accesskit|\.role\(|aria_' --glob '*.rs' crates kiln` is empty.
  Every surface has no `.id()` + `.role()` pair, so gpui-pre adds no nodes for it.
- What a screen reader gets is what gpui-pre builds without help: a root
  `Role::Window` node labelled with the window title (see the platform setup in
  `window.rs`, `initial_root_node`). Nothing below it.
- **`Window::debug_a11y_tree_json` yields no tree in a headless test window.**
  Measured by the new test: after `activate_window()`, focus, `run_until_parked()`
  and a draw, `Window::is_a11y_active()` is `false` and the dump is `None`. The
  tree is built only while the platform's screen-reader activation callback has
  set the active flag, and the test platform never fires it (`A11y::sync_active_flag`
  reads that flag in `draw_roots`). So the per-surface tree cannot be dumped
  headless; it has to come from the running app. That makes the dump hook (card 1)
  the first follow-up.

## Baseline

"Today" means gpui-pre 0.3.9 with no fastedit a11y code. For every surface the
dump cannot reach (headless gives no tree), the reason is the same unless noted:
**no tree headless**. What each surface exposes is read from the source: with no
role, nothing.

| Surface | What the tree exposes today | Source |
|---|---|---|
| Window | Root `Window` node with the window title as label, plus whatever the platform adapter adds (title bar, close/zoom buttons). No landmarks, no regions. Dump: not reachable headless. | `crates/fe-workbench/src/workbench.rs`, `kiln/crates/kiln-ui/src/layout.rs` |
| Project tree | Nothing. Rows are divs without id+role, so no `Tree`/`TreeItem`, no expanded/selected state, no level or set position. Not reachable headless. | `crates/fe-workbench/src/project_panel.rs`, `kiln/crates/kiln-ui/src/tree.rs`, `tree_row.rs` |
| Tabs | Nothing. No `TabList`/`Tab`, no selected state, no dirty/pinned marker, no close action. Not reachable headless. | `kiln/crates/kiln-ui/src/tab_bar.rs`, `tabs.rs` |
| Editor | Nothing. `EditorElement` paints shaped lines straight to the scene; there is no `TextInput`, value, selection, caret or text runs. A screen reader announces the whole window on focus. Not reachable headless. | `crates/fe-gpui/src/element.rs` |
| Terminal | Nothing. `TerminalElement` paints the grid; no `Terminal` role, no text, no live region for output. Not reachable headless. | `crates/fe-terminal-view/src/element.rs` |
| Chat | Nothing. Transcript, composer and message actions have no roles; new streamed text is never announced. Not reachable headless. | `crates/fe-chat-view`, `crates/fe-workbench/src/chat_panel.rs` |
| Find bar | Nothing. Query field (`TextLine`) has no role or value; match count and toggles (case, word, regex) have no labels or toggled state. Not reachable headless. | `crates/fe-workbench/src/find_bar.rs`, `kiln/crates/kiln-ui/src/text_field.rs` |
| Settings | Nothing. Controls (toggles, dropdowns, text fields, search box) have no roles, labels or values. Not reachable headless. | `crates/fe-workbench/src/settings_editor.rs`, `settings_editor_controls.rs` |
| Toasts | Nothing, and nothing announces them: no live region, so a message appears visually and is never spoken. Not reachable headless. | `crates/fe-workbench/src/toast.rs` |

Reproduce: `cargo test -p fe-workbench --test a11y_tree_dump` pins the headless
result (no tree; and if a gpui-pre upgrade starts producing one, that it parses
and has a root). `cargo test -p fe-workbench --test a11y_tree_dump -- --ignored --nocapture`
prints the JSON, or "no a11y tree in this window".

## API surface

gpui-pre 0.3.9 ships AccessKit. Guide: `src/_accessibility.rs`; example `examples/a11y`.

| Builder or hook | Use | Surface needing it |
|---|---|---|
| `.id(..)` + `.role(Role)` (`elements/div.rs`) | A node exists only with both | Every surface |
| `.aria_label`, `.aria_description`, `.aria_keyshortcuts` | Names, hints, shortcut announce | Rail buttons, tabs, find toggles, toasts |
| `.aria_selected`, `.aria_expanded`, `.aria_toggled` | State | Tree rows, tabs, find/settings toggles |
| `.aria_level`, `.aria_position_in_set`, `.aria_size_of_set` | Tree/list structure | Project tree, tab list, chat list |
| `.aria_row_*`, `.aria_column_*` | Grids | Settings, shortcuts tables |
| `.aria_value`, `.aria_numeric_value*`, `.aria_placeholder` | Field values | Settings, find query, kiln text fields |
| `.aria_orientation` | Orientation | Tab bar, split panes |
| `.aria_active_descendant()` | Focus stays on a list, a child is "current" | Palette, completion list, project tree |
| `.on_click` auto-registers `Action::Click`; `Window::on_a11y_action` | Respond to screen-reader actions | Tabs (close), buttons, tree (expand) |
| `Element::a11y_role`, `write_a11y_info` | Custom elements set role and any `accesskit::Node` property | All four custom elements; toast live region |
| `Element::a11y_synthetic_children` + `A11ySubtreeBuilder` | Add `TextRun` children and set `TextSelection` on the parent | Editor, viewer, terminal, `TextLine` |
| `Window::is_a11y_active` | Skip costly work when no assistive tech is attached | Editor/terminal text runs |
| `Window::debug_a11y_tree_json` | Dump the last `TreeUpdate` | Dump hook |

What a code editor lacks from the gpui side:

- **Text runs for a viewport, not the file.** AccessKit's `TextRun` carries `value`,
  `character_lengths`, `word_lengths` and per-run bounds; `TextSelection` carries
  an anchor and focus `TextPosition { node, character_index }`. Both are supplied
  by the element in `a11y_synthetic_children`, so we must build runs for the
  visible lines (and decide how far above/below to expose) with correct UTF-8
  character lengths, and map our offsets, folds, wrap rows and inlays to
  `TextPosition`s. Multi-cursor: AccessKit has one selection, so only the primary
  is exposed.
- **No live-region helper.** There is no builder method for `aria-live`; toasts,
  terminal output and the chat stream need `write_a11y_info` on a custom element
  to set `Node::set_live`, or a small announcer element. Verify it forwards to
  VoiceOver on macOS.
- **No caret-moved announcement API**; it follows from selection changes in the
  next tree update.
- **Large content**: a tree update per frame with many text runs needs to be
  limited to the viewport and skipped when `is_a11y_active()` is false.

## Gap list

What a screen reader user cannot do or hear today:

- **Window**: no landmarks (explorer, editor, terminal, chat, status bar); cannot
  jump between regions.
- **Project tree**: cannot hear file names, folder state or git status, or expand/collapse.
- **Tabs**: cannot hear which tab is active, dirty state or switch tabs by name.
- **Editor**: cannot read text, hear the caret line or selection, or know the
  language/mode; the editor is effectively a silent box. Vim mode, diagnostics,
  completion list, hover and ghost text are also unannounced.
- **File viewer**: same as the editor, read-only.
- **Terminal**: cannot read output, prompt or selection; new output is silent.
- **Chat**: cannot read messages, hear streaming replies or reach actions (apply, retry).
- **Find bar**: cannot hear the query, match count ("3 of 12") or toggle states.
- **Settings**: cannot hear control names, values or toggle states.
- **Toasts**: never announced.
- **Palette and menus**: no list semantics; selection changes are silent.

## Human checks

**Not done.** This card does not require them; they need the running app on macOS.

1. Build and run fastedit (`cargo run`), open a project and a file.
2. Accessibility Inspector (Xcode > Open Developer Tool): use the pointer-inspection
   target on the window, the tab bar, a tree row, the editor and the terminal.
   Record the role, label and value it reports for each; expect only the window.
3. Turn on VoiceOver (cmd-F5). Tab/VO-arrow through the window and note what is
   announced for each surface in the Baseline table.
4. Focus the editor, type, and move the caret. Expect silence; record anything
   VoiceOver says.
5. Trigger a toast (for example save) and a terminal command with output. Note
   whether anything is spoken.
6. Save the Inspector tree and the notes next to this doc to compare with the dump hook output.

## Proposed follow-up cards

Ordered. Sizes are xs-xl.

1. **A11y tree dump from the running app** — type: feature; size: xs.
   Files: `crates/fastedit/src/main.rs` (an env var such as `FASTEDIT_A11Y_DUMP`
   makes it write `Window::debug_a11y_tree_json()` after a frame; T1257 is editing
   this file). Dependencies: none. Needed because headless gives no tree.
2. **Window landmarks and labelled chrome** — feature; m.
   Files: `kiln/crates/kiln-ui/src/layout.rs`, `activity_rail.rs`, `status_bar.rs`,
   `menu_bar.rs`, `icon_button.rs`, `toggle_button.rs`, `crates/fe-workbench/src/workbench.rs`.
   Roles (`Navigation`, `Main`, `Complementary`, `Status`), labels, button names. Depends on 1.
3. **Tabs and project tree semantics** — feature; m.
   Files: `kiln/crates/kiln-ui/src/tab_bar.rs`, `tabs.rs`, `tree.rs`, `tree_row.rs`,
   `crates/fe-workbench/src/project_panel.rs`. `TabList`/`Tab`/`Tree`/`TreeItem`,
   selected/expanded/level/set, active-descendant, click and close actions. Depends on 2.
4. **Find bar, palette and settings controls** — feature; m.
   Files: `crates/fe-workbench/src/find_bar.rs`, `settings_editor.rs`,
   `settings_editor_controls.rs`, `kiln/crates/kiln-ui/src/palette.rs`, `controls.rs`.
   Labels, toggled state, values, match-count status, list semantics. Depends on 2.
5. **Live regions for toasts and status** — feature; s.
   Files: `crates/fe-workbench/src/toast.rs`, `kiln/crates/kiln-ui/src/status_bar.rs`.
   Polite live region via `write_a11y_info`; verify under VoiceOver. Depends on 2.
6. **Text-input a11y for `TextLine` and kiln text fields** — feature; m.
   Files: `kiln/crates/kiln-ui/src/text_field.rs`, `text_input.rs`. The smallest text
   element: `TextInput` role, `TextRun` child, selection and caret in
   `a11y_synthetic_children`; the template for 7 and 8. Depends on 1.
7. **Editor and file viewer text-input a11y** — feature; l.
   Files: `crates/fe-gpui/src/element.rs`, `crates/fe-gpui/src/file_viewer/element.rs`
   (and shared helpers in `crates/fe-gpui/src`). Viewport text runs with folds,
   wrap and inlays mapped to positions, selection and caret, diagnostics, a
   limit on exposed lines, skipped when inactive. Depends on 6.
8. **Terminal a11y** — feature; l.
   Files: `crates/fe-terminal-view/src/element.rs`. Grid rows as text runs, cursor
   as selection, a live region for new output without flooding. Depends on 5, 6.
9. **Chat transcript a11y** — feature; m.
   Files: `crates/fe-chat-view/src`, `crates/fe-workbench/src/chat_panel.rs`. List of
   messages, streaming announcements, action buttons, composer (a `TextLine`). Depends on 5, 6.
10. **VoiceOver pass and fixes** — chore; m. Run the Human checks on the finished
    work and file bugs. Files: this doc (`docs/accessibility-audit.md`, to record
    results) plus whatever fixes the pass finds, expected in the files of cards 2-9
    (`kiln/crates/kiln-ui/src/*`, `crates/fe-workbench/src/*`, `crates/fe-gpui/src/element.rs`,
    `crates/fe-terminal-view/src/element.rs`). Depends on 2-9.

The four custom elements that need text-input a11y, by exact file:
`crates/fe-gpui/src/element.rs` (`EditorElement`), `crates/fe-gpui/src/file_viewer/element.rs`
(`FileViewerElement`), `crates/fe-terminal-view/src/element.rs` (`TerminalElement`),
`kiln/crates/kiln-ui/src/text_field.rs` (`TextLine`).
