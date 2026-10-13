---
task_uid: "01m4sjp42ksx48yghy7w7jmajb"
task: "T88"
rev: 2
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test palette_accepts_the_suggestion (crates/fe-workbench/tests/palette_ghost_text.rs): picking Accept Suggestion in the palette inserts the suggestion, the editor emits Dismissed then Accepted { whole: true }, and focus is back in the editor; escaping the palette instead leaves the buffer unchanged with no suggestion shown"
    required: true
    manual: false
  - id: "C2"
    text: "a suggestion dropped by blur is not restored by an ordinary focus return or by any command other than a palette ghost_text accept; view::ghost_text_key_tests pass unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-gpui -p fe-workbench and cargo clippy on both --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Accept ghost text from the command palette: the suggestion its blur dropped is taken back for a palette accept (T53 review F2) (Mac T391 follow-up)

Follow-up from T53 (Mac T391, ghost text accept/dismiss, landed as f7aae89), filed on helium. Review 01m4sh38nqjmfmrwem6gzepvk9
(`rgt review show T53`), finding F2. Depends on the T53 F1/F3 card, which changes the same blur listener.

## Problem

T53 put `ghost_text.accept`, `ghost_text.accept_word` and `ghost_text.accept_line` in the command palette (Accept
Suggestion, Accept Next Word, and Accept Next Line, which is unbound). None of them can work from there. Opening
the palette (`kiln_shell::command_palette`) focuses its picker. That blurs the editor, and the blur listener in
`crates/fe-gpui/src/view/ghost_text.rs` (:133) drops the suggestion. When a pick runs,
`CommandPalette::confirm` gives focus back and dispatches the command with `Source::Palette` in a `window.defer`
(`kiln/crates/kiln-shell/src/command_palette.rs` :242). By then there is no suggestion, so the command does
nothing. The registry test never opens the palette.

## Change

All of this is in fe-gpui, `crates/fe-gpui/src/view/ghost_text.rs` and the field in `crates/fe-gpui/src/view.rs`.
kiln-shell doesn't change.

- On blur, the view drops the suggestion and emits `Dismissed` as now, and keeps it in a new
  `blurred_ghost_text: Option<GhostText>` field.
- If a `ghost_text.accept*` command with `Source::Palette` reaches `run_ghost_text_command` while no suggestion
  shows, the view takes `blurred_ghost_text` back, provided it is still live at the caret (`is_live`, and the view
  accepts text). It then runs the command on it as usual: an `Accepted` event, and a partial accept keeps the rest
  showing.
- `blurred_ghost_text` is cleared by `set_ghost_text`, by any other command or edit run in the view, and by the
  first frame the view draws while focused. So escaping out of the palette doesn't bring the suggestion back, and
  an ordinary focus change still just dismisses it.
- Document on `GhostTextEvent` that an `Accepted` from the palette follows the `Dismissed` its blur sent.

## Tests

Add a new integration test `crates/fe-workbench/tests/palette_ghost_text.rs`. Open a workbench on a file, as
`crates/fe-workbench/src/palette_tests.rs` does, with `fe_gpui::init` and `fe_workbench::init`. Set a suggestion
on `active_editor` with `set_ghost_text`, then draw.

- `palette_accepts_the_suggestion`: cmd-shift-p, type `accept suggestion`, enter. The buffer has the suggestion's
  text, the editor emitted `Dismissed` then `Accepted { whole: true }`, and focus is back in the editor. Opening the
  palette and pressing escape instead leaves the buffer unchanged and shows no suggestion.

## Implementation notes

- `run_ghost_text_command` now takes the invocation's `Source`, so the one-line call in
  `crates/fe-gpui/src/commands.rs` changes too; `run_command` there also forgets `blurred_ghost_text` up front for
  anything but a palette `ghost_text.accept*` (`forget_blurred_ghost_text_unless_taken`). `changed` forgets it as
  well, for edits and caret moves that don't come through `run_command`.
- Blur listeners only hear from an active window, so both tests activate theirs before focusing.
- Besides the workbench test, `view::ghost_text_tests` gains two unit tests: a palette accept-word then accept
  taking back the blurred suggestion, and a keyboard accept, another palette command, `set_ghost_text(None)`, and
  a focused frame each leaving nothing for a later palette accept to take.
- The workbench test is the first integration test to open a `Workbench`, and outside the crate its `cfg!(test)`
  guards are off: with a project root or a user config directory it starts real project and config watchers,
  whose threads wake gpui tasks and trip the test scheduler's determinism check (seen once in the gate). So it
  opens the file with no root and unsets `FASTEDIT_CONFIG_DIR`, `XDG_CONFIG_HOME` and `HOME` before gpui starts,
  the binary's only test; it also no longer reads the user's settings.
