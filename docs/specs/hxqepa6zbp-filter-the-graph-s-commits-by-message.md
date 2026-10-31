---
task_uid: "01m67vc711dgagyfhxqepa6zbp"
task: "T262"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-git test commit_filter::by_text: LogFilter { text } matches case-insensitively in the subject or body (git log --grep -i --all-match); by author name or email; a hex string of 4+ chars matches an id prefix"
    required: true
    manual: false
  - id: "C2"
    text: "commit_filter::paging: a filtered log pages as the unfiltered one does (log(filter, 0, 2) then (2, 100) equals (0, 100))"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test git_log_filter_tests::box: typing in the box (a TextInput, debounced) re-reads the list; Escape or the clear button restores it; no matches shows 'No commits match'"
    required: true
    manual: false
  - id: "C4"
    text: "git_log_filter_tests::graph: a filtered list draws a lane per remaining parent chain without panicking on missing parents (layout over the filtered rows)"
    required: true
    manual: false
  - id: "C5"
    text: "with vim on, '/' in the list focuses the box and Enter returns to the list; cargo test -p fe-git and -p fe-workbench pass; clippy clean"
    required: true
    manual: false
---

# Filter the Graph's commits by message, author or sha from a search box at its top, listing only the matches with their lanes (Mac T1225)

## Problem
My recommendation, not the user's list: VS Code's Graph and GitLens filter by message, author and sha. commit_log.rs `Repo::log` takes a scope only.

## Implementation notes

- fe-git: `LogFilter { text }` with `LogFilter::matches`, and `Repo::log_matching` /
  `Repo::log_from_matching` beside `log` / `log_from` (whose signatures stay, so their
  callers and tests are untouched). A commit matches, ignoring case (Unicode lower-casing),
  when every whitespace-separated word of the trimmed text is in its message (subject or
  body, as `git log -i -F --all-match` with a `--grep` per word), or the whole text is in
  the author's name or email, or the text is 4+ hex digits beginning its id. The matching is
  done in Rust over the `git log --topo-order` walk, not with `--grep`/`--author`: git ANDs
  `--author` with `--grep`, and runs with `LC_ALL=C`, which would fold ASCII only. `skip`
  and `limit` count matches, so paging a filtered log reads as the unfiltered one does.
  Cost: git lists the whole history for a non-empty filter and each commit is read until
  `skip + limit` match; a filter matching nothing in a synthetic 50k-commit repo takes
  ~0.46 s (release), against ~0.17 s for an unfiltered page.
- `crates/fe-git/src/lib.rs` (outside the scope) re-exports `LogFilter`: `commit_log` is a
  private module.
- Panel: a bordered `TextInput` at the top of the Graph view. Edits read the list again
  `FILTER_DELAY` (250 ms) after the last; Enter reads at once and focuses the list; Escape
  and the clear button (shown while the box has text) empty it and list every commit at once.
  Filtered reads reuse the list's snapshot tips. With vim on, `/` in the list focuses the box
  with its text selected; the panel drops its `vim` flag and ignores its list keys while
  the box has focus. The box's editing keys are the `text_input.*` ones bound in every
  `text_input` (backspace, word delete, clear, paste); the keymap is outside the scope, so
  its caret keys (left, right, home, end) aren't bound there.
- Graph: filtered, every page lays the whole graph out again over the rows listed, a
  commit's lanes running only to its parents that are listed (one in a later page joins once
  read); a parent that isn't listed ends the lane at the dot, so no lane is left open for
  a commit that never comes. Merge dots still follow the commit's real parents.
