---
task_uid: "01m54vs16bv4px6n2a780t0hc6"
task: "T144"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-terminal-view test ge_and_big_ge_move_back_to_the_previous_word_end (tests/vi_word_end_back.rs, top level) passes: in a /bin/sh view printing \"one two three four\", \"ab cd(ef) gh\", \"ready\", after ctrl-shift-space, in sequence k k $ g e → (\"one two three four\", 12); g e → 6; $ 2 g e → 6; $ g shift-e → 12; $ 3 g shift-e → 2; j $ g shift-e → (\"ab cd(ef) gh\", 8); g shift-e → 1; $ g e → 8; g e → 7 (vim's columns); it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Unchanged: g g still goes to the top, g followed by any other key still drops the g, and no key reaches the pty in vi mode (tests/vi_mode.rs no_key_reaches_the_pty_in_vi_mode passes as is)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-terminal -p fe-terminal-view passes with no existing test edited; cargo clippy -p fe-terminal -p fe-terminal-view --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Move a terminal's vi copy-mode cursor back to the previous word's or WORD's end on ge and gE, counted, as vim does, instead of forward as e (Mac T771)

## Problem

At Mac main 8f606ec8 a terminal's vi copy mode takes `ge` as `e`: `crates/fe-terminal-view/src/vi.rs` (`ViKeys`'
key reader) matches `(Some('g'), ViKey::Char('g'))` only, and "anything else drops the first key and is taken on
its own", so `g e` moves *forward* to the next word's end (onto the next row from a row's last word) and `g E` to
the next WORD's end. In vim, and in alacritty's and tmux's vi modes, `ge`/`gE` go back to the end of the previous
word/WORD, with a count.

Repro (prototyped 2026-10-17 in a `/bin/sh` view printing `one two three four`, `ab cd(ef) gh`, `ready`):
`ctrl-shift-space k k $ g e` leaves the vi cursor on `ab cd(ef) gh` column 1, where vim's `ge` gives `one two three
four` column 12.

alacritty_terminal already has the motions: `ViMotion::SemanticLeftEnd` (as `b` uses `SemanticLeft`) and
`ViMotion::WordLeftEnd` (as `B` uses `WordLeft`).

## Change

- `crates/fe-terminal/src/vi.rs`: `ViMove::WordEndPrevious` (`ge`) and `ViMove::BigWordEndPrevious` (`gE`), with
  doc lines like their neighbours', mapped in `ViMove::motion` to `ViMotion::SemanticLeftEnd` and
  `ViMotion::WordLeftEnd`.
- `crates/fe-terminal-view/src/vi.rs`: `(Some('g'), ViKey::Char('e'))` and `(Some('g'), ViKey::Char('E'))` return
  `ViCommand::Move` of those, counted (`times`), as `w`/`e` are; update the module's key list in the docs if it
  lists the motions.

New integration test `crates/fe-terminal-view/tests/vi_word_end_back.rs` (its own copy of vi_mode.rs's `style`,
`open`, `draw` and `on` helpers; no shared test file edited), top-level gpui test
`ge_and_big_ge_move_back_to_the_previous_word_end`: in a view running
`printf 'one two three four\nab cd(ef) gh\nready'; sleep 60`, after `ctrl-shift-space`, in sequence:
`k k $ g e` → (`one two three four`, 12); `g e` → 6; `$ 2 g e` → 6; `$ g shift-e` → 12; `$ 3 g shift-e` → 2;
`j $ g shift-e` → (`ab cd(ef) gh`, 8); `g shift-e` → 1; `$ g e` → 8; `g e` → 7 (vim's columns for both rows).
