---
task_uid: "01m4ewv0xxapvy4gxtr7m04sq7"
task: "T221"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test workbench::vim_host_tests::tab_commands: in a temp folder with vim on, :w saves the tab (clean, contents on disk); after an edit :q leaves the tab open with \"E37: No write since last change (add ! to override)\" as the status bar's VIM_MESSAGE and cx.pending_prompt() is None; :q! closes it with no prompt and the file on disk unchanged; :wq saves and closes; :x on a clean tab closes it with the file's mtime unchanged; :q on a dirty tab whose buffer another pane shows closes it without E37; :w on an untitled tab shows \"E32: No file name\""
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test workbench::vim_host_tests::last_tab_closes_its_window: with two workbench windows open, :q on the only tab of one removes that window from cx.windows() and the other stays open with its tabs; :q on a dirty only tab shows E37 and the window stays; :q! on it closes the window with no prompt"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test workbench::vim_host_tests::window_commands: :wa saves two dirty tabs of the window and leaves a dirty tab in a second window unsaved; :qa with the current tab dirty shows E37, and with only another tab dirty shows E162 naming it, every tab staying open with no prompt; :qa with nothing dirty and :qa! with dirty tabs each close only their window, with no prompt; :e other.txt opens root/other.txt in the tab's pane, :e with an absolute path opens that file, :e missing.txt opens an empty tab for it, and :e and :e! alone show \"Not supported yet: reloading a file from disk\""
    required: true
    manual: false
  - id: "C4"
    text: "Apart from the task's own spec under docs/specs/ and its decision record, only the scope's files change: fe-vim gains only VimState::set_host_message (in ex.rs) and fe-gpui only EditorView::take_vim_host_command and EditorView::set_vim_message (in pane_item.rs); crates/fe-workbench/src/lib.rs and crates/fe-workbench/src/tests/mod.rs don't change; workbench::close_tests passes unchanged; in crates/fe-workbench/src/tests/vim_line.rs only tests::vim_line::ex_through_the_line's :w step changes (its assertion and comment now expect :w to save, as C1 requires), and every tests::vim_line test passes"
    required: true
    manual: false
  - id: "C5"
    text: "The running-app check (:w, :q E37 with no prompt, :q!, :q on a second window's last tab, :e, :wa) is T356, a person's action task; this task needs no running-app evidence"
    required: true
    manual: false
---

# Vim :w :q :wq :x :e :qa :wa run as fastedit commands from the workbench's command line, with E37 on a dirty tab

# Vim :w :q :wq :x :e :qa :wa run as fastedit commands from the workbench's command line

Milestone M7 "Vim", second batch (resized), card 06c: the host-command part of the old card 6. Read
T217 (`fe_vim::HostCommand`, `VimState::take_host_command()`, `crates/fe-vim/src/ex.rs`) and T219 (the
line, `crates/fe-workbench/src/vim_line.rs`, a child module of `status_bar.rs`).

**Clean room** as every M7 card: no Vim/Neovim source, no hjkl, no Zed, no Helix.

Respecced 2026-10-11 for the tree after kiln's K5 (main at e6bab88). Read
`docs/decisions/app-framework.md` §7, "Close vetoes" and "Multiple windows".

## Problem

fe-vim parses `:w :q :wq :x :e :qa :wa` (with `!`) and leaves a `HostCommand` (T217), and the
workbench's command line submits them (T219), but nothing takes the command: typing `:w` in the
running app does nothing. The card as first written (2026-10-08) predates K5 and names APIs that
are gone (`Pane::take`) or now behave differently:

- **Close vetoes (K5.7).** Every close asks its items through `kiln_shell::close::confirm`
  (`kiln/crates/kiln-shell/src/close.rs`). A dirty editor's `PaneItem::may_close`
  (`crates/fe-gpui/src/pane_item.rs`) shows a Discard/Cancel prompt
  ("Discard unsaved changes to {name}?"). `Pane::close_active` → `PaneView::close_active_item`,
  `Workbench::close_active_pane`, `window.close` (`close::close_window`) and `app.quit`
  (`close::quit`, every window) all go through it. Cancel becomes the prompt's default in 9023d31
  (branch `kiln-cancel-9023d31`, not on main at e6bab88); this card never shows that prompt, so it
  doesn't wait for it.
