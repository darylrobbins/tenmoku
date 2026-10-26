---
task_uid: "01m5sajkn1wgn7z26whv3795jr"
task: "T340"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "After a document link to an already-open target is reserved by Save As and the reservation is released, an independent open of that target (quick open, navigation) before the continuation runs keeps its own position: the old link landing is cancelled. Test: independent_open_after_reservation_release_cancels_landing"
    required: true
    manual: false
  - id: "C2"
    text: "A delayed viewer landing never activates its tab or steals focus from another pane, and an oversized fragment column on a long viewer row is converted over bounded chunks without a multi-GiB allocation on the UI thread. Tests delayed_landing_keeps_focus, oversized_column_on_long_row_is_bounded"
    required: true
    manual: false
  - id: "C3"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Tell the Save As continuation from an independent open of the same target so an old link landing never overrides it (T338 F2)

## Problem
T338 (document-link column landings) review round 2 left F2 (medium, `crates/fe-workbench/src/lsp_links.rs:596`), carried as a documented limit of T338: the Save As continuation guard infers "this is the continuation" from reservation state and serial arithmetic, so an independent open of the same already-open target (quick open, navigation) after the reservation is released but before its callback runs advances the serial by one and is adopted as the continuation; the old `#L2,2` landing is then applied over the newer open.

## Change
Identify the actual Save As continuation explicitly (a token carried by the continuation, not inferred), and cancel the pending landing for any independent open.

## Tests
In `crates/fe-workbench/src/lsp_links_landing_tests.rs`: a pre-existing target tab, follow L2,2 while it is reserved, release the reservation, open the target at L1 via a non-link open before the callback runs; the old landing must not apply.

## Added (review round 3 of T338)
- F3 (medium, `lsp_links.rs:702`): completing a viewer landing calls `pane.open_at`, which activates and focuses the viewer even when its pane no longer has focus; apply the converted column directly to the existing viewer without activating its tab or changing focus; regression: focus another pane while indexing is held, completing the landing preserves focus.
- F4 (medium, `lsp_links.rs:673`): the viewer conversion collects the row prefix into one String on the UI thread, so an oversized fragment column on a long row can read/allocate ~4 GiB; convert UTF-16 units incrementally over bounded chunks, scan large rows off the UI thread with stale-landing checks; coverage for an oversized column on a long viewer row.
