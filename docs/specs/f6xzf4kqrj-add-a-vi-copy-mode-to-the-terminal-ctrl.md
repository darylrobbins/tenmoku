---
task_uid: "01m4smyntccnprxef6xzf4kqrj"
task: "T77"
rev: 3
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-terminal tests/vi_mode.rs: entering vi mode puts the vi cursor on the shell's cursor; h j k l w b e W B E 0 ^ $ H M L gg G move it as vim does over a printed fixture with scrollback (gg to the oldest history line, G to the last screen line); ctrl-u/ctrl-d-sized vi_scroll moves view and cursor together; the snapshot reports Modes::vi and the vi cursor as a Block cursor in viewport coordinates"
    required: true
    manual: false
  - id: "C2"
    text: "fe-terminal tests/vi_mode.rs: Simple, Line and Block selections started at the vi cursor follow motions and selected_text returns the char, line and block text; vi_search moves the cursor to the next and previous regex match from it (wrapping) and extends a selection; with vi mode on at the bottom, 50 more lines of output (also with the history full) leave the snapshot's rows and the vi cursor's text unchanged; leaving vi mode clears the selection and scrolls to the bottom (Deferred to follow-up card T83, not required here: review F10, the shell scanner's 32-byte CSI parameter limit missing a valid long alternate-screen exit such as CSI ?1;1000;1002;1003;1004;1005;1006;2004;1049l, and F11 (low), the vi search prompt not handling a query wider than the view.)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-terminal-view tests/vi_mode.rs: ctrl-shift-space toggles vi mode; in vi mode no keystroke reaches the pty; counts (5j, 3w), ctrl-e/y/u/d/b/f, [[ and ]] (to T613's prompt marks) move the cursor; y copies the selection to the clipboard and leaves; Esc clears a selection, then leaves; q, i and ctrl-c leave"
    required: true
    manual: false
  - id: "C4"
    text: "fe-terminal-view tests/vi_mode.rs: / and ? open a prompt drawn at the bottom of the view; Enter moves the cursor to the match and highlights every match through set_find; n repeats in the search's direction and N reverses, also with a query set by set_find from the find row; Esc and backspace on an empty query cancel without moving"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench terminal_vi_mode_tests.rs: terminal.vi_mode is a palette command in the terminal context that toggles vi mode in the active session; cargo test and cargo clippy --all-targets -- -D warnings pass for fe-terminal, fe-terminal-view and fe-workbench"
    required: true
    manual: false
---

# Add a vi copy mode to the terminal: ctrl-shift-space or terminal.vi_mode freezes the view under a block cursor that moves with vim motions, searches with / ? n N, selects with v V ctrl-v and yanks with y (Mac T614)

Filed on argon from Mac T614 (card ~/coordination/argon-cards/T614.json rev 1). Mac task uid 01m4smrwtfcas9ysfqprnvq62a. Depends on argon T76 (Mac T613: shared files, and it uses T613's prompt marks). Its other dependencies, Mac T573 (argon T60) and Mac T585 (argon T64), are on argon main. The body's "Decided by the lead overnight" section stands: take those calls as made. Title the host commit "... (Mac T614)".

**Needs argon's trunk.** fe-terminal, fe-terminal-view and the terminal panel (T573 find engine, T574,
T578, T579, T585 view find, T586 find row) are on argon's main only; stack 6 is not yet on Mac main.
Every path below is as on argon's main. **Builds on T613 (shell integration)**, which it depends on (its
`Terminal::prompt_lines` for `[[`/`]]`, and its exact rows-scrolled count for the frozen view).

## Problem

The terminal has no keyboard way to read or copy its scrollback: selecting means the mouse
(`crates/fe-terminal-view/src/mouse.rs`), and every key goes to the shell
(`crates/fe-terminal-view/src/input.rs` `key_down`). The user's standing rule is that all features are vi
friendly. Alacritty (vi mode, ctrl-shift-space), tmux (copy-mode-vi), kitty and WezTerm (copy mode) all
let you freeze the screen, move a block cursor with vim motions, select and yank.

