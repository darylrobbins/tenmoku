---
task_uid: "01m72f64tcde12s12ewp79x1c1"
task: "T563"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "`kiln_core::platform::ProcessTree` exists: on Unix the child leads a new process group, cancel sends SIGTERM to the group then SIGKILL after 2 s; on Windows the process is created suspended, assigned to a job with KILL_ON_JOB_CLOSE, then resumed, and kill is `TerminateJobObject` with no grace. On Unix cancel returns before the grace is up only when nothing but the unreaped program is known to remain in the group; anything it cannot confirm counts as still running: on Linux /proc stat records are parsed as bytes, any listing or read error other than the process having gone (NotFound/ESRCH) is unknown, and a Z/X member counts as ended only when every thread under /proc/\u003cpid\u003e/task has ended; on macOS a failed proc_listpids (0 with errno set, or negative) is unknown."
    required: true
    manual: false
  - id: "C2"
    text: "New test `process_tree_kill_ends_grandchild_holding_stdout` in kiln/crates/kiln-core/tests/process_tree.rs: a `sh` script (Unix) or `.cmd` script (Windows) starts a grandchild that holds stdout; killing the tree ends both and the reader sees EOF."
    required: true
    manual: false
  - id: "C3"
    text: "New test `process_tree_unix_sigterm_grace_then_sigkill` (cfg(unix)): a child that traps SIGTERM exits on SIGTERM within the 2 s grace; one that ignores it is SIGKILLed after 2 s (use an injectable grace so the test takes under 1 s)."
    required: true
    manual: false
  - id: "C4"
    text: "New test `process_tree_windows_nothing_escapes_the_job` (cfg(windows)): a grandchild started immediately after resume is also terminated."
    required: true
    manual: false
  - id: "C5"
    text: "kiln-plugin's `run` and `Running::kill` in src/process.rs use ProcessTree; the existing kiln-plugin process tests still pass."
    required: true
    manual: false
  - id: "C6"
    text: "`cargo check --target x86_64-pc-windows-msvc -p kiln-core` type-checks where the build scripts allow it; windows-sys features are added to kiln-core/Cargo.toml only under cfg(windows)."
    required: true
    manual: false
  - id: "C7"
    text: "Linux unit tests in kiln-core against a fake /proc tree (others_in_proc) pass: platform::process_tree::tests::a_member_with_a_non_utf8_name_still_runs, an_unreadable_stat_is_unknown, a_malformed_stat_is_unknown, a_listing_that_fails_is_unknown, a_member_gone_since_the_listing_is_skipped, a_zombie_with_a_live_thread_still_runs, a_zombie_whose_threads_all_ended_has_ended and the_leader_and_other_groups_are_not_counted (review F4, F5); the existing tests in kiln/crates/kiln-core/tests/process_tree.rs still pass"
    required: true
    manual: false
---

# ProcessTree: kill a plugin program's whole process tree on Unix and Windows (P2, W-c) (Mac T1687)

# ProcessTree: kill a plugin program's whole process tree on Unix and Windows (P2, W-c)

Draft id P2 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Section 2.2 row W-c. Do not touch fe-lsp's own job object (moving it onto ProcessTree is a follow-up note in section 7). Keep unsafe code confined to platform/process_tree.rs with a SAFETY comment per block.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

## Helium note (lead)

Helium has no Windows target installed (`rustup target list --installed`: aarch64-apple-darwin, wasm32-wasip2, x86_64-unknown-linux-gnu). Try `rustup target add x86_64-pc-windows-msvc` and run the `cargo check --target x86_64-pc-windows-msvc -p kiln-core` criterion. If the target or a build script can't run here, say so in the finish notes with the exact error. The cfg(windows) test can't run on Linux: say that it was type-checked only, and the Mac's Windows CI runs it.

## Review-cap respec (2026-11-10)

Review `kaj2t4` (candidate `4ymd1f`, round 3) left C1 unmet on two mediums, F4 and F5, both in `unix::others_in` (`kiln/crates/kiln-core/src/platform/process_tree.rs` ~339–401). F3 is fixed; C2–C6 are met. `cancel_within` already treats `others_remain() == true` (and `others_in == None`, through `unwrap_or(true)`) as "keep the grace". Both findings are places where `others_in` answers "nothing left" when it doesn't know, which cuts the grace short and SIGKILLs a live worker. The fix is only in `others_in`. `cancel_within`, `Group`, Windows and kiln-plugin stay as they are.

