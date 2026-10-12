---
task_uid: "01m4nzg32cajsk75sdttw0ca0h"
task: "T367"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-vim tests tests::jump::session_*: with HeadlessVim set_window_rows(10) on a 40-row buffer, gs{c1}{c2} whose visible matches fit the safe labels autojumps to the nearest and labels the rest, a label then goes to its target, and a non-label key falls through and acts as itself; more matches leave the caret and label from the full set, \u003cSpace\u003e/\u003cBS\u003e page them; matches outside the viewport get no label; \u003cEsc\u003e changes nothing; \u003cCR\u003e as c1 reuses the last chars"
    required: true
    manual: false
  - id: "C2"
    text: "fe-vim tests tests::jump::operator_*: dgs{c1}{c2} with several visible matches never autojumps, a label deletes through the target (exclusive backward with gS), register and undo as for the counted motion, . repeats it as that counted jump from the new caret; in v and V a label extends the selection; jump_overlay gives Preview labels after c1, Live then Concealed pages after c2 and None with no jump; jump_query gives the typed chars"
    required: true
    manual: false
  - id: "C3"
    text: "fe-vim test tests::jump::macro_records_resolved_jump: qq then a labelled jump then q leaves register q holding {n}gs{c1}{c2}, and @q after \u003cC-e\u003e scrolls the view lands on the same match; a cancelled jump leaves none of its keys in the register"
    required: true
    manual: false
  - id: "C4"
    text: "fe-vim tests tests::jump::opt_in_s: with set_jump_on_s(true), s and S start the forward and backward jump in normal mode, visual mode and after an operator (ds{c1}{c2}); with the default false, s S v_s v_S do what they did and gs gS still jump"
    required: true
    manual: false
  - id: "C5"
    text: "fe-vim test jump_spec (tests/jump_spec.rs) passes over tests/corpus/jump_labels.toml, at least 25 hand-written cases with window_rows, expected cursor, buffer, mode and labels shown; every earlier corpus and test passes unchanged; scripts/check-no-gpui.sh prints ok: fe-vim"
    required: true
    manual: false
---

# fe-vim: the labelled jump session: autojump to the nearest visible match with safe labels on the rest, label paging, macros that record the resolved jump, and an s/S opt-in

# fe-vim: the labelled jump session: autojump to the nearest match, labels on the rest, macros that record the resolved jump, and an `s`/`S` opt-in

Phase 1a(ii) of the leap/flash plan (`leap-flash-design.md` §3A and §6). This is the stateful half on
top of the jump motion card it depends on, which provides `gs`/`gS`, `Motion::Jump` and
`jump::label`.

## Problem

After the motion card, an uncounted `gs{c1}{c2}` goes to the nearest match, wherever that is. The user's
2026-10-11 decisions still need the labelled UX:
- **Style:** "leap first: 2 characters, autojump to the nearest match, labels on the rest".
- **Macros:** they record the resolved jump, so a replay reaches the same target whatever is on screen.
- **Keys:** "allow opt-in to replace s / S".
- **Safe labels:** leap's set (`sfnut/SFNLHMUGTZ?`, from the motion card), confirmed by the user on 2026-10-11:
  typing one of them right after an autojump picks a label.

## Change

### The session (`crates/fe-vim/src/jump.rs`, `src/state.rs`)

Without a count, `{c2}` commits `vim.jump {chars, forward, count, register, operator}` instead of the
motion. A count still commits the motion directly, with no labels. `jump::FEATURE.register` registers
`vim.jump`. Its handler does the following:
- **Finding targets.** It reads the viewport through `EditorHost::viewport()` (T229; its documented
  fallback when `None`) and finds `jump::targets` in it.
- **No target:** fails as the motion does.
- **One target**, or an **autojump** (outside op-pending, everything after the first fits the safe
  labels): it runs the counted motion to the first target through the same code a committed
  `vim.move`/`vim.operate` runs. So `.`, registers and marks see a counted jump.
- **Other cases:** it keeps the label table on `VimState` and leaves the pending jump waiting for a label.
  `vim.pending_kind` is `jump`, so the host routes every key to vim, and a click's `cancel_vim` (it feeds
  `<Esc>`) ends the session.

While labels wait, `VimState::feed` takes the key before `step`:
- **A label** commits `vim.move`/`vim.operate` with `Motion::Jump` and count = that target's
  direction-relative index. After an autojump, it counts from where the jump started. `''` is the
  position before the jump, even after an autojump and a label.