alacritty_terminal 0.26 already carries most of the model: `Term::toggle_vi_mode`, `Term::vi_motion`
(`vi_mode.rs` `ViMotion`: Up/Down/Left/Right, First/Last/FirstOccupied, High/Middle/Low,
Semantic*/Word* left/right/end, Bracket, Paragraph*), `ViModeCursor::scroll`, `Term::vi_goto_point`,
and `scroll_up_relative` keeps the vi cursor and selection on their text as output scrolls. fe-terminal
(`crates/fe-terminal/src/session.rs`) exposes none of it, and alacritty's types must stay out of its API.

## Change

### Model: fe-terminal (GPUI-free)

New `crates/fe-terminal/src/vi.rs` with an `impl Terminal` block (make `Terminal`'s `term` field
`pub(crate)` in `session.rs`) and this crate's own types re-exported from `lib.rs`:

- `set_vi_mode(on)`: entering puts the vi cursor on the shell's cursor (alacritty's rule: top-left of the
  view when that's off screen). Leaving clears the selection and scrolls to the bottom (tmux).
- `vi_motion(ViMove)`, `ViMove` mapping to alacritty's motions: h j k l, w b e (semantic word), W B E
  (whitespace WORD), 0 ^ $, H M L, `%` (bracket), `{` `}` (paragraph), plus `Top` (gg: first column of the
  oldest history line) and `Bottom` (G: last screen line) through `vi_goto_point`.
- `vi_scroll(lines)` for ctrl-e/ctrl-y (1), ctrl-u/ctrl-d (half the screen), ctrl-b/ctrl-f (a screen):
  the view scrolls and the cursor keeps its screen row, as vim's do, clamped at both ends.
