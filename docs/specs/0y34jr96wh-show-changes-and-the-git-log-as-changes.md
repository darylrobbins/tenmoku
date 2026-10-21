---
task_uid: "01m5f77657k2hkn10y34jr96wh"
task: "T1023"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test dock::source_control_views_tests::the_rail_has_one_source_control_button: no GitGraph button, one GitBranch titled Source Control; views are Changes then Graph"
    required: true
    manual: false
  - id: "C2"
    text: "dock::source_control_views_tests::show_changes_and_git_log_show_their_view: git.show_changes shows the container with Changes expanded and focused; git.log expands a collapsed Graph headed 'Graph: main' listing HEAD's commit; git.log_all retitles it 'Graph: All Branches' keeping its weight"
    required: true
    manual: false
  - id: "C3"
    text: "dock::source_control_views_tests::views_collapse_and_weights_survive_a_restart and ::an_old_layout_on_git_log_restores_the_graph pass (saved active 'git_log' -\u003e container with Graph expanded; 'changes' -\u003e container)"
    required: true
    manual: false
  - id: "C4"
    text: "Existing git_log_tests, git_log_open_tests, git_panel_tests and git_history_tests pass with only rail/DockPanel addressing changed; no command id changes; workbench.png regenerated on the Mac"
    required: true
    manual: false
---

# Show Changes and the Git Log as Changes and Graph views of one Source Control view container, with one source-control button on the rail

# Show Changes and the Git Log as Changes and Graph views of one Source Control view container, with one source-control button on the rail

## Problem

The user (2026-10-21): "What happened to work to combine changes and git log views?" T808's spec
deferred it ("a 'Graph' section folded into the Changes panel, as VS Code's Source Control view has since
1.93") until kiln-shell's ViewStack/ViewContainer (T981-T983) landed. They have, and the Explorer already
uses them (T984): dock.rs `explorer()`/`folders()` build a `ViewContainer` of Folders (project tree) over
Outline, `DockOp::ShowOutline` shows the Explorer and expands Outline, and `Dock::restore` maps a saved
`"outline"` active panel to the Explorer. The rail still has two git buttons: Changes
(`git_panel::SIDEBAR`, `Icon::GitBranch`, order 40) and Git Log (`git_log::SIDEBAR`, `Icon::GitGraph`,
order 45). VS Code has one Source Control button whose container holds the Changes view and the Graph view.

## Change

Do for Changes and Git Log what T984 did for the project tree and the outline (commit b66e82c0 is the
pattern).

- **Container.** New crates/fe-workbench/src/source_control.rs, declared from dock.rs by
  `#[path = "source_control.rs"] mod source_control;` (no lib.rs change). Its `SIDEBAR`: panel id
  `"scm"`, name "Source Control", `Icon::GitBranch`, show command `git.show_changes`, show op
  `DockOp::ShowChanges`, order 40, always on the rail, focused on show. A `ViewContainer` of two views:
  `CHANGES` ("Changes", order 10, expanded: the `GitPanel`, commit box and all) and `GRAPH` (order 20,
  expanded, as VS Code's: the `GitLogPanel`). Remove `git_panel::SIDEBAR` and `git_log::SIDEBAR` from
  `PANELS` and their `dock_item!` impls; `DockPanel::Changes` becomes the container (rename to
  `SourceControl` or keep the name, one or the other everywhere) and `DockPanel::GitLog` goes; the git
  log is addressed as the `GRAPH` view. Add the container's views' state to the dock's saved state, as
  the Explorer's is (subscribe and notify like `_explorer_changed`).
- **Graph's header** reads "Graph: main" / "Graph: All Branches" (what `GitLogPanel::title` gives, with
  "Graph" for "Git Log"). Retitle it by `ViewContainer::add_view` with the same id after each listing:
  a replace keeps the view's collapse, weight and header focus (view_container.rs:102), so no kiln-shell
  change is needed.
- **Commands keep working.** `git.show_changes` and a rail click show the container with Changes
  expanded and focused, as today. `git.log`, `git.log_all` and `DockOp::ShowGitLog` show the container
  with Graph expanded and list into it, focus as today. Graph's first listing still happens on its first
  draw (T808/T1002), now when the container first draws with Graph expanded. `git.refresh`,
  `git.file_history` (History stays its own panel) and the commit commands are unchanged.
  `Dock::set_changes` (new root) replaces the Changes view as `set_project` replaces Folders.
- **Old layouts.** `Dock::restore` maps a saved active `"changes"` to the container and `"git_log"` to
  the container with Graph expanded, as it maps `"outline"`.
- The rail becomes Explorer, Search, Source Control (one `GitBranch`, no `GitGraph`), then the others; the
  title row says "Source Control". Update the module docs in dock.rs, git_panel.rs and git_log.rs.

Update the tests that pin the rail or address `DockPanel::Changes`/`GitLog`: sidebar_panels_tests.rs,
git_log_rail_tests.rs, git_panel_tests.rs, git_log_tests.rs, git_log_open_tests.rs, git_history_tests.rs,
chat_panel_tests.rs, chat_rail_tests.rs, dock_position_tests.rs, panel_width_tests.rs,
explorer_views_tests.rs, layout_compat_tests.rs, tests/activity_rail.rs, tests/references_panel.rs.
Regenerate crates/fe-workbench/examples/screenshots/golden/workbench.png (one git button; Mac only).

New tests in crates/fe-workbench/src/source_control_views_tests.rs, declared from dock.rs next to
`explorer_views_tests`:

- `the_rail_has_one_source_control_button`: a fresh workbench on a repository has no `GitGraph` button and
  one `GitBranch`, titled "Source Control"; the container's views are Changes then Graph.
- `show_changes_and_git_log_show_their_view`: `git.show_changes` shows the container with Changes
  expanded and the git panel focused; collapse Graph, then `git.log` expands it and its header reads
  "Graph: main" listing HEAD's commit; `git.log_all` retitles it "Graph: All Branches" keeping its weight.
- `views_collapse_and_weights_survive_a_restart`: collapse Changes, drag the divider; a workbench restored
  from the saved state has them so.
- `an_old_layout_on_git_log_restores_the_graph`: saved state with `"active": "git_log"` restores to the
  container with Graph expanded; `"changes"` to the container.

Not in this card: Graph header actions (scope picker, fetch, refresh) and VS Code's incoming/outgoing
markers; a Timeline view for file history.

## Implementation notes

- `DockPanel::Changes` is renamed `DockPanel::SourceControl` everywhere (panel id `"scm"`); `DockPanel::GitLog` is gone.
- kiln-shell's `ViewContainer` has no `focus_on_show`, so `Dock::run` focuses the view itself: the
  `GitPanel` on `ShowChanges`, the `GitLogPanel` on `ShowGitLog`.
- The dock observes the `GitLogPanel` and re-adds the Graph view only when `GitLogPanel::title`
  changes, keeping the title it last gave (`Dock::graph_title`, which the tests read: `ViewStack` has
  no title accessor).
- `GitLogPanel::title` now gives the Graph header's text ("Graph: main", "Graph: All Branches",
  "Graph: HEAD (detached)", "Graph"); `git_log::OLD_PANEL` is the id old layouts name.
- `git_log_tests::keys_and_pages` sends its wheel event to Graph's body rather than the dock column's
  centre, which is now the Changes view.
