---
task_uid: "01m417z450xbmgjtnwgpm977t1"
task: "T17"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-syntax test injection::tests::random_edits_leave_layers_as_a_fresh_parse_would passes"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax test injection::tests::an_edit_reparses_only_the_layers_it_touches passes"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax test injection::tests::layers_far_from_the_viewport_are_dropped passes"
    required: true
    manual: false
  - id: "C4"
    text: "fe-syntax test tests::golden::highlights_match_golden_files passes"
    required: true
    manual: false
---

# Reparse injected layers incrementally and keep only layers near the viewport

## Problem

After T-12 part 1, injected layers (Markdown inline and fenced code, HTML script/style, JS template strings) are parsed from scratch for the visible range every time the host tree changes, inside `Syntax::highlights`' budget. Every keystroke reparses every visible layer. A large fenced code block or `<script>` never fits the budget and stays plain. T-12's acceptance requires that "injected layers reparse incrementally and only near the viewport".

## Change

In `crates/fe-syntax/src/injection.rs` and `crates/fe-syntax/src/syntax.rs`:

- Keep each layer's `Tree` across edits, keyed by its injection site (the host node and the language). When `Syntax::edit` applies an `InputEdit` to the host tree, apply the same edit to every kept layer tree whose ranges it touches, shifting the included ranges of layers after it.
- On the next highlight of a range, reparse a touched layer with its old tree, as tree-sitter's incremental parse expects, and with its new included ranges, recomputed from the host after the host reparses. Layers the edit didn't touch are reused unparsed.
- A layer whose injection site disappears (fence deleted, `<script>` removed) is dropped. Layers more than one viewport's worth of bytes away from the last highlighted ranges are dropped too, so memory stays proportional to what was recently shown, not to the file.
- Results stay identical to a fresh parse. The existing `incremental_reparse_matches_a_full_parse` property (`crates/fe-syntax/src/tests.rs`) is the model: random edit batches, with background parses finishing late, must leave the same highlights a fresh `Syntax` would give.
- Count layer parses behind a `#[cfg(test)]` counter so tests can assert what was reparsed.

Tests go in `crates/fe-syntax/src/injection/tests.rs`, next to part 1's.

## Clean room

Same as part 1: no Zed `language`/`languages` crates or Zed queries. gpui-kit (Apache-2.0) is an allowed reference, with a header and a `THIRD_PARTY.md` row for anything adapted.

## Implementation notes

- **Identifying a layer across edits.** A layer is found again by its language and its ranges, which `Layers::edit` moves along with each edit the way tree-sitter moves a tree's included ranges. If a layer's ranges are unchanged and no edit touched its span, it is reused without reparsing. If an edit touched it, it matches the injection of the same language whose span meets its own, and is reparsed from its old tree with the new included ranges. This stands in for "the host node": host node ids don't survive a reparse. Tree-sitter treats included ranges that differ from the old tree's as changed, so a mismatched old tree still gives a fresh parse's result, just with less reuse. Each untouched layer is matched only by its exact ranges, so two injections at the same place (nested template strings) can't take each other's layers.
- **Before the host reparses**, `Syntax::highlights` uses edited layers as they are, without reparsing, just as it uses the edited host tree. Reparsing waits until the injections can be recomputed from a reparsed host.
- **Dropping layers.** After a complete highlight of a range on a reparsed host, any layer the range should have used but didn't is dropped: one with bytes in the range, or an edited one whose span meets it. Separately, the last 8 highlighted ranges (`RECENT_RANGES`, the same count as the highlight cache) are remembered and moved by edits, and a layer is dropped once it is more than one range's length from all of them.
- **Test counter.** `Layers::parses` (cfg(test)) logs every layer parse, fresh or incremental. `Syntax::take_layer_parses` drains it.
- `a_layer_whose_injection_is_gone_is_dropped` covers deleting a fence and deleting a `<script>`.