- `vi_select(Option<SelectionKind>)`: v, V, ctrl-v start a Simple, Line or new `SelectionKind::Block`
  selection at the vi cursor; the same key again clears it, a different one switches kind keeping the
  anchor (vim). Motions extend it (alacritty's `vi_mode_recompute_selection`). `selected_text` reads it.
- `vi_search(&TerminalQuery, forward) -> Result<Option<TerminalMatch>, QueryError>`: the next match
  strictly after (or before) the vi cursor, wrapping once, through `crates/fe-terminal/src/search.rs`'s
  compiled query and `term.search_next` (factor out a `find_from(origin)`); the vi cursor moves to the
  match's start and the view scrolls to it; a visual selection extends to it.
- `vi_prompt(forward) -> bool` for `[[`/`]]`: the vi cursor goes to column 0 of the previous or next
  prompt mark (T613's `prompt_lines`) above or below its line.
- **Frozen view.** While vi mode is on, output keeps arriving but the rows shown and the vi cursor stay on
  their text: in `pty_loop.rs`, when the view was at the bottom and rows scrolled into history during a
  parser call (T613's exact count), scroll the display back by that many rows and
  put the vi cursor back on its text (alacritty clamps it to the viewport top). Once the display offset is
  non-zero alacritty holds the view itself.
- `crates/fe-terminal/src/snapshot.rs`: `Modes::vi`; in vi mode the snapshot's `cursor` is the vi cursor in
  viewport coordinates with `CursorShape::Block`, so `fe-terminal-view`'s element draws it unchanged
  (no `element.rs` change).

### View: fe-terminal-view

New `crates/fe-terminal-view/src/vi.rs` holding the key parser and the search prompt; `input.rs`'s
`key_down` hands every keystroke to it while vi mode is on, and stops propagation so neither the pty nor
the input handler sees it (composed text in vi mode is dropped).

- ctrl-shift-space toggles vi mode on every platform (alacritty's default), handled in the view's own key
  handling since the keymap files are held by helium's T462. `pub fn TerminalView::toggle_vi_mode(cx)` and
  `is_vi_mode()`.
- Keys: the motions above (`gg` as a two-key sequence), a count prefix for motions and scrolls (`5j`, `3w`;
  `0` with no pending count is the motion), ctrl-e/y/u/d/b/f, v V ctrl-v, `y` (copies the selection to the
  clipboard with `cx.write_to_clipboard` and leaves vi mode; with no selection it does nothing), `[[` `]]`,
  `/` `?` `n` `N`. Esc clears a selection, else leaves; `q`, `i`, ctrl-c and ctrl-shift-space leave. The
  platform paste key leaves vi mode and pastes. Other keys do nothing.
- `/` and `?` open a one-line prompt at the bottom of the view (`/` or `?` then the query and a caret),
  drawn as an overlay in `view.rs`'s `render`, not in the element. Typed characters append, backspace
  deletes (on an empty query it cancels), Esc cancels, Enter runs `vi_search` as a smart-case regex
  (`TerminalQuery::regex`) and sets the view's find query (`set_find`) so every match is highlighted as
  T585 does. A query that doesn't compile or has no match leaves the cursor where it was.
- `n` repeats the last search in its direction, `N` the other way, using the view's find query, so a
  query set by the panel's find row (T586) also works (forward).

### Workbench

`terminal.vi_mode` ("Terminal: Vi Copy Mode") joins the registry in
`crates/fe-workbench/src/terminal_panel.rs` (`TerminalOp::ViMode`, terminal key context) and
`Workbench::run_terminal_op` in `workbench.rs`: it shows the dock and toggles vi mode in the active
session, focused. Test in `crates/fe-workbench/src/terminal_vi_mode_tests.rs`, declared with `#[path]` from
`terminal_panel.rs` like `terminal_find_tests.rs`.

Tests: model in `crates/fe-terminal/tests/vi_mode.rs` (a real session printing a fixture with `printf`,
as `tests/search.rs` does); keys and prompt in `crates/fe-terminal-view/tests/vi_mode.rs`
(as `tests/keyboard.rs` and `tests/find.rs` drive the view).

## Decided by the lead overnight

- **Alacritty's key and model, tmux's exits.** ctrl-shift-space (alacritty's default on macOS and Linux;
  cmd-shift-space is avoided as macOS input-source territory) plus a palette command. Leaving scrolls to
  the bottom and drops the selection, as tmux's copy mode does, rather than alacritty's keep-in-place.
- **Frozen view** as tmux/kitty: output doesn't move what you're reading (alacritty only holds the view
  once scrolled up).
- **Search is a vim-style bottom prompt in the view**, not the panel's find row: it keeps vi mode
  self-contained and needs no element change. It shares the view's find query, so highlights and `n`/`N`
  work with the find row too. Searches are regex, smart case, as vim's `/` with `smartcase`.
- **`y` with no selection does nothing** (alacritty); no `yy`/`Y`, no registers, no mouse changes in vi
  mode (dragging still selects as now).
- **Counts are in** (vim users type `5j`); `o` (swap selection ends) and text objects are left for a
  later card.
- **Ordered after T613** because both edit `fe-terminal/src/session.rs`, `lib.rs`,
  `pty_loop.rs`, `terminal_panel.rs` and `workbench.rs`, and because `[[`/`]]` and the frozen view use its
  prompt marks and rows-scrolled count.

## Implementation notes

- **Frozen view.** `vi::Hold` wraps each parser call in `pty_loop.rs`'s `Feeder::parse`: before it, in vi
  mode on the primary screen with the view at the bottom, it notes `ShellState::scrolled` (current after
  every primary-screen parser call and resize) and the vi cursor; after it (and the history trim), the
  rows scrolled are the new `scrolled` minus the old, the vi cursor goes back on its text (clamped to the
  oldest line), then the display scrolls back by them. Calls on the alternate screen aren't held; the
  scanner (`shell.rs`, outside the card's scope list) now also finds `CSI ? 1049 l` and the loop feeds its
  final byte alone, as for a history clear. That call's `advanced` brings `scrolled` in step with any
  resize made while the primary was hidden (rows a shrink pushed into history), so the primary output after
  it, even in the same read, is held from a true baseline and a hidden resize isn't counted as output
  (review F6). The cursor is set first because `scroll_display`
  moves a selection's end to the vi cursor. The unused `Terminal::prompt_lines` and `scrolled_rows` T613
  left for this card are gone: `vi.rs` reads `ShellState` under the term lock it already holds.
- **The vi cursor stays valid.** alacritty doesn't clamp it on a screen switch, and trimming history back
  to the scrollback can take its line. `vi::keep_in_view` runs after every parser call and resize and
  puts it back in the view (a selection's end follows); the snapshot indexes its cell only when it's in
  view.
- **`vi_scroll`** moves the cursor by the full count, clamped to the view, while the view moves as far
  as history allows: at either end the cursor goes on alone (vim's ctrl-u at the top), and it keeps its
  column. `G` goes to column 0 of the last screen line, like `gg`.
- **Search** is strict: forward finds a match starting after the vi cursor, back one starting before it
  (`find_from` with `Side::Left` both ways; `Terminal::find` keeps `Side::Right` back from a selection).
  An empty `/` query repeats the view's find query. `n` follows the last vi search's direction until
  `set_find` replaces or clears the query, which sets it forward. That is a four-line change to
  `crates/fe-terminal-view/src/find.rs`, outside the card's scope list: only `set_find` sees the find row
  replace a query, and the review (round 2, F5) asked for it there.
- **Switching selection kind** (v, V, ctrl-v over a selection) re-runs `include_all`, since each kind
  orders its ends' sides its own way.
- **A pending `g`, `[` or `]`** gives way to any other second key, which is then taken on its own (with
  the count), so Esc and the keys that leave always work. The clipboard keys work in the search prompt.
- **Entering** vi mode also drops any selection, so v always starts fresh. `Terminal::has_selection`
  is new, for Esc (a selection of blank cells has no text but is still cleared first).
- **Keys.** The parser (`ViKeys`) is a pure state machine with unit tests; arrows, Home/End and
  PageUp/PageDown are aliases of h j k l, 0 $ and ctrl-b/ctrl-f. The platform copy key copies the selection
  without leaving. The prompt is an absolute row over the grid in `render`; its drawn bounds are recorded
  through a `canvas` for the test (`vi_prompt_bounds`, doc-hidden).
- **Focus reports** now use `input_in_place`, so a focus change doesn't scroll a held view to the bottom.
- **Vi mode's keys win over the keymap** (review F7). kiln's keystroke interceptor matches the keymap
  before any view's key listener runs, so Linux's `ctrl-b` (`dock.toggle`, workbench context) never
  reached vi mode. `kiln_platform::keys::HOLDS_KEYS` (`holds_keys`) is a new key context identifier: while
  the focused element's own context carries it, keystrokes without the platform modifier skip the keymap
  and go on to the element. The terminal view adds it while vi mode is on. This is a change to
  `kiln/crates/kiln-platform/src/keys.rs`, outside the card's scope list: nothing in scope runs before
  that interceptor. The keymap files are untouched; cmd shortcuts stay the app's on macOS, while on Linux
  vi mode holds every key until it's left.
- **M on a one-row view** stays on its row (F8): alacritty's middle of one row is the row above it.
- **No text input in vi mode** (F9): `accepts_text_input` is false while vi mode is on, so the platform
  doesn't route printable keys through an input method; the composition guards stay.
- **The scanner counts CSI parameters as vte does** (F10): up to 32 parameters and sub-parameters, each
  value accumulating digits saturating at `u16`, in place of a 32-byte cap. A CSI with more is ignored by
  the parser and the scanner alike, so a long exit such as `CSI ?1;1000;…;2004;1049l` is split on and its
  following output held.
- **Backward search from a wide character** (F2) starts from its first cell (`expand_wide`), where the
  vi cursor is drawn when it sits on the spacer, so the character it's on isn't found again.
- **Alt is no vi key's** (F1): alt-q, alt-y, alt-j and the rest do nothing. On macOS a character option
  types (one that differs from the key) is taken as typed, for layouts that need option for `[` or `{`.
