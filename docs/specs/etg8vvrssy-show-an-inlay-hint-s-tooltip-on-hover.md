---
task_uid: "01m55v38xrw8yxh3etg8vvrssy"
task: "T172"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui unit test view::window_tests::inlay_hint_actions::hovering_shows_the_tooltip_and_double_click_applies_edits passes: resting on a hint shows the host's resolved tooltip as popover_text() after the hover delay, asked once; double-clicking a hint with edits leaves let a: u8 = 1; with the hint gone and one undo restoring the text; a hint without edits double-clicks as before; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit test tests::lsp_hover::inlay_hints::resolved_hints_insert_their_edits passes: double-clicking a type hint sends inlayHint/resolve with its data and applies the resolved textEdits"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-gpui -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-gpui -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Show an inlay hint's tooltip on hover and apply its edits on double-click, resolved from the server, as VS Code does (Mac T935)

Filed on argon from Mac T935 (card ~/coordination/argon-cards/zzr/T935.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55txsyk8s2y5spgqqwq3st1. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Argon deps: T171 (Mac T934). Title the host commit "... (Mac T935)".

## Problem

LSP completeness track, item 3b (user-approved 2026-10-17: inlay hints with "request/resolve/refresh"). Once the
cards this depends on land, editors show servers' inlay hints, and fe-lsp can resolve a hint for its `tooltip`
and `textEdits` (`LspManager::resolve_inlay_hint`). Nothing asks it to. In VS Code, resting the pointer on a hint
shows its tooltip in a hover (resolved first if the server resolves), and double-clicking a hint that has
`textEdits` applies them: rust-analyzer's type hints insert the type annotation they show (`: Vec<String>`).

## Change

1. `crates/fe-gpui/src/inlay_hints.rs`:
   - New `pub trait InlayHintHost { fn resolve(&self, hint: InlayHint) -> Pin<Box<dyn Future<Output =
     InlayHint>>>; }` and `EditorView::set_inlay_hint_host(Option<Rc<dyn InlayHintHost>>)`.
   - `InlayHint` gains `pub tooltip: Option<String>` (markdown or plain) and `pub edits: Vec<(Range<usize>,
     String)>` in bytes of the hints' version.
2. `crates/fe-gpui/src/view.rs` and `crates/fe-gpui/src/hover.rs`:
   - The pointer resting on a hint's painted bounds for the hover delay resolves it through the host (once
     per hint; a hint without a host or with nothing to resolve is used as it is) and shows its tooltip in
     the hover popover, as hover text is shown; moving off hides it. A hint with no tooltip shows nothing.
   - A double-click on a hint whose (resolved) `edits` aren't empty applies them in one undoable
     transaction, rebased from the hints' version as completion's additional edits are, and the hint goes.
     A double-click on a hint with no edits does what a double-click there does today.
3. `crates/fe-workbench/src/lsp_inlay_hints.rs`: the workbench's host resolves through
   `resolve_inlay_hint` without holding the manager's lock across the await, mapping fe-lsp's `tooltip` and
   `edits`; hints are created with the listed ones.
4. Tests:
   - New `crates/fe-gpui/src/view/inlay_hint_actions_tests.rs`, declared at the end of
     `crates/fe-gpui/src/view/window_tests.rs` as `#[path = "inlay_hint_actions_tests.rs"] mod inlay_hint_actions;`,
     with a host of its own: `hovering_shows_the_tooltip_and_double_click_applies_edits`: resting on a hint
     shows `popover_text()` equal to the host's resolved tooltip after the hover delay, the host having been
     asked once; double-clicking a hint whose resolved edits insert `: u8` after `a` leaves `let a: u8 = 1;`,
     the hint gone, and one `editor.undo` restores the text; double-clicking a hint without edits selects the
     word under it as before.
   - `crates/fe-workbench/src/tests/lsp_inlay_hints.rs` gains `resolved_hints_insert_their_edits`: with a
     fake whose provider resolves, double-clicking a type hint sends `inlayHint/resolve` with its `data` and
     applies the resolved `textEdits`.

Order: `hover.rs` is held by T816 and T916, `view.rs` by T816, T858 and T917; this follows the inlay-hint view
and workbench cards. Linux-OK.

## Implementation notes

- Outside the listed scope but unavoidable: `crates/fe-gpui/src/lib.rs` re-exports `InlayHintHost`/`InlayHintFuture`; `view/inlay_hints_tests.rs`'s `hint()` fills the two new `InlayHint` fields; `element.rs` records painted hint bounds outside tests.
- `EditorView::inlay_hint_bounds` exposes painted hints so the workbench test can click one.
- Carried T193 F1: the hide grace now applies only to popovers with content (regressions `leaving_during_a_pending_request`, `leaving_after_a_none_answer` in hover.rs).
- Carried T171 F1: a direct server change clears hints and the in-flight request. No test: needs two servers.
- A double-click on a hint not yet resolved selects the word as before and applies the edits when the host's answer has them.
- Review round 1: hints have stable ids and a per-view resolve state (unresolved, pending with an accept flag, resolved), so hover and double-click share one request, and late answers for replaced hints are dropped; edits are validated against the base snapshot (text replaced must be unchanged) inside `edit_and_reveal`'s plan, after vim's session ends; the workbench reinstalls hosts and clears hints when the manager changes. No vim-session regression test was added.
