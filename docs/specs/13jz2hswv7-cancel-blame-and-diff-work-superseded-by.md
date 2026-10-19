---
task_uid: "01m545a11atba5da13jz2hswv7"
task: "T135"
rev: 4
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench tests workbench::git_blame::git_blame_tests::save_as_round_trip_keeps_cached_blame and head_change_during_blame_revalidates pass: a Save As round trip keeps A's annotation, and a HEAD change during blame is revalidated"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench tests workbench::git_blame::git_blame_tests::superseded_diff_open_does_not_steal_focus and pane_focus_round_trip_drops_pending_requests pass: a superseded open opens nothing and keeps focus, and a pane-focus round trip drops pending requests; the activation generation counting every workspace ActiveChanged event (a workspace-driven A to B to A within one update) and the HEAD-change test capturing the old-HEAD blame while HEAD changes are separate follow-ups"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench passes with no existing test weakened; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Cancel blame and diff work superseded by Save As, HEAD changes, caret moves or pane focus round trips in the inline git blame (T127 review F2, F4)

## Problem

T127 (Mac T555) review round 3, findings F2 and F4 (codex, medium), deferred out of T127 by the lead so it can land
its quoted-path and CRLF fixes. Both are supersession races in `crates/fe-workbench/src/git_blame.rs`:

- **F2 (around line 426):** pending blame work survives when the current path has cached blame. With A's blame cached,
  Save As to B, then back to A before B's blame completes: the cache branch returns without cancelling B, whose
  completion replaces A's cache and removes A's annotation until the caret moves again. HEAD checks also drop a
  required validation whenever blame is pending, and never retry it afterwards.
- **F4 (around line 901):** the uncommitted-row branch of "open the commit's change" drops its Ask guard when it starts
  the asynchronous DiffHead. The final completion checks pane navigation and diff requests but ignores later blame
  requests, caret movement, buffer edits and Save As, so an obsolete request can still open a diff and steal focus.
  Focusing another pane and returning leaves the captured pane counters unchanged, which revives pending blame requests.

## Change

In `git_blame.rs`:
- Before accepting cached blame, cancel pending work whose file context differs. Keep a required HEAD validation
  pending until it can run, rather than dropping it while blame is pending.
- Carry the blame request's guard through the diff's final completion, so any later blame request, caret movement,
  edit or Save As supersedes it. Track pane activation with a generation counter that changes on every activation,
  so a pane-focus round trip invalidates the earlier requests.

## Tests

In the existing `crates/fe-workbench/src/git_blame_tests.rs` (not a new file), each failing at the base:
- `save_as_round_trip_keeps_cached_blame`: A cached; Save As B; back to A before B's blame completes; once B's work
  would have finished, A's annotation is still shown.
- `head_change_during_blame_revalidates`: HEAD moves while blame is pending; after it completes, the annotation
  reflects the new HEAD.
- `superseded_diff_open_does_not_steal_focus`: an uncommitted row's open is started, then a later blame request (and
  separately caret movement, an edit, a Save As) supersedes it; no diff opens and focus stays.
- `pane_focus_round_trip_drops_pending_requests`: focus another pane and return; the earlier pending request opens
  nothing.
