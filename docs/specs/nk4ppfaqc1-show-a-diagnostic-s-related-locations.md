---
task_uid: "01m55sbpd45qrapsnk4ppfaqc1"
task: "T168"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test view::diagnostics_tests::diagnostic_related_tests::the_hover_lists_related_locations_and_the_code_link passes (new crates/fe-gpui/src/view/diagnostic_related_tests.rs, declared in view/diagnostics_tests.rs): after the hover delay painted_popover holds the message, a ↳ line for the same-file location (line N) and one for the other file (name:row), and the code with its href"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test problems_panel::problems_panel_tests::problems_related_tests::related_rows_open_their_locations passes (new crates/fe-workbench/src/problems_related_tests.rs, declared in problems_panel_tests.rs): the fake's diagnostic in a.rs with a related range in a.rs and row 3 of unopened b.rs shows two child rows; after an edit above it, clicking the first selects the moved range in a.rs's editor; clicking the second opens b.rs with the caret at row 3's start; select_next steps over child rows"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-editor -p fe-gpui -p fe-workbench passes, any existing test edited only to construct the new Diagnostic fields (named in the finish notes); cargo clippy --workspace --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Show a diagnostic's related locations and its code's link in the hover, and the related locations as clickable rows under it in the Problems panel, as VS Code does (Mac T916)

## Problem

Once the card this depends on lands, `fe_lsp::Diagnostic` carries a diagnostic's related locations and its
code's documentation link (`related`, `code_description`). The workbench still drops both:

- `crates/fe-workbench/src/lsp.rs` `diagnostic()` (`:1975-1997`) builds an `fe_editor::diagnostics::Diagnostic`
  (`crates/fe-editor/src/diagnostics.rs:62-69`), which has only severity, message, source, code and tags.
- So the diagnostic hover (`crates/fe-gpui/src/hover.rs` `popover_text`, messages a line each) and the
  Problems panel (`crates/fe-workbench/src/problems_panel.rs`, one row per problem) show neither.

VS Code lists related information under the message in its hover and as child rows in Problems, and links
the code to its `href`. Gap audit 2026-10-17.

## Change

1. `crates/fe-editor/src/diagnostics.rs`:
   - `Diagnostic` gains `pub related: Vec<RelatedInfo>` and `pub code_href: Option<String>`.
     `Diagnostic::new` leaves them empty.
   - New `RelatedInfo { pub path: Option<PathBuf>, pub row: u32, pub range: Option<Range<usize>>, pub
     message: String }`. A same-file `range` is kept and moved with edits as the diagnostic's own range
     is.
   - `Problem`, from `group_problems`, carries both.
2. `crates/fe-workbench/src/lsp.rs` `diagnostic()`: map `related` (its byte ranges rebased from the event's
   version as the diagnostic's are) and `code_description` (as a string).
3. `crates/fe-gpui/src/hover.rs`: under each diagnostic's message, the popover shows one line per related
   location, `↳ <file name>:<row+1>: <message>`, the file name being the location's own, else "line" for
   the same file (`↳ line 12: first borrow here`). After them, if the diagnostic has a code and an href,
   the line `<code>: <href>`.
4. `crates/fe-workbench/src/problems_panel.rs`: under each problem row, one indented child row per related
   location, with its message and `file:row` (`row` alone for the same file). Clicking one, or
   `problems.open` while it's selected, opens it:
   - for the same file, select its range in that file's editor, as a problem's click does;
   - for another file, open it with the caret at the start of `row`.

   `problems.select_next` and `select_previous` step over child rows too.
5. Tests (new files, each declared from an existing `*_tests.rs`):
   - `crates/fe-gpui/src/view/diagnostic_related_tests.rs`, declared in
     `crates/fe-gpui/src/view/diagnostics_tests.rs`: `the_hover_lists_related_locations_and_the_code_link`.
     A diagnostic with a same-file and an other-file related location and a code href: after the hover
     delay, `painted_popover` holds the message, the two `↳` lines and `E0499: <href>`.
   - `crates/fe-workbench/src/problems_related_tests.rs`, declared in
     `crates/fe-workbench/src/problems_panel_tests.rs`: `related_rows_open_their_locations`. The fake
     publishes a diagnostic in `a.rs` related to a range in `a.rs` and to row 3 of an unopened `b.rs`.
     The panel shows two child rows. Clicking the first selects that range in `a.rs`'s editor; clicking
     the second opens `b.rs` with the caret at row 3's start.

The one literal construction of fe-editor's `Diagnostic` outside fe-editor is `lsp.rs:1989`. Check for
others with `rg 'Diagnostic \{'` before starting.

## Implementation notes

- `DiagnosticSet` anchors each same-file related range beside the diagnostic's own, rebases it on
  `apply` from the event's version and on `refresh`, and resolves it in `entries`:
  `DiagnosticEntry::related` holds the related locations as of the snapshot, a same-file one's
  `row` recomputed from its moved range. `Problem::related` is copied from it.
- A related location is "in the same file" when it has a range (fe-lsp sets one only for the
  published document); the hover then says `line N` and the panel `N`.
- Problems panel: `Row::Related` rows follow their problem's and are selectable;
  `select_next`/`select_previous` step onto them; the selection keeps the same related row of
  the kept problem. An other-file location opens through `Pane::open_at` with a row `Reveal`,
  in the pane showing the file or else the active pane.
- Existing test edited only to construct the new fields: `crates/fe-editor/src/diagnostics_nav_tests.rs`
  (`problem()` builds a `Problem` literal, which gained `related` and `code_href`).
