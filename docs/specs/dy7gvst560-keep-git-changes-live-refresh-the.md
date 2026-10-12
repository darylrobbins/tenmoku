---
task_uid: "01m4p1wp08wz1yj5dy7gvst560"
task: "T4"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "crates/fe-workbench/src/git_live_tests.rs: after a file under the root is modified and a watcher batch naming it is applied, the Changes panel lists it after the 300 ms debounce, and 50 batches applied within the debounce run status at most twice"
    required: true
    manual: false
  - id: "C2"
    text: "Opening a modified tracked file gives its editor the HEAD text as diff base (row_changes marks the changed row), an untracked file an empty base (all rows Added), and a gitignored file or one outside the repository none, before any watcher batch"
    required: true
    manual: false
  - id: "C3"
    text: "After git commit -am in the temp repo and a batch naming .git/index, the open editor's base is the new HEAD text and its row_changes is empty"
    required: true
    manual: false
  - id: "C4"
    text: "A batch arriving while a status runs causes exactly one more run, and a result for a root replaced by opening another folder is never shown"
    required: true
    manual: false
---

# Keep git changes live: refresh the Changes panel after file and .git changes, and give each open editor its HEAD text as its diff base (Mac T377)

Filed on helium from Mac T377 (~/coordination/helium-cards/T377.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-a.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The user asked on 2026-10-11 for a list of changed files and "subtle highlighting of the changed rows in
files". After the cards this depends on, the Changes panel only refreshes when shown or on `git.refresh`,
and no editor is given a diff base, so no rows highlight. Changes come from three places: edits in the
editor (already live: `EditorModel::row_changes` diffs the unsaved text), saves and other programs writing
files, and git itself moving HEAD or the index (a commit, checkout, stash, `git add`). The project
panel's watcher already forwards every batch, `.git/` paths included, as `PanelEvent::FsChanged`
(crates/fe-workbench/src/project_panel.rs `apply_fs_changes`), which `Workbench::follow_root`
(workbench.rs ~line 396) passes on to quick-open and nav.

## Change

Add `crates/fe-workbench/src/git.rs`, a `GitState` the workbench owns per root, and call it from
workbench.rs in a few places:

- **Refresh trigger:** `follow_root`'s `FsChanged` arm hands each batch to `GitState`. Any batch
  (a path anywhere under the root, `.git/` included, or `FsChange::Rescan`) schedules a refresh 300 ms
  after the last batch (debounce). One status runs at a time in the background; batches during a run
  cause exactly one more run after it. Results for a root that's no longer open are dropped.
- **Each refresh:** runs `Repo::status` and `Repo::head_id`, pushes the statuses to the Changes panel
  with `GitPanel::set_statuses`, and, when HEAD's id changed or a path's status changed, reloads the diff
  base of each affected open editor.
- **Diff bases:** for each open editor with a path (`Workbench::open_editors`), set
  `model_mut().set_diff_base(..)` and notify it: `Repo::head_text` when the file is in HEAD; `""` when
  it's `Untracked` or `Added` (every row shows as added); `None` when it's outside the repository,
  ignored, or binary. Read HEAD texts in the background, never on the main thread. A newly opened editor
  gets its base when the workbench starts observing it (`Workbench::track_editors`, workbench.rs
  ~line 859), without waiting for a refresh.
- Saving needs nothing extra: the editor's highlighting already follows its text, and the save's watcher
  event refreshes the panel.
- Known limit, documented in git.rs: when the repository's `.git` is outside the project root, its
  changes aren't watched, so a commit shows only after the next change under the root or a
  `git.refresh`.

Scope also covers crates/fe-workbench/src/git_panel.rs for small changes it needs and
crates/fe-workbench/src/lib.rs for the module. Tests in `crates/fe-workbench/src/git_live_tests.rs`
(included by `#[path]` from git.rs), driving batches through `ProjectPanel::apply_fs_changes` as the
existing watcher tests do, on temp repositories built with the `git` CLI.

## Out of scope

Polling, watching a `.git` outside the root, the gutter painting itself, staging.

## Implementation notes

- `GitState` (crates/fe-workbench/src/git.rs) is an entity the workbench owns per root. Its runs are
  serialized: a refresh (status, HEAD id, and the HEAD texts of affected editors' files) or a light job
  giving newly tracked editors their bases from the last refresh's statuses. A refresh asked for while
  either runs runs once after it. Before the first refresh lands, a newly tracked editor starts one at
  once (no debounce).
- On `set_root` the old state is `close()`d, cancelling its runs, so its results land nowhere even if
  something still holds it.
- Editor paths are resolved through symlinks (in the background) before they're matched against the
  working tree, so a root opened through a symlink or `..` still gets bases; files in `.git` get none.
- Carried T3 findings fixed in git_panel.rs: the root is always located through canonical paths (F1),
  and a list given by `set_statuses` no longer stops the panel's first read, keeps a location found by
  an older run, and is listed again once the root is located (F2). Regression tests are in
  git_live_tests.rs.
- Review round 1: a refresh that lands while a batch's debounce is still running leaves the rerun to
  that debounce (one more run, at the latest batch's deadline). A newly opened file that is neither in
  HEAD nor in the cached statuses starts a fresh status run rather than trusting them; untracked and
  added files that are binary get no base; paths resolve through their nearest existing ancestor, so
  files in deleted directories under an aliased root still match; a HEAD text that can't be read leaves
  the editor's base as it was and is retried at the next refresh. `fe-editor`'s `diff_rows` treats an
  empty base as having no rows, so every row of a new file is Added with or without a final line break
  (scope widened to crates/fe-editor/src/row_changes.rs and its tests by respec).
- Review round 2: `GitState` is now the only status runner. The Changes panel's first-draw read and
  `git.refresh` ask it to refresh rather than running their own. Each run takes its sequence number
  from the panel as it starts (`GitPanel::begin`) and lands through `GitPanel::landed`, so an older
  run never replaces a newer list. This replaces the spec's `set_statuses` push, which gave an older
  run a newer number; `set_statuses` stays for lists given from outside. A newly opened file not in
  HEAD always gets a fresh status run, because cached statuses can't tell a since-ignored file from
  an untracked one. Path resolution follows dangling symlinks (bounded to 40) before it climbs to
  the nearest existing ancestor.
- Review round 3: a refresh records the open editors and paths as its status read starts. Only those
  are rebased against it; an editor opened during the read goes through the HEAD-or-fresh-status path,
  since the status may be older than its file.