- **`<Space>` / `<BS>`** page the labels forward and back when there are more targets than labels.
- **`<Esc>`** ends the session. After an autojump the caret stays on the first target.
- **Any other key after an autojump** ends the session and is fed to `step` as an ordinary key, so it acts
  in normal or visual mode. Without an autojump, a non-label key cancels.
- **`<CR>` as `{c1}`** jumps with the last jump's chars and direction.

In op-pending there is never an autojump with more than one target. A forward pick is inclusive and a
backward one exclusive, as the motion card defines.

### The overlay and query (for the GUI card)

- `VimState::jump_overlay(&self, text, rows: Range<u32>) -> Option<JumpOverlay>`. It returns
  `labels: Vec<JumpLabel { offset, text, state: Preview | Live | Concealed }>`, where `offset` is the
  matched `{c1}`.
  - **After `{c1}`:** each candidate in `rows` shows, as `Preview`, the label it would get in its `{c2}`
    group.
  - **While labels wait:** the stored table's current page is `Live`, and targets on other pages are
    `Concealed`.
  - **With no jump pending:** `None`.
- `VimState::jump_query() -> Option<&str>`: the typed chars (`""`, `"a"`, `"ab"`) while a jump is
  pending.
- `HeadlessVim::jump_overlay()` uses its virtual viewport (T229).

### Macros (`src/macros.rs`)

- **A jump lands** (directly, by autojump or by a label): the keys it recorded, from its trigger through
  the key that picked it (paging keys and an `<Esc>` after an autojump included), are replaced by
  `{n}{trigger}{c1}{c2}`. `n` is the direction-relative index, so the replay is a counted jump that shows
  no labels and reaches the same match whatever the viewport. `{c1}{c2}` are the actual chars even after
  `<CR>`.
- **A key that falls through** after an autojump is recorded after them as itself.
- **A cancelled jump** leaves none of its keys.

### The `s`/`S` opt-in (`src/state.rs`, `src/headless.rs`)

- `VimState::set_jump_on_s(bool)` and `HeadlessVim::set_jump_on_s(bool)`, default `false`.
- **When on:** `feed` takes `s` and `S` with no prefix pending, in normal mode, a visual mode or after an
  operator, before `step`, as `/` is taken (`Pending::command_line`). They then start the forward and
  backward jump exactly as `gs` and `gS` do. `gs`/`gS` keep working either way.
- **When off:** `s`, `S`, `v_s` and `v_S` are unchanged (`cl`, `cc`, `c`, linewise change).
- The editor's setting that sets this flag is a separate card.

## Tests (new files; the motion card's `src/tests/jump.rs` is extended)

- `crates/fe-vim/src/tests/jump.rs`: session unit tests with `HeadlessVim` and `set_window_rows`.
- `crates/fe-vim/tests/jump_spec.rs`, plus the hand-written `tests/corpus/jump_labels.toml`.
  - Each case gives `buffer`, `cursor`, `window_rows`, a scroll before the keys if needed, `keys`, and the
    expected `cursor`, `buffer`, `mode` and the labels shown (`[[row, col, "label", "live"]]`) after the
    keys.
  - No recorder. The file comment says these are our spec, after leap.
  - The operator, register, visual and `.` semantics stay oracle-checked by the motion card's
    differential corpus, because a picked label is that counted motion.

## Out of scope

- Painting and the status bar (GUI card).
- The fe-config setting.
- Bidirectional and cross-window jumps.
- Folds.
- Leap's traversal (`<CR><CR>`).
- Label sets as settings.
- Any change outside `crates/fe-vim`.

## Size

About 550 non-test lines.

## Implementation notes

- **Spec conflict with the motion card's tests (C2 vs C5).** Seven `jump.toml` cases (`dot_*`) and
  the unit test `dot_finds_the_match_again` type an uncounted `dgsab.` with several matches in view.
  Under C2 that jump now waits for a label, so `.` cancelled it. Those cases now type the first label,
  `s` (`dgsabs.`), which is the counted jump to the nearest, as before. Their `oracle_keys` didn't
  change. `jump.expected.json` changed only in those `keys` fields, and re-recording every corpus with
  nvim 0.12.4 reproduces all expected files byte for byte. Nothing else earlier changed.
