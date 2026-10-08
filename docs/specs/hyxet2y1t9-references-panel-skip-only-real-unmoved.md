---
task_uid: "01m4e2kdy2x0yyswhyxet2y1t9"
task: "T196"
rev: 2
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "Regression (T195 F1, first case) fe-workbench test tests::references_landing::revisiting_the_latest_landing_after_moving_away_is_recorded: open a src/a.rs reference from the panel. Once it lands, press right in the editor, then select exactly the range the reveal selected (read with at after landing), and open main.rs's reference. Back returns to (src/a.rs, that range's end), and Back again to where shift-F12 was pressed. Fails at base"
    required: true
    manual: false
  - id: "C2"
    text: "Regression (T195 F1, second case) tests::references_landing::a_reveal_cancelled_by_a_tab_switch_is_not_a_landing: with src/a.rs not open, open its reference and, in the same update, activate the pane's main.rs tab. After the read lands in the background, activate a.rs's tab, set the cursor to that reference's end, and open main.rs's reference. Back returns to (src/a.rs, that end). Fails at base"
    required: true
    manual: false
  - id: "C3"
    text: "Regression (reveal failure) tests::references_landing::a_reveal_whose_read_fails_is_not_a_landing: with src/a.rs replaced by a directory, opening its reference shows the read error. Then restore the file with its old text, Pane::open it in the same pane, set the cursor to the reference's end, and open main.rs's reference. Back returns to (src/a.rs, that end). Fails at base"
    required: true
    manual: false
  - id: "C4"
    text: "Regression (superseded outside the panel) tests::references_landing::a_reveal_superseded_by_a_plain_open_is_not_a_landing: while a.rs's reference is being read, Pane::open the same file in that pane. Once it lands at offset 0, set the cursor to the reference's end and open main.rs's reference. Back returns to (src/a.rs, that end). Fails at base"
    required: true
    manual: false
  - id: "C5"
    text: "Existing tests pass unchanged: tests::references_panel::* (including moving_to_an_unopened_reference_end_is_recorded for T112 F4, moving_back_to_an_earlier_opened_end_is_recorded and moving_to_the_end_of_a_superseded_reference_is_recorded for T191 F1, opening_records_where_the_caret_moved_to, a_clamped_reference_counts_as_where_the_caret_landed, a_landing_is_followed_in_its_own_pane, a_reference_landing_in_another_pane_meanwhile, another_reference_opened_while_one_is_read and asked_for_at_the_end_of_a_reference), tests::nav::* and tests::nav_pending::*. The one exception is back_and_forward_while_a_reference_is_read, whose final steps change as the spec's Decided section says, with a comment. tests/references_panel.rs otherwise changes only helper visibility"
    required: true
    manual: false
  - id: "C6"
    text: "Check: a landing is decided by the destination's lifecycle (pending, landed with the reveal's actual selection, moved, dropped), per pane. rg -n 'fn lands_at|opened:' crates/fe-workbench/src/references_panel.rs prints nothing, and no landing decision compares the caret with a reference's range, raw or clamped. nav.rs, workbench.rs, the keymap and fe-editor are unchanged"
    required: true
    manual: false
  - id: "C7"
    text: "Check: cargo nextest -p fe-workbench and clippy pass, T81's golden screenshot check passes unchanged, and tests/references_landing.rs and the edited tests/references_panel.rs build against base"
    required: true
    manual: false
---

# References panel: skip only real, unmoved panel landings in Back history

# References panel: skip only real, unmoved panel landings in Back history

This card replaces the review-follow-up chain T112 → T191 → T195 → T196. The user decided
(2026-10-08) to stop patching it round by round: rework the approach as a whole, with a test for
every case the reviews raised.

## Symptom

Opening a reference from the References panel (`ReferencesPanel::open`,
`crates/fe-workbench/src/references_panel.rs:316`) records where the caret is in Back history,
unless the caret is still where the panel's previous open left it. That way, going through the list
and then pressing Back returns at once to where shift-F12 was pressed. The check, `lands_at`
(references_panel.rs:351–371), compares the caret's offset with remembered reference ends. So a
place the user chose is skipped whenever its offset happens to equal one of them, and Back no longer
returns there. Each round narrowed the remembered set, and each review found the same mistake again:

- **T112 F4** (references_panel.rs:352–353): every listed reference's end, raw or clamped, counted as
  a landing, including references never opened. Fixed in T191
  (`moving_to_an_unopened_reference_end_is_recorded`).
- **T191 F1** (references_panel.rs:78, :330, :355–359): a run-wide `opened` list. Open A, open B, move
  back to A's end, open C: A's end is skipped. A superseded pending read stayed eligible forever, and
  entries had no pane. T195 made it a per-pane map
  (`moving_back_to_an_earlier_opened_end_is_recorded`,
  `moving_to_the_end_of_a_superseded_reference_is_recorded`).
- **T195 F1** (references_panel.rs:80, :332, :335, :338, :365), still open: the per-pane `opened`
  map holds requested offsets, which change only on panel opens.
  - Open A, move away and back to A's end, open C: A's end is skipped.
  - Request A, cancel its pending reveal by activating another tab, then activate A's tab once it's
    read and move to A's end: that counts as a landing, though the reveal never landed.

The reviewer asked each time for the same fix. Track pending requests and confirmed landings per
pane, observe caret movement and reveal cancellation or failure, and invalidate stale entries on
those transitions. Preserve clamping and Back/Forward behaviour.

## The model

A panel open in pane P is a **destination**. It has a lifecycle, and only its current state counts:

- **Pending**: P is still reading the file for *this* open, with this open's reveal. The caret
  place the workbench reports then (`Pane::pending_place`) is the destination's, so it is not
  recorded. This keeps `another_reference_opened_while_one_is_read` passing.
