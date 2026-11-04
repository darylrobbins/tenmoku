---
task_uid: "01m6j5s6b07verbx4nmtwvfjy0"
task: "T283"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::coloured_icon_surfaces::every_surface_draws_coloured_ref_as_image: with a stub set answering IconRef::Coloured, each listed surface draws an image and none draws svg() for its file or folder icon"
    required: true
    manual: false
  - id: "C2"
    text: "tests::coloured_icon_surfaces::lucide_reset_redraws_svg: after reset_icon_set each surface draws svg() again"
    required: true
    manual: false
  - id: "C3"
    text: "tests::file_icon_set_surfaces: the existing tests pass unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Draw fe-workbench file and folder icons through IconRef::element so a coloured set shows in the pickers, Changes panel, Graph, AI Changes panel, commit diff and pane crumbs (Mac T1360)

## Problem

After card A, kiln's own surfaces draw an `IconRef` through `IconRef::element`, but fe-workbench draws file and folder icons itself with `svg().path(file_icon_path(..))`, which would draw a coloured icon as a tinted blob (or nothing).

## Change

Replace each `svg().path(file_icon_path/folder_icon_path(..))` with `file_icon_ref/folder_icon_ref(..).element(size, tint)` (sizes and tints unchanged) in: `crates/fe-workbench/src/git_log.rs`, `git_panel.rs`, `ai_changes_panel.rs`, `recent_picker.rs`, `toolbar_picker.rs`, `commit_diff_view.rs`, and the crumb/tab code in `pane.rs`. Callers of `fe_ui::file_icon_path` that only need a path for a debug selector keep it. Update `crates/fe-workbench/src/tests/file_icon_set_surfaces.rs` only if a changed selector name requires it; its Stub sets keep passing.

Surfaces to check against the list above: project tree and tabs (kiln, card A), breadcrumbs, quick open, recent picker, search results (confirm whether search results show an icon; if one is drawn, include it), Changes panel, Graph file rows, AI Changes panel, commit diff headers. The status bar shows no file icons.

## Tests

New `crates/fe-workbench/src/tests/coloured_icon_surfaces.rs` (declared in `tests/mod.rs`): with a stub set that answers `IconRef::Coloured` for files and folders, each surface above draws an image element for its icon, none draws an `svg()` for it, and swapping back to `LucideIcons` redraws them as `svg()`.

## Implementation notes

- Changed: the Changes panel (`git_panel.rs`), Graph file rows (`git_log.rs`), AI Changes panel, recent picker, toolbar picker and commit diff headers now draw `file_icon_ref`/`folder_icon_ref(..).element(14px, tint)`. Where a debug selector was on the `svg()`, it moved to a 14px `flex_none` wrapper `div` (the same selector text, `IconRef::path()`, as kiln's tree rows, tabs and crumbs do), so `file_icon_set_surfaces.rs` is unchanged. The Graph's icon cell still notes the icon's path, and the commit diff's `HeaderSeen::icon` stays a path, so `git_log_file_rows_tests` and `commit_diff_view_tests` are unchanged.
- `pane.rs` needed no change: its file crumb already passes `file_icon_ref` to kiln's `Breadcrumbs`, which draws it through `IconRef::element` (card A); tabs come from `PaneItem::tab_icon_ref`.
- Search results draw no file icon (only a chevron) and quick open draws none, so neither changed.
- Not in this card's scope: `project_panel_ops.rs` `render_field` (the project tree's new-file/new-folder/rename name field) still draws `svg().path(icon.path())`, so a coloured set draws nothing there while a name is being typed. A one-line follow-up.
- Test: `coloured_icon_surfaces.rs` covers the project tree (a root file and the closed folder `d`, shown by `dock.show_project` once the Changes panel has taken the dock) and each surface above, the recent picker with a recent file and a recent folder. It runs the workbench under `HeadlessAppContext` with a renderer that records each frame's sprites: each `svg()` by its atlas key's asset path, each image by its pixels. Each surface's file has a name of its own and the stub gives each name a solid icon of its own colour, so an image tells which surface drew it; the project tree shows only the closed folder `d`. Each surface is checked by what it adds to the frame, before it's shown against after, under each set. Run against the pre-change source, C1 fails at the first surface (the Changes panel draws the coloured path as `svg()`).
