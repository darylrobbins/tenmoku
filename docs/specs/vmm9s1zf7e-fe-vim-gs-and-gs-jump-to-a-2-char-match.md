---
task_uid: "01m4nzg1sc84xp2nvmm9s1zf7e"
task: "T366"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim proptests tests::jump::labeler_*: for any target count and op_pending, jump::label gives labels unique within a page, from the safe set when it autojumps and the full set otherwise, never autojumps in op-pending with more than one target, reaches every target through its pages, and gives the same result for the same input; jump::targets returns only matches inside the given rows, nearest first in the direction"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim tests tests::jump::*: gs{c1}{c2} and gS{c1}{c2} move to the nearest match forward and backward over the whole text, {n}gs to the nth; matching folds case, \u003cSpace\u003e matches space, tab or a line end (\u003cSpace\u003e\u003cSpace\u003e an empty row), a same-char pair matches a run only at its start, where two chars are the same only when case folding makes them equal (the \u003cSpace\u003e wildcard plays no part, so a space then a tab, or a tab then a space, is a distinct pair matched wherever it occurs, with tests for both); no match fails with nothing changed; vim.pending_kind is jump while the chars are awaited and \u003cEsc\u003e cancels"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test jump_matches_neovim (tests/jump_oracle.rs): tests/corpus/jump.toml has at least 30 hand-written cases with oracle_keys (mark z set at the spec target, then v`z or `z; an equivalent v/ search for dot cases) covering d c y with and without a register, backward and counted jumps, v V and \u003cC-v\u003e extending, undo and dot-repeat; each matches nvim 0.12.4 but for mark z and register /"
    required: true
    manual: false
  - id: "C4"
    text: "Every earlier corpus expected file is byte-identical and every earlier fe-vim test passes; src/reducer.rs gains only Prefix::Jump, PendingKind::Jump, the hook constructor and one complete arm; scripts/check-no-gpui.sh prints ok: fe-vim"
    required: true
    manual: false
---

# fe-vim: gs and gS jump to a 2-char match as a counted motion that composes with operators, visual mode and dot-repeat, checked against nvim by swapping in a mark motion

# fe-vim: the 2-char jump as a motion (gs / gS), counted, composing with operators, visual mode and `.`

Phase 1a(i) of the leap/flash plan (design note `leap-flash-design.md` §3A and §6, 2026-10-11; the
leap survey beside it). This card is the static half: the matcher, the pure labeler and the jump
**as a motion** with no labels on screen. The next card adds the labelled session (autojump, labels,
paging, macros, the `s`/`S` opt-in) on top of it.

## Problem

fastedit's vim has no leap/flash-style jump, the plugin feature nvim users rely on most (`s{c1}{c2}`
with labels, also as an operator target: `ds…`, `ys…`). nvim has no built-in jump, so there's nothing to
record it against.

**Decided by the user (2026-10-11):**
- Keys: "Other default keys but allow opt-in to replace s / S". vim's `s` and `S` stay `cl` and `cc` by
  default; a setting (later card) lets the jump take them.
- Style: leap first: 2 characters, autojump to the nearest match, labels on the rest.
- Fidelity: "Our own spec": our behaviour spec, borrowing leap's ideas; no pinned leap oracle. Test with
  property tests for the labeler, a hand-written HeadlessVim corpus, and a differential check against
  nvim that swaps each jump for a mark motion to the same spot.
- Macros record the resolved jump (next card).

## Change

### Keys (confirmed by the user, 2026-10-11)

- **`gs{c1}{c2}`** jumps forward, **`gS{c1}{c2}`** backward, in normal mode, the visual modes and after
  an operator. `gs` is nvim's `:sleep` (`:help gs`, no mapping by default) and `gS` has no meaning in
  nvim 0.12 (no help tag, no default mapping, checked with `nvim --clean`); fe-vim's `g` prefix takes
  only `gg` and `gv`, and no queued card takes either (T227 takes `g<C-a>`/`g<C-x>`). They are the
  `s`/`S` pair behind `g`, which is also where users who opt in later find them. A directional pair
  (as sneak and classic leap) gives `S` a meaning now and gives counts, `.` and macros a
  direction-relative index.
