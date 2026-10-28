---
task_uid: "01m5n8y56snzkcjvpzs82fa4zn"
task: "T208"
rev: 7
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "TextInput has an optional per-character typing filter, off by default; pickers, the find bar and the search panel behave as before (their existing tests pass unchanged)"
    required: true
    manual: false
  - id: "C2"
    text: "With cnoremap jk \u003cCR\u003e (or an expansion that types text) in the headless TestAppContext workbench, typing :s/a/b/ then j,k on the status-bar line submits the line, and typing j then x leaves jx in the line"
    required: true
    manual: false
  - id: "C3"
    text: "A held cmdline prefix resolves after CHORD_TIMEOUT, on blur, on Enter and on Esc (the typed key lands in the line first); Backspace, Up, Down, Tab and ctrl keys on the line are not remapped. Known limits, waived by the user on 2026-10-27/28 and not fixed here (tracked by follow-ups T253 and T222): review F1 (pasted, IME, history-recalled and completed text is not recorded as unmapped input), F3 (a cancelling Backspace from a map and host-command continuations), F5 (replay decides mapping eligibility from key shape rather than provenance), F8 (a \u003cCmd\u003e continuation edits text it cannot see), F9 (a held prefix resolving during IME composition, or before multi-character non-IME platform input, can drop or reorder that input: with j held, platform input xy can give xyj after the timeout), and F10 (RHS keys the live line ignores, such as \u003cLeft\u003e, abort a macro replay of the same map)."
    required: true
    manual: false
  - id: "C4"
    text: "EditorView::replay_key's cmdline arm applies c maps to a replayed or expanded key; record_command_line records the line as typed, so a macro recorded across a mapped line replays identically. Backspace ordering on the live line is preserved: a physical \u003cBS\u003e is recorded in typing order before any later mapping key (test a_bypassed_edit_is_recorded_before_the_mapping_keys). Known limits, waived by the user on 2026-10-27/28 and not fixed here (tracked by follow-ups T253 and T222): review F1 (pasted, IME, history-recalled and completed text is not recorded as unmapped input), F3 (a cancelling Backspace from a map and host-command continuations), F5 (replay decides mapping eligibility from key shape rather than provenance), F8 (a \u003cCmd\u003e continuation edits text it cannot see), F9 (a held prefix resolving during IME composition, or before multi-character non-IME platform input, can drop or reorder that input: with j held, platform input xy can give xyj after the timeout), and F10 (RHS keys the live line ignores, such as \u003cLeft\u003e, abort a macro replay of the same map)."
    required: true
    manual: false
  - id: "C5"
    text: "Existing vim_line tests (history_tests, incsearch_tests, complete_tests, mode_segment_tests), kiln-ui tests and fe-gpui macro_tests pass unchanged"
    required: true
    manual: false
---

# Vim cnoremap/cmap on the status bar's command line: a typing hook on TextInput, remap_insert per typed character, held prefix with timeout, replay (T877 D) (Mac T1092)

## Problem

T877 split, card D (the hardest piece; defer if A-C are enough). The status bar's
command line is not a key stream: `crates/fe-workbench/src/vim_line.rs` shows a
`TextInput` (`kiln/crates/kiln-ui/src/text_input.rs`) that edits its own text from
the platform input handler (`replace_text_in_range`). The editor hears nothing until
the line ends: `StatusBar::end_line` calls `EditorView::record_command_line(text,
key)`, which records the whole typed line, then dispatches
`vim.command_line_submit` / `vim.command_line_cancel`. So `cnoremap` has no seam:
no per-key hook, no way to hold a prefix back or to insert an expansion.
The other cmdline typist, `EditorView::replay_key`'s `Mode::CmdLine` arm
(`crates/fe-gpui/src/vim.rs` ~2575-2625), builds the line in a local `String`.

## Change

- `kiln/crates/kiln-ui/src/text_input.rs`: a small hook, e.g. an optional
  `set_typing_filter(Fn(&str, &mut App) -> Typed)` consulted by the input handler
  for text typed one character at a time (not paste, not IME composition),
  where `Typed::Pass` types it, `Typed::Swallow` does nothing, and the filter may
  call `insert`. Default None: no behaviour change for pickers or the find bar.
- `crates/fe-workbench/src/vim_line.rs`: install the filter when the line opens
  (`open_line`); it calls `EditorView::remap_line_key(ch)` (new, in
  `crates/fe-gpui/src/vim.rs`: a thin wrapper over `VimState::remap_insert`
  returning the keys to type) and arms the `CHORD_TIMEOUT` timer on `Wait`;
  `end_line`, `cancel_line` and `line_blurred` first resolve a held prefix
  (`remap_timeout`) and type what it gives into the input, so Enter submits it.
  Backspace/Up/Down/Tab/ctrl keys stay the line's own (not remapped here).
- `crates/fe-gpui/src/vim.rs`: `replay_key`'s CmdLine arm asks `remap_insert` for a
  typed key like the Insert arm (card C), and `record_command_line` must record the
  line as typed, not as expanded (a held-and-expanded prefix is not recorded twice).
- New tests in NEW files `crates/fe-workbench/src/vim_line/remap_tests.rs`
  (registered by one `#[cfg(test)] #[path]` line next to the existing ones at the
  end of `vim_line.rs`) and `crates/fe-gpui/src/vim/remap_line_tests.rs`.

## Notes

- Needs cards A and C merged (C also edits `fe-gpui/src/vim.rs`; D follows it).
- Verify first that TextInput's input handler can be intercepted per character
  without breaking IME composition; if the kiln-ui change grows past ~300 lines,
  split it into its own card.
- Keymap context here is `vim_line` (`KEY_CONTEXT`), not `vim.mode == cmdline`;
  card E's conflict note must check `vim_line` for `c` maps.

## Implementation notes

- The filter is `Fn(&str, &mut Window, &mut App) -> Typed` (`Pass`, `Swallow`, `Insert(String)`); it runs while the input is updated, so it can't edit the input itself: leading characters come back as `Insert`, the rest (BS, Enter, Esc, ^C) is deferred to `StatusBar::type_line_keys`.
- A line that took keys through the `c` maps records them per key (`vim_line_keyed`); `record_command_line` then records only the end key (none when a map's right side ended it). Known gap: line edits that aren't typed characters (history recall, Tab, paste, Backspace) on such a line are not recorded.

- Rev 4 (F1): an edit the maps never see (a physical `<BS>`, a paste) is recorded as it happens: the line's `Changed` event calls `EditorView::sync_line_text`, which records the difference to the editor's echo of the line, so it comes before the mapping keys typed after it. Held back while a map's keys are still on their way into the input (`typing`). A replayed bypassed *character* is still matched against the `c` maps (key provenance is F5, T222).
