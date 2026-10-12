---
task_uid: "01m4p1wtc8gg129x1rgdq9qgze"
task: "T5"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test terminal_panel_tests::toggle_cycles_focus: from the editor, ctrl-backtick shows the bottom dock with the terminal focused; when the dock is shown but unfocused, ctrl-backtick focuses the terminal; when the terminal is focused, ctrl-backtick hides the dock and focuses the active editor"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test terminal_panel_tests::starts_in_the_project_root: the first show spawns one session (test shell /bin/sh) whose pwd is the launch root, and hiding then showing again keeps that same session"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test terminal_panel_tests::exit_hides_the_panel: when the shell exits, the dock hides and focus returns to the editor, and the next toggle starts a new session"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test terminal_panel_tests::terminal_keys: in DEFAULT_MACOS, with the terminal focused, f12, shift-f12, ctrl-- and ctrl-shift-- match no binding, while in the editor they still run nav.*; ctrl-backtick resolves to terminal.toggle on both macOS and Linux"
    required: true
    manual: false
  - id: "C5"
    text: "fe-keymap's one_keymap test passes, and terminal.toggle is in fe-workbench's command_list() for the palette"
    required: true
    manual: false
---

# Toggle a terminal in a bottom panel of the workbench with ctrl-backtick, started in the project root (Mac T382)

Filed on argon from Mac T382 (card ~/coordination/argon-cards/T382.json, Mac lead message ~/coordination/from-mac-lead-2026-10-11-argon-a.md). Mac task uid 01m4p1gn9tq1kz8a1d1dsjs4qv.

Held in argon's inbox: it also depends on Mac T376 (git Changes panel, landing on the Mac side, touches crates/fe-workbench/src/workbench.rs and lib.rs). Ready it once T376 reaches mac/main and has been synced into argon's trunk.

## Problem

The user asked on 2026-10-11: "let's also spec out a terminal pane that we can run in the app". The view card
gives a `TerminalView` (`crates/fe-terminal-view`), but the app never shows one. The workbench has only a
right dock (`crates/fe-workbench/src/workbench.rs:268–292`, `shell.set_dock(DockPosition::Right, …)`), and
kiln-shell already lays out a bottom one (`DockPosition::Bottom`, `kiln/crates/kiln-shell/src/dock.rs:46–52`,
drawn by `kiln/crates/kiln-shell/src/shell.rs:371`).

## Change

**Bottom panel.** A new `crates/fe-workbench/src/terminal_panel.rs`:

- `TerminalPanel` implements `kiln_shell::DockItem` (`id` `PanelId("terminal")`, title "Terminal" or the
  shell's title from `TerminalView::title`, icon `Icon::FileTerminal`, `show_command` `"terminal.toggle"`,
  `focus_on_show` true). It holds `Option<Entity<TerminalView>>`, created on first show with
  `TerminalOptions { cwd, shell: None, .. }`: `cwd` is the launch root, else the opened file's directory, else
  `$HOME`. Its `TerminalStyle` comes from the workbench's `EditorTheme` (font family, font size, background,
  foreground, cursor, selection; `ansi` from `TerminalStyle::ansi_for(background)`).
- `Workbench::new` adds a `kiln_shell::Dock` at `DockPosition::Bottom` holding the panel, hidden at launch,
  with `shell.set_dock(DockPosition::Bottom, …)`. Its height is kiln's default dock size.
- When the shell exits (`TerminalViewEvent::Exited`), the panel drops the view and hides the dock, focusing the
  active editor; the next show starts a new shell. Closing the window or quitting drops the session (SIGHUP).
  No close veto for a running process, and the terminal isn't restored with the saved layout: both later cards.

**Command and key.** `terminal.toggle` ("Toggle Terminal"), registered in `terminal_panel::init` (called from
`fe_workbench::init`) in the `workbench` context, the way `outline_panel::init` registers `outline.show`, so it
is in the command palette and the keymap catalog. It behaves as VS Code's: dock hidden → show it and focus the
terminal (spawning on first show); shown but not focused → focus the terminal; terminal focused → hide the
dock and focus the active editor. Bind `ctrl-\`` to it in the `workbench` context on every platform, in
`crates/fe-keymap/keymaps/fastedit.toml`, `default-macos.toml` and `default-linux.toml` (fe-keymap's
`one_keymap` test requires the three to agree). `ctrl-\`` is unbound today in all three files; vim gives
ctrl-` no meaning, so taking it in the editor loses nothing.

**Keys the terminal must receive.** On macOS four workbench bindings take keys a shell uses: `f12`
(`nav.goto_definition`), `shift-f12` (`nav.find_references`), `ctrl--` (`nav.back`) and `ctrl-shift--`
(`nav.forward`, which shadows readline's undo, ctrl-_). Change those four rows' context from `workbench` to
`workbench && !terminal` in all three keymap files. `terminal` is the view's key context (`fe_terminal_view::KEY_CONTEXT`).

**Linux (note for Neon, not done here).** On Linux secondary is ctrl, so workbench bindings such as ctrl-b,
ctrl-p, ctrl-w and ctrl-shift-p take keys a shell needs. The Linux policy (VS Code's: most ctrl- keys go to the
shell while the terminal is focused, ctrl-shift- ones stay the app's) is a follow-up card for the Linux port.

Tests in a new `crates/fe-workbench/src/terminal_panel_tests.rs` (declared `#[cfg(test)]` from
`terminal_panel.rs`, so `src/tests/mod.rs` isn't touched), using the `opened` workbench helper style of
`src/tests/activity_rail.rs`. A test whose shell must not be the user's sets the panel's shell to
`/bin/sh` through a `#[cfg(test)]` hook on `TerminalPanel`.

## Implementation notes

- The panel is not `always_on_rail`: kiln's bottom dock draws its rail row even while hidden, so the
  terminal's button joins it on first show and the bottom dock takes no room until then.
- The workbench starts the session before `Dock::show`, so `focus_on_show` focuses the view's own handle.
  A rail click after the shell exited shows the panel with no session: its render starts one and moves
  focus onto it. A rail click that hides the focused terminal (`DockEvent::HidFocus`) focuses the editor.
- The session's directory follows `Workbench::set_root` for the next session; a running one stays.
  A directory that has gone falls back to `$HOME`, then `/`. The style follows settings reloads.
- Tests outside the listed scope that pinned the keymap changed with it: fe-gpui's binding counts
  (`command_tests`, `keymap_tests`, `vim::tests`, which also lists ctrl-` among the ctrl keys a vim
  editor gives up) and `tests::references_panel`'s shift-f12 context.
