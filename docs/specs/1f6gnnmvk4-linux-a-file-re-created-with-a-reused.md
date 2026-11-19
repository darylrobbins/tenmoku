---
task_uid: "01m7qkp1bh94vc9p1f6gnnmvk4"
task: "T694"
rev: 6
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "On Linux, where the filesystem reports a birth time, an entry deleted and re-created at the same path with the same inode at least one kernel clock tick later no longer matches the identity taken before, so the Trash confirmation reports it as moved or replaced; the identity compares device, inode and birth time as separate fields (no folding into the inode); macOS and Windows behaviour unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "A regression test in crates/fe-project/tests/entry_identity_reuse.rs fails at base on Linux ext4 (asserting the inode was reused) and passes with the fix"
    required: true
    manual: false
  - id: "C3"
    text: "Where no birth time is known, the identity compares exactly as (dev, ino) did before, and the spec's Known limits (no birth time; re-creation within one clock tick) are recorded in docs/decisions/1f6gnnmvk4-entry-identity-reuse.md"
    required: true
    manual: false
---

# Linux: a file re-created with a reused inode passes the Trash confirmation's 'moved or replaced' check, so another program's new file is trashed silently (fe-project EntryId is (dev, ino) only) (Mac T1953)

Found by helium while fixing T1952 (helium T693). Project-tree file operations identify an entry by (device, inode) only: crates/fe-project/src/fs_ops.rs:156 (EntryId) and crates/fe-project/src/listing.rs:888. On Linux ext4/xfs, deleting a file and creating another in the same directory hands the freed inode straight back (helium measured 7340594 -> 7340594), so two different files look like the same entry.

Where a user can hit it: the Trash confirmation's "moved or replaced" check. If another program deletes and re-creates the file while the confirmation is open, the new file is sent to the Trash with no error. Restore is not affected on Linux (SYSTEM_TRASH_RESTORES is macOS-only, and APFS does not reuse inodes at once). Trash is recoverable, so the impact is low.

Fix: strengthen EntryId on Linux so a re-created file differs, e.g. add the birth time from statx (btime) where the filesystem reports it, falling back to ctime; keep (dev, ino) as the base on every OS. Write the choice in the decision file.

Regression test: a new test file that deletes and re-creates a file in a directory on a filesystem that reuses inodes (ext4; Linux /tmp is often tmpfs, which does not reuse, so place the directory under the target dir or $CARGO_TARGET_TMPDIR on the repo's filesystem and assert the inode was reused before checking, skipping with a message if it was not). It must fail at base on Linux ext4. Linux-only by nature.

## Helium filing note

- Lent from the Mac as y148 (production follow-up to T1952 / helium T693). No dependency.
- **Regression test name (fixed on the card):** `a_recreated_file_with_a_reused_inode_is_not_the_same_entry`, a
  `#[test] fn` in the new integration test crates/fe-project/tests/entry_identity_reuse.rs. Use exactly that name;
  it is unique, so the criteria file and the card both use it BARE (no `fe-project::` prefix).
- **It must compile at base:** an integration test that calls an API this card adds fails to build at base and
  finish fails as infra. Drive it through fe-project's public API as base has it (whatever the Trash
  confirmation's moved-or-replaced check uses: EntryId / the listing identity), and check by hand that the test
  compiles and FAILS on base before the first finish.
- **ext4, not tmpfs:** helium's /tmp is tmpfs (never reuses inodes), the repo disk is ext4. Put the test's dir on
  the repo's filesystem (e.g. `tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR"))` if that lands on ext4 under the
  gate, else a dir under `env!("CARGO_MANIFEST_DIR")`'s target), assert the inode really was reused before the
  identity check, and skip with a message only when it wasn't. Confirm the gate's regression run lands on ext4
  (a skip at base would make the regression check pass at base and fail the criterion).
- Record the identity choice in your decision file (statx btime, no ctime fallback: a rename or chmod changes ctime); keep (dev, ino)
  as the base on every OS, macOS/Windows behaviour unchanged.
- Builds and loops: CARGO_TARGET_DIR inside your worktree, never /tmp.
- Ask the lead before rgt finish if anything in the criteria can't be met as written.

## Decision after review round 2 (the user chose this, 2026-11-18)

- **F2, fix it:** the identity keeps device, inode and birth time as separately compared fields: a struct
  (for example `EntryId { dev, ino, born: Option<..> }`), never a fold into the inode half. Where no birth time
  is known (`born: None`, and on macOS/Windows) it compares exactly as (dev, ino) did. Prefer a struct per crate
  (fe-project and kiln-core) over a new fe-project -> kiln-core dependency; Cargo.toml/Cargo.lock are in scope
  only if you truly need a shared type. Fold F6/F7 (test-helper typing) into the same pass: tests should use the
  production identity helper where it exists at base, and compute it themselves only where base lacks it.
- **F1, narrow it:** C1 now covers filesystems that report a birth time, with the re-creation at least one
  kernel clock tick later.

## Known limits (what this card does NOT do)

- On a filesystem that reports no birth time, a replacement with a reused inode still matches, as before.
- A file deleted and re-created within one kernel clock tick (a few ms) gets the same birth time and still
  matches. A person-paced Trash confirmation does not hit this.
- Closing both needs the inode generation number (FS_IOC_GETVERSION on ext4/xfs/btrfs), which means opening
  each file: a separate follow-up card, not this one.
