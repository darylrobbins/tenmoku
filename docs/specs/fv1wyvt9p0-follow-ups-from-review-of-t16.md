---
task_uid: "01m41gg43tcqmqz4fv1wyvt9p0"
task: "T34"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T16

Findings of the review of T16 that don't block the merge (below merge.block_severity):

- F3 medium crates/fe-syntax/src/injection.rs:270: Preparation still exceeds the shared deadline. The shipped JS/TS tagged-template query sets injection.include-children, so content_ranges bypasses its only deadline check and always returns true; the sibling loop therefore continues through every fragment and interpolation after expiry. Sorting, intersection, layer-key cloning/hashing and included-range installation also perform bulk preparation without cancellation or size bounds. The new template test starts with an already-expired deadline, so it never exercises cancellation during preparation.
  fix: Check the deadline for every content node and sibling, including nonmatching siblings and the include-children path, and propagate cancellation immediately. Bound or make cancellable the remaining bulk preparation operations. Add coverage where a finite budget expires after combined-template preparation begins.
  instances: crates/fe-syntax/src/injection.rs:259, crates/fe-syntax/src/injection.rs:270, crates/fe-syntax/src/injection.rs:281, crates/fe-syntax/src/injection.rs:283, crates/fe-syntax/src/injection.rs:303, crates/fe-syntax/src/injection.rs:335, crates/fe-syntax/src/injection.rs:357

- F4 medium crates/fe-syntax/src/syntax.rs:279: Filtering compiled languages loses a required retry. If another view finishes compiling a language after this frame's highlights recorded it as missing but before load_injected_languages calls missing_languages, retain removes the entry and the loader returns without scheduling a redraw. This frame still contains host-only highlights and can remain that way until interaction. The same race lets highlights_compiling return incomplete spans. The UI test manually calls highlights after loading, so it does not verify that the displayed frame was repainted.
  fix: Preserve the fact that the current highlights missed a language until highlighting retries, even if its query has since compiled, and ensure that condition schedules a redraw. Make the test helper retry too. Add deterministic coverage for compilation completing between highlighting and missing-language inspection, asserting the rendered frame without manually recomputing highlights.
  instances: crates/fe-syntax/src/syntax.rs:279, crates/fe-syntax/src/syntax.rs:290, crates/fe-gpui/src/view.rs:289

## Implementation notes

- **F3.** `match_ranges` (split out of `Painter::find`) checks the deadline at every content node and every sibling of a combined node, matching or not, including on the `include-children` path, and returns false at once; `find` then stops. What follows collection is linear in the ranges collected, each within the deadline: the nodes come in document order, so the sort only checks it, and `intersect` and finding the kept layer are linear scans. Combined parents are tracked in a `HashSet` rather than scanned, which was quadratic in the number of combined matches. Test builds count deadline checks per thread and can make every deadline pass after N of them (`DEADLINE_CHECKS`, `DEADLINE_AFTER` in `syntax.rs`). `combined_templates_stop_at_the_deadline` uses that to make the deadline pass partway through a 2000-piece template: collection makes one check per piece and stops right after the deadline passes, and `find` reports incomplete within three checks of it, wherever it passes.
- **F4.** `Syntax::missing_languages` became `take_missing_languages`. It drains the list rather than filtering out languages compiled since, so a language compiled by another view between a frame's highlights and the view's check still schedules the compile (a no-op) and the redraw. The view takes the list even while a load runs, since that load's redraw highlights again and records any language still missing. `highlights_compiling` and the fuzz target's helper retry for the same reason. `injected_languages_compiled_meanwhile_draw_again` compiles Lua through a test-only hook that runs just before the view takes the list, then checks the colour the last drawn frame used for `local`; `injected_languages_load_without_input` now also checks the drawn frame rather than recomputing highlights. Both tests, and `missing_languages_are_reported`, fail with the old filter. The change reaches `crates/fe-gpui/src/{view,element,tests}.rs` and `fuzz/fuzz_targets/syntax_edits.rs`, outside the card's scope.