- **Removing a tab** without asking is `kiln_shell::PaneView::take_item(ix, window, cx)` (the
  pane's `take_item`; `Pane::take` is gone). The workbench observes each pane: a pane emptied by
  it closes in `Workbench::pane_changed` → `close_pane`, unless it's the window's last pane, which
  stays, empty.
- **Multiple windows (K5.10).** Each window has its own `Workbench`; `close::set_window_items`
  gives each window's editors (`Workbench::follow_window_items`). fastedit's `main.rs` quits the
  app once its last window closes (`cx.on_window_closed` … `cx.windows().is_empty()`).
- **Messages.** The status bar shows `VimState::message()` as the built-in item
  `StatusItem::VIM_MESSAGE` until the next key fe-vim reads, but its field is `pub(crate)`: a host
  can't leave a message. `EditorView` gives only `vim_state(&self)`, so a host can't call
  `take_host_command(&mut self)` either.

## Change

### Seams (two small additions, outside the hot files)

- `crates/fe-vim/src/ex.rs`: `impl VimState { pub fn set_host_message(&mut self, message: impl Into<String>) }`
  — the host's answer to a `HostCommand` (an error such as E37, or a report), left in
  `VimState::message` and cleared as any message is (the next key `feed` reads, the next line,
  `clear_message`). It sets no `failed` flag. It lives in `ex.rs`, beside `HostCommand`, because
  `state.rs` is held by T226 and T229.
- `crates/fe-gpui/src/pane_item.rs`: `impl EditorView` gains
  `pub fn take_vim_host_command(&mut self) -> Option<fe_vim::HostCommand>` (`None` with vim off)
  and `pub fn set_vim_message(&mut self, message: impl Into<String>, cx: &mut Context<Self>)`
  (sets it through `set_host_message` and notifies, so the bar redraws). They use the
  `pub(crate) vim` field; they live in `pane_item.rs`, not `vim.rs`, because `vim.rs` is held by
  T352, T223 and T313.

### Hand-off (`crates/fe-workbench/src/vim_line.rs`, `crates/fe-workbench/src/workbench.rs`)

- In `StatusBar::end_line`, right after a submit's dispatch (`run_in` with
  `vim.command_line_submit`, which runs synchronously), the bar takes
  `editor.take_vim_host_command()` and, if there is one, emits
  `VimHostCommand { editor: WeakEntity<EditorView>, command: HostCommand }`
  (`impl EventEmitter<VimHostCommand> for StatusBar`, in `vim_line.rs`).
- `Workbench::new` subscribes to it (`cx.subscribe_in(&status_bar, window, …)`) and calls
  `Workbench::run_vim_host_command`, in a new file `crates/fe-workbench/src/vim_host.rs`,
  declared from `workbench.rs` as `#[path = "vim_host.rs"] mod vim_host;` (as `status_bar.rs`
  declares `vim_line.rs`), so `lib.rs` doesn't change. The editor is found in its pane by entity
  (`Pane::tabs()`), not assumed to be the active one; a closed editor runs nothing.

### What each command does (`vim_host.rs`)

"The window" is this workbench's window; "the tab" is the editor the line was typed in.
"Dirty" is `is_dirty`; a tab "shown elsewhere" is one whose buffer another tab of the window
shows (`PaneState::shown_elsewhere`), so closing it loses nothing. Every message goes to the
line's editor through `set_vim_message`, so it shows in the status bar as `VIM_MESSAGE`.

