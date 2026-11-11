---
task_uid: "01m7595bbn77vmtnt7dg6dzq3w"
task: "T585"
rev: 7
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Through the A11Y-2 Announcer, with the politeness section 5 gives each: toasts, vim mode changes and messages, the find match count, the diagnostic at the caret, folds, the cursor count, clipped selections, ghost text, completion opening and the selected item, and an \"Editor: Announce Position\" command. Known limit (review F11, owned by follow-up T587): uncoded Git, quickfix and commit-box failures that reach the vim line through Workbench::answer_git_op (git_commands.rs), quickfix_commands::say and the commit box's host_command message closure (git_commit.rs) are announced politely, because those shared helpers, outside this card's scope, don't tell errors from reports; E-coded messages and failures set through set_vim_error are assertive. The spec's notes record the limit"
    required: true
    manual: false
  - id: "C2"
    text: "Tests in crates/fe-gpui/src/view/a11y_announce_tests.rs and crates/fe-workbench/tests/a11y_announce.rs assert each announcement's text and politeness, and that completion opening, refiltering, accepting and dismissing leave the text area the focused node; both pass"
    required: true
    manual: false
  - id: "C3"
    text: "The finish notes list this card's section 12 human-check lines (H-numbers and text), for the A11Y-11 VoiceOver pass"
    required: true
    manual: false
---

# A11Y-9 Announcements: toasts, vim mode, find count, diagnostics at the caret, folds, cursors, completion and Announce Position (T294 design) (Mac T1675)

# A11Y-9 Announcements

Spec: `docs/decisions/qenw9xxdzk-accessibility-design.md`, the **A11Y-9 Announcements** row of section 11 "Proposed card breakdown", and section 5 "Announcements" and section 6 (completion).
The user approved the design's platform path (section 3, option (a): gpui-pre 0.3.9's
AccessKit, no vendoring) and its first slice (section 10) as written on 2026-11-09.

Build what that row and those sections specify, with the tests section 9 "Testing strategy"
assigns (shape 1 for text elements, shape 2 for chrome). If the design's text differs from
this card's criteria, the design wins.

Platform: **Linux-OK**.

## Implementation notes

- **Where.** The editor's announcements live in `crates/fe-gpui/src/view/a11y_announce.rs`: `EditorView::announce_changes` runs from the view's render, compares what the view shows with what it last saw (`Announced`), and says what changed while the editor has focus in the active window (the announcer speaks in the active window, so another window's changes are noted, not said); a frame's messages go as one, assertive if any is. The first frame only takes note.
- **Vim.** Mode changes say "Normal", "Insert", "Visual" … (new `gpui-a11y-vim-mode-*` strings, not the bar's capitals); an operator waiting is normal mode, and the command line says nothing (it takes focus and is named by its prefix: "Command", "Search forward", "Search backward", via `status::command_line_name`, the vim line making its input again when the prefix changes, since `TextInput::name` is a builder). Messages are assertive when vim failed the key with them (`VimState::failed`, coded `E123:` or not, as `register / is read-only`), when they carry nvim's code, or when the host set them with the new `EditorView::set_vim_error`, which `vim_host` and fe-gpui's own map-command failures (`vim.rs`: a `<Cmd>fe:…<CR>` that fails, wants arguments or doesn't apply here) now use; `set_vim_message` is a report, polite unless coded. A key ends the message and what the host said of it, and each host setting counts as new, so the same text set again (or as an error after a report) is said again.
- **Diagnostics at the caret.** Said politely as "error: <message>" (warning/info/hint) once the caret has rested `HOVER_DELAY` in them (each caret move restarts the wait; losing focus ends the visit, by a blur subscription, drawn or not; when the wait is up the editor must still have focus in the active window, else the visit ends unsaid), once each a visit, each diagnostic known by its range too so two with the same text are two, with the hover's containment rule (the character after the caret, or an empty range at it). The rule is restated in `a11y_announce.rs` rather than reused from `hover.rs`, so that `hover.rs` stays untouched (its carried T556 finding needs element painting outside this card's scope). `diagnostics.next`/`previous` (`problems_panel.rs`) call `EditorView::announce_caret_diagnostics`, which says them at once.
- **Folds** say "Folded N lines" (newly hidden rows, nested folds counted once) or "Unfolded", only when the text's version is unchanged, so an edit that moves folds says nothing.
- **Cursors** say "N cursors" going from one to more and "One cursor" back. **Ghost text** says "Suggestion available" when one appears. **Completion** says "N suggestions, <selected label>" on opening, the selected label on a selection move, on each refilter as the text changes or the caret moves back within what was typed (the menu counts its lists, by text version and caret) or on a new answer, nothing on accept or dismiss; it draws no node, and the view itself applies `aria_active_descendant` to the text area while the editor has focus (`EditorView::text_area_active_descendant`, with a screen reader on or not; the element's adapter no longer does), noting it as it applies it (`text_area_active`), which the tests assert.
- **Clipped selections.** While a screen reader is on, an empty canvas after the editor element reads the frame's text node and, if an end of the non-empty primary selection was moved to the window's edge or a cut row's end (`clips`), says "N lines selected" once per selection, only from the focused editor of the active window (a split's other editor notes its selection quietly); an end hidden by a fold whose header is a run isn't clipping, even past the last run (a fold through the end of the file), which a test checks against the builder's own output for a real frame. The tests give it a hand-built `A11yText`, since a test window never builds one.
- **Editor: Announce Position** (`editor.announce_position`, no key) says "line N, column M of T", T the text's rows; it doesn't split vim's insert session.
- **Toasts** (`toast.rs`) are `Alert` nodes named by their message, the detail their description; a toast is said (message, then detail) when it shows, again when an identical one counts it up, assertively for an error; one waiting is said once it shows. Only the active window's toasts are said.
- **Find count** (`find_bar.rs`): 300 ms after the query's last change (each edit clears a count still owed, and one owed while another window is active is dropped), at once after Find Next/Previous or an option's toggle, as soon as the count is known. With the bar closed, Find Next/Previous count the last query's matches in the background for the announcement alone, said only if the editor's text, the match it reached and its focus in the active window still stand when the count lands.

### Known limits

- Uncoded failures that the Git commands' `answer_git_op`, quickfix's `say` and the commit box pass to `set_vim_message` (which can't tell them from reports) are polite; follow-up T587 gives those helpers an error/report distinction. Their coded errors (`E13`, `E37`, `E99`) are assertive already.

### Human checks (section 12) for A11Y-11

- **H7. Announcements.** Save, which shows a toast: the toast is spoken. `i`, then Escape: "Insert", then "Normal". Find with three matches: "1 of 3", and next gives "2 of 3". Rest the caret on the error: the message is read once. Fold a block: "Folded N lines". Run "Editor: Announce Position": the line and column are read. Does a later focus change cut any of them off? (Section 5's open question: whether Medium priority is interrupted by the focus speech after a toast's action.)
