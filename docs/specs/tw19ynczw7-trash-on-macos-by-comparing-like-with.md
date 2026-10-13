---
task_uid: "01m4snfpazejwnmrtw19ynczw7"
task: "T91"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "fs_ops::tests::trash_identity_compares_like_with_like passes on Linux: with the Mac's values (URL volume id 6571367, st_dev 16777231, matching file ids) names_the_entry accepts, and refuses mismatched file ids, differing URL volume ids, differing st_devs, and a pair that matches only by comparing a URL volume id with an st_dev"
    required: true
    manual: false
  - id: "C2"
    text: "macOS trash_target decides through names_the_entry, using the folder's own file reference URL and listing::identity of the folder handle, both checked before Point::Checked; fs_id and every comparison of a URL volume id with st_dev or f_fsid are gone"
    required: true
    manual: false
  - id: "C3"
    text: "CC_aarch64_apple_darwin=true AR_aarch64_apple_darwin=true CARGO_TARGET_DIR=\u003cscratch dir\u003e cargo clippy -p fe-project --all-targets --target aarch64-apple-darwin -- -D warnings passes, with its output in the decision file"
    required: true
    manual: false
  - id: "C4"
    text: "The Linux trash and T64's hardening are unchanged: every existing fe-project test passes unchanged in the gate, and Cargo.toml, Cargo.lock and deny.toml are untouched"
    required: true
    manual: false
---

# Trash on macOS by comparing like with like: the entry's reference URL volume with its folder's, st_dev with st_dev (Mac T510 fix, T59/T64)

## Problem

T59 (Mac T510, `ce437eb`) and its follow-up T64 (`4b8c9b8`) were dropped from Mac stack 6 because four
fe-project trash tests fail on macOS. The Mac lead (2026-10-13, coordination note `helium-zl`):

> trash_target compares the file reference URL's volume id with st_dev/f_fsid, and on this Mac they
> differ (URL volume id 6571367, st_dev 16777231; the file id matches). Please fix it so it works on
> macOS (compare against the URL's own resource values, or check volume identity another way), with a
> test that runs on both.

The macOS `trash_target` (`crates/fe-project/src/fs_ops.rs`, ~line 343) takes the volume id from the
entry's file reference URL (`reference_ids`, `file:///.file/id=<volume>.<file>`) and requires it to be
one of `[dev as i32, fs_id(&target.parent)?]`: the entry's `st_dev` (from `listing::stat_at`,
`ATTR_CMN_DEVID`) or the folder's `f_fsid.val[0]` (`fs_id`, ~line 372). That compares a URL volume id
with a stat device id. They are different numbering schemes, and on the Mac's APFS volume they
differ, so every trash is refused with "the path no longer leads to the entry found under the project
root". The file id check (`file == ino`) is right: it compares the URL's file id with the entry's
inode.

The four tests that fail on macOS, all through this check:
- `trash_moves_a_file_and_a_folder_to_the_system_trash` (tests/fs_ops.rs)
- `broad_folders_and_selections_need_few_open_files` (tests/fs_ops.rs, its child trashes 300 files)
- `fs_ops::tests::trash_never_follows_a_parent_swapped_for_a_symlink`
- `fs_ops::tests::trash_never_follows_a_root_swapped_for_a_symlink` (their `Swap::AfterCheck` case
  unwraps the result)

## Change

Compare like with like: URL ids with URL ids, stat ids with stat ids, never one against the other.

1. **A pure decision, compiled on every target under test.** In `crates/fe-project/src/fs_ops.rs`, beside
   `reference_ids` and gated the same way (`#[cfg(any(target_os = "macos", test))]`), add:
   - `struct Seen { url: (i32, u64), stat: (u64, u64) }`: one entry as its file reference URL names it
     (volume id, file id, from `reference_ids`) and as its handle names it (`st_dev`, inode).
   - `fn names_the_entry(entry: Seen, folder: Seen) -> bool`, true only when all of these hold:
     `entry.url.1 == entry.stat.1`, `folder.url.1 == folder.stat.1`, `entry.url.0 == folder.url.0`
     (both URL volume ids) and `entry.stat.0 == folder.stat.0` (both `st_dev`s). No URL volume id is
     ever compared with a `st_dev` or `f_fsid`.

