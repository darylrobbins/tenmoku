---
task_uid: "01m725cjr3kk75de6t9nrr2g9e"
task: "T559"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C2"
    text: "nav.rs's implementation/implementations pair is a plural message; en-US byte-equal; cargo nextest run -p fe-workbench passes"
    required: true
    manual: false
---

# fe-workbench navigation and picker strings (i18n W3, rest) (Mac T1558)

# fe-workbench navigation and picker strings (W3, rest)

Spec: card **W3** of `docs/decisions/xte5e7hqsw-i18n-design.md` § 9, for the fe-workbench files the B3 surface
cards don't restyle (they move their own files' text). Sections 3–8 are the
design. en-US stays byte-equal to today's, so no golden and no behaviour test
changes except tests that construct removed literals.

Files: `nav.rs`, `nav_picker.rs`, `symbol_picker.rs`, `go_to_line.rs`, `quick_open.rs`, `harpoon.rs`, `local_history_view.rs`, `workbench_files.rs`, `ignore_assist.rs`, `buffer_picker.rs`, `buffer_lines_picker.rs`, `recent_picker.rs`, `config_files_picker.rs`.

Add `crates/fe-workbench/l10n/en-US/navigation.ftl` (keys `workbench-navigation-…`) to
`resources!` in `crates/fe-workbench/src/l10n.rs`, and these files to
`l10n-allow.toml`'s `migrated` list. English plural helpers become plural
messages; fragments become whole messages.

Waits for T1488 (`harpoon.rs`). `nav.rs`'s `("implementation", "implementations")` helper becomes a plural message.

Platform: **Linux-OK**.

## Implementation notes

- **Scope.** Respecced (revision 2) to add `src/l10n.rs` and `l10n-allow.toml`, which the spec names, plus `harpoon_tests.rs` (it imported the removed `NO_FOLDER` const, and now asserts the English text), `buffer_lines_picker_tests.rs` and `local_history_view_tests.rs` (tests for the carried findings).
- **Whole messages.** Every status that appended `by(provider, precision)`'s ` (by search)` is now one message with a `$source` selector (`search` names `$provider`; `exact` shows nothing). Go to definition's noun/verb pair is a `$kind` selector (`definition`, `declaration`, `type-definition`, `implementation`), and its count is a CLDR plural (`workbench-navigation-found`). The ignore assist's hover is four whole messages (exact or at least, with or without "and N more"), the fenced path list passed as `$paths`. The local history row (when, why, size) is one message with selectors on `$ago`, `$count` and `$reason`. Symbol kinds in the workspace symbols picker are a `$kind` selector too.
- **Counts of one.** Plural messages say `1 file` and `1 definition of` where the old helpers said `Indexing… 1 files` and `1 definitions of` (go to definition lists one item only when its provider isn't sure). Every other en-US string is byte-equal; no existing assertion changed.
- **Stored text resolves when drawn.** Picker placeholders are `text!`. The definitions picker stores its status as a `Text`; the symbols picker stores the search error and formats it in `status()`; local history rows store a `Text`, resolved to draw and to match. Toasts and prompts resolve at once (design § 4). The local history diff's title goes to `begin_history_diff` as a resolved `String`, since that type isn't in scope. The `nav_source` status item takes `tr!` output, as § 4 says `StatusItem` does.
- **`nav::by` stays** for `references_panel.rs` (W6), which still appends it to its status. Its one literal is allow-listed with that reason; nav's own statuses don't use it. The debug selectors and the element id the lint reported are allow-listed as ids, as W1's are.
- **Carried findings.**
  - T396 F2 (local history): each listing gets a number in a per-window `Listings` global. Only the latest lands, and its picker shows only if the same editor is active, focus is where it was, and no modal shows but the one that did (the command palette it was run from, closing). Tests: `a_listing_landing_under_a_newer_modal_shows_nothing` and `an_older_listing_landing_late_is_dropped`, which use a test-only `ListingHolds` to hold landings. Both fail without the fix.
  - T491 F2 (buffer lines): rows are matched whole. A row over `MAX_ROW_CHARS` shows an excerpt of that many chars from just before its first highlight, with ellipses and the highlights shifted (`excerpt`). Tests: `matches_past_the_chars_a_row_shows` and `excerpts_cut_only_long_rows`.
  - T491 F3 (buffer lines): the picker puts its preview back on release too, which follows the modal layer's blur hide, and leaves focus where it went. Test: `focus_moving_away_restores_the_scroll` (fails without the hook). `diagnostics_picker.rs`'s instance is outside this card's scope. `RowPreview` can't restore itself without its picker's context, so that picker needs the same one-line `on_release` hook.
  - T474 F1 and F4 (harpoon) were already fixed on main by T1422 (`50c76b40`). `Asked` compares the pane's `open_serial()` and `navigations()`, and the picker cancels its pending open on release however it goes. Tests are in `tests/harpoon_supersede.rs`. This card only moves harpoon's text.
