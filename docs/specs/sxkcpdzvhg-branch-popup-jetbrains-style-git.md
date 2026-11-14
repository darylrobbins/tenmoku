---
task_uid: "01m79d44t299h8rhsxkcpdzvhg"
task: "T369"
rev: 1
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "tests::git_branch_menu::{the_widget_click_opens_the_menu,filter_narrows_every_section,header_update_runs_fetch_then_ff_pull,branch_submenu_checkout_and_new_branch_from,rename_and_delete_ask_first,keyboard_navigation_and_escape,diverged_update_says_cannot_fast_forward} pass"
    required: true
    manual: false
  - id: "C2"
    text: "git_branch_tests, git_remote_tests, tests::git_sync_ui and the title_bar tests pass; the l10n lint passes"
    required: true
    manual: false
---

# Branch popup: JetBrains-style Git branches menu from the title bar branch widget (Update Project, Push, Fetch, New Branch, search, Recent/Local/Remote, per-branch actions) (Mac T1768)

# Branch popup: a JetBrains-style Git branches menu (depends on branch-api, branch-counts)

Replace the title bar branch widget's click (git.switch_branch picker) with a popup, anchored under the widget, built from the existing MenuPanel/MenuEntry (project_panel_menu.rs pattern: anchored+deferred, Up/Down/Enter/Escape, focus-leave closes, own key context).
Layout (JetBrains): 
- Header rows: Update Project (fetch + ff-only pull), Commit… (git.commit/Changes panel), Push (Publish with no upstream), Fetch; | New Branch…, (Checkout Tag or Revision… omitted).
- A filter field at top ("Search branches"), typing filters all sections.
- Sections: Recent (last 5 checked out: from reflog `checkout: moving from`, new fe-git `recent_branches(n)` reading `git reflog` -- include in branch-api if small, else omit and note), Local, Remote (grouped by remote). Current branch marked, each row with `↑N ↓M` (Branch.ahead/behind exist) and a star-less layout; row detail = tip summary + age as the picker does.
- Each branch row opens a submenu (right arrow / hover chevron): Checkout; New Branch from Selected…; Merge into Current; Rename… (local, not remote); Delete (local non-current; asks via the existing question dialog if unmerged); on the current branch: Push, Pull, Update. Remote rows: Checkout (tracking local), New Branch from Selected…, Delete only if branch-api allows (not in this card).
- `git.switch_branch` (palette, :Git switch) keeps the picker; the title widget calls a new `git.branches_menu` command (no default key).
- Outcomes: toasts via git_remote/git_branch Outcome; refresh after each as T1747.
## Scope
crates/fe-workbench/src/git_branch.rs, new git_branch_menu.rs, title_bar.rs, workbench_chrome.rs, workbench_commands.rs (command), git_commands.rs, l10n/en-US/git.ftl + chrome.ftl, new tests/git_branch_menu.rs declared in tests/mod.rs (the_widget_click_opens_the_menu, filter_narrows_every_section, header_update_runs_fetch_then_ff_pull, branch_submenu_checkout_and_new_branch_from, rename_and_delete_ask_first, keyboard_navigation_and_escape, diverged_update_says_cannot_fast_forward).
## Goldens/audit
title_bar tests pin the click (`git.switch_branch` named in title_bar.rs tests): update them. Add an audit scene for the open menu in audit_scenes.rs (scenes live there, T1750); no existing golden moves. l10n lint: new strings in .ftl; command titles in commands.ftl.
Platform: linux-ok. Size L; split submenus into a follow-up if the first pass exceeds it.

## Implementation notes

- **The menu** is `git_branch_menu.rs`, a modal view shown as `Palette::Branch` (so another palette replaces it, and it replaces the branch pickers), anchored under the widget's bounds, which the title bar records each frame (`title_bar::branch_anchor`); from the palette with no title bar it opens at the top of the window. Its own key context (`git_branches_menu`) takes Up/Down/Left/Right/Enter/Escape/Backspace and typed characters. `MenuPanel` has no section titles, filter row or two-part rows, so the main list draws its own rows in `MenuPanel`'s look; each branch's actions are a `MenuPanel`.
- **Search** is kiln's `TextInput`, focused while the menu shows, so composed (IME) and pasted text work; the menu runs its editing commands (`text_input.backspace`, `text_input.paste`, …), as the shortcuts editor does, and keeps Up/Down/Left/Right/Enter/Escape (no `text_input` binding takes those for an unnamed host).
- **Mutations** (switch, create from a ref, merge, rename, delete) take their turn in `git_stage`'s queue, as pulls, Update Project and commits do (`queue_branch_work`), and hold it until they land. An unmerged delete's yes is for the tip it was asked about: a branch that moved since asks again. A merge landing after a root change still checks the kept tabs against disk.
- **Focus** goes back to where it was as the menu opened on every close (Escape, a click outside, the widget again, an action), unless something else took it.
- **Recent** reads `git reflog show --format=%gs` in the workbench (`recent_branches`), not a new fe-git `recent_branches(n)`: fe-git is outside this card's scope.
- **Update Project** is a new typed command, `git.update_project` (Git: Update Project), in `git_remote.rs`: `git fetch --no-prune` of the upstream's remote, then `git merge --ff-only --no-autostash <upstream ref>`, in the pull's queue. The branch and its upstream's full ref are pinned before the fetch and checked again after it: if either changed, nothing is merged. This answers the carried T368 F1 finding (no pruning whatever `fetch.prune` says; divergence is the `Can't update: … can't fast-forward` toast with no choices). It runs git itself, as fe-git's fetch does (no prompts, BatchMode ssh), since fe-git's `fetch(false)` defers to `fetch.prune` and fe-git is out of scope.
- **Forced files outside the scope list**: `tests/title_bar_b3.rs` and `git_branch_tests.rs` pin the widget's click on `git.switch_branch`; both now expect `git.branches_menu` (and `git.switch_branch` still opens the picker).
- **Audit scene**: `examples/screenshots/audit/branch_menu.rs`, registered in `examples/screenshots/audit_scenes.rs` (the card's `src/audit_scenes.rs` is that file). The audit is macOS-only, so it wasn't run here: its probes are placed from the layout (the menu under the widget, 460 px wide) and may need nudging on the Mac. No golden added or moved.
- Submenus, Merge, Rename and Delete all landed in this card; nothing was split out.