| Ex | fastedit |
|---|---|
| `:w`, `:w!` | Saves the tab through its `PaneItem::save` (what `editor.save` does). A tab with no path: `E32: No file name`. A failed save: its error as the message. |
| `:q` | Dirty and not shown elsewhere: nothing closes, the message is `E37: No write since last change (add ! to override)`, and **no Discard/Cancel prompt shows**. Otherwise the tab closes. |
| `:q!` | The tab closes, discarding its changes, **without asking** (no `close::confirm`, no prompt). |
| `:wq`, `:wq!` | Saves as `:w`, then closes as `:q` if the save succeeded; a failed save closes nothing. |
| `:x`, `:x!` | As `:wq`, but saves only a dirty tab: a clean one closes without writing (an untitled clean tab too). |
| `:e {path}`, `:e! {path}` | Opens `path` in the tab's pane with `Pane::open` (an open file is shown; a missing one opens as a new empty tab, which `:w` creates; a read error shows as any open's does). A relative path is against the window's root (`Workbench::root()`), else the tab's file's folder; `~` and `~/…` expand to the home folder. With no root and an untitled tab, a relative path shows `Can't open {path}: no folder to resolve it against`. The current tab stays open whatever its state, so `!` changes nothing. |
| `:e`, `:e!` alone | The message `Not supported yet: reloading a file from disk`. Nothing else changes. |
| `:wa` | Saves every dirty tab in the window, each buffer once (shared buffers aren't saved twice); other windows' tabs aren't touched. Any failure (an untitled tab: `E141: No file name for buffer "{tab title}"`, or a save error) is the message, the first one met in pane order, after every other tab has been saved. |
| `:qa` | If the line's tab is dirty: E37 as above; else if another tab of the window is dirty: `E162: No write since last change for buffer "{tab title}"`, the first in pane order. Either way nothing closes and no prompt shows. With nothing dirty, the window closes. |
| `:qa!` | The window closes, discarding every tab's changes, without asking. |

**Closing a tab** (`:q`, `:q!`, `:wq`, `:x`) removes it with `PaneView::take_item` at its
position in its pane, never through `close::confirm`: `:q` has already refused a dirty tab with
E37, and `:q!` discards by decision. An emptied pane then closes as any does (`pane_changed`).
**If the window then has no tab in any pane, the window closes** (`window.remove_window()`; there
is nothing left to ask). **Closing the window** (that, `:qa`, `:qa!`) removes only this window:
other windows stay open, and if it was the last window, fastedit's `main.rs` quits the app as it
does when any last window closes — as Vim exits when its last window goes.

## Decided

User decisions, 2026-10-11 (approved as proposed):

- **`:q` on a dirty tab gives Vim's E37** (`E37: No write since last change (add ! to override)`)
  on the command line and does **not** show the Discard/Cancel prompt; on a clean tab it closes
  the tab.
- **`:q!` discards and closes without asking**, bypassing the close veto.
- **`:qa` and `:wa` act on the current window only.** `:qa` gives E37/E162 if any tab in the window
  is dirty; `:qa!` discards them all without asking; `:wa` writes every dirty tab in the window.

Lead decisions, 2026-10-11 (the user may revisit):

- **`:q` on a window's last tab closes the window**, as Vim closes when its last window goes; the
  app quits only if that was its last window (fastedit's existing rule), never because of `:q`
  itself. A dirty last tab gives E37 like any other.
- **`:wq` and `:x` are save-then-`:q`**: a failed save closes nothing; `:x` writes only a dirty
  tab; `!` adds nothing (there's no read-only override yet).
- **`:e {path}` opens a tab in the same pane and never abandons the current one**, so it needs no
  E37 and `:e!` with a path is the same; paths are relative to the window's root, then the tab's
  folder, never the process's working directory. `:e`/`:e!` alone (re-reading the file) isn't
  supported until fastedit can revert a buffer from disk.
- A tab whose buffer another tab of the window shows closes on `:q` without E37: its changes live
  on there, as the close veto already treats it.

## Tests (new file)

`crates/fe-workbench/src/vim_host_tests.rs`, declared in `workbench.rs` beside `close_tests` as
`#[cfg(test)] #[path = "vim_host_tests.rs"] mod vim_host_tests;` (not in `tests/mod.rs`, which
T302 holds). Drive the line as `tests::vim_line` does (`:` then the text, then Enter, vim on), on
temp folders; read the message as the bar's `VIM_MESSAGE` item; check prompts with
`cx.pending_prompt()`; check windows with `cx.windows()`, as `close_tests` does.

## Size

About 250 non-test lines.

## Out of scope

`:e!` / `:e` revert from disk, `:w {path}` (fe-vim refuses it), `:sav`, buffer lists, `:qa` across
every window, changing which button the close prompt defaults to, and any fe-vim parsing change.

## Implementation notes

- `VimHostCommand` lives in `vim_line.rs`, a private child of `status_bar.rs`, which isn't in
  scope, so nothing outside `status_bar` can name it. `Workbench::new`'s `subscribe_in` closure
  leaves the event's type to inference: `VimHostCommand` is `StatusBar`'s only `EventEmitter`
  impl. The closure passes the editor and the command on to `run_vim_host_command`.
- Saves go through `ItemHandle::save` and are awaited (`cx.spawn_in`), so `:wq` and `:x` close
  once the save has landed. `:wa` awaits every save before it shows the first failure in pane
  order.
- `:e` resolves `.` and `..` by name, so `:e ./a.txt` finds an open `a.txt`. `~user/…` isn't
  expanded.
- `tests::vim_line::ex_through_the_line` (T219) asserted that `:w` leaves the editor dirty
  ("nothing here saves"), which C1's `:w` contradicts. Rev 3 let its `:w` step change: it now
  expects the editor clean, and nothing else in that file changes.
