---
task_uid: "01m476fvgs7tx6q6tw71tn9v5a"
task: "T88"
rev: 7
type: "bugfix"
complexity: "m"
criteria: []
---

# Close fe-project's known limits from the T61 review: watcher freshness races and git ignore edge cases

# Close fe-project's known limits from the T61 review: watcher freshness races and git ignore edge cases

T61 merged with these review findings accepted as known limits (its spec's Decided section). Fix each with a deterministic regression test.

## F5 (high) `crates/fe-project/src/tree.rs:397`

Ancestor freshness still misses some retry markers. An overlapping child listing can finish after a newer stored child listing, be rejected, and mark only the child as owing a refresh. A parent snapshot started after the stored child listing, captured while the child was temporarily away, is then accepted and removes the restored child and its retry marker. Root recovery has the same gap: it advances every descendant directory's applied_seq but excludes the root, allowing root snapshots run before the recovery fill to erase those marked directories.

**Fix:** Propagate every retry marker into ancestor freshness, including rejected-result retries and the root-recovery marker on the root itself, while retaining each affected directory's refresh obligation. Add deterministic regressions for both interleavings.

## F6 (medium) `crates/fe-project/src/tree.rs:480`

A batch containing both Path(root) and Rescan discards the root-path evidence by passing root_reported=false. After deleting an ancestor containing the project, the root listing reports Lost::Root and preserves all loaded rows instead of clearing the deleted project. Repeated rescans and root listings continue retaining these ghost rows.

**Fix:** Determine whether the root path is reported before selecting the rescan branch and pass that information into the full rescan. Test ancestor deletion with a batch containing both Path(root) and Rescan.

## F7 (medium) `crates/fe-project/src/listing.rs:337`

The custom ignore matcher crosses repository boundaries for both .gitignore and info/exclude rules. Unlike WalkBuilder, it keeps consulting ancestors above the nearest repository even with require_git(false). For example, an ancestor .gitignore containing *.txt incorrectly hides keep.txt inside a separate repository whose own ignore files do not ignore it; listings and apply share this behavior.

**Fix:** Track repository boundaries and stop consulting ancestor Git ignore and exclude rules beyond the nearest repository, while continuing applicable .ignore rules. Extend walker-equivalence tests with ancestor rules and nested repositories.

## F8 (medium) `crates/fe-project/src/listing.rs:306`

Git exclude loading assumes .git is a directory. In a linked worktree, .git is a regular gitfile, so opening it as a directory fails and the common repository's info/exclude is never loaded. Files ignored there consequently appear in listings and apply, unlike the configured ignore walker.

**Fix:** Support gitfile and commondir resolution with the existing safe file-opening constraints, and add a linked-worktree walker-equivalence regression.


## F1 (medium) `crates/fe-project/src/listing.rs:129` (raised on T61 rev 3)

A Listing captures parent_ignored when requested but timestamps freshness when run starts. With show_ignored enabled, request an unloaded directory's listing, change its ignore status through Rescan, then run and fill the pending listing: it is accepted as fresh although its inherited ignore flag is obsolete. Children of a now-ignored directory can remain unflagged, or children of a now-unignored directory remain flagged, with listing(id) returning None indefinitely. This is outside the accepted F5 interleavings and concerns a directory not previously loaded.

**Fix:** Carry and validate the captured ignore context when filling, rejecting and requesting a fresh listing if it changed, or otherwise ensure run uses current inherited ignore state; add deterministic request/rescan/run/fill tests for both transitions.
