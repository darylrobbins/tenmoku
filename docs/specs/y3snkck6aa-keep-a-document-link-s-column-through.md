---
task_uid: "01m5rpcgqwgmd14xy3snkck6aa"
task: "T338"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "A file link's #line,col lands on the right column for viewer targets, overlapping opens in different panes, a target reserved by Save As, and an oversized fragment column is clamped. Tests link_column_lands_in_viewer, overlapping_opens_keep_their_landings, link_survives_save_as_reservation, oversized_fragment_column_is_clamped. Known limits (T340): the Save As continuation is inferred from reservation state and serial arithmetic, so an independent open of the same already-open target can be adopted as the continuation (review F2); a delayed viewer landing can activate its tab and take focus from another pane (F3); and an oversized column on a very long viewer row is converted on the UI thread by collecting the row prefix into one String (F4). T340 fixes all three."
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Keep a document link's column through viewer targets, overlapping opens, Save As and oversized fragments (T330 F5-F8)

## Problem
T330 (Mac T1112, document links) review round 3 left these findings, carried as documented limits of T330:
- F5 (medium, `crates/fe-workbench/src/lsp_links.rs`): file links with a `#line,col` fragment reveal column zero at first and the later column landing is lost for viewer targets; the requested UTF-16 column should be kept through the viewer's pending reveal and converted when the target row becomes available.
- F6 (medium): the single workbench-wide column-landing subscription is cleared whenever another link opens, so overlapping opens in different panes lose their landings; track pending landings per pane.
- F7 (medium): a link to an existing target reserved by an ongoing Save As loses its UTF-16 reveal; carry it through the pane's Save As continuation.
- F8 (low): the fragment parser accepts usize columns but the conversion truncates; use a checked or saturating conversion to u32.

## Change
Fix the four in `crates/fe-workbench/src/lsp_links.rs`.

## Tests
`crates/fe-workbench/src/lsp_links_landing_tests.rs` (`#[path]`): one regression per finding (viewer target column, overlapping opens in two panes, Save As reservation, oversized fragment column).
