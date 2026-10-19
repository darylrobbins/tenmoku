---
task_uid: "01m55b08qp8smpd7296s3j0y89"
task: "T808"
rev: 6
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test fe-workbench dock::sidebar_panels_tests::git_log_rail_tests::the_rail_has_git_log_under_changes_from_the_start_with_its_own_icon passes: a fresh workbench on a repository, before any git command, has the rail Project, Search, Outline, Changes, Git Log, with no two rail buttons drawing the same icon and Git Log's not Icon::GitBranch; after git.log and git.file_history there is still no duplicate icon"
    required: true
    manual: false
  - id: "C2"
    text: "Test fe-workbench dock::sidebar_panels_tests::git_log_rail_tests::clicking_git_log_on_the_rail_lists_heads_log passes: clicking the Git Log rail button of a fresh workbench shows the dock with Git Log titled 'Git Log: main' listing HEAD's commits, and a second click hides the dock"
    required: true
    manual: false
  - id: "C3"
    text: "Test fe-workbench dock::sidebar_panels_tests::git_log_rail_tests::git_log_from_the_rail_outside_a_repository_says_so_without_a_toast passes: rooted at a folder outside any repository, the rail button shows the panel saying 'Not in a git repository' and no toast shows"
    required: true
    manual: false
  - id: "C4"
    text: "Git Log's rail icon is Lucide's git-graph, bundled unmodified as Icon::GitGraph (kiln-ui tests every_icon_is_bundled_and_parses counts 41 and passes; kiln/THIRD_PARTY.md and THIRD_PARTY.md list git-graph.svg); Changes keeps Icon::GitBranch; git.log and git.log_all behave as before (git_log_tests and git_log_open_tests pass unchanged)"
    required: true
    manual: false
  - id: "C5"
    text: "The rail-pinning tests in sidebar_panels_tests.rs, git_panel_tests.rs, tests/activity_rail.rs and tests/references_panel.rs are updated to the new rail and pass; no file outside the scope changes, and no command registration changes"
    required: true
    manual: false
---

# Put the Git Log on the activity rail from the start, under Changes, with its own git-graph icon, instead of a second branch icon once git.log has run

## Problem

The user, from the running app (2026-10-17, main 92c57c57): "We should make it [the Git Log] available
somehow from the side bar." Their screenshot shows the activity rail with **two identical branch icons**,
one under the other.

What they are:

- The first is **Changes** (`crate::git_panel::SIDEBAR`, crates/fe-workbench/src/git_panel.rs:96:
  `icon: Icon::GitBranch`, `always_on_rail: true`, order 40).
- The second is **Git Log** (`git_log::SIDEBAR`, crates/fe-workbench/src/git_log.rs:68:
  `icon: Icon::GitBranch` as well, `always_on_rail: false`, order 80). It joins the rail only once
  `git.log` or `git.log_all` has made the panel (`Workbench::show_git_log` → `Dock::set_git_log`,
  git_log.rs:198, dock.rs:338). Until then, the sidebar can't reach the Git Log at all. Afterwards its
  button looks exactly like Changes'. sidebar_panels_tests.rs:81 and :263 pin that shared icon.

So: no rail entry until a command has run, then a duplicate icon that doesn't say what it opens.

## Change

Give the Git Log its own rail entry from the start, right under Changes, with its own icon.

- **Icon.** Add Lucide's `git-graph` to kiln-ui, unmodified, from the commit THIRD_PARTY.md already pins:
  `https://raw.githubusercontent.com/lucide-icons/lucide/500620a2e8123f8d1db191538886dc0c223f69a9/icons/git-graph.svg`
  → kiln/crates/kiln-ui/assets/icons/git-graph.svg. Add `GitGraph => "git-graph"` to the `icons!` list in
  kiln/crates/kiln-ui/src/file_icons.rs, bump `every_icon_is_bundled_and_parses`' count to 41
  (kiln/crates/kiln-ui/src/tests/assets.rs), and add `git-graph.svg` to the Lucide rows of
  kiln/THIRD_PARTY.md and THIRD_PARTY.md. It's ISC only, not in Lucide's Feather list.
