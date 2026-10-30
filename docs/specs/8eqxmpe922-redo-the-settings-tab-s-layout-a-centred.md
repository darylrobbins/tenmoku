---
task_uid: "01m646py8ymkvpsg8eqxmpe922"
task: "T1237"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-workbench test settings_editor::layout_tests::first_heading_is_at_the_top: the first group heading's top is within 40px of the list's top (at the base it is ~100px lower: every Row::Group is a ROW_HEIGHT=126px row with its text bottom-aligned)"
    required: true
    manual: false
  - id: "C2"
    text: "settings_editor::layout_tests::heading_is_shorter_than_a_setting: a group heading row is under 48px tall while a setting row is taller (rows are variable height)"
    required: true
    manual: false
  - id: "C3"
    text: "settings_editor::layout_tests::content_column_is_capped_and_centred: in a 1600px-wide window the rows' content is at most 760px wide and centred; at 500px wide it fills the width minus gutters, no horizontal scroll"
    required: true
    manual: false
  - id: "C4"
    text: "settings_editor::layout_tests::title_and_control_share_a_line: a Bool, Enum and Int row's control sits right of its title block (control left edge \u003e title left edge + 200px) at 1000px wide, and below it at 480px wide"
    required: true
    manual: false
  - id: "C5"
    text: "settings_editor::layout_tests::category_nav_scrolls_and_follows: a nav entry per top-level group (Editor, Chat, Terminal, ...) lists with its count; clicking one scrolls its heading to the top; scrolling the list highlights the group in view; with a search active the nav shows only groups with hits. Known limit, not in scope here: a navigation spacer kept across a resize that narrows and shortens the window, then scrolling to the end, can leave the list blank (review F5; fixed by T1247)"
    required: true
    manual: false
  - id: "C6"
    text: "settings_editor::layout_tests::hover_shade_stays_inside_the_column: a hovered row's shaded bounds equal the content column's, not the tab's full width"
    required: true
    manual: false
  - id: "C7"
    text: "only visible rows are built: the existing settings_editor_tests only_rows_in_view_are_built (adapt to variable heights: gpui list with ListState, not uniform_list) passes; key navigation (up/down scrolls the selected row into view, Enter, Space, /) and every existing settings_editor_tests / settings_editor_controls_tests pass, with expectations changed only where they depend on ROW_HEIGHT"
    required: true
    manual: false
  - id: "C8"
    text: "cargo test -p fe-workbench settings_editor passes; cargo clippy -p fe-workbench clean"
    required: true
    manual: false
  - id: "C9"
    text: "Manual (screenshot in TESTING.md style): the Settings tab at 1400px and 700px wide, light and dark, looks tidy; the first section is directly under the scope tabs"
    required: true
    manual: false
---

# Redo the Settings tab's layout: a centred content column, section headers with no empty gap, title and control side by side, left-aligned category navigation, a hover shade that fits the column

## Problem (settings_editor.rs)
- The empty gap: the list is a `uniform_list` (`render`), so every `Row::Group` heading is built at the same `ROW_HEIGHT` (126px) as a setting, with its text `justify_end` (`render_rows`). The first heading therefore sits ~100px down.
- One long full-width column; every row is title, grey dotted key, 2 description lines, then the control stacked below (setting_row), with fixed 2-line description and a reserved error line (`MESSAGE_HEIGHT`), which is why spacing feels crowded yet empty.
- Hover/selected shade fills the full tab width (`row_hover`/`row_selected` on the row div, which is `w_full`).
- No category navigation; search and User/Workspace tabs are the only chrome.
## Change (in this order, one PR)
1. Variable-height list: swap `uniform_list` for gpui `list` + `ListState` (no existing use in crates; check the vendored gpui for `list`, or fall back to a second sticky heading layer), keep `take_built`/`built` test hooks working. Heading row ~40px, section subtitle from `SettingGroup.description`.
2. Content column: max width ~760px, centred (`mx_auto`), 24px gutters; rows get a hairline separator rather than a boxed look; the shade and the 2px modified bar are drawn inside the column.
3. Row layout as VS Code/Zed: left block = bold title, description (up to 3 lines, no fixed height, muted, the dotted key only on hover or as a small trailing copyable label), right block = the control, vertically top-aligned; wraps under at narrow widths. Number inputs get a fixed ~96px width and right alignment. The error message line is rendered only when present.
4. Category nav (decided by the user, 10-29): a slim left column (~180px) listing the top-level groups with counts, hidden below ~700px, highlight follows scroll (VS Code's table of contents). No sticky section headers. Build the nav from `SettingsModel::groups()` and the same scroll handle as the list.
5. Header strip: search, scope tabs and the Open settings.toml button (T1231, landed) on one line.
Do not change controls' behaviour, write path, or key handling. Keep tests small.
