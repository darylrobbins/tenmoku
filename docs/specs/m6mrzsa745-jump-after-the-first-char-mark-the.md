---
task_uid: "01m4rbhwvk57qygwm6mrzsa745"
task: "T35"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-vim test jump_direct::every_match_shows_after_first_char: in a buffer with wh, wa, wo, a www run and a w at a row end, after g s w the overlay's offsets are every w after the caret in the view's rows except the interior of the www run; in each {c2} group the nearest is Direct with empty text and the rest are Preview; the same holds backward after g S w"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim test jump_direct::direct_is_where_c2_lands: after g s w h the cursor is on the offset that was Direct for the h group after g s w"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim tests jump_spec (jump_labels.toml) and src/tests/jump.rs operator_overlay_previews_then_shows_live_labels pass, listing the direct entries ((0, 3) and (0, 9) after gs a in ab ab ab ac)"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test element::jump_direct_tests::direct_target_marked: after g s a in ab ab ab ac, each Direct target paints a 2 px border of theme.jump_label around its char's block, with no fill and no glyph, and no label box at its offset"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-vim -p fe-gpui and cargo clippy --workspace --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Jump: after the first char, mark the targets the jump would go straight to, so every match of it in view shows (Mac T483)

Filed on helium from Mac T483 (~/coordination/helium-cards/T483.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-q.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium note (Mac note q): Mac T418 (fe-vim g operators) is on helium's trunk since a053c05, so it is not a helium dependency. Title the host commit "... (Mac T483)".

# Jump: after the first char, mark the targets the jump would go straight to

## Problem

After `{c1}`, `label::preview` (`crates/fe-vim/src/jump/label.rs` ~L93-98) leaves out the match each
`{c2}` group would autojump to ("A match its group ... would jump to has none, and is left out"). The user
typed `s w` and saw nothing on the first `wh`, `wa` and `wo` matches, so they looked missed. In
`ab ab ab ac` after `gs a`, only (0, 6) shows; (0, 3), where `b` would go, and (0, 9), where `c` would
go, show nothing (`operator_overlay_previews_then_shows_live_labels`, `crates/fe-vim/src/tests/jump.rs`
~L802).

## Change

Autojump doesn't change: the target and where `{c2}` lands stay the same. Only the overlay changes.

### fe-vim

- `JumpLabelState` (`crates/fe-vim/src/jump.rs` ~L628) gains `Direct`: after `{c1}`, the match that typing
  its `{c2}` jumps straight to. A `Direct` entry's `text` is empty.
- `label::preview` returns those targets too, as `(offset, Option<Label>)` with `None` for the autojump
  target. A match inside a same-char run is still left out. `VimState::jump_overlay` maps `None` to
  `Direct` and `Some` to `Preview`. Update both doc comments.
- While labels wait (after `{c2}`) nothing changes: the target jumped to has the caret.

### fe-gpui (`crates/fe-gpui/src/element.rs`)

- `paint_jump` draws a `Direct` entry as a marker, never as a label: a 2 px border of `theme.jump_label`
  around the char's block, with no fill and no glyph, so the char shows through. Record it in the jump
  paint log.
- The `MAX_MATCHES` cap ranks `Direct` with `Live` and `Preview`, ahead of `Concealed`.

### Expectations that list the overlay

- `crates/fe-vim/tests/jump_spec.rs`: `state_name` maps `Direct` to `"direct"`.
- `crates/fe-vim/tests/corpus/jump_labels.toml`: the header comment lists `direct`. `preview_after_first_char`
  and `preview_after_operator` list their direct entries, with label `""`.
- `crates/fe-vim/src/tests/jump.rs`: `operator_overlay_previews_then_shows_live_labels` expects the direct
  entries. T418 also edits this file, so this card depends on T418.

## Tests

- New file `crates/fe-vim/tests/jump_direct.rs`, an integration test through `HeadlessVim`, so
  `src/tests/mod.rs` doesn't change.
- New file `crates/fe-gpui/src/element/jump_direct_tests.rs`, declared from `element.rs`.

## Out of scope

Labels or markers after `{c2}`, autojump rules, theme colours (the card this depends on).

## Suggested follow-up (human action task, not a dependency)

In the running app, `s w` in a file with several `wh`/`wa`/`wo` matches shows a marker on the first of
each pair and teal labels on the rest. Typing `h` lands on the marked `wh`.