2. **macOS `trash_target`.** Keep the entry's reference URL exactly as now (it is what
   `trashItemAtURL` is given). Also make a file reference URL for the folder's path
   (`target.path.parent()`), the same way (`fileURLWithPath` then `fileReferenceURL`, then
   `reference_ids` on its `absoluteString`). The entry's `Seen` is its URL's ids and `stat_at(...).id`;
   the folder's is its URL's ids and `listing::identity(&target.parent)`. Refuse with the existing
   `invalid("the path no longer leads to the entry found under the project root")` unless
   `names_the_entry` holds (a missing reference or unparsable URL refuses too, as now). Both URLs are
   made and checked before `reach(Point::Checked)`, so the swap tests keep their meaning: a folder
   swapped for a symlink before the check makes the folder's URL name another directory (its file id
   no longer matches the held handle), and is refused with `InvalidInput`. Remove `fs_id` and its
   `fstatfs` call, which nothing else uses.

3. **Tests.**
   - In `fs_ops.rs`'s existing `#[cfg(test)] mod tests` (beside `reference_ids_read_a_file_reference_url`),
     a platform-independent unit test `trash_identity_compares_like_with_like`. With the Mac's values
     (entry URL `(6571367, 2773272)`, entry stat `(16777231, 2773272)`, folder URL `(6571367, F)`,
     folder stat `(16777231, F)`) it accepts. It refuses when the entry's URL file id differs from its
     inode, when the folder's does, when the two URL volume ids differ, and when the two `st_dev`s
     differ. It also refuses a case that only a cross comparison would accept: entry URL volume id
     equal to the `st_dev` (16777231) while the folder's URL volume id is 6571367.
   - In the same module, a `#[cfg(target_os = "macos")]` test
     `a_reference_url_and_its_folder_name_one_volume`: for a real file in a temp folder, the `Seen`
     values built as `trash_target` builds them satisfy `names_the_entry`. It runs only on the Mac
     gate; helium checks it compiles (below).
   - The four tests above keep their assertions unchanged; they are the macOS proof, run when the Mac
     lead re-batches T59, T64 and this fix.

## Not in scope

- The Linux trash (`freedesktop`), and T64's hardening: concurrent replacement (`Point::Checked`,
  the swap tests), the `/proc/self/fdinfo` and `statx` mount identity and the `Unsupported` error for
  unidentified mounts all stay as they are.
- No new dependency and no new `objc2-foundation` feature: `NSURL::fileURLWithPath`,
  `fileReferenceURL` and `absoluteString` are already in use. No `Cargo.toml`, `Cargo.lock` or
  `deny.toml` change.

## Verifying on helium

helium can't run macOS code. Type-check the macOS targets with a check-only cross clippy (nothing
links or runs), in a target dir of its own:

```
CC_aarch64_apple_darwin=true AR_aarch64_apple_darwin=true CARGO_TARGET_DIR=<scratch dir> \
  cargo clippy -p fe-project --all-targets --target aarch64-apple-darwin -- -D warnings
```

Record its output in the decision file. It passes at `251b053`.

## Why chore, not bugfix

This is a production fix, but its failure is macOS-only. Regatta's regression check runs on helium
(Linux) at the base with only test files overlaid. The only test that fails here without the fix is
the unit test of `names_the_entry`, which doesn't exist at the base, so it can't compile there (T27
and T64 both got an infra exception from a base that couldn't build or select the test). The macOS
tests pass on Linux at the base (their code is `cfg`-gated). No bugfix shape gets `red_confirmed`
here without a person's approval, so the regression proof is the four macOS tests on the Mac gate.
