---
task_uid: "01m4t7mhxh87e04r2w6xqa81ay"
task: "T103"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-git test discard_arg_overhead::discard_many_short_paths_under_a_small_arg_max (Linux): re-run under ulimit -s 512 with getconf ARG_MAX asserted \u003c= 131072, Repo::discard of 12,000 modified tracked four-character files plus 3 untracked ones returns Ok, every tracked file reads its committed text, the untracked files are gone and status_split is empty"
    required: true
    manual: false
  - id: "C2"
    text: "Repo::tracked_names charges each path, fixed argument and inherited environment entry its bytes + NUL + one pointer against a budget of half of 128 KiB, and every batch takes at least one path; stage, unstage, the checkout call, cli.rs and the public API are unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "The tests in crates/fe-git/tests/stage.rs and stage_many_paths.rs pass unchanged; cargo test -p fe-git and cargo clippy -p fe-git --all-targets -- -D warnings pass; no Cargo.toml or Cargo.lock change"
    required: true
    manual: false
---

# Batch fe-git discard's ls-files lookup by what exec charges, pointers and environment included, so many short paths never hit E2BIG (T99 review F4) (Mac T631 follow-up)

## Problem

T99 (217d1ce, Mac T631) moved `stage`, `unstage` and the `checkout` half of `discard` onto stdin
(`--pathspec-from-file=- --pathspec-file-nul`), but `Repo::discard` still asks which requested paths are
tracked with `git ls-files -z -- <paths>` on the command line, in `Repo::tracked_names`
(crates/fe-git/src/stage.rs:163-188). `git ls-files` has no `--pathspec-from-file` (git 2.53:
"error: unknown option `pathspec-from-file=-'"), so it batches, and the batch limit is wrong:
`BATCH_BYTES = 64 * 1024` counts only each path's bytes plus its NUL.

Review finding T99 F4 (medium, stage.rs:168): the kernel charges every argument and every environment
variable a pointer (8 bytes) as well as its bytes, and the environment, the program name and git's
fixed arguments (`--literal-pathspecs -C <workdir> ls-files -z --`, plus `GIT_TERMINAL_PROMPT`, `LC_ALL`
and `GIT_OPTIONAL_LOCKS` from `cli::run_raw`) share the same space. Linux's argument space is
max(128 KiB, stack limit / 4). With `ulimit -s 512`, `getconf ARG_MAX` is 131072; 12,000 distinct
four-character paths are 60,000 bytes with terminators, so they enter one batch, but with pointers they
need about 156,000 bytes and the spawn fails `Argument list too long` (E2BIG). `discard` then returns
`GitError::Io` and discards nothing. Reproduced on Linux 2026-10-13:
`sh -c 'ulimit -s 512 && git ls-files -z -- <12,000 short names>'` exits 126 "Argument list too long".

## Change

Keep `git ls-files` (it expands sparse directory entries and lists every stage, which is why
`tracked_names` uses it), and make each batch fit the argument space by what exec actually charges.

In crates/fe-git/src/stage.rs, `Repo::tracked_names`:

- Replace `BATCH_BYTES` with a budget derived from `ARG_SPACE = 128 * 1024` (Linux's floor for the argument
  space whatever the stack limit; macOS's is 1 MiB), halved for headroom: `ARG_SPACE / 2`.
- From that budget subtract, once per call: the environment the child inherits (for each
  `std::env::vars_os()` entry, key + value + 2 bytes for `=` and NUL, plus `size_of::<usize>()` for its
  pointer), the three variables `cli::run_raw` adds, and the fixed arguments (`git`, `--literal-pathspecs`,
  `-C`, the workdir, `ls-files`, `-z`, `--`), each with its NUL and pointer.
- Charge each path its bytes + 1 + `size_of::<usize>()`. Close a batch before the path that would exceed what
  is left; a batch always takes at least one path, so a huge environment degrades to one path per call
  rather than looping.
- Put the cost of one argument in a small private fn (`exec_cost(&OsStr) -> usize`) used for paths, fixed
  arguments and environment entries alike, and update `tracked_names`' doc comment to say what is counted.

No change to `stage`, `unstage`, the `checkout` call in `discard`, `cli.rs`, or any public API.

Regression test: a new integration test file crates/fe-git/tests/discard_arg_overhead.rs, so Regatta's
regression check (which applies only test files to base) runs it at base. It must not depend on the
machine's stack limit, so the test re-executes its own binary under a lowered limit:

- `discard_many_short_paths_under_a_small_arg_max`, `#[cfg(target_os = "linux")]`. When the env var
  `FE_GIT_SMALL_ARG_MAX_CHILD` is unset, it runs
  `sh -c 'ulimit -s 512 && exec "$0" "$@"' <std::env::current_exe()> --exact discard_many_short_paths_under_a_small_arg_max --nocapture --test-threads=1`
  with `FE_GIT_SMALL_ARG_MAX_CHILD=1` set, and asserts the child exits 0 (printing its stdout and stderr
  on failure). No new dependency: `sh`'s `ulimit` lowers RLIMIT_STACK for the child only.
- When the variable is set (the child), it first asserts `getconf ARG_MAX` is at most 131072, so the test
  can't pass because the limit didn't take. It then initialises a repository (same `git` helper shape as
  tests/stage_many_paths.rs: `-c user.name/user.email`, `commit.gpgsign=false`, `GIT_CONFIG_NOSYSTEM=1`,
  `GIT_CONFIG_GLOBAL=/dev/null`), writes 12,000 files named `format!("{n:04x}")` (four characters each) with
  "one\n", commits them, rewrites each with "two\n", adds 3 untracked files, and calls `Repo::discard` on
  all 12,003 paths. It asserts `discard` returns `Ok`, `status_split()` is empty, every tracked file reads
  "one\n" and the untracked files are gone.
- At base this fails deterministically: 12,000 x 5 bytes = 60,000 < 64 KiB puts every path in one
  `ls-files` batch, whose argv + pointers (~156,000 bytes) exceed 131072, so `discard` returns
  `GitError::Io("Argument list too long ...")`. With the fix each path costs 13 bytes against a budget under
  64 KiB, so about 5,000 paths go per batch and every spawn fits.
