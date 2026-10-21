---
task_uid: "01m5f7ft301drdm5ptrvx9arp4"
task: "T1028"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "outline_scopes for rows inside fn f in 'mod m { impl S { fn f() {..} } }' returns m, impl, f as header..=last with headers on the name rows, outermost first; single-row items and items outside the rows are absent (crates/fe-syntax/tests/sticky_outline_scopes.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "A Python decorated def's header is its def row, not the decorator; Markdown heading sections nest by level"
    required: true
    manual: false
  - id: "C3"
    text: "On a generated 50k-row Rust file, outline_scopes for rows deep inside equals the full outline's items containing those rows (same header and last)"
    required: true
    manual: false
  - id: "C4"
    text: "sticky_tests.rs covers: no band at scroll 0 or when a header is exactly at the top; a 3-deep chain pinned; the innermost row pushed up by exactly the overlap in px as its last row scrolls under; the max_rows cap; a header inside a closed fold skipped; a wrapped header takes one slot; lens block rows counted; keep_above drops rows at or below it"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-syntax -p fe-display passes with no fe-gpui change"
    required: true
    manual: false
---

# Sticky scroll model: the outline scopes enclosing a row range from fe-syntax, and the pinned rows with VS Code's push-up for a scroll position in fe-display

# Sticky scroll 1/4: the sticky-row model and the outline scope source (Linux-OK)

VS Code's editor sticky scroll (user-approved feature): the header rows of the scopes enclosing the
topmost visible row stay pinned at the top of the editor. This card is the pure part: which rows
are pinned, and where, for a scroll position. No GPUI, no settings, no drawing (cards 2-4).

## Scope source: outline (VS Code's default `outlineModel`)

fastedit has no LSP `documentSymbol` (fe-lsp doesn't request it), so the outline model is
fe-syntax's tree-sitter outline query, the one the Outline panel and breadcrumbs use. Today that
query only runs over the whole tree (`OutlineJob::run`, off-thread in fe-workbench's
`OutlinePanel`, active editor only). Sticky scroll needs every visible editor's scopes every
frame it scrolls, so add a range-limited run in `crates/fe-syntax/src/outline.rs`:

```rust
/// The outline items overlapping `rows` that span at least two rows, as
/// `header..=last` row ranges (fold_ranges' shape), sorted by header then outermost first, one per header row
/// (the outermost). The query runs only over `rows`' bytes
/// (`QueryCursor::set_byte_range`), so ancestors spanning the whole file
/// cost their depth, not their size.
pub fn outline_scopes(syntax: &Syntax, rows: Range<u32>) -> Result<Vec<RangeInclusive<u32>>, NoOutline>
```

- `header` is the item's `row` (its name's row: `def f` not its `@decorator`, a Rust `fn` not its
  `#[test]`); `last` is the row of its range's end, the row before when the range ends at column 0.
  Markdown headings use their section's range, as the outline does.
- Share the match loop with `OutlineJob::run` (predicates, kinds, section closing) rather than
  copying it; detail strings are not computed for scopes.
- Export `outline_scopes` from `crates/fe-syntax/src/lib.rs`.

## Placement model: new `crates/fe-display/src/sticky.rs`

Pure, in display/screen terms (fe-display must not depend on fe-syntax, hence plain row ranges) (folds, soft wrap, block rows such as code lenses all count):

```rust
pub struct StickyRow { pub buffer_row: u32, /// px from the view's top, ≤ index × line height
    pub y: f32 }
pub fn sticky_rows(scopes: &[RangeInclusive<u32>], display: &DisplayMap, blocks: &BlockMap,
    scroll_y: f32, line_height: f32, max_rows: usize, keep_above: Option<f32>) -> Vec<StickyRow>
```

VS Code's rule: slot `i` (0-based) of the band holds the `i`-th scope of the chain whose header's
screen row starts strictly above `scroll_y + i × lh` and whose last row's bottom is below it. The
innermost pinned row is pushed up as its scope ends: `y = min(i × lh, bottom_of_last − scroll_y − lh)`
(a pushed row's nested rows are not shown). A scope whose header is hidden in a closed fold is
skipped; a wrapped header takes one slot (its first display row). At most `max_rows` rows.
`keep_above` (the caret's screen y relative to the view, set by card 3) drops every row whose
bottom would pass it, so the band never covers the caret.

`pub mod sticky` (or `pub use`) from `crates/fe-display/src/lib.rs`; tests in a new
`crates/fe-display/src/sticky_tests.rs` via `#[cfg(test)] #[path]`.

Not in scope: rendering, settings, the folding/indentation models (card 4), LSP documentSymbol.

## Implementation notes

- `OutlineJob::run`'s match loop is now `find` in `outline.rs`, which takes a `Find`: every item, or
  only those overlapping some bytes (`QueryCursor::set_byte_range`); `run` and `outline_scopes` both call it. Tree-sitter keeps
  advancing in-progress matches through children outside the range, so an `impl` whose name is
  far above the rows still gets its `@name`.
- Tree-sitter's byte range limits where a pattern starts, not what it matches below: a pattern
  rooted at an enclosing `impl` or class still matches every one of its methods. In scope mode
  (`Find::Overlapping`) `find` drops a match whose item node misses the rows before its
  predicates run, and doesn't apply `MAX_OUTLINE_ITEMS`, so unrelated items can't push the rows'
  scopes past a cap (`scopes_past_the_outline_cap_are_found`, and the internal
  `outline_scope_tests.rs` checks that only the rows' items are kept). Tree-sitter itself still
  steps over each ancestor's children, as any range-limited query (highlights) does.
- C++'s `#member?` only chooses Method over Function for the same node and name, and costs the
  whole file's namespace index, so scope mode treats it as holding: kinds aren't kept.
- Markdown: a heading's node is just its line, and its section's end is the next heading of the
  same or a higher level, which the grammar's `section` nodes don't give reliably (setext
  headings aren't sectioned). So `outline_scopes` finds every Markdown scope once a parse, uncapped,
  into a `ScopeIndex` cached on `Syntax` beside `wide_brackets` (emptied on edit or reparse), and
  answers each call by binary search plus a parent walk. That's a small change to `syntax.rs`,
  outside the card's listed files.
- `samples_match_the_whole_outline_row_by_row` checks every row (and 7-row windows) of every
  `tests/outline_samples/` file against the whole outline, so every outlined language's
  predicates are covered by the range-limited run.
- `sticky_rows`: with consecutive header rows, VS Code's rule pins a header as soon as the band's
  earlier slots cover it, e.g. scrolled by 1 px, `mod m { impl S { fn f() {` pins all three. A
  closed fold's own header (its last row hidden) is pinned only while it's under its slot, at its
  real position. Scopes on the same header row as the previous pinned one are skipped.
