---
task_uid: "01m4t4de6adpbq3kae3z19x54m"
task: "T111"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "crates/fe-project/tests/search_only_parent.rs has a test named exactly tree_lists_a_project_beneath_a_search_only_folder: with locked/ at mode 0o111 above project/, FileTree::open(project) succeeds and lists a.rs and src (asserts skipped when euid is 0). It fails at base on Linux"
    required: true
    manual: false
  - id: "C2"
    text: "The same file's finder_walks_a_project_beneath_a_search_only_folder (a FinderIndex walk of project contains a.rs and src/b.rs) and open_file_under_reads_beneath_a_search_only_folder (open_file_under(project, src/b.rs) reads its text) pass on Linux and macOS"
    required: true
    manual: false
  - id: "C3"
    text: "listing::dir_flags opens Access::Search with O_PATH on Linux and Android, O_SEARCH on macOS (unchanged) and O_RDONLY on other unixes; any Access::Search call site that needs a readable fd is switched to Access::Read"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-project -p fe-nav passes on Linux, the symlink-refusal tests (tree, finder_walk_safety, fe-nav open_under) included"
    required: true
    manual: false
---

# Reach a project beneath a search-only folder on Linux: open fe-project's search handles with O_PATH, as macOS uses O_SEARCH (Mac T630)

## Problem

fe-project reaches every directory from `/` a component at a time (`listing::open_dirs`, `open_at`),
opening the ones it only passes through or looks things up in with `Access::Search`. `dir_flags`
(crates/fe-project/src/listing.rs:698-708) turns that into `O_SEARCH` on macOS, which needs search
(`x`) permission alone, but into `O_RDONLY` everywhere else, which needs read (`r`) permission too.

So on Linux a project beneath a folder that can be searched but not listed (mode `0o111`, or `--x` for
the user, as with some shared home and hosting layouts) can't be opened at all:

- `FileTree::open` (crates/fe-project/src/tree.rs:210) and `FileTree::open_dir` (tree.rs:806, 820)
  fail with EACCES on the locked ancestor;
- `FinderIndex`'s walk (`Chain::open`, crates/fe-project/src/finder.rs:1348; finder.rs:350, 1221, 1393)
  finds nothing;
- `open_file_under` (listing.rs:639, 656), which fe-nav's extraction, references and SCIP loading open
  files through, fails;
- the Linux watcher's `watch_tree` (crates/fe-project/src/watcher.rs:355) fails on the root.

Meanwhile the project search (`ignore`'s walker, path based) and the workbench's replace already reach
such a project on Linux: crates/fe-workbench/src/search_replace.rs:372-385 opens its folders with
`O_PATH` on Linux "as fe-project's walk opens them" on macOS, and its test
`a_project_beneath_a_folder_that_can_only_be_searched` (crates/fe-workbench/src/search_replace_tests.rs:1155)
notes "On Linux the search's walk opens folders to read". The tree, finder and go-to-definition
therefore disagree with search on Linux only.

## Change

In `dir_flags` (crates/fe-project/src/listing.rs), open `Access::Search` directories with `O_PATH` on
Linux and Android, as search_replace.rs does, keeping `O_SEARCH` on macOS and `O_RDONLY` on other
unixes; update the `Access` doc comment to say so. Keep `O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC`.

Every `Access::Search` handle is used only as an `openat`/`fstatat` dirfd, for `File::metadata`
(statx with `AT_EMPTY_PATH`) and `try_clone`, all of which work on an `O_PATH` descriptor. Audit the
call sites listed above (and any others `rg 'Access::Search'` finds) for a use that needs a real open
file (reading entries, `fdopendir`, `fsync`, `flock`, `fpathconf`); if one exists, open that one with
`Access::Read` instead. A symlink must still be refused: on Linux `O_PATH | O_NOFOLLOW | O_DIRECTORY`
on a symlink fails with `ENOTDIR`, as on macOS, and the existing symlink tests must keep passing.

Add crates/fe-project/tests/search_only_parent.rs, an integration test through the public API: a
temp dir `locked/` holding `project/a.rs` and `project/src/b.rs`, then `locked` chmod `0o111`
(restored to `0o755` before the asserts so the temp dir cleans up). Skip the asserts when
`geteuid() == 0`, where permissions don't bind. It fails at base on Linux and passes on macOS before
and after.
