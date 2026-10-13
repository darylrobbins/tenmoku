---
task_uid: "01m4sjhexfdjb58gj6z441s438"
task: "T81"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "test non_utf8_file_names (cargo test -p fe-git --test file_log_paths, Linux): file_log of a file named caf\\xe9.rs lists its two commits with the name byte for byte, and blame attributes its rows to them (F1)"
    required: true
    manual: false
  - id: "C2"
    text: "test root_commit_listed_with_show_root_off (cargo test -p fe-git --test file_log_paths): with log.showRoot=false, file_log lists the root commit, including for a file changed only there (F2)"
    required: true
    manual: false
  - id: "C3"
    text: "every test in crates/fe-git/tests/file_log.rs and crates/fe-git/tests/blame.rs passes unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-git and cargo clippy -p fe-git --all-targets -- -D warnings pass; no Cargo.toml or Cargo.lock change"
    required: true
    manual: false
---

# Keep non-UTF-8 file names and the root commit in fe-git's file history and blame (T70 review F1, F2) (Mac T588 follow-up)

Follow-up from T70 (Mac T588, landed aae5bef), filed on helium: review findings F1 and F2 (medium) of review
01m4se1v65mvn4p4ea0hqb7dqj (`rgt review show T70`).

## Problem

- F1 (`crates/fe-git/src/log.rs` :37-41, `crates/fe-git/src/blame.rs` :88-92). Both build the git path from the
  relative path's components with `to_string_lossy`, which turns a valid non-UTF-8 Unix filename into U+FFFD.
  `Repo::file_log` then asks git about a different name and returns empty history; `Repo::blame` looks up the
  blob by the real bytes (`commit_blob`) but asks gix's `blame_file` for the mangled name.
- F2 (log.rs :44-57). The `git log` inherits `log.showRoot`. With `log.showRoot=false` git omits the root
  commit, so a file's history loses its first commit, and a file changed only in the root commit has none.
  Checked on helium with git 2.53: `-c log.showRoot=false log --follow -z --name-status --format=%H -- f` lists
  only the second commit; adding `--root` lists both.

## Change

- In `crates/fe-git/src/blame.rs`, a `pub(crate) fn git_path(rela_path: &Path) -> BString`: the normalized
  components (as now: no `.`, no doubled separators) joined with `/`, each component's bytes kept exactly
  (`std::os::unix::ffi::OsStrExt::as_bytes` on Unix; elsewhere `to_string_lossy` as today). `blame` passes it to
  `blame_file`.
- In `crates/fe-git/src/log.rs`, `file_log` uses `git_path` and hands git the same bytes as its path argument
  (`OsStr::from_bytes` on Unix), and passes `--root` with the other explicit flags. The comment over the flags
  names `log.showRoot` among the config they shut out.
- No change to `lib.rs`.

## Tests

New file `crates/fe-git/tests/file_log_paths.rs`, self-contained (copy the temp-repo helpers that shut out
developer config from `crates/fe-git/tests/file_log.rs`; don't edit that file).

- `non_utf8_file_names` (`#[cfg(target_os = "linux")]`: macOS file systems reject such names): a file named
  `b"caf\xe9.rs"` committed (c1) and edited (c2). `file_log` of it is `[c2, c1]`, each `path` equal to the real
  name byte for byte; `blame` of its current text gives runs whose commits are c1 and c2, not uncommitted rows.
- `root_commit_listed_with_show_root_off`: repo config `log.showRoot=false`; `a.rs` added in the root commit c1
  and edited in c2, `only_root.rs` added in c1 and never touched again. `file_log(a.rs)` is `[c2, c1]`,
  `file_log(only_root.rs)` is `[c1]`.
