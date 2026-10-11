# Linux menu bar: the menu table, drawn in the window

Status: **build**. Decided 2026-10-10.

## Problem

On macOS, `fe_gpui::menu::install` hands the menu table
(`fe_keymap::MenuBar`) to AppKit. GPUI has no native menu bar on Linux, so
there `set_menus` keeps the menus and shows nothing. Thirteen commands are in
a menu and bound to no key (Open File…, Untrust This Folder, Restart Language
Server, the View toggles…), so on Linux nothing reaches them. There is no
command palette yet.

## Decision

Draw the same kind of table in the window, as a row of menu names above the
panes, each opening a dropdown of its items with their shortcuts. It is the
whole fix for Linux and stays useful once a command palette exists: a menu is
how a newcomer finds commands, and every Linux editor has one.

**The table.** A Linux table, `fe-keymap/menus/default-linux.toml`
(`DEFAULT_LINUX_MENUS`, `MenuBar::default_linux`), not the macOS one
transformed: Linux has no app menu, so its items move where Linux apps keep
them: Settings… and Quit to File. Minimize, Zoom, Hide, Hide Others, Show All
and Services are macOS's and are left out; everything else is the same items
in the same menus and order. `MenuBar::default_for_platform` picks the table.
A test checks the two tables hold the same commands but for those.

**Where it lives.** Stateless rendering in fe-ui (`MenuStrip`, the row of
names, and `MenuPanel`, the dropdown), like `PaletteFrame` and `TabBar`. The
stateful view, `MenuBarView`, in fe-workbench beside `title_bar.rs`: the
window frame moves to `fe-shell` together (`app-shell.md` §1). fe-gpui's
`menu` module gains what both kinds of menu share: `installed`, the table in
use, and `run`, which runs an item as `choose` does for AppKit.

Since kiln's K4.1, `MenuBarView` and the window frame live in kiln-shell
(`kiln/crates/kiln-shell/src/menu_bar.rs`, `window_frame.rs`), and
fe-workbench draws them.

**Showing it.** `Workbench::set_menu_bar(true)`, which the standalone app
calls where the platform has no native menu bar, as it calls `set_title_bar`
on macOS. Tests call it on every platform, so CI on macOS covers the view.

**Behaviour.**

- A click on a name opens its menu, another click closes it. While one is
  open, hovering another name opens that one instead.
- A click on an item runs it and closes the menu; a click outside closes it.
- F10 opens the first menu, as in GNOME and KDE apps. While open: Left and
  Right move between menus, Up and Down between items (skipping separators),
  Enter runs the highlighted item, Escape closes.
- Opening takes focus, so the keys above reach the menu; closing gives it
  back to what had it. An item runs after focus is back, in that element's
  context, so Edit > Paste pastes into the search box when that had focus,
  through the item's `alternatives`, as on macOS.
- Shortcuts show as `Ctrl+Shift+F` (`fe_keymap::shortcut_label`), read from
  the keymap when the menu opens, so they follow hot reloads.
- Every item is enabled, as on macOS (`fe_gpui::menu`'s known limit): an item
  that doesn't apply where focus is runs nothing, as its key wouldn't.

**Not now.** Alt+letter mnemonics, and submenus (the table has none but
Services). Under client-side decorations (GNOME on Wayland) the row also
becomes the title bar, with the window's buttons: a separate change.
