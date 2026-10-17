---
task_uid: "01m54hm9dg9094mvs5nzbhp0hj"
task: "T758"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-editor test a_save_keeps_the_files_extended_attributes (new crates/fe-editor/tests/save_xattrs.rs, macOS, top-level fn) passes: after xattr -w com.example.fastedit-test on a file, EditorModel::open, a whole-text edit and save(), the file has the new text and xattr -p gives the value; it fails at the base (No such xattr)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test search_panel::search_replace::tests::replacing_keeps_a_files_extended_attributes (appended to crates/fe-workbench/src/search_replace_tests.rs, macOS) passes: Replace All of foo with bar over an unopened a.rs carrying the attribute leaves 'bar a' on disk with the attribute's value; it fails at the base (No such xattr)"
    required: true
    manual: false
  - id: "C3"
    text: "SaveJob::run_with (when a file exists at the target) and write_beneath copy the original's attributes onto the new file with fcopyfile and COPYFILE_XATTR from a read-only handle on the original (write_beneath through open_in) before the rename; fcopyfile is declared in disk.rs by an unsafe extern \"C\" block, so there is no Cargo or Cargo.lock change; a failed copy logs log::warn! naming the file and the save still succeeds; create_in_place, new files and Linux are unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-editor and cargo nextest run -p fe-workbench -E 'test(/search_replace/)' pass with no existing test edited; cargo clippy -p fe-editor -p fe-workbench --all-targets -- -D warnings is clean; git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Keep a file's extended attributes, such as Finder tags and quarantine, when a save or project-wide Replace All writes it

## Problem

A save drops the file's extended attributes: Finder tags and colour labels
(`com.apple.metadata:_kMDItemUserTags`), Finder comments, `com.apple.quarantine`,
`com.apple.FinderInfo` and any app's own attributes. Both atomic writers make a new file beside the
target and rename it over the original, and copy only the permission bits onto it:

- `SaveJob::run_with` in crates/fe-editor/src/disk.rs (cmd-s, :w, close, auto-save, Save As over an
  existing file): `tempfile::Builder::tempfile_in`, `set_permissions`, `persist`.
- `write_beneath` in crates/fe-workbench/src/search_replace.rs (project-wide Replace All for files that
  aren't open): `open_at(... O_CREAT | O_EXCL ...)`, `set_permissions`, `renameat`.

TextEdit, BBEdit and VS Code keep a file's attributes across a save. T753 fixed the same loss for
Duplicate in the project tree.

Reproduced at Mac main b7bd02cf (scratch worktree, own target dir):

- `xattr -w com.example.fastedit-test kept tagged.txt`, `EditorModel::open`, replace the text, `save()`:
  the new text is on disk and `xattr -p com.example.fastedit-test tagged.txt` says "No such xattr".
- A project with `a.rs` = "foo a", the attribute set on it, search `foo`, Replace All with `bar`:
  `a.rs` reads "bar a" and the attribute is gone ("No such xattr").

With the change below prototyped, both pass.

## Change

On macOS, copy the original's extended attributes onto the new file with `fcopyfile(3)` and
`COPYFILE_XATTR` (attributes only: no data, no ACL, no mode) before the new file is renamed over it.

- **crates/fe-editor/src/disk.rs**, `SaveJob::run_with`: when `permissions` is `Some` (a file exists at
  `target`), open `target` read-only (`std::fs::File::open`; `target` is already resolved and checked
  regular) and call `fcopyfile(original_fd, temp_fd, null, COPYFILE_XATTR)` right after
  `builder.tempfile_in(dir)?`, before `set_permissions` and the write. fe-editor has no libc dependency
  and Cargo.lock is held by T693/T401/T410, so declare it in this file, as
  crates/fe-editor/src/tests/batch_structure.rs declares `clock_gettime`:
  `#[cfg(target_os = "macos")] unsafe extern "C" { fn fcopyfile(from: c_int, to: c_int, state: *mut c_void, flags: u32) -> c_int; }`
  with `const COPYFILE_XATTR: u32 = 1 << 2;` (libSystem, always linked). No Cargo change.
  `create_in_place` (no file was there) is unchanged.
- **crates/fe-workbench/src/search_replace.rs**, `write_beneath`: after the temp file is created by
  `open_at`, and before the write, open the original with the existing `open_in(&folder, name)` (no
  path lookup outside the checked folder, `O_NOFOLLOW`) and call `libc::fcopyfile(original_fd,
  temp_fd, ptr::null_mut(), libc::COPYFILE_XATTR)` (fe-workbench already depends on libc).

Either way, a failed attribute copy (or failing to open the original for it) does not fail the save:
the save goes on and `log::warn!` names the file and the error. Linux is unchanged. Update
`SaveJob::run`'s doc paragraph on permissions to say the original's extended attributes are copied too
(macOS).

Regression tests, using only base API:

- new integration test crates/fe-editor/tests/save_xattrs.rs (`#![cfg(target_os = "macos")]`,
  top-level fn `a_save_keeps_the_files_extended_attributes`): write a file, `/usr/bin/xattr -w` an
  attribute, `EditorModel::open`, replace the whole text through `buffer_mut().edit(.., Origin::User)`,
  `save()`, then check the new text and `xattr -p` gives the value.
- a new `#[cfg(target_os = "macos")] #[gpui::test] fn replacing_keeps_a_files_extended_attributes` at
  the end of crates/fe-workbench/src/search_replace_tests.rs: `project(&[("a.rs", b"foo a\n")])`, set
  the attribute, `searching(cx, dir, "foo")`, `replace_all(&search, "bar", cx)`, check `read` gives
  "bar a\n" and `xattr -p` the value. Both were run at b7bd02cf and fail there with "No such xattr".

## Implementation notes

- T752 F1 (create_in_place cleanup race) is not addressed: `create_in_place` is explicitly unchanged by this task and no identity-safe unlink exists without a scope-external change.
