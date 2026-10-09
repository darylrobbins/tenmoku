---
task_uid: "01m4fjzm0v6s13wfnfy5ta94rh"
task: "T268"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T241

Findings of the review of T241 that don't block the merge (below merge.block_severity):

- F5 medium crates/fe-fileview/src/stamp.rs:103: The new Windows identity query propagates every FileIdInfo failure, causing FileView::open to reject otherwise readable files on FAT and some CIFS/SMB filesystems. These filesystems can reject this information class while GetFileInformationByHandle still provides an identity, as documented in [Gnulib's Windows implementation](https://github.com/coreutils/gnulib/blob/master/lib/stat-w32.c). This introduces a filesystem compatibility regression.
  fix: Fall back to GetFileInformationByHandle's volume serial number and 64-bit file index when the extended identity class is unsupported, preserving other errors and using the same identity representation for open and check; add deterministic fallback coverage.
  instances: crates/fe-fileview/src/stamp.rs:103
