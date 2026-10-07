---
task_uid: "01m49rd5krh1wmzc39txtzmqme"
task: "T150"
rev: 2
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "F14: the request file's disk copy is excluded from the project search before match accounting, with allocation still bounded. With more than 10,000 hits in that disk copy, every other file's references are reported up to the cap, and truncated is false when no eligible hit was dropped. Tests cover this with and without other matching files"
    required: true
    manual: false
  - id: "C2"
    text: "F6: in the request's own text, row counting and matching are bounded per step whatever the selected word's length, with cancel checks inside long matches and while rejected overlapping candidates are processed. A cancel mid-scan returns promptly, both with a very long selected identifier and with many overlapping rejected candidates (deterministic tests)"
    required: true
    manual: false
  - id: "C3"
    text: "F15: fe_project's search reads each file through open_file_under, so no symlink is followed at any component below the root. When a directory is swapped for a symlink to an outside tree during a references request, no hit or preview from the outside file is reported, including with verify_files = 0"
    required: true
    manual: false
  - id: "C4"
    text: "SearchOptions gains no field. File exclusion is a new fe-project item, re-exported from crates/fe-project/src/lib.rs, and existing callers compile unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "New tests live in crates/fe-nav/src/references_followup_tests.rs, declared from references.rs by a #[cfg(test)] #[path] mod line, and fail at base"
    required: true
    manual: false
---

# Follow-ups from review of T106

Findings of the review of T106 that don't block the merge (below merge.block_severity):

- F14 medium crates/fe-nav/src/references.rs:213: Clamping disk-copy compensation can let the skipped request file exhaust the search cap. With one unsaved hit and 30,000 disk hits, max_matches is 20,000: searching the disk copy first consumes every slot, so other files' references are omitted. Even without other files, the summary incorrectly reports truncated for just one eligible hit.
  fix: Exclude the request's disk copy before search match accounting while retaining bounded allocation. Add regressions with more than 10,000 disk-copy hits, both with and without other matching files, checking reference counts and truncation.
  instances: crates/fe-nav/src/references.rs:154, crates/fe-nav/src/references.rs:213

- F6 medium crates/fe-nav/src/references.rs:334: The row scan remains unbounded for long selected identifiers. An accepted word advances at by word.len(), so next can exceed SCAN_CHUNK arbitrarily and Rows::count scans that entire span without checking cancellation. Matching likewise searches a window enlarged by the unrestricted word length; rejected overlapping candidates repeatedly search it without an inner cancellation check. The new cancellation tests use only the short word helper and miss these cases.
  fix: Bound row counting and matching work independently of word length, with cancellation checkpoints during long matches and rejected-candidate processing. Add cancellation regressions using a long selected identifier and overlapping rejected candidates.
  instances: crates/fe-nav/src/references.rs:310, crates/fe-nav/src/references.rs:313, crates/fe-nav/src/references.rs:322, crates/fe-nav/src/references.rs:334, crates/fe-nav/src/references.rs:900

- F15 medium crates/fe-nav/src/references.rs:173: A sibling of F11 remains in the underlying project search: search_entry opens paths with O_NOFOLLOW only on the final component. A swapped intermediate directory can therefore redirect the initial search outside the project. The safe verification reopen does not remove the outside preview, which is emitted even when reopening fails. This predates the fixes and is a nonblocking follow-up.
  fix: Use open_file_under for project search file reads and extend the directory-swap regression to reject an outside-only preview sentinel, including when verification is disabled.
  instances: crates/fe-project/src/search.rs:550, crates/fe-nav/src/references.rs:173, crates/fe-nav/src/references.rs:498

## Lead notes

All findings checked on main 4037dd1. All three still hold: F14 (search_cap still adds the clamped disk-copy count, references.rs:152-155 and 212-214), F6 (Rows::count and the `seen` window still grow with word.len(), references.rs:310-337) and F15 (fe-project search_entry still opens with O_NOFOLLOW on the last component only, search.rs:548-551).

Scope is narrowed to the fix's files plus one new test file, crates/fe-nav/src/references_followup_tests.rs. It is declared from references.rs by `#[cfg(test)] #[path = "references_followup_tests.rs"] mod followup_tests;`, the same pattern as search.rs's search_followup_tests.rs. crates/fe-nav/tests/references.rs is out of scope, so put the extended directory-swap regression in the new file. fe-nav lib.rs, provider.rs and search.rs were dropped from scope because the fix doesn't need them.

F14 needs fe-project's search to skip one file before it counts matches (it can't today). Don't add a field to `SearchOptions`: struct literals without `..Default::default()` exist in crates/fe-nav/src/search.rs, crates/fe-workbench/src/tests/search_panel.rs and crates/fe-project/src/tests/search.rs, which are all out of scope. Add a new item instead (for example a `search_excluding` function or an extra argument type), and re-export it from crates/fe-project/src/lib.rs. For F15, use `open_file_under` (listing.rs) inside search_entry.

## Implementation notes

- F14: `fe_project::search_excluding` (new, re-exported) skips the walk entry at the excluded path before it's opened or counted against `max_matches`. It still counts in `files_searched` when walked, so `ReferencesSummary::files_searched` keeps its documented meaning (the request's file on disk included). The references search cap is now the rest of `MAX_HITS` plus one, and the disk-copy counter (`disk_copy_hits`, `count_whole_words`, `Utf16Reader`) is gone.
- F6: the request's text is scanned in one Knuth–Morris–Pratt pass (with a `str::find` skip to the word's first char when nothing is matched), counting rows as it goes. Building the table, each fallback and each byte step count towards `SCAN_CHUNK` steps between cancel checks, so a check comes at most about every two chunks of work whatever the word's length or overlap. Accepted hits match the old semantics: a whole occurrence that starts at or past the end of the previous hit.
- F15: `search_entry` opens every file with `open_file_under(canonical root, rel)`. That's one `openat` per path component (root included) per file instead of one `open`. A root that is itself a file is opened under its parent directory, as before.
- The directory-swap regression puts 100 files in the swapped directory and holds each state for 300 µs, so at base the search reliably reads through the symlink. With the old single file and a tight loop, base passed. The out-of-scope `tests/references.rs::files_are_never_read_through_a_directory_being_swapped` is flaky under load ("never caught the directory in place") at base too: 3 of 10 runs failed at base and 5 of 10 with this change, at load averages around 35 to 45.
