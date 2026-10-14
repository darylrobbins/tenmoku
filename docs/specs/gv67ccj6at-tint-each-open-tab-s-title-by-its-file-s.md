---
task_uid: "01m4w17wxt5t3ngygv67ccj6at"
task: "T714"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-shell tests/tab_title_color: the tabs PaneState builds for drawing carry an item's PaneItem::tab_title_color (Some(c) gives a title in c; the default None keeps the theme's tab text colours), active and inactive, and the painted dirty dot is unchanged in both states. Asserting the drawn title colour through the render path is split out to T715 (user decision 2026-10-14)."
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench tab_git_status_tests: with a.rs Modified and n.rs Untracked set through GitPanel::set_statuses, open tabs a.rs, n.rs and b.rs: a.rs and n.rs titles take git_panel::letter_color for their kind and b.rs none; after set_statuses(vec![]) none is tinted"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench tab_git_status_tests: a tab opened on a.rs after its Modified status landed is tinted as soon as it opens, before any further refresh"
    required: true
    manual: false
---

# Tint each open tab's title by its file's git status, in the Changes panel's status colours

## Problem

The user (2026-10-14) asked for "better git file status indicators" and picked "Tabs show status: tint an
open tab's title by its file's git status". Tab labels are drawn by kiln-shell's `PaneState::tab_bar`
(`kiln/crates/kiln-shell/src/pane.rs` ~line 501) from `PaneItem::tab_title` and `is_dirty`
(`kiln/crates/kiln-shell/src/item.rs`, forwarded by `ItemHandle`) into `kiln_ui::Tab`
(`kiln/crates/kiln-ui/src/tab_bar.rs`), which has no label colour. fastedit's tab item is `EditorView`
(`crates/fe-gpui/src/pane_item.rs`), which knows nothing of git.

The tree card (depends-on) adds `GitPanelEvent::Listed`, `GitPanel::statuses()` and makes
`git_panel::letter_color` `pub(crate)`; this card reuses them, with no second status read.

## Change

1. **kiln-ui `Tab`**: add `label_color(Option<Hsla>)`; `Some` colours the title, active or not, in place of
   the theme's tab text colour; the dirty dot and close button are unchanged.
2. **kiln-shell**: add `PaneItem::tab_title_color(&self, cx: &App) -> Option<Hsla>`, default `None`,
   forwarded through `ItemHandle`; `PaneState::tab_bar` passes it to `Tab::label_color`. Test in the new
   `kiln/crates/kiln-shell/tests/tab_title_color.rs`.
3. **fe-gpui**: `EditorView` gets a `tab_tint: Option<Hsla>` field (`view.rs`) with
   `set_tab_tint(Option<Hsla>)`, notifying only on change; its `PaneItem::tab_title_color` returns it.
4. **fe-workbench `workbench.rs`**: a helper that sets every open editor's tint from the Changes panel's
   statuses by the editor's absolute path (`git_panel::letter_color(kind, ui)`, `None` when unlisted or
   pathless). Call it on `GitPanelEvent::Listed` and where newly opened editors are tracked (next to
   `git.track`, ~line 1279), so a tab opened after the list landed is tinted at once.

Tests in the new `crates/fe-workbench/src/tab_git_status_tests.rs`, registered from `workbench.rs` with
`#[path]`. Drive statuses with `GitPanel::set_statuses`.

## Implementation notes

- `PaneState::tabs` builds the tabs as drawn, minus handlers; `tab_bar` adds handlers to them. The
  C1 test reads each title colour from those `Tab`s (which draw with `Tab::title_color`) and checks
  the painted dirty-dot quads (`Window::painted_quads`) in each tab, active and inactive. GPUI
  exposes no painted glyph colours to tests, so the title colour stops at that seam.
- Editor and listed paths are compared through `resolved`: the folder's longest existing ancestor
  canonicalised, the rest kept, then the entry's own name undereferenced, so symlinked roots, `..`
  and deleted files match while a listed symlink keeps its own status.
