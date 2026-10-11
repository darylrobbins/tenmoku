---
task_uid: "01m4m92sgh7d6bedwwsc9frj35"
task: "T340"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "cargo metadata --manifest-path fuzz/Cargo.toml --locked --format-version 1 succeeds (it fails on main 6e65173 because the lock is stale)"
    required: true
    manual: false
  - id: "C2"
    text: "Only fuzz/Cargo.lock changes (the task's own spec under docs/specs/ and its decision record excepted); every package already in it keeps its version, and the root Cargo.lock is unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "The gate's full nextest run passes"
    required: true
    manual: false
---

# fuzz/Cargo.lock: add fe-text's encoding dependencies, so cargo in fuzz/ stops rewriting it

fuzz/ is its own cargo workspace with its own Cargo.lock (fuzz/README.md). That lock predates fe-text's
encoding work (T249/T250), so it lacks chardetng, encoding_rs and their dependencies, and any cargo command
in fuzz/ rewrites it. On 2026-10-10 that left the primary checkout dirty and blocked T222's merge
(primary_dirty).

Regenerate fuzz/Cargo.lock against the current fe-text, fe-buffer and fe-syntax manifests, changing only
what cargo needs: no version bumps beyond what resolution requires (`cargo update --workspace` style, not
a blanket `cargo update`). Nothing else changes.
