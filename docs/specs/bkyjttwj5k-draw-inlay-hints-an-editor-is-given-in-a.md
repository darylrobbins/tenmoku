---
task_uid: "01m55v37nes4my2bbkyjttwj5k"
task: "T169"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-gpui unit test view::window_tests::inlay_hints::hints_paint_after_their_offset_and_follow_edits passes: painted_inlay_hints holds ' : u8' and 'x: ' at their offsets' x, later text shifted; typing before them moves them; deleting a range strictly around a hint's offset drops it only; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui unit test view::window_tests::inlay_hints::clicks_and_arrows_skip_hints passes: a click inside a hint puts the caret at its offset; editor.move_right visits the offsets it visits without hints and no painted caret is inside a hint"
    required: true
    manual: false
  - id: "C3"
    text: "With no hints set, every frame lays out as at the base: the existing fe-gpui tests and screenshot scenes pass unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-gpui passes with no existing test edited beyond what the spec names; cargo clippy -p fe-gpui --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Draw inlay hints an editor is given in a muted chip inside the text, following edits, with the caret never inside one, as VS Code draws them (Mac T932)

Filed on argon from Mac T932 (card ~/coordination/argon-cards/zzr/T932.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55txrph7tj9h20ma4n3kfvs. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Argon deps: T168 (Mac T931). Held in the inbox: waits until Mac T858, Mac T917 is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T932)".

## Problem

LSP completeness track, item 3a (user-approved 2026-10-17). Once the card this depends on lands, fe-display lays
out any number of hints per row (`layout_frame_with_hints`, `Frame.hints`). fe-gpui's element passes only the
composition or ghost-text inlay (`crates/fe-gpui/src/element.rs:674-680`), and a host has no way to give an
editor hints. VS Code draws inlay hints in a smaller, muted font on a faint rounded background
(`editorInlayHint.foreground` / `.background`), with a space on the side a hint's `paddingLeft` /
`paddingRight` asks for; they follow edits until the server's next answer replaces them.

## Change

1. New `crates/fe-gpui/src/inlay_hints.rs`:
   - `pub struct InlayHint { pub offset: usize, pub label: String, pub kind: Option<InlayHintKind>, pub
     padding_left: bool, pub padding_right: bool, pub data: Option<Rc<dyn Any>> }` and `pub enum
     InlayHintKind { Type, Parameter }`; `data` is the host's, for the later tooltip and edit card.
   - `EditorView::set_inlay_hints(&mut self, version: Version, hints: Vec<InlayHint>)`: the hints, their
     offsets in the buffer at `version`. They are anchored and follow edits; a hint whose offset falls strictly
     inside a deleted range is dropped. `clear_inlay_hints()`.
     Hints for a version the buffer has never had are ignored.
   - `pub fn inlay_hints(&self) -> Vec<(usize, String)>` (offset now, label), for hosts and tests.
2. `crates/fe-gpui/src/element.rs`: lay out through `layout_frame_with_hints` (and the gutterless variant),
   each hint's text its label with a space added for each padding side; painted in the theme's muted
   foreground, at 0.9 × the editor font size, over a rounded background in the muted foreground at 10 %
   alpha. Hints on folded rows are not laid out. With no hints, every frame is laid out as today.
3. `crates/fe-gpui/src/view.rs`: record `painted_inlay_hints: Vec<(String, Bounds<Pixels>)>` (label, bounds) for
   tests, beside `painted_ghost`; clicks and caret moves use the frame's mappings, so the caret never lands
   inside a hint.
4. `crates/fe-gpui/src/lib.rs`: `pub mod inlay_hints`, and export `InlayHint`, `InlayHintKind`.
5. Tests: new `crates/fe-gpui/src/view/inlay_hints_tests.rs`, declared at the end of
   `crates/fe-gpui/src/view/window_tests.rs` as `#[path = "inlay_hints_tests.rs"] mod inlay_hints;`, in a
   headless window as `view/diagnostics_tests.rs` builds one:
   - `hints_paint_after_their_offset_and_follow_edits`: hints `: u8` (padding left) after `a` and `x:`
     (padding right) before `1` in `let a = f(1);`: `painted_inlay_hints` holds `" : u8"` and `"x: "`, each
     starting at the x where its offset's text was, and `f` is drawn after `a`'s hint; typing `bc` before
     `let` moves both hints with their text; deleting `a =` (the hint's offset strictly inside the deleted
     range) drops that hint only, and `x:` moves left with its text.
   - `clicks_and_arrows_skip_hints`: a click inside a hint's bounds puts the caret at its offset;
     `editor.move_right` across the row visits the same offsets as with no hints, and each painted caret x is
     outside every hint's bounds.

Order: the view and element files are held by T816 (mergeable), T858 (ready) and T917 (tags, also in
`element.rs`'s style spans), so this follows them. Linux-OK (painted state, no pixel goldens).

## Implementation notes

- fe-display has no layout entry point combining an annotation column with hints, so a frame with a non-zero annotation column lays out without hints; with none (width 0) it uses `layout_frame_with_hints`.
- Hints are laid out in a transparent style (so the row's line paints nothing for them) and their text is shaped and painted separately at 0.9 × the font size, centred in the laid-out rect, over a rounded 10 % chip in `gutter_foreground`.
- A hint is dropped when any edit's replaced or deleted range lies strictly around its offset; text inserted exactly at its offset goes before the hint.
- Carried T45 F1: box-selection columns now come from the shaped row (`FrameLayout::display_column_at`) when the row is shaped whole from its start with no hint or inlay; other rows keep the offset route. Test: `box_anchor_inside_a_tab_keeps_its_column`.
- Review round 1: hints whose source version was compacted away are dropped (`hints_from_a_compacted_version_are_dropped`); `shift_frame` now shifts `Frame.hints` too. Hints beside a non-zero annotation column remain unlaid-out: composing them needs a combined entry point in fe-display (`lay_out_frame` is private), outside this card's scope.
