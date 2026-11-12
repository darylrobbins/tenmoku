---
task_uid: "01m78dys3816wbasgqhm8q9k5d"
task: "T332"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::git_sync_ui (cargo nextest run -p fe-workbench) pass: the_branch_widget_shows_ahead_and_behind_counts, no_upstream_shows_a_publish_affordance_not_counts, counts_refresh_after_fetch_pull_push_and_commit, the_panel_header_buttons_run_the_git_commands_and_disable_while_one_runs, a_diverged_branch_says_pull_cannot_fast_forward_and_offers_no_merge, nothing_shows_outside_a_repository"
    required: true
    manual: false
  - id: "C2"
    text: "git_remote_tests, git_branch_marks_tests, git_branch_tests and git_panel_tests still pass; the l10n lint passes"
    required: true
    manual: false
---

# Git sync in the UI: Pull, Push and Fetch in the Changes panel header, ahead/behind counts on the branch widget, refreshed after each (Mac T1747)

# Push/pull/fetch reachable from the UI, with ahead/behind

User report (2026-11-12): "Today I really have no way of pushing or pulling changes from Git." The commands exist (git.fetch/pull/push/push_force_with_lease in workbench_commands.rs ~:596, routed by git_commands.rs to git_remote.rs; fe-git/src/remote.rs), but there is no button and no hint of how far the branch is from its upstream. `Repo::upstream_counts()` (fe-git/src/refs.rs:242, `Some((ahead, behind))`, None with no upstream) is unused outside fe-git.

## Do
- Read counts each time the git refresh reads status (git.rs; background, same executor) and after fetch/pull/push/commit/branch switch; show as "main ↑2 ↓1" on the title bar branch widget (git_branch.rs; today `*`/`+`/`!` marks). None/0 shows nothing; no upstream shows nothing extra. The tooltip names the upstream.
- Changes panel header (git_panel.rs): Pull, Push and Fetch buttons (kiln-ui IconButton, accessible names) plus the counts; Push with no upstream reads "Publish" (git_remote already publishes to origin/the only remote). Buttons disable while a remote op runs (git_remote's ALREADY_RUNNING).
- Behaviour stays git_remote's: pull is fast-forward only; a diverged branch says pull cannot fast-forward and offers no merge (a rebase/merge choice is a later policy decision). Outcomes as toasts. No new force-push UI.
- Ahead/behind needs no network: it reflects the last fetch. Fetch is a button; no auto-fetch here.

Platform: Linux-OK. Overlaps T1591 in git_panel.rs (header vs box region); land in either order.

## Implementation notes

- **Reading.** `fe_git::Repo::upstream_status()` (refs.rs) gives HEAD's upstream's short name with ahead/behind from one `for-each-ref` (`%(upstream:short)`, `%(upstream:track)`), no network. It's `None` for a detached or unborn HEAD, no upstream, a gone upstream, or one the fetch refspec doesn't map (where `upstream_counts` errors): nothing to show in each. A local-branch upstream (`remote = .`) counts, so a repo with no remote can have one (`fe-git/tests/upstream_counts_remote_less.rs`). `git.rs` reads it with every status refresh, on the background executor; fetch, pull, push, commit and branch switch already refresh once they land, so the counts follow them.
- **Branch widget.** `main*+ ↑2 ↓1`: counts after the marks, each left out at 0. The tooltip reads "Switch Branch… (upstream: origin/main)" when there's an upstream. That needed an `upstream` field on `title_bar::State` and `Workbench::title_upstream`, so title_bar.rs, workbench.rs and workbench_chrome.rs grew by a few lines (outside the listed scope).
- **Panel header.** At the top of the Changes panel: the upstream's name and counts, then Pull (ChevronDown), Push/Publish Branch (ChevronUp) and Fetch (RefreshCw) `IconButton`s, each with its key in the tooltip if one is bound. kiln-ui has no arrow-up/arrow-down icon. Adding Lucide's would have meant a kiln-ui change plus both THIRD_PARTY tables and NOTICE, so the chevrons stand in for now. Publish Branch shows only on a branch with no upstream.
- **Busy.** git_remote's `Running` global now has a read-only `is_busy(repository)`, and it tells observers when a command finishes (`forget_done`). The panel observes the global and disables all three buttons while any fetch, pull or push runs in its repository, from any window. The repository key (shared git dir) is found once, when git.rs discovers the repository, by the same `git_remote::repository` that `find` uses.
- **Diverged pull.** No new behaviour: git_remote's fast-forward-only pull fails with "…have diverged, so it can't fast-forward. Merge or rebase them from a terminal.", an error toast with no choices.
