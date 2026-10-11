---
task_uid: "01m4p1wnt66q7tsezbmm332sfs"
task: "T3"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "crates/fe-workbench/src/git_panel_tests.rs: a workbench on a temp repo with a modified, a deleted and an untracked file shows, after git.show_changes, the Changes panel listing the three sorted by path with M, D and U, and the rail has a Changes button after Outline"
    required: true
    manual: false
  - id: "C2"
    text: "Clicking or pressing Enter on the modified row opens that file in the active pane; the deleted row opens nothing"
    required: true
    manual: false
  - id: "C3"
    text: "A root outside any repository shows \"Not a git repository\", a clean repository \"No changes\", and files of the repository outside the project root are not listed"
    required: true
    manual: false
  - id: "C4"
    text: "Status runs off the main thread, and a git.refresh during a running status gives exactly one more run, whose result is the one shown"
    required: true
    manual: false
---

# Add a Changes panel to the dock listing the project's git-changed files with their status, opening one on click (Mac T376)

Filed on helium from Mac T376 (~/coordination/helium-cards/T376.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-a.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The user asked on 2026-10-11 for "a side pane with the list of changed files". The right dock
(crates/fe-workbench/src/dock.rs) shows one panel at a time, picked from the activity rail:
`DockPanel::{Project, Search, Outline, References}`, each with a show command registered in
`dock::registry()` and an `Icon`. There's no panel for git changes. fe-git (the card this depends on)
provides `Repo::discover` and `Repo::status`.

## Change

Add a **Changes** panel, `crates/fe-workbench/src/git_panel.rs` (`GitPanel`), in the dock:

- `DockPanel::Changes`: name "Changes", id `"changes"`, icon `Icon::GitBranch` (already in kiln-ui),
  show command `git.show_changes` ("Show Changes"), placed after Outline in `DockPanel::ALL`. Register
  `git.show_changes` and `git.refresh` ("Refresh Changes") with the dock's commands in
  `dock::registry()`; no default keys.
- `GitPanel::new(root: Option<PathBuf>, ui, cx)` discovers the repository for the project root and runs
  `Repo::status` with `cx.background_spawn`, never on the main thread: when it is first shown, and on
  `git.refresh`. A refresh while one is running runs once more after it; an older result never replaces
  a newer one. `GitPanel::set_statuses(Vec<FileStatus>, cx)` replaces the list (the live-refresh card
  calls it).
- Only files under the project root are listed (the repository may be larger), sorted by path relative
  to the root. Each row, in a `uniform_list` like the outline panel's, shows a one-letter status
  (`M`, `A`, `D`, `U`, `C`), the file name, and its directory in `muted_text`. Letter colours come from
  existing `UiTheme` fields, no new ones (the theme cards own those): `D` and `C` in `error_text`, `M` in
  `accent`, `A` and `U` in `accent` with its hue set to 130° (green), derived in git_panel.rs.
- Click or Enter on a row emits `GitPanelEvent::OpenFile(path)`, which the workbench handles like the
  project panel's `PanelEvent::OpenFile` (opens it in the active pane). A `Deleted` row is shown
  dimmed and doesn't open.
- Empty states: no folder open or no repository: "Not a git repository"; a clean repository: "No
  changes"; a status error: its message.
- Wire it in crates/fe-workbench/src/workbench.rs (create it with the root, pass it to `Dock::new`,
  subscribe to its event; and on opening another folder, replace it as the project panel is) and
  crates/fe-workbench/src/lib.rs (the module, its `init`), plus the `fe-git` dependency in
  crates/fe-workbench/Cargo.toml.

Tests in `crates/fe-workbench/src/git_panel_tests.rs`, included from git_panel.rs by `#[path]`, on temp
repositories built with the `git` CLI as fe-git's tests do. Update existing rail tests that count the
rail's buttons.

## Out of scope

Live refresh from the file watcher and the editors' diff bases (the next card), staging, diffs in the
panel, a tree view of the changes.

## Implementation notes

- Enter, Up and Down reach the panel through an `on_key_down` listener on its element (key context
  `git_panel`), not keymap bindings: the keymaps are outside this card's scope, and nothing binds those
  keys in the workbench's context, so the listener sees them. The first Up or Down selects the first row.
- The panel is focused when shown (`focus_on_show`), as its rows take keys.
- The first status runs from the panel's first render, which covers `git.show_changes`, a rail click
  and a restored dock alike. The repository found is kept for later runs.
- Paths are listed relative to the root as given; when the repository's working tree is reached through
  a symlink (macOS's `/var` → `/private/var`), the root's place in it is found by canonicalizing both.
- `tests/references_panel.rs` counts the rail's buttons too, so it now expects the Changes button.
