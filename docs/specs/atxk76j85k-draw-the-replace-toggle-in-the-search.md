---
task_uid: "01m69aet3g4x53raatxk76j85k"
task: "T383"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The search panel's replace toggle (search_panel.rs:2064, debug selector search-replace-toggle) and the find bar's (find_bar.rs:606, find-replace-toggle) show Icon::ChevronRight when replace is hidden and Icon::ChevronDown when shown, drawn as an svg at the same icon size and muted colour as the search panel's toolbar icons (refresh, clear, tree view, collapse), vertically centred on the input row; no ▸/▾ text glyph remains in either."
    required: true
    manual: false
  - id: "C2"
    text: "Clicking either toggle still shows/hides replace, keeps focus where it was (find bar's prevent_default stays), and keeps its on/off state and hover feedback; the chevron's hit area is at least as large as before. If kiln-ui's ToggleButton cannot take an icon, use IconButton or add an icon constructor to ToggleButton."
    required: true
    manual: false
  - id: "C3"
    text: "search_replace_tests and find_replace_tests (any test that clicks the toggles) pass; a test in each asserts the toggle renders the right chevron icon for both states; cargo test -p fe-workbench -p kiln-ui and clippy clean; screenshot examples run."
    required: true
    manual: false
---

# Draw the replace toggle in the search panel and the in-file find bar as a chevron icon at the toolbar icons' size, not a tiny ▸/▾ text glyph (Mac T1253)

## Problem
The user, with a screenshot of the Search sidebar: "Let's fix this tiny triangle." The replace toggle left of the search input is a ToggleButton with the text "▸"/"▾": a tiny glyph, much smaller and dimmer than the toolbar icons above it. The in-file find bar (find_bar.rs:606) uses the same glyph.
## Change
Use kiln-ui's chevron icons (assets/icons/chevron-right.svg / chevron-down.svg, Icon::ChevronRight/ChevronDown, already used at search_panel.rs:1292 for result groups) in both toggles, sized like the panel's toolbar IconButtons (search_panel.rs:1991). VS Code shows a chevron here too.

## Implementation notes

The chevron click on a hidden replace row shows it without moving focus (explicit show_replace commands still focus the replace field). The fe-gpui and fe-workbench screenshot examples build and run on Linux but print "skipped: the goldens are rendered with Metal, on macOS only"; neither scene draws the replace toggle, so no golden changes.
