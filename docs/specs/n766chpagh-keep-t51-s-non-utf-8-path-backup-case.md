---
task_uid: "01m4rxfjcw6jres1n766chpagh"
task: "T57"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "write_then_read_round_trips_text_and_meta round-trips a non-UTF-8 path through BackupMeta and Buffer::from_backup on every unix target. On Apple targets (cfg(target_vendor = \"apple\")) a creation failure with EILSEQ (92) builds the case without a disk file instead of failing; no other errno, and no other target, takes that fallback"
    required: true
    manual: false
  - id: "C2"
    text: "On Linux the test still creates the non-UTF-8-named file and asserts it could; on every non-Apple target any creation failure panics; no existing assertion is removed or weakened. Other unix targets (BSDs, tvOS-like variants aside from Apple's, AIX, QNX, Haiku) need no special errno handling"
    required: true
    manual: false
  - id: "C3"
    text: "Test-only: no production code or Cargo.toml changes; fe-buffer's tests pass under nextest"
    required: true
    manual: false
---

# Keep T51's non-UTF-8 path backup case portable: macOS APFS refuses the file name, so round-trip the path without a disk file there (Mac T508 fix)

Portability fix for T51 (Mac T508, BackupStore), reported by the Mac lead. On macOS, `write_then_read_round_trips_text_and_meta` in crates/fe-buffer/tests/backup.rs fails every time (2 of 2). Its `#[cfg(unix)]` case creates a file named `not-utf8-\xFF.txt`, and APFS refuses non-UTF-8 names (os error 92, "Illegal byte sequence", EILSEQ). Linux filesystems accept them. Because of this, Mac T508 was left out of Mac host stack 4.

What the case is for: a backup of a buffer whose path isn't UTF-8 must round-trip the path bytes through BackupMeta (path_bytes / path_from_bytes in crates/fe-buffer/src/backup.rs) and restore with Buffer::from_backup.

Fix (test-only, crates/fe-buffer/tests/backup.rs):
- Keep the non-UTF-8 path case on every unix, and don't weaken any assertion about it.
- Create the file on disk only where the filesystem accepts the name. If creating it fails with EILSEQ (`raw_os_error() == Some(libc::EILSEQ)`, or match the io::Error some other way that doesn't need a new dependency), build the buffer for that path without a disk file. The "gone" case already does this: a buffer with the path whose file isn't on disk. Then still run the meta and text round-trip and the from_backup checks for the path bytes.
- On Linux (`cfg(target_os = "linux")`), creating the file must succeed: assert it, so a regression there can't slip into the skip path.
- If libc isn't already a dev-dependency of fe-buffer, use the numeric EILSEQ per platform via cfg (92 on macOS, 84 on Linux) rather than adding a dependency, or compare `io::Error::kind()` if it maps EILSEQ stably. Don't change Cargo.toml.
- Keep the final count assertions (`store.ids().len()`, the read_dir count) right in both branches.

No production code changes.