- **Sidebar spec** (git_log.rs `SIDEBAR`): `icon: Icon::GitGraph`, `always_on_rail: true`, `order: 45`.
  In crates/fe-workbench/src/dock.rs, move it after `git_panel::SIDEBAR` in `PANELS` (rail order), make
  `DockPanel::ALL` `[DockPanel; 5]`, and update the module and `ALL` docs. Changes keeps
  `Icon::GitBranch` and its name. The rail tooltips then read "Changes" and "Git Log", each with its key.
- **The panel exists from the start.** In crates/fe-workbench/src/workbench.rs, right after `Dock::new`,
  make the `GitLogPanel` (`GitLogPanel::new(cx.weak_entity(), ui, cx)`) and `dock.set_git_log` it, so
  `Dock::git_log()` is always `Some`. This is one call, with no change to command registration.
  `show_git_log` keeps working as now: `git.log` and `git.log_all` list into that panel and show it.
- **First show lists.** A rail click runs `DockOp::ShowGitLog`, which only shows the panel. When the
  panel is drawn and has never listed (`read == 0`), it lists once, deferred past the frame as the git
  panel's `ask_for_box` is. It lists HEAD's log (`LogScope::Head`) of the active file's repository, else
  the root's, as `git.log` does, without moving focus. A restored session whose dock was left on Git Log
  lists the same way. Outside a repository, the panel says "Not in a git repository" and raises no toast:
  the first-show op isn't typed and has no editor, so it mustn't reach `answer_git_op`'s toast.

Update the rail-pinning tests to the new rail (Project, Search, Outline, Changes, Git Log, then the
Terminal action): crates/fe-workbench/src/sidebar_panels_tests.rs (`BASE`, the rail order and always-on
tests, and `the_rail_lists_the_panels_in_base_order_and_others_join_once_shown`),
crates/fe-workbench/src/git_panel_tests.rs (rail icons at :226),
crates/fe-workbench/src/tests/activity_rail.rs (`buttons_draw_lucide_icons`) and
crates/fe-workbench/src/tests/references_panel.rs (rail at :300).

New tests go in crates/fe-workbench/src/git_log_rail_tests.rs, declared at the end of
crates/fe-workbench/src/sidebar_panels_tests.rs by `#[path = "git_log_rail_tests.rs"] mod git_log_rail_tests;`.
They use only base API and the parent's helpers (`repo`, `opened`, `rail`, `shown`, `dispatch`, `draw`),
`Dock::rail_button_bounds` and `VisualTestContext::simulate_click`, plus `DockPanel::GitLog` and
`fe_ui::Icon`, not `Icon::GitGraph` (absent at base). Read the panel's rows through the dock's
`git_log()` and `GitLogPanel::lines()`, as git_log_open_tests.rs does.

- `the_rail_has_git_log_under_changes_from_the_start_with_its_own_icon`: on a fresh workbench, before any
  git command, `rail()` lists Project, Search, Outline, Changes, Git Log in that order. No two rail
  buttons draw the same icon, and Git Log's isn't `Icon::GitBranch`. After `git.log`, then
  `git.file_history`, there's still no duplicate. It fails at base: Git Log isn't on the rail until
  `git.log` runs, and then it draws a second `GitBranch`.
- `clicking_git_log_on_the_rail_lists_heads_log`: a click on the Git Log rail button of a fresh workbench
  shows the dock with Git Log, titled "Git Log: main", listing the repository's commit ("init"). A second
  click hides the dock. It fails at base: `rail_button_bounds(DockPanel::GitLog)` is `None`.
- `git_log_from_the_rail_outside_a_repository_says_so_without_a_toast`: rooted at a plain temp folder, the
  click shows "Not in a git repository" in the panel (`shown_message`), and no toast shows.

Not in this card: a "Graph" section folded into the Changes panel, as VS Code's Source Control view has
since 1.93. That needs git_panel.rs (after T795), git.rs (after T793), git_log.rs and workbench.rs
(after T704), and it's about an m.

Files T704 (helium) also touches: git_log.rs (only `SIDEBAR` and a first-draw list in `render`/`list`)
and workbench.rs (one call after `Dock::new`). Nothing here touches command registration,
git_commands.rs, workbench_commands.rs, lib.rs or typed_command_ids_tests.rs. dock.rs is T696's (neon).
