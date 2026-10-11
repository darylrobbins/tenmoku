---
task_uid: "01m4p2a0d73rg3e83dtnn7y05v"
task: "T10"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "decorations_tests: a DecorationSet applied at version V follows later edits (insert before, inside, delete over); typing at either end doesn't grow a range; a deleted-over one is empty at the deletion point"
    required: true
    manual: false
  - id: "C2"
    text: "apply/splice with a version older than the last applied, below oldest_version, or newer than the snapshot return false and change nothing; a query or refresh with a snapshot older than the set's anchors returns nothing and doesn't panic"
    required: true
    manual: false
  - id: "C3"
    text: "a query returns exactly the decorations overlapping the range (one starting before it and spanning in included); with 100k one-row decorations a cfg(test) counter shows a 50-row query examines at most 100 entries"
    required: true
    manual: false
  - id: "C4"
    text: "splice replaces only the entries inside its range, rebased from its version, and leaves the rest"
    required: true
    manual: false
  - id: "C5"
    text: "EditorModel: decorations(range) merges providers in (priority, registration, start) order; a split shares them; an edit through the split reaches a provider's refresh on sync_display; remove_decoration_provider drops one; scripts/check-no-gpui.sh passes"
    required: true
    manual: false
---

# Add decoration providers to fe-editor: an anchored DecorationSet and an EditorModel registry queried by visible range, headless (Mac T386)

Filed on helium from Mac T386 (~/coordination/helium-cards/T386.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-b.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The approved plan's M8 ("AI seams: `ProposedChange` diff blocks, ghost text, decoration providers, streaming
edits, demo host"; ~/.claude/plans/glittery-foraging-graham.md, crate table: `fe-editor` holds
"`DecorationProvider`") calls for a general way for a host or a feature to put decorations on the editor's
rows. The user asked on 2026-10-11 to "go ahead with the M8 drafters". Today every decoration is ad hoc,
computed and painted inside crates/fe-gpui/src/element.rs:

- search and find matches: `match_rects` (element.rs:1270), colour `search_match_color` (foreground at 0.22);
- diagnostics: `DiagnosticSet` in crates/fe-editor/src/diagnostics.rs (anchored, rebased from the version a
  server computed them on, refreshed in `EditorModel::sync_display`), painted by `diagnostic_marks`
  (element.rs:978) as wavy underlines and a gutter dot;
- vim visual: `selection_rects` (element.rs:1172); hover link underline: `underlines` (element.rs:914);
- coming: T255 diff blocks (block layer, element/proposal.rs), T368 jump labels, and the git changed-rows
  cards T374/T375, which add their own `EditorModel::row_changes` and their own painting module.

Each new kind adds model state, a prepaint pass and paint code to element.rs, which eight open tasks
already have in scope. A host (the AI backend, git) has no way to add a decoration without an fe-gpui change.

This card is the GPUI-free half: the provider model and an anchored decoration set, tested headless. The
next card paints what providers return.

## Change

New module `crates/fe-editor/src/decorations.rs` (`pub mod decorations;` in crates/fe-editor/src/lib.rs),
with its tests in `crates/fe-editor/src/decorations_tests.rs`, included by
`#[cfg(test)] #[path = "decorations_tests.rs"] mod tests;` (not crates/fe-editor/src/tests/mod.rs).

**Types** (all `Clone + Debug`):

- `pub enum Tint { Error, Warning, Information, Hint, Accent, Muted, Added, Modified, Removed }`: a role,
  never a colour. The painter resolves it from the theme, so decorations follow theme changes.
- `pub enum GutterMark { Bar, Deletion }`: a bar beside every display row covered; a short horizontal mark on
  the top edge of the first row covered (rows removed just above it).