- The keys after an operator never reach the keymap (`crates/fe-gpui/src/keymap.rs` hands every key to
  vim while `vim.pending` is set), so the trigger is fe-vim's, registered through the feature seam.

### The feature module (`crates/fe-vim/src/jump.rs`, new, `pub mod jump`)

- `pub(crate) const FEATURE` added to `feature::FEATURES` (`src/feature.rs`, from T365). Its `g` hook
  takes `s` and `S` after `g` (with whatever count, register, operator or visual mode is pending) and
  returns `Step::Wait` with the pending jump. Every other key returns `None`.
- `src/reducer.rs` gains only `Prefix::Jump(jump::JumpPrefix)`, `PendingKind::Jump` (`vim.pending_kind`
  `jump`), a `pub(crate)` way for the hook to build that `Pending`, and one arm in `complete` that hands a
  `Prefix::Jump` key to `jump.rs`. Collecting the two chars stays pure. `<Esc>` cancels with nothing
  changed. Keys and behaviour live in `jump.rs`.
- After `{c2}`, the jump commits `vim.move` (with `extend` in a visual mode) or `vim.operate`, carrying
  `Target::Motion(Motion::Jump(JumpMotion { chars, forward }))` and the pending count, like any motion.
  So registers, `'[`/`']`, undo grouping and `.` work through the existing paths.

### The motion (`src/args.rs`, `src/motion.rs`)

