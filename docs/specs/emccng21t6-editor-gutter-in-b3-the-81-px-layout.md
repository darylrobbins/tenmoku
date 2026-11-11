---
task_uid: "01m6vvvadakanpckemccng21t6"
task: "T1549"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in gutter_b3 (cargo nextest run -p fe-gpui --test gutter_b3) pass: at 13 px the gutter is 81 px (numbers 38, dot 10, fold 16, change bar 3, gap 10) and scales with the font; the text's x does not move when the pointer enters the gutter or a fold toggles; chevrons are drawn only while the pointer is in the gutter (folded rows always); the hovered fold's closing row has the up-chevron; known limit: a final else or elseif body inside a Lua or PHP alternative-syntax branch wrapper (its end/endif belongs to the enclosing if) still puts the closing up-chevron on the body's own last row; that case is T1724 (T1549 review F1) and is not required here"
    required: true
    manual: false
  - id: "C2"
    text: "gutter_b3::agent_lines_draw_a_dotted_agent_lane and gutter_b3::removed_lines_draw_a_notch pass; indent guides only inside leading whitespace, every 4 columns, the caret's block one step brighter"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-gpui --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist); cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (every fe-gpui golden with cargo run -p fe-gpui --example screenshots, and workbench.png)"
    required: true
    manual: false
---

# Editor gutter in B3: the 81 px layout, problem dot, a fold column always reserved, hover-only chevrons with the closing up-chevron, the change bar's removed notch and dotted agent lane, indent guides at 4 columns

# Editor gutter, folding and indent guides in B3

Spec: `docs/design/design-system.md#editor` (Gutter, Folding, Indent guides)
and `#agent-provenance` (the dotted agent lane).

## Change (fe-gpui painter, `crates/fe-gpui/src/element.rs` and helpers)

- Gutter left to right at 13 px code, 81 px in all: line numbers 38 px
  right-aligned (faint; the caret's row ink); problem dot 10 px (kiseto
  warning, shinsha error); fold column 16 px, always reserved so code never
  shifts; change bar 3 px (ruri modified, celadon added, a shinsha notch for
  removed); 10 px to the code. Widths scale with the font size.
- Agent-written lines (the provenance data `crates/fe-workbench/src/provenance_column.rs`
  already has) draw as a dotted `agent.base` lane in the change bar instead of
  the "AI · agent" annotation column.
- Folding: chevrons on every block opener only while the pointer is in the
  gutter (folded rows keep theirs); the hovered one at full strength on a hover
  tile (`fold_hover`), its closing line a small up-chevron, its indent guide one
  step brighter. No range lines.
- Indent guides: 1 px at every 4 columns inside leading whitespace only, the
  caret's block (or a hovered fold's) one step brighter
  (`crates/fe-gpui/src/element/indent_guides.rs`).
- Files: `element.rs`, `element/indent_guides.rs`, `view/change_bar.rs`,
  `gutter_annotations.rs`, `crates/fe-workbench/src/provenance_column.rs`.
  `element.rs` is also in T1501–T1507's scopes: expect to wait for them.
- Regenerate every fe-gpui golden (they all show the gutter) and
  `workbench.png` on the Mac.

## i18n

fe-gpui's text is already in its catalogue (card G1). Any text this card adds
or rewrites goes to `crates/fe-gpui/l10n/en-US/` as keys; the crate's lint
fails otherwise.

## i18n

This card is the one that moves these files' text into the catalogue
(`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4–5 and 10): every
user-visible string in `provenance_column.rs`, the new ones and the old, goes to
`crates/fe-workbench/l10n/en-US/editor.ftl` (keys `workbench-editor-…`), through `tr!` / `text!`, whole messages, plural
selectors for counts; add the file to `resources!` in the crate's
`src/l10n.rs` and the files to `l10n-allow.toml`'s `migrated` list. Text the
design rewrites follows the design; text it keeps stays byte-equal. Tooltips
and accessible names count.

Platform: **Mac-only** (goldens).

## Implementation notes

- **Widths.** The five widths the spec lists sum to 77 px; the other 4 px
  is an inset left of the line numbers (the 4 px layout grid), so the gutter
  is 81 px at 13 px: inset 4, numbers 38 (right-aligned), problem dot 10,
  fold column 16, change bar 3, gap 10. Each scales by `font_size / 13`,
  rounded to whole pixels (`element.rs` `gutter_columns`). The numbers'
  column grows once the digits and an 8 px pad need more than 42 px (five
  digits at 13 px).
- **fe-display.** The gutter's geometry is laid out in
  `crates/fe-display/src/layout.rs`, outside the listed files. It gained a
  `Gutter` (the columns' widths in pixels), `layout_frame_with_gutter_and_hints`,
  `wrap_width_with_gutter` and `Frame::problem_column`; the em-based entry
  points keep their layout as `Gutter::ems`, so fe-display's own tests are
  unchanged.
- **Hover.** The chevrons show while the pointer is anywhere in the gutter
  (right of any annotation column, left of the code). A window counts as
  hovered only once a pointer event has placed the pointer in it, since a
  headless or offscreen window reports its corner (now inside the gutter).
  The hovered toggle is the one whose fold-column cell holds the pointer; a
  move onto another toggle draws again.
- **Closing row.** fe-syntax's candidates stop before a row their closing
  delimiter starts (`}`, `end`, `fi`, `*/`, a closing tag or fence), so it
  now reports that row too: `fe_syntax::fold_candidate_closing` (in
  `crates/fe-syntax/src/fold.rs`, outside the listed files) gives the
  innermost candidate a row heads and its node's last row with text, where
  the up-chevron goes; for a body whose closer is its parent's (Lua's `end`,
  Swift's `}` after a control-flow body, PHP's `endif`), the row of the
  closer right after it, when that closer starts its row
  (`fold/closing_tests.rs`, run with `--features php,swift` too). A server's range says nothing of a closer: its
  up-chevron goes on the range's last row. A closing row that heads a fold
  of its own (`} else {`) keeps its own chevron instead.
- **Agent lane.** `EditorView::set_gutter_annotations_with_agent_lane` takes
  the agent's rows with the annotations, anchored at their version and
  blanked on edited rows as entries are. Provenance gives agent rows no
  entry. The lane replaces the change bar on those rows: circles a bar wide,
  one every two bar widths, phased by document y so they don't crawl when
  scrolling.
- **Indent guides.** Every 4 columns whatever the file's indent (a 2-space
  file gets guides at 0 and 4, a file of 8-wide tabs two per tab), only
  inside leading whitespace: a display row whose text starts at or left of
  a guide, such as a soft-wrapped row's continuation, breaks it.
- **Removed notch.** Twice the bar's width, two-thirds of it tall, centred
  on the row edge, with round ends. A click within a quarter line of the
  edge (plus 3 px) still hits it, as the old wedge's did.
- **Tests outside the listed files** pinned the old geometry: the wedge
  (`element/change_bar_tests.rs`, `change_rail_tests.rs`,
  `decorations_tests.rs`, `deletion_wedge_tests.rs`), the fold-column-only
  hover (`view/fold_hover_tests.rs`), the glyph's 2 em slot
  (`view/gutter_glyph_tests.rs`), guides per indent level and through
  wrapped continuations (`element/indent_guides_tests.rs`) and the AI entry
  (`fe-workbench/src/provenance_column_tests.rs`). The painters for the
  notch (`element/decorations.rs`) and the sticky scroll band's line
  numbers (`element/sticky_scroll.rs`) also changed.
