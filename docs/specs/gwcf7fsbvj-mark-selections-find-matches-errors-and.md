---
task_uid: "01m5f7gx87hx9wvmgwcf7fsbvj"
task: "T1035"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Selections (interior rows to row end), counted find matches, errors (minimap_error) and warnings (minimap_warning) paint at their rows and columns under the code tiles; carets, information and hint diagnostics, and a stale find count draw nothing (minimap_marks_tests.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "Git row changes paint in a lane at the minimap's inner edge: added/modified bars and deletion marks in minimap_gutter_added/_modified/_deleted"
    required: true
    manual: false
  - id: "C3"
    text: "Only marks on the minimap's shown rows are built (a 100k-match find stays bounded), and a caret move, find count or diagnostics update rebuilds no tile"
    required: true
    manual: false
  - id: "C4"
    text: "minimap_marks screenshot golden committed and listed in TESTING.md; other goldens unchanged"
    required: true
    manual: false
---

# Mark selections, find matches, errors and warnings, and git added, modified and deleted rows on the minimap in their minimap theme colours

# Minimap 4/5: selections, find matches, git changes and errors and warnings on the minimap

VS Code marks the minimap with selections (minimap.selectionHighlight), find matches
(minimap.findMatchHighlight), errors and warnings (minimap.errorHighlight / warningHighlight) at their
columns, and git changes in a gutter lane (minimapGutter.addedBackground / modifiedBackground /
deletedBackground). Builds on cards 2-3 and card 1's theme colours. Mac-only (paint, golden).

## Sources (read directly; no new DecorationKind)

- Selections: `model.selections()`; every selection, the primary included, at its columns; interior rows
  of a multi-row selection to the row's last column (clipped at `max_column`). Carets alone draw nothing.
- Find matches: the find bar's counted ranges (`Find::counted`, up to 10,000, at the text's version only:
  a stale count draws nothing). The current match the same colour.
- Diagnostics: `model.diagnostics()` errors in `minimap_error`, warnings in `minimap_warning`, at their
  columns (a zero-width one one column wide); information and hints not drawn, as VS Code.
- Git: `model.row_changes()` in a lane at the minimap's inner edge, a few px wide like VS Code's minimap
  gutter: added and modified rows as bars, a deletion as a short mark at its edge, in
  `minimap_gutter_added/_modified/_deleted`.

A generic "this decoration also shows on the minimap" option on decoration providers (the two specs that
deferred "minimap marks", docs/specs/8rqy3ehvbx-… and zwca0fhghe-…) is out of scope. `Find`'s counted
ranges get a `pub(crate)` accessor in view/find.rs.

## Paint

Marks for the rows the minimap shows only (binary search sorted sources; the diagnostics list filtered by
row), as quads under card 3's transparent tiles so the code shape stays visible on top, the git lane and
the slider over them. They change often (every caret move), so they are not baked into tiles and never
force a tile rebuild. Selection changes, find counts landing and diagnostics arriving repaint without
rebuilding any tile.

## Screenshot

A `minimap_marks` scene in crates/fe-gpui/examples/screenshots/scenes.rs: a multi-row selection, a find
pattern with several matches, one error and one warning, and added, modified and deleted rows (a
`set_diff_base` base text and the model's diagnostics API), slider `always`; commit its golden and add it to TESTING.md's screenshot list.

## Tests (new file)

crates/fe-gpui/src/element/minimap_marks_tests.rs: each source lands on the right rows and columns in
its colour; marks off the shown rows aren't built; a 100k-match find draws only the counted, shown ones;
a caret move rebuilds no tile; a stale find count draws nothing; information diagnostics draw nothing.
