---
task_uid: "01m54dpz3yxz3gjx6xh10brjks"
task: "T753"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-project test duplicate_keeps_extended_attributes (crates/fe-project/tests/duplicate_xattrs.rs, macOS) passes: after xattr -w com.example.fastedit on a.txt, d and d/b.txt, fe_project::duplicate of a.txt and of d gives a copy.txt, d copy and d copy/b.txt each with the original's value, read with xattr -p; it fails at the base (No such xattr)"
    required: true
    manual: false
  - id: "C2"
    text: "Attributes are copied with libc::fcopyfile and COPYFILE_XATTR between the handles copy_new already holds, before a file's permissions and time are set and before a folder's children are copied; symlinks are copied as today; a failed attribute copy keeps the copy and logs a warning naming the entry; nothing changes on Linux"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-project passes with no existing test edited (tests/fs_ops.rs duplicate tests and fs_ops_race_tests among them); cargo clippy -p fe-project --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Keep a file's and a folder's extended attributes, such as Finder tags, when Duplicate copies them in the project tree, as the Finder does

## Problem

Duplicate in the project tree (`fe_project::duplicate`, crates/fe-project/src/fs_ops.rs, `copy_new`)
copies a file's bytes with `io::copy`, then its permission bits and modification time, and makes a
folder with `mkdir_at` then fills it. Extended attributes are never copied, so the copy loses the
original's Finder tags and colour label (`com.apple.metadata:_kMDItemUserTags`), Finder comments,
`com.apple.quarantine` and any app's own attributes. The Finder's Duplicate keeps them, and so does
VS Code's (libuv's `uv_fs_copyfile` uses `copyfile(3)` on macOS).

Reproduced at Mac main 5837b415: with `xattr -w com.example.fastedit <value>` on a file `a.txt`, a
folder `d` and `d/b.txt`, `duplicate(root, a.txt)` and `duplicate(root, d)` give `a copy.txt`,
`d copy` and `d copy/b.txt` with no such attribute (`xattr -p`: "No such xattr").

## Change

In crates/fe-project/src/fs_ops.rs, on macOS, copy each copied entry's extended attributes with
`libc::fcopyfile(source_fd, copy_fd, ptr::null_mut(), libc::COPYFILE_XATTR)` through the handles
`copy_new` already holds (no path lookups, so nothing is followed):

- **File:** after `io::copy` and before `set_permissions`/`set_modified`, from `source` to `copy`.
- **Folder:** from the source folder's handle (`open_dir_at(from, name, Access::Read)`) to the
  copy's (`own_dir`'s, opened to read) before its children are copied. Checked by hand at main:
  `fcopyfile` with `COPYFILE_XATTR` copies a folder's attributes between two `O_RDONLY` folder fds.
- **Symlink:** unchanged (a link's own attributes are left out, as today).

A failure to copy attributes doesn't fail the duplicate: the copy is kept and the error is logged
with `log::warn!` naming the entry. On Linux nothing changes (no xattr copying there; noted in
`duplicate`'s doc as a known limit). Update `duplicate`'s doc: "Contents, permission bits, extended
attributes (macOS) and a file's modification time are copied".

The regression test is a new integration test, crates/fe-project/tests/duplicate_xattrs.rs
(`#![cfg(target_os = "macos")]`, top-level fn), using only the base API: it sets an attribute with
`/usr/bin/xattr -w` on a file, a folder and a file in it, duplicates the file and the folder, and
reads each copy's attribute with `xattr -p`.