- `Motion::Jump(JumpMotion)` is serializable (`{"jump": {"chars": "ab", "forward": true}}`) and stays
  `Copy` (two `char`s). It goes to the `count`th match (1 if there's no count) after the caret, or before
  it if not `forward`, over the **whole text** with no wrap. It is resolved in `motion.rs` from the
  caret, as `Search` is. With no such match it fails like a failed motion: it beeps, changes nothing and
  drops the pending command.
- **Matching** (our spec, after leap):
  - Each typed char matches regardless of case (folding is symmetric, so preview groups stay exact).
  - `<Space>` typed as a char matches a space, a tab or a line end. So `{c}<Space>` reaches a row's last
    char, and `<Space><Space>` reaches an empty row (at its start).
  - A pair of the same char (`==`) matches a run (`=====`) only at its start.
  - The jump lands on `{c1}`'s position.
- `class()`: forward is `Inclusive` (`dgsab` deletes through the `a`) and backward is `Exclusive`, as
  `f`/`F`. `is_jump()` is true, so `''` is set. `writes_numbered()` is true, as `` ` ``.
- `.` re-resolves the motion from the new caret. `{n}.` replaces the count, as for every other motion.

### Matching and labelling, pure (`src/jump/label.rs`, new)

- `pub fn targets(text, rows: Range<u32>, caret, chars, forward) -> Vec<usize>`: the matches within
  `rows`, nearest first in that direction.
- `pub fn label(count: usize, op_pending: bool) -> Labeled`. It returns whether to autojump and each
  target's label, page by page.
  - **Labels:** leap's defaults (the safe set confirmed by the user, 2026-10-11). The safe set is `sfnut/SFNLHMUGTZ?` and the full set is
    `sfnjklhodweimbuyvrgtaqpcxz/SFNJKLHODWEIMBUYVRGTAQPCXZ?`.
  - **Autojump** happens when everything after the first target fits the safe set, never in op-pending
    with more than one target.
  - **Other cases:** without autojump, the full set labels page 1 and further pages repeat it. A single
    target needs no label.
  - Unused by keys until the next card. It is `pub` so the host's overlay can share it.
- The k-th labelled target in a direction is the k-th match from the caret in that direction: the rows
  between a visible caret and a visible target are all visible. So a label always resolves to the
  counted motion.

### Oracle support (`tests/support/mod.rs`, `tests/support/oracle.lua`)

- `Case` gains an optional `oracle_keys`. When it is set, `oracle.lua` feeds those keys to nvim instead of
  `keys`. It is skipped when serialized, so every earlier expected file stays byte-identical.
- The jump runner compares fastedit's outcome for `keys` with nvim's outcome for `oracle_keys`. It drops
  mark `z` and register `/` from nvim's side, since only the swap leaves them.

## Tests (new files)

- `crates/fe-vim/src/tests/jump.rs` (one `mod` line in `src/tests/mod.rs`): unit tests and proptests.
- `crates/fe-vim/tests/jump_oracle.rs` (`#[allow(dead_code)] mod support;`), plus
  `tests/corpus/jump.toml` and `jump.expected.json`.
  - **Hand-written cases** with `oracle_keys` that swap the jump for a mark motion to where our spec says
    it lands: `` :call setpos("'z",[0,R,C,0])<CR> `` and then `` v`z `` (forward, after an operator) or
    `` `z `` (backward, normal or visual).
  - **`.` cases** use the equivalent search instead (`dv/ab<CR>` then `.`, patterns with no regex
    specials and no wrap), because nvim's `.` repeats a mark motion to a fixed place.
  - The file comment says this.

## Out of scope

- Labels, autojump, paging, `<CR>` repeat, macros, the `s`/`S` opt-in (next card).
- Painting (later card).
- Closed folds and long rows (follow-ups).
- Bidirectional and cross-window jumps.
- Remote/visit (Phase 2).
- Any change outside `crates/fe-vim`.

## Size

About 450 non-test lines.

## Implementation notes

- **`src/operator.rs` changed by one guard** (outside the listed files, inside fe-vim). The corpus
  case `delete_line_end` (`dgsh<Space>` from the first row's start to the last row's last char)
  showed that `` dv`z `` keeps the delete charwise: a `v`-forced motion skips the `:help d` rule
  that makes a multi-row delete linewise. fe-vim applied that rule to `dgs`. The forward jump now
  skips it, as `v` does; `gS` stays a plain exclusive motion, as `` d`z `` is.
- **`''` on a failed jump:** `marks::jump` sets the jump mark for any jump motion before it runs.
  `motion.rs` now sets it only if the jump finds its match from the vim cursor or primary caret
  (`jump::lands`), so a jump with no match changes nothing. The operator path already skips a
  target that fails.
- **Property tests without a crate:** fe-vim has no `proptest` dev-dependency, and adding one would
  change `Cargo.toml` and `Cargo.lock`. The `labeler_*` tests check every count up to four full pages
  (216) with and without an operator. That covers the labeler's whole input space up to that size,
  and past one page it only repeats. `labeler_targets_*` runs 3000 seeded-random texts, row ranges,
  carets, chars and directions against a slow reference matcher.
- `parse_keys` has no `<Space>`, so the tests and corpus type a space as itself.
- **`src/visual.rs` gains `Motion::Jump` in one arm** (outside the listed files, inside fe-vim). A
  visual cursor at a row's end (after `$`) resolves the jump from that row end, not clamped to the
  last char, as `/`, `?` and `f` do. So `v$gSc<Space>` reaches the row's own last `c`, and the
  k-th counted jump is `targets(…)[k-1]` from the visual cursor (review round 1, F1). `jump::lands`
  starts from the same place. The corpus covers it in `v`, `V` and `<C-v>`
  (`visual_*row_end_*`).
- **Case folding** is Unicode simple case folding (CaseFolding.txt's `C` and `S` mappings), taken
  from `regex-syntax`, already a dependency (`ClassUnicode::case_fold_simple`). Each typed char
  matches its fold's whole orbit, and run detection uses the same rule. So `Σ σ ς`, `S s ſ`, `k K`
  with the Kelvin sign, `ΐ ΐ`, `ΰ ΰ` and `ﬅ ﬆ` each match alike, and `ı`, `İ` and `ß` match only
  themselves. The random matcher test draws from such classes, not only ASCII (review rounds 1
  and 2, F2).
- **Same-char runs (rev 3, F3):** two typed chars are the same only when their case folds are,
  without the `<Space>` wildcard. So `[' ', '\t']` and `['\t', ' ']` are distinct pairs, matched
  wherever they occur. A tab can be typed only through a motion's args, since the key cancels.
  `<Space>` still takes a tab or a line end within a pair. The random matcher test now types tabs
  as well.