- **Landed**: P applied this open's reveal to an editor, at once or when the read finished. This
  records that editor and the selection the reveal actually made, after clamping. A later open
  from this caret is not recorded while the editor is P's active one and its selection hasn't moved
  since. This keeps the clamped and own-pane tests passing.
- **Moved**: after landing, the panel observes the editor. The first change to its primary selection
  away from the landed one ends the landing for good. Coming back to the same offset, or the same
  range, is the user's own choice and is recorded. The check at open time also compares the current
  selection, so a change made without a notify is caught too.
- **Dropped**: the reveal will never land. Another `open_at` in P superseded it (from the panel or
  anywhere else, a plain open of the same file included), a tab activation replaced the read, the
  read failed, or P went away. A dropped destination is never a landing.

Each pane holds at most one destination: a new open in P replaces P's. "Where shift-F12 was
pressed" needs no special case and `from` stops being part of the check.

### Mechanism (suggested; the requirement is the model above)

- `crates/fe-workbench/src/pane.rs`, a small hunk:
  - an `open_serial` that every `open_at` (and so `open`) bumps;
  - `revealed: Option<Revealed { serial, editor: EntityId, selection: Range<usize> }>`, set where a
    reveal is applied (`open_at`'s two immediate paths and `file_read`) from the editor's selection
    right after `Reveal::select`;
  - `pub(crate)` getters for both.
  Then "landed" is `revealed.serial == destination.serial` exactly, with no offset inference.
- `references_panel.rs`:
  - replace `opened` and `lands_at` with a per-pane `Destination { serial, path, state }`;
  - observe the pane while a destination is pending, and the landed editor after it lands;
  - keep the `Subscription`s in the destination, so replacing it drops them.

  If this would push `references_panel.rs` past about 1,000 lines, put the model in a new
  `crates/fe-workbench/src/references_panel/landing.rs`, declared by `mod landing;` in
  `references_panel.rs`.
- `Workbench::list_jump` (nav.rs) keeps its signature and its `last != here` check. Don't change
  nav.rs.

## Decided (lead default, 2026-10-08; the user may revisit)

A place that Back or Forward restored is not a panel landing. In
`tests::references_panel::back_and_forward_while_a_reference_is_read`, the reveal for a.rs is dropped
by Back. Forward then puts the caret on a.rs's reference end, and opening main.rs's reference from
there records it. The test's final steps become: Back returns to (a.rs, that end), then Back again to
where shift-F12 was pressed. Change only that expectation, with a comment, and list it in the finish
notes. Every other existing test in `tests/references_panel.rs` keeps its expectations.

## Tests

- New file `crates/fe-workbench/src/tests/references_landing.rs`, declared by
  `mod references_landing;` in `crates/fe-workbench/src/tests/mod.rs` (no `#[path]`).
- The helpers it needs from `tests/references_panel.rs` (`project`, `opened`, `settle`,
  `focus_editor`, `caret_on`, `at`, `references`, `shown`, `click`, `click_now`) become `pub(super)`.
  That is the only other change to that file, apart from the decided expectation above.
- The tests may use only items that exist at base: `Pane::open`, `activate`, `is_opening` and
  `tabs`, `PaneEvent`, the editor model's `set_cursor` and `select_range`, and keystrokes. Never the
  new serial or `revealed` getters. Each is headless GPUI on the a.rs/main.rs fixture the existing
  landing tests use.
- Run each regression test at base before implementing. Each must fail there.

## Don't

- Don't decide "landed" by comparing the caret with reference offsets, raw or clamped. Don't keep a
  run-wide or per-pane list of requested ends.
- Don't put Back/Forward or `from` special cases back in.
- Don't change nav.rs, workbench.rs, the keymap or fe-editor.

## Implementation notes

- `Pane` has an `open_serial`, bumped by every `open_at`, and `revealed: Option<Revealed { serial,
  editor, selection, shown }>`, set by `Pane::apply` wherever a reveal is applied (both immediate
  paths and `file_read`). `shown_serial` counts changes to the editor shown in the active tab's
  place (`activate` to another editor, `add`, `take` of the active tab, the host's `opening`).
  `activate` also drops the reveal of a read it replaces, so a cancelled reveal can't land later.
- The model lives in `references_panel/landing.rs`: one `Destination { serial, path, state }` per
  pane, with `State::{Pending, Landed, Moved, Dropped}`. A pending destination observes its pane and
  lands when `revealed.serial` is its serial; it's dropped once the pane's serial moves on or it no
  longer reads the file. A landed one observes its editor and moves for good on the first change to
  the primary selection.
- A landing also ends once the pane makes any later open (Back/Forward included) or shows anything
  else in the editor's place (pre-review finding: switching tabs away and back, or Back then a tab
  click, otherwise kept an unmoved landing). Tests:
  `returning_to_a_landing_by_its_tab_is_recorded`, `returning_to_a_landing_after_back_is_recorded`.
- The panel's `from` field is gone; `restart` clears the destinations.
- `back_and_forward_while_a_reference_is_read`'s final steps changed as Decided, with a comment.
- Review round 1: the pane tracks the shown editor by identity (`note_shown`, called after every
  change to the active tab, a read or a read error), so dragging tabs away and back ends a landing
  and reordering the landed tab in place keeps it. `Revealed::selection` is the whole
  `ResolvedSelection`, so reversing a landed selection ends the landing. Tests:
  `returning_to_a_landing_by_dragging_tabs_is_recorded`, `reordering_a_landed_tab_keeps_the_landing`,
  `reversing_a_landed_selection_is_recorded`, `reversing_a_landed_selection_and_back_is_recorded`.
