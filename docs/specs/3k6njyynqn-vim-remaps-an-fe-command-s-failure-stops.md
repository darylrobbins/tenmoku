---
task_uid: "01m59sr9nb0rhpns3k6njyynqn"
task: "T227"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test vim::remap_catalog_tests::a_failing_editor_command_stops_the_map passes: a map whose fe: editor command fails at runtime does not run the keys after it; and vim::remap_catalog_tests::workbench_panel_commands_are_known passes: a map with fe:project_panel.focus is not E492 once the host catalog is set; both fail at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-gpui -p fe-workbench passes with no existing test edited; cargo clippy -p fe-gpui -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change"
    required: true
    manual: false
---

# Vim remaps: an fe: command's failure stops the map and E492 knows the host's full command catalog (T220 review F5, F9)

## Problem

T220 (Mac T876, `<Cmd>...<CR>` in a vim remap) ships with two known limits (review findings F5 and F9):

- **F5** (`crates/fe-gpui/src/vim.rs:~464`): dispatch acceptance is treated as execution success. An editor or proposal command's runtime failure does not stop the map, because `EditorView::run_command` (`crates/fe-gpui/src/commands.rs`) does not return its result.
- **F9** (`vim.rs:~413`): `known_command` consults only fe-gpui's catalogs plus typed and app commands, so workbench panel registry commands (`project_panel.*`, search) read as unknown (E492).

## Change

Make `EditorView::run_command` return its result and stop the map on a failure; hand the host's full command catalog to fe-gpui (a set-catalog hook plus a call from `crates/fe-workbench/src/lib.rs`) so `known_command` knows every real command. New tests in new files declared from test files.
