---
task_uid: "01m4fbwm90kjne84r6q408nhtb"
task: "T253"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-buffer test proposed_dry_run::reports_clean_and_changed_hunks: after a user edit inside one of two hunks of a from_text proposal, dry_run reports the other Outcome::Clean and that one Conflict::Changed with expected = its old text and found = the text there now, at the same ranges ProposedChange::hunks gives, and applies_cleanly() is false"
    required: true
    manual: false
  - id: "C2"
    text: "fe-buffer test proposed_dry_run::changes_nothing: dry_run on a proposal whose only pending hunk was edited leaves the buffer's text and version unchanged and is_resolved() false (no status recorded), a later hunks() call then records Stale, and accepted and rejected hunks report Outcome::Resolved(Accepted) and Resolved(Rejected)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-buffer test proposed_dry_run::text_matches_accept_all: for (a) clean hunks only, (b) one edited hunk, (c) two pure insertions whose anchors collapse to one offset after a deletion, (d) a hunk recorded Stale whose old text an undo restored (Conflict::WasStale), dry_run(\u0026snapshot).text() equals the buffer text after accept_all on the same state, its Clean ids are the hunks accept_all applied and its Conflict ids the stale ids accept_all returns"
    required: true
    manual: false
  - id: "C4"
    text: "fe-buffer test proposed_dry_run::owned_and_thread_safe: a static assertion that DryRun is Send + Sync + 'static, a DryRun moved to another thread reads the same outcomes, and ProposedChange::buffer() returns the base snapshot's BufferId"
    required: true
    manual: false
---

# Dry-run a ProposedChange against a snapshot: report each hunk as clean or conflicting with the reason, and the text accepting the clean ones would give, without changing anything

## Problem

T240 (1799485) added `ProposedChange` to fe-buffer (`crates/fe-buffer/src/proposed.rs`): an AI's edits against a
snapshot as anchored hunks, each accepted (`accept`, `accept_all`) or rejected. The user's approved plan
(2026-10-08) files its later half once T240 landed; part 2 is "a dry-run executor — applying a ProposedChange
against the current buffer without committing, reporting which hunks apply cleanly, which conflict and why".

Today a host can only find out by trying: `accept` commits and `hunks()` records staleness as a side effect
(`Hunk::check`, proposed.rs). Nothing says *why* a hunk can't apply, nothing gives the text the buffer would
have, and nothing runs off the UI thread. The diff blocks (a later card) need the "why" for stale hunks, and the
host app (an AI-first dev tool, which will drive proposals) needs it to tell the agent what didn't apply.

`ProposedChange` also keeps its buffer private (`buffer: BufferId`, only checked by `check_buffer`), so a view
can't tell whether a proposal belongs to its buffer without panicking.

## Change

GPUI-free, in fe-buffer. Add `crates/fe-buffer/src/proposed/dry_run.rs` as a child module of `proposed.rs`
(`mod dry_run;` plus a `pub use`), so it reads the hunks' private fields. Export the new types from
`crates/fe-buffer/src/lib.rs` and extend the `ProposedChange` bullet in its `//!` list by one clause.

- `ProposedChange::buffer(&self) -> BufferId`.
- `ProposedChange::dry_run(&self, snapshot: &BufferSnapshot) -> DryRun`. It takes `&self` and changes nothing:
  not the buffer, and not any hunk's status (unlike `hunks()`, it does not record staleness). It panics as
  `hunks()` does (another buffer, a snapshot older than the anchors).
- `DryRun`, `Send + Sync`, owning everything (no borrow of the proposal), so a host can run it on a
  background thread with a snapshot:
  - `version(&self) -> Version`: the snapshot's.
  - `hunks(&self) -> &[HunkOutcome]`, every hunk in text order. `HunkOutcome { id: HunkId, range: Range<usize>,
    outcome: Outcome }`, with `range` resolved in the snapshot exactly as `ProposedChange::hunks` resolves it.
  - `pub enum Outcome { Clean, Conflict(Conflict), Resolved(HunkStatus) }`: `Resolved` for a hunk already
    `Accepted` or `Rejected`, which a dry run doesn't apply.
  - `pub enum Conflict { Changed { expected: Arc<str>, found: Arc<str> }, WasStale }`: `Changed` when the text
    at `range` differs from the hunk's old text (`expected` is the old text, `found` the text there now);
    `WasStale` when the hunk was already recorded `Stale` though its old text is back (say, after an undo), since
    `accept` refuses it. Each implements `Display` with a one-line reason for a UI ("the text under this change
    was edited", "this change went stale earlier").
  - `applies_cleanly(&self) -> bool`: no hunk is `Conflict`.
  - `text(&self) -> &Text`: the snapshot's text with every `Clean` hunk applied, built on a clone of the
    snapshot's `Text` (`Text::replace`), merging edits with `crate::edit::normalize` exactly as `Buffer::edit`
    does, so it matches what `accept_all` would produce.

The rule that decides `Clean` vs `Conflict` must be the one `accept`/`accept_all` use (`Hunk::check` plus the
recorded status), shared rather than copied.

Not in this card: any UI, streaming hunks, running a command in dry-run mode
(`docs/decisions/command-model.md` § "Dry run and proposed changes").

## Tests

All in the new file `crates/fe-buffer/tests/proposed_dry_run.rs`, over the public API, reusing the shapes of
`crates/fe-buffer/tests/proposed_change.rs` (copy its small helpers; don't edit that file).
