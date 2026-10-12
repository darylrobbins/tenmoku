---
task_uid: "01m4rqekk9cz6xpsayf1f1d4pe"
task: "T58"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "recent_picker_tests::records: opening a folder and two files records them, most recent first, in the temp state dir's recent.toml"
    required: true
    manual: false
  - id: "C2"
    text: "recent_picker_tests::picker: workbench.open_recent lists folders then files, filters by the query, leaves out deleted paths; Enter on a file opens it in the active pane, Enter on a folder re-roots the window"
    required: true
    manual: false
  - id: "C3"
    text: "recent_picker_tests::new_window_and_clear: the new-window confirm opens the folder through the window opener, and workbench.clear_recent empties the list"
    required: true
    manual: false
  - id: "C4"
    text: "recent_picker_tests::vim: with vim on, opening the picker from normal mode and escaping it returns focus to the editor in normal mode, and a file chosen opens in normal mode"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass; nothing outside the scope changes except this card's docs/specs file"
    required: true
    manual: false
---

# Open Recent: record opened folders and files, and pick one from an Open Recent picker that re-roots on a folder and opens a file (Mac T513)

Filed on helium from Mac T513 (~/coordination/helium-cards/T513.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-x.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium notes (Mac note x, stream 2, Open Recent): helium is Linux. Where the card says Finder, read the file manager; test drops through GPUI's external-paths drop events in a gpui test, not a real drag. Paths, config locations and the recent list follow fe-config's existing per-platform rules (XDG on Linux).

## Problem

There's no Open Recent (day-one audit 2026-10-12). The previous card adds `fe_config::recent`.

## Change

- Recording, in `crates/fe-workbench/src/workbench.rs`: a folder each time a window takes a root (`set_root` and
  launch), a file each time an editor with a path is tracked (`track_editors`), off the UI thread through
  `recent::record`.
- New `crates/fe-workbench/src/recent_picker.rs`: a `PickerDelegate`, as `language_picker.rs` is, opened by
  `workbench.open_recent` "Open Recent…" (a new `Palette` in workbench.rs). Folders first, then files, each with its
  path shortened by `~`; fuzzy-filtered by the query; paths that no longer exist are left out. Enter on a folder
  re-roots this window as Open Folder does; cmd-Enter (`picker.confirm_secondary` if the picker has one, else a
  new `recent.open_in_new_window` command) opens it in a new window; Enter on a file opens it in the active pane.
  `workbench.clear_recent` "Clear Recently Opened" empties the list.
- The File menu gets a single "Open Recent…" item (the keys card): kiln's menu tables have no submenus or
  dynamic items, so a native Open Recent submenu needs a kiln card first.
- Vim: the picker's input is `text_input`, so vim never sees its keys; ctrl-n/ctrl-p move as in every picker, and
  pickers have no normal mode, so j/k type. Opening it from vim normal mode and dismissing it (escape) returns
  focus to the editor in normal mode; a file chosen opens in normal mode.

## Tests

New file `crates/fe-workbench/src/recent_picker_tests.rs`, declared from recent_picker.rs, with a temp state dir.

## Implementation notes

- `recent_picker.rs` is declared from `workbench.rs` (`#[path]`, as `external_drop.rs` and `toast.rs` are), so
  `lib.rs` stays out of the change. Its commands register, and join the command palette, from the first workbench,
  as `toast.dismiss_all` does; like it, they aren't in `command_catalog` yet, which the keys card does with the
  File menu item.
- The list's reads and writes run on one background queue per app, in the order asked, so a list read for the
  picker sees every record made before it opened, and records land in the order the opens happened.
- Under `cfg(test)` nothing is recorded unless a test sets the list's file (`set_recent_file`), so no test
  touches the user's `recent.toml`.
- A launch on a file records the file but not its parent folder: the folder is only where it opens, not one the
  user opened. A launch on a folder, and every `set_root` (Open Folder, a drop, the picker), records the folder.
- A file is recorded when its editor is first tracked, so a split view of it, or a restored layout's files, record
  it again (it only moves to the front).
- There's no `picker.confirm_secondary`, so a folder row has an "Open in New Window" button, which opens it through
  the window opener, and `recent.open_in_new_window` is a command in the `picker` context, for the keys card to
  bind, that the workbench runs (after its own update) while Open Recent shows: a folder opens in a new window; a
  file, which a new window can't take, opens as Enter does. It isn't listed in the command palette, which would
  replace the picker it acts on.
- A file chosen that's already open in the pane keeps the vim mode it was left in, so it's escaped back to normal
  mode, as a dropped file is.
- Clearing while the picker's list is still being read drops that read, which may predate the clear.
- Filtering is the toolbar picker's subsequence match, on the `~`-shortened path, highlighted.
