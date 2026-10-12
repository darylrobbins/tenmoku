---
task_uid: "01m4p2a0xf54s3rq8rqy3ehvbx"
task: "T12"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "row_change_decorations_tests: with base \"a\\nb\\nc\\n\", an added, a modified and a deleted row give, from EditorModel::decorations over the whole text, a RowBackground and a Gutter(Bar) with Tint::Added over the added row's bytes, the same with Tint::Modified over the modified row, and one Gutter(Deletion) with Tint::Removed as an empty range at the start of the row after the deleted one (the text's end when it was the last)"
    required: true
    manual: false
  - id: "C2"
    text: "A decorations query returns only the changes whose rows overlap its range, all at priority -100; after an edit the decorations follow without set_diff_base being called again, and T374's cfg(test) diff counter shows one diff per buffer version across row_changes() and the provider together"
    required: true
    manual: false
  - id: "C3"
    text: "set_diff_base(None) removes the provider and its decorations; setting a base twice, or the same text again, leaves one provider registered; a split shares the base and the provider, registered once"
    required: true
    manual: false
  - id: "C4"
    text: "No base, or a base or text over 5 MiB, gives no decorations; git diff against the base touches nothing under crates/fe-gpui/ or crates/fe-workbench/"
    required: true
    manual: false
---

# Highlight git-changed rows through a decoration provider the editor registers when a diff base is set (Mac T375)

Filed on helium from Mac T375 (~/coordination/helium-cards/T375.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-b.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

On 2026-10-11 the user asked for "subtle highlighting of the changed rows in files" as part of git support.
T374 gives `EditorModel::set_diff_base` and `EditorModel::row_changes()`: the current text's added, modified
and deleted rows against a base, cached per buffer version. T377 sets each open editor's base to its HEAD
text. Nothing paints the result yet.

T375's first revision painted it with its own module and a hook in crates/fe-gpui/src/element.rs. Ten open
cards have that file in scope. Since then, M8's decoration seam has been filed: T386 (the
`DecorationProvider` registry on `EditorModel`) and T387 (fe-gpui paints every provider's row backgrounds and
gutter marks). Changed rows are the seam's first user. This revision turns T375 into an adapter: a provider
that maps row changes to decorations. It has no fe-gpui code of its own.

## Change

**Where it's registered: by the editor, in `set_diff_base`.** It's the smallest place that works for every
host. T377, and any later host (the AI demo host), only calls `set_diff_base` and gets highlighting, so T377
and fe-workbench don't change. The provider also reads the model's own per-version cache, so the text is
diffed once per version and never twice. A provider registered by the host would see only the
`BufferSnapshot` that `DecorationProvider::decorations` passes. It can't reach `EditorModel::row_changes()`,
so it would have to diff again.

- In crates/fe-editor/src/row_changes.rs, move T374's base and per-version cache into an
  `Rc<RowChangeState>` (interior `RefCell`, as now). Give `RowChangeState` a
  `changes(&self, snapshot: &BufferSnapshot) -> Arc<[RowChange]>`. Both `EditorModel::row_changes()` and the
  provider call it, so a version is diffed once whoever asks first. Behaviour of `row_changes()` is unchanged.
- In crates/fe-editor/src/lib.rs:
  - `set_diff_base(Some(base))` registers the provider through T386's `add_decoration_provider` the first
    time and keeps its `ProviderId`. A new base reuses the provider.
  - `set_diff_base(None)` removes the provider (`remove_decoration_provider`).
  - Setting the same text again stays a no-op.
  - `split()` shares the `Rc<RowChangeState>` and the `ProviderId` (T386's split already shares the
    provider), so a base set on either model applies to both and the provider is registered once.
- New `crates/fe-editor/src/row_change_decorations.rs`:
  `pub(crate) struct RowChangeDecorations { state: Rc<RowChangeState> }`, `impl DecorationProvider`.
  `decorations(snapshot, range, out)` takes `state.changes(snapshot)`, binary-searches the changes whose rows
  overlap `range`'s rows, and appends at `priority` -100, below other providers:
  - `Added` and `Modified`, over `text.row_start(rows.start)..text.row_start(rows.end)` (the text's end
    when `rows.end` is the row count): a `RowBackground` and a `Gutter(GutterMark::Bar)`, both
    `Tint::Added` or `Tint::Modified`.
  - `Deleted`: a `Gutter(GutterMark::Deletion)` with `Tint::Removed`, an empty range at
    `text.row_start(rows.start)` (the text's end when it is the row count). T387 draws it on that row's top
    edge, or on the last row's bottom edge at the text's end.

  `refresh` is the default no-op: the cache is keyed by version.
- Tests in a new `crates/fe-editor/src/row_change_decorations_tests.rs`, included from
  row_change_decorations.rs with `#[cfg(test)] #[path = "row_change_decorations_tests.rs"] mod tests;`.
  They go through `EditorModel::decorations(range)`, the API T387 paints from.

T387 paints these as a 2 px bar at the fold column's left edge, a 6×2 px deletion mark and a 0.08-alpha row
tint. Added is `information` with hue 130°, Modified is `information`, Removed is `error`. These are the
geometry and colours this card's first revision specified.

## Not in this card

Any change to crates/fe-gpui or crates/fe-workbench. Theme slots for the colours. Clicking a bar to show the
old text. Minimap and scrollbar marks.

## Round 4: fix F1 and F3 (Mac lead decision, 2026-10-11, ~/coordination/from-mac-lead-2026-10-11-helium-e.md)

Review round 3 left two blocking findings. Both are real bugs, so no override: fix them on this candidate.

**F3 (medium), crates/fe-editor/src/row_changes.rs:111 (also lib.rs ~294, ~519, ~1393).** Live-version
tracking misses the BufferMut catch-up path: version_token updates only in refresh(), but dropping
buffer_mut() advances the model's snapshot directly. Edit model A through buffer_mut() to version 1 and
query its changes; advance split B to version 2 and query B; querying A again repeats its diff, because
pruning sees A's outdated token and evicts version 1. Fix: update the tracked version on every snapshot
catch-up, including BufferMut::drop, preferably by sharing the existing caught_up token. Add counter
tests mixing direct buffer edits with split edits and covering more than four distinct live versions.

**F1 (high), crates/fe-editor/src/lib.rs:380 (also ~215, ~419, ~431, ~455).** Changed-row provider
membership is still local to each split. The query fallback temporarily upgrades a weak reference without
registering or retaining it. Create two models without a base, set the base on one split, then drop that
split: the remaining model's row_changes() reflects the base but decorations() returns nothing until
sync_display(). Clearing the base also leaves other splits' providers registered until synchronization.
Fix: membership and ownership of the changed-row provider belong in state the splits already share, so
setting, clearing and restoring the base takes effect on every live split at once, while unrelated
providers keep their per-split semantics. Check the code for what the splits share. Test registry
membership and queries without synchronization, including dropping the split that set the base.

If F1 needs a design change bigger than this card (beyond lib.rs and row_changes.rs, or reshaping
EditorModel's sharing), stop and block (rgt task block) with a short proposal: the Mac lead decides.