- `pub enum DecorationKind { RowBackground, Gutter(GutterMark), Highlight, Underline { wavy: bool },
  AfterLine(Arc<str>) }`: a full-width band behind each covered row; a gutter mark; a background behind the
  range's text; an underline under it; text shown after the end of the range's last buffer row (it never
  moves or wraps the buffer's text).
- `pub struct Decoration { pub range: Range<usize>, pub kind: DecorationKind, pub tint: Tint, pub priority: i32 }`.
  `range` is bytes of the snapshot it was resolved in. A decoration *covers* the buffer rows from its
  start's row through its end's row, not counting an end row it ends at the start of (diagnostics' rule,
  element.rs:1001); an empty one covers the row it is on.

**Provider.**

```rust
pub trait DecorationProvider {
    /// Appends the decorations overlapping `range` (bytes of `snapshot`), an empty one at either end
    /// included. Called once per painted frame with the frame's shown range only.
    fn decorations(&self, snapshot: &BufferSnapshot, range: Range<usize>, out: &mut Vec<Decoration>);
    /// The model caught up to `snapshot`. Default: nothing.
    fn refresh(&mut self, _snapshot: &BufferSnapshot) {}
}
```

A provider either stores decorations (a `DecorationSet`, below) or computes them from the snapshot on
demand, caching by `snapshot.version()` itself (a git provider diffing against a base text, a bracket matcher).

**`DecorationSet`**, the stock store, `impl DecorationProvider`. Entries hold `Range<Anchor>` (start
`Bias::Right`, end `Bias::Left`, as `DiagnosticSet`, so typing beside a decoration doesn't grow it), sorted
by start, plus each entry's running maximum end, so a query binary-searches to the first entry that can reach
the range and stops at the first that starts past it.

- `apply(&mut self, snapshot, version, items: Vec<Decoration>) -> bool`: replaces every entry. Item ranges
  are bytes of buffer `version`, rebased onto `snapshot` with `BufferSnapshot::map_offset` (as
  `DiagnosticSet::apply`), so a result computed off-thread on an older version lands in the right place.
- `splice(&mut self, snapshot, version, within: Range<usize>, items) -> bool`: incremental update. Removes
  only the entries lying inside `within` (bytes of `version`, rebased the same way), then inserts `items`.
  For a git re-diff of one hunk or an AI edit streaming in.
- Both return false and change nothing when `version` is older than the last version applied or spliced
  (a late result never overwrites a newer one), below `snapshot.oldest_version()`, or newer than `snapshot`.
- `refresh(snapshot)` rebases every anchor onto `snapshot` with one `resolve_many` and rebuilds the running
  maxima; `clear()`, `len()`, `is_empty()`.
- A `decorations` query or `refresh` with a snapshot older than the set's anchors returns nothing / does
  nothing; it never panics (a split that hasn't caught up yet shares the set).
- A decoration whose text is deleted keeps an empty range at the deletion point, as diagnostics do.

**`EditorModel`** (crates/fe-editor/src/lib.rs, a few lines):

- field `decorations: Vec<(ProviderId, Rc<RefCell<dyn DecorationProvider>>)>`; `pub struct ProviderId(u64)`.
- `add_decoration_provider(&mut self, provider: Rc<RefCell<dyn DecorationProvider>>) -> ProviderId` and
  `remove_decoration_provider(&mut self, id) -> bool`. The host keeps its own typed `Rc<RefCell<P>>` to push
  updates (no downcasting), then notifies the view.
- `decorations(&self, range: Range<usize>) -> Vec<Decoration>`: every provider's, in the text as of the last
  catch-up, sorted by `(priority, registration order, range.start)`: the paint order, last on top.
- `sync_display` calls `refresh` on each provider after catching up (next to the diagnostics refresh).
- `split()` shares the providers (the same `Rc`s and ids): decorations are anchored to the shared buffer.
  A provider added to one model after a split isn't added to the other.

## Not in this card

- Painting (the next card). No migration: diagnostics, search/find matches, vim visual, the hover underline,
  T255's blocks and T368's labels keep their own paths. Each has behaviour the seam doesn't (per-frame search
  budgets, T363; hover hit areas; layout-changing blocks; transient overlays), and moving them would collide
  with the open element.rs cards. A later card can move diagnostics once the seam has users.
- Inline text that pushes the buffer's text aside (inlays, ghost text): that is fe-display's inlay layer.
- Theme slots for added/modified/removed.

## Implementation notes

- `splice` also returns false when the set has entries whose anchors don't resolve in `snapshot` (it's
  older than them, or its log was compacted past them): what it keeps couldn't be rebased, and dropping it
  silently would lose decorations outside `within`. The host applies the whole set again. `apply` replaces everything, so it accepts such a snapshot.
- A query's reach test uses each entry's running maximum end *and* its own start, since an end anchor
  (`Bias::Left`) can resolve before its start (`Bias::Right`) after an insert at an empty decoration.
  Mapping is monotone, so both orders survive edits without a refresh.
- `ProviderId`s come from a process-wide counter, so ids from two splits never collide.
- `Decoration::covered_rows(text)` implements the covered-rows rule for the painter; `decorations::overlaps`
  is the shared overlap test (an empty range on either side touches, ends included).
- `clear()` keeps the last applied version, so a late result is still rejected after it.