- **`tests/corpus/jump_labels.toml` isn't an nvim corpus.** `tests/oracle.rs` took every
  `tests/corpus/*.toml` as one, for `corpus_has_at_least_150_cases` and the recorder. They now share
  `support::recorded_corpora()`, which leaves out `jump_labels`.
- **Outside the listed files, inside fe-vim:** `src/reducer.rs` gains accessors (`operator`,
  `has_prefix`, `jump`), a constructor for the pending jump while labels wait (`jump_labels`), and
  `jumping` ends a typed count (for `3s`). `src/commands.rs` moves the bodies of `vim.move` and
  `vim.operate` into `run_move` and `run_operate`, which `vim.jump` calls ("the same code a committed
  `vim.move`/`vim.operate` runs"). `src/motion.rs` calls `jump::restart` first (next note).
  `src/scroll.rs`'s `viewport` (the documented fallback) is `pub(crate)`.
- **A label after an autojump counts from where the jump started.** The label commits `vim.move` with
  that count. Before the motion runs, `jump::restart` puts the carets (or the visual ends) and goal
  columns back where the jump started. So the count resolves from there, and `''` is that position.
- **Counts are found over the whole text.** A target's count is its index among all the matches from
  the caret, not only those in view. They're the same whenever the caret is in view. A host whose
  view doesn't hold the caret still gets the target the label shows.
- **`jump_overlay` takes the model:** `jump_overlay(&self, model: &EditorModel, rows)`, like
  `visual_cursor`. The preview needs the caret (the vim cursor in a visual mode) to order each
  group's targets, and the state alone doesn't know it. Labels come in text order. A target the jump
  went to (an autojump's first, or one a group would autojump to) has no label.
- **Preview groups:** a candidate's `{c2}` group is the base char of the grapheme after it, or
  `<Space>` for a blank, a tab or the row's end. A match inside a same-char run has no group label.
- **Paging** goes round: `<Space>` on the last page goes to the first, and `<BS>` on the first goes to
  the last. With one page they aren't paging keys: after an autojump they fall through, otherwise they
  cancel.
- **Macros write the trigger as `gs`/`gS`,** even after an opted-in `s`/`S`. So the replay is the
  same jump whether or not the opt-in is still on. The count goes after any register or operator
  typed before the trigger (`"ad2gsab`, `v2gsab`). A cancelled jump drops its whole command's keys,
  from the first key typed with nothing pending (`"a3d` included). Dropping only the trigger would
  leave a lone operator to take the next recorded key. A jump that finds no target isn't cancelled.
  Its keys stay, as a failed motion's do.
- **`<CR>` as `{c1}`** with no earlier jump cancels, failing as a cancelling key does. A count before
  it gives the counted motion with the last chars.
- **`vim.jump`** takes `{chars, forward, count, register, operator}` (`JumpArgs`, in `jump.rs`). With
  a `count` it runs the counted motion. Keys never commit it with one.
- **Stale labels (review rounds 1 and 2, F1).** A session records a `Basis`: the text's version, the
  caret it expects (the jump's start, or after an autojump the first target) and every selection.
  If another origin's edit, or a caret added, removed or moved some other way (a secondary one
  too), changes any of it while labels wait, `jump_overlay` shows none. A label
  picked then fails, with nothing changed and the message "jump: the labels are stale".
  `jump::check_pick` makes the same check when the label's `vim.move` or `vim.operate` runs, the first
  thing `commands::run_move` and `run_operate` do. That same step puts the carets back after an
  autojump (this replaces `jump::restart` in `motion.rs`), and only then is the jump recorded as
  landed. A failed pick leaves the recording with the autojump, if there was one, and otherwise none
  of the jump's keys.
- **`:normal` (T424, rebased onto).** Its keys aren't typed (`Macros::hold`), so the macro jump hooks
  (`command_start`, `jump_start`, `jump_landed`, `jump_cancelled`) skip them as `record` does: a
  recording keeps the `:normal` line, not the jump its keys make. A session its keys leave, after an
  autojump too, ends with the `<Esc>` that `:normal` feeds to a pending command
  (`tests::jump::session_a_normal_starts_ends_with_it_unrecorded`).
- **T414's hooks** (`Pending::motion`, `with_operator`, `operate`, `object`) aren't needed: `vim.jump`
  commits from `jump::key`, and `Motion::Jump` was already in place. This branch leaves `motion.rs`
  unchanged; `Pending::operator` serves both.
