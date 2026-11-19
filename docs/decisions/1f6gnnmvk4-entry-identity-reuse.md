# Entry identity on Linux carries the birth time (T694)

**Problem.** The project tree's Trash/undo operations identify an entry by
(device, inode). ext4 and xfs hand a freed inode straight to the next file
made, so a file another program deletes and re-creates while the Trash
confirmation is open looked like the same entry and was trashed.

**Choice.** `fe_project::fs_ops::EntryId` is a struct of `dev`, `ino` and
`born: Option<(i64, u32)>`, compared field by field, never folded into the
inode. `born` is the file system's birth time (statx `btime`, read through
`Metadata::created` or `statx`) on Linux, and `None` everywhere else (macOS,
Windows). With `born: None` on both sides two entries are the same when their
device and inode are, exactly as before.

**One identity.** fe-workbench's Trash, rename and undo checks take the id from
`fs_ops::entry_identity` (unix) so they compare with the ids `fs_ops` records
(`Placed::id`, `Trashed::id`). Each crate keeps its own type (kiln-core's
`path_identity`/`file_identity` are unchanged); fe-project does not depend on
kiln-core for it. In test builds an `EntryId` also compares equal to a bare
`(dev, ino)` pair, ignoring `born`, so older tests that build one from
`MetadataExt` still apply.

**No ctime fallback.** ctime changes when an entry is merely renamed (the Trash
move itself) or has its mode changed, so recorded ids would stop matching the
entries they describe.

## Known limits

- On a file system that reports no birth time, a replacement with a reused
  inode still matches, as before.
- A file deleted and re-created within one kernel clock tick (a few ms) gets
  the same birth time and still matches. A person-paced Trash confirmation
  does not hit this.
- Closing both needs the inode generation number (FS_IOC_GETVERSION on
  ext4/xfs/btrfs), which means opening each file: a separate follow-up card.
