---
task_uid: "01m79d3xgefnxgbwajprp5vdct"
task: "T367"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::git_sync_ui and the title_bar tests pass (nextest -p fe-workbench), including a test that a long branch name ellipsizes while the counts stay. Known limit: this test does not yet inspect the rendered text or compare the count bounds with the full measured count-text width and the chevron bounds at both window sizes; that is follow-up T370"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes"
    required: true
    manual: false
---

# Title bar branch widget: ahead/behind counts as their own non-shrinking element, tooltip for no upstream (Mac T1766)

# Title bar branch widget: ahead/behind counts stay visible beside the branch name

T1747 (landed e05c4f571) already puts `↑N ↓M` in the title bar branch label (git_branch.rs `branch_label`, git.rs `sync_counts`, title_bar.rs `widget`). Why a user may not see them:
1. The label is ONE ghost Button string capped at BRANCH_MAX_WIDTH 200 px with an end ellipsis (title_bar.rs `widget`), so on a long branch name or with `*+!` marks the counts, last in the string, are what get cut.
2. Nothing shows with no upstream (a never-pushed branch) and at 0/0; no hint says why.
3. Counts are as of the last fetch (no network); nothing fetches by itself, so `↓` stays 0 until Fetch/Pull is pressed.
4. A running build older than e05c4f571 has none of it.

## Change
- Draw the counts as their own non-shrinking child of the widget (after the name, before the chevron): name+marks shrink and ellipsize, the counts never do. `State` gets `sync: Option<SharedString>` (the `↑2 ↓1` text) beside `branch`; `branch_label` stops appending it. Accessible name still includes the counts.
- No upstream: nothing in the label; the tooltip reads "No upstream: Push publishes this branch" (new string).
- Tooltip with an upstream names it and the counts ("2 to push, 1 to pull from origin/main, as of the last fetch").
- No auto-fetch here (separate policy call).

## Pins
tests::git_sync_ui (the_branch_widget_shows_ahead_and_behind_counts etc.) and title_bar tests must pass; adapt only for the label/child split. Gallery/title-bar goldens: the branch widget golden only moves if a test repo has an upstream with counts; check `rg title-branch` goldens and regenerate if so.
Platform: linux-ok.