C1 is reworded to state this rule. The rule: cancel may return before the grace is up only when the group is *known* to have nothing left running; anything that can't be confirmed counts as still running. C7 is new, for the tests below. C2–C6 are unchanged.

### Required fix, Linux (F4 + F5)
Split the walk out so it can be tested against a fake tree: `fn others_in_proc(proc: &Path, group: libc::pid_t) -> Option<bool>`, with `others_in(group)` = `others_in_proc(Path::new("/proc"), group)`. `Some(true)`: a member other than the leader still runs. `Some(false)`: confirmed none. `None`: can't tell. Rules, in order:
1. `read_dir(proc)` fails → `None`. Any `Err` item from the iteration → `None` (today it `continue`s).
2. A name that isn't a decimal pid → skip. `pid == group` (the unreaped leader, judged by `waitid`) → skip.
3. Read `<proc>/<pid>/stat` with `std::fs::read` (bytes, **not** `read_to_string`: `comm` may be any bytes). An error whose `kind()` is `NotFound` or whose `raw_os_error()` is `Some(libc::ESRCH)` means the process went away since the listing → skip. Any other error (EACCES, EISDIR, …) → `None`.
4. Parse as bytes: the fields start after the **last** `b')'` (`rposition`). With no `)`, or a missing or unparsable state or pgrp field, → `None`. The state is the first field, the pgrp the third, split on ASCII whitespace. Parse the pgrp from its ASCII digits (`std::str::from_utf8` on that field alone is fine).
5. `pgrp != group` → skip.
6. A state other than `Z` and `X` → `Some(true)`.
7. A state of `Z` or `X` (F5): a zombie leader of a thread group can still have live threads (its main thread called `pthread_exit`). List `<proc>/<pid>/task`. `NotFound`/`ESRCH` on the listing → skip (gone). Any other error → `None`. For each tid, read `<proc>/<pid>/task/<tid>/stat` with the same byte parse and error rules (`NotFound`/`ESRCH` → that thread is gone, skip it). Any thread whose state isn't `Z`/`X` → `Some(true)`. All threads ended or gone → this process has ended; skip it.
8. After the walk → `Some(false)`.

