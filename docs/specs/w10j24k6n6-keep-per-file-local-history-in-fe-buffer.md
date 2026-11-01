---
task_uid: "01m6c61x9sasy00nw10j24k6n6"
task: "T394"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-buffer tests records_and_lists_newest_first, identical_text_is_not_recorded_twice and oversize_text_is_not_recorded pass (crates/fe-buffer/tests/local_history.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-buffer tests count_and_age_limits_drop_oldest, prune_fits_the_total_limit_oldest_first_across_files and prune_removes_crash_temp_files pass"
    required: true
    manual: false
  - id: "C3"
    text: "fe-buffer tests two_paths_with_one_hash_key_stay_apart, two_stores_on_one_dir_record_concurrently and store_dirs_are_private (mode 0700) pass"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-buffer passes and crates/fe-buffer/Cargo.toml is unchanged"
    required: true
    manual: false
---

# Keep per-file local history in fe-buffer: a store of past texts by path, newest first, bounded by count, age and total size (Mac T1275)

## Problem

User direction 2026-11-01: local history, as JetBrains has it: per-file snapshots on save and on external
change, kept under the user state dir (`~/.local/state/tenmoku`, `fe_config::paths::state_dir()`), browsable,
diffable and restorable, bounded in size and age. Nothing records past versions today: hot exit's
`fe_buffer::backup::BackupStore` (`crates/fe-buffer/src/backup.rs`) keeps only the newest unsaved text per open
buffer, and git covers only committed text. This card is the GPUI-free store; recording (next card) and the
view (the one after) build on it.

## Change

New `crates/fe-buffer/src/local_history.rs`, exported from `crates/fe-buffer/src/lib.rs`, modelled on
`BackupStore` (atomic temp-and-rename writes, directory mode 0700 on Unix since the store holds file contents,
a lock file for pruning):

- `LocalHistory::new(dir)`, `LocalHistory::with_limits(dir, Limits)`. `Limits` defaults: at most 100 entries per
  file, entries older than 30 days dropped, a text over 4 MB never recorded, the whole store at most 256 MB.
- Layout: `<dir>/<key>/` per file, `<key>` the 16-hex FNV-1a 64 of the absolute path's bytes (written inline:
  stable across Rust versions, unlike `DefaultHasher`); `<dir>/<key>/path` holds the path, and a key whose
  `path` names another file takes the next suffix (`<key>-1`). Each entry is `<unix_millis>-<n>.<reason>`
  holding the text's bytes; `<reason>` is `saved` or `external`.
- `record(&self, path, text: &[u8], reason: Reason, now: SystemTime) -> io::Result<Option<EntryId>>`: writes an
  entry unless the text is over the size limit or equals the newest entry's bytes (`None` then), then drops that
  file's entries past the count and age limits.
- `entries(&self, path) -> io::Result<Vec<Entry>>`, newest first: `id`, `time`, `reason`, `len`. A file never
  recorded gives an empty list.
- `read(&self, path, id) -> io::Result<Vec<u8>>`.
- `prune(&self, now) -> io::Result<Pruned>`: applies the age limit everywhere, then removes oldest entries
  across all files until the store fits the total limit; empty file directories go. Leftover temp files from a
  crash are removed.
- Two stores on one directory (two windows, two processes) never corrupt each other: names are unique, writes
  atomic, pruning holds the lock.

Tests in new `crates/fe-buffer/tests/local_history.rs` (top-level fns, `tempfile` dirs, explicit `now`).

## Implementation notes

- `record` holds the lock exclusively, not shared as `BackupStore`'s writes do: it makes `<n>`, the key suffixes
  and "equals the newest entry" exact across stores, at the cost of serialising a few small writes.
- `<n>` counts up per file and orders its entries, so "newest" means recorded last even if the clock moved back;
  with `<unix_millis>` alone, a clock moved back would have each new save dropped by the count limit. Age and
  `prune`'s order across files still go by time.
- `record` applies the age limit before comparing with the newest entry, so an expired newest entry can't stand
  in for the current text and leave the file with no history after a prune.
- Slots are found by listing the store, because a prune can remove `<key>` and leave `<key>-1`.
- `two_paths_with_one_hash_key_stay_apart` uses a real FNV-1a 64 collision, so there is no test-only key seam.
- Types other than `LocalHistory` are reached as `fe_buffer::local_history::*`: `Entry`, `Reason` and `Limits` are
  too general for the crate root.