### Required fix, macOS (F4)
`proc_listpids` returns 0 on failure, with errno set ([libproc.c](https://github.com/apple-oss-distributions/xnu/blob/main/libsyscall/wrappers/libproc/libproc.c)). Before each call, clear errno (`*libc::__error() = 0`, in an `unsafe` block with its SAFETY comment). After it: a negative result → `None`; 0 with errno non-zero → `None`; 0 with errno 0 → `Some(false)`. Keep the grow-and-retry loop. macOS is type-checked from helium (`cargo clippy --target aarch64-apple-darwin -p kiln-core --tests`; stub `CC`/`AR` if a build script needs them). Say in the finish notes that it was type-checked only.

### Required tests (C7)
Unit tests in `process_tree.rs`, in `#[cfg(all(test, any(target_os = "linux", target_os = "android")))] mod tests`, against a fake tree in a `tempfile::tempdir()` (already a dev-dependency). Write stat records as bytes, e.g. `b"4242 (w\xff) S 1 777 777 0 -1 ..."`. `group` = 777 throughout. Gate paths are `platform::process_tree::tests::<name>`:
- `a_member_with_a_non_utf8_name_still_runs`: `4242/stat` with comm `w\xff) x` (non-UTF-8, and a `)` and a space inside comm), state `S`, pgrp 777 → `Some(true)`.
- `an_unreadable_stat_is_unknown`: `4242/stat` is a directory (EISDIR; this works as root too, unlike chmod) → `None`.
- `a_malformed_stat_is_unknown`: `4242/stat` with no `)` → `None`.
- `a_listing_that_fails_is_unknown`: `proc` is a path that doesn't exist → `None`.
- `a_member_gone_since_the_listing_is_skipped`: `4242/` with no `stat` file → `Some(false)`.
- `a_zombie_with_a_live_thread_still_runs`: `4242/stat` state `Z` pgrp 777, `4242/task/4242/stat` state `Z`, `4242/task/4243/stat` state `S` → `Some(true)` (F5).
- `a_zombie_whose_threads_all_ended_has_ended`: the same with both threads `Z` → `Some(false)`.
- `the_leader_and_other_groups_are_not_counted`: `777/stat` (the leader, state `S`) and `5000/stat` (state `S`, pgrp 999) → `Some(false)`.

Keep every existing test in `tests/process_tree.rs` passing. In particular `process_tree_unix_grace_outlives_the_program` must still return early, which shows the early return still works on a real `/proc`. Don't add a Python or C helper. A real-process test for a worker whose main thread exits isn't needed: the fake-tree tests are the regression coverage for F4 and F5.

Add a line to the Implementation notes for round 3: unknown counts as running, and why (F4, F5). Finish once.

## Implementation notes

- `kiln_core::platform::ProcessTree` (`kiln/crates/kiln-core/src/platform/process_tree.rs`):
  `spawn`, `try_wait`, `wait`, `kill`, `cancel` (grace `ProcessTree::GRACE`, 2 s) and
  `cancel_within(grace)`, the injectable grace C3 asks for. Dropping a tree kills what is left
  of it and reaps the program.
- **Unix.** `process_group(0)`. `try_wait` peeks with `waitid(WEXITED | WNOWAIT)`, so the
  program stays an unreaped zombie and its pid, the group's id, can't be reused while the tree
  may still signal the group. The group is signalled only while the program is unreaped, and
  the program is reaped only by `kill`, right after the group's last signal (SIGKILL, also sent
  to the program itself in case it left the group); a reaped tree signals nothing more, so a
  second `kill`, `cancel`, `wait` or the drop return the first status. `cancel` sends SIGTERM
  to the group and gives the whole group the grace, the program's exit included: it returns
  as soon as nothing in the group runs, else SIGKILL when the grace is up (review round 2,
  F3). A zombie leader still counts as a group member (checked: `kill(-pgid, 0)` succeeds for
  a group holding only a zombie), so the group's members are listed to tell the rest from
  the unreaped program: `/proc/*/stat` (pgrp, not a zombie) on Linux, `proc_listpids`
  (`PROC_PGRP_ONLY`) on macOS; other Unixes wait out the grace once the program has exited.
  The macOS half was type-checked from helium (`clippy --target aarch64-apple-darwin`).
  `wait` closes a piped stdin first, as `Child::wait` does (F2).
- **Round 3 (F4, F5): unknown counts as running.** An early return from `cancel` cuts a live
  worker's grace short, so the group listing answers "nothing left" only when it confirms it.
  `others_in_proc` (the Linux walk, tested against a fake `/proc` tree) reads `stat` as bytes,
  since `comm` may be any bytes (F4); passes over a process only when reading it fails with
  `NotFound`/`ESRCH`, since then it has gone, and treats any other listing or read error, or a
  record it can't parse, as unknown (F4); and counts a `Z`/`X` member as ended only once every
  thread under its `task` directory has, since a thread group's main thread can exit while
  the others run on (F5). On macOS a `proc_listpids` returning 0 with errno set, or a negative
  result, is unknown (errno is cleared before each call). The macOS code was type-checked
  only (`clippy --target aarch64-apple-darwin -p kiln-core --tests`, stubbed `CC`/`AR`).
- **Windows.** The job (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, no breakaway) is made before the
  spawn; the process is created `CREATE_SUSPENDED | CREATE_NO_WINDOW` (setting creation flags
  replaces them, so the tree sets `CREATE_NO_WINDOW` itself), assigned, and its thread resumed
  via a Toolhelp32 thread snapshot. A failure there terminates it. `kill` and `cancel` are
  `TerminateJobObject`, with no grace.
- **kiln-plugin.** `run` spawns a `ProcessTree`; the `Running` wrapper and its `kill` are gone,
  replaced by the tree's `kill` (at the output cap) and its drop (every other return, including
  the deadline). The interactive run's kill has no grace: the run is already over and the call
  waits on it; the 2 s grace is for §3.3's cancellation of streamed programs (B4). A grandchild
  holding the output still makes the run time out, as before, but is now killed with it.
- **Windows check (C6).** `rustup target add x86_64-pc-windows-msvc` worked on helium;
  `cargo check`/`clippy --target x86_64-pc-windows-msvc -p kiln-core --tests` pass. The
  `cfg(windows)` halves of the tests, including C4, were type-checked only: the Mac's Windows
  CI runs them.
