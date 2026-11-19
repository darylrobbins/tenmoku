---
task_uid: "01m7rf6hdjfc4m5qp3gfz8pg29"
task: "T429"
rev: 2
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "crates/fe-workbench/src/tests/mod.rs has a pub(crate) helper giving the POSIX sh to run: /bin/sh on unix, on Windows sh.exe found on PATH (Git for Windows), panicking with a clear message when none is found (never a silent skip)"
    required: true
    manual: false
  - id: "C2"
    text: "On this Linux host the fe-workbench tests::run_process tests pass using the shared helper, and if a Windows target (x86_64-pc-windows-gnu or -msvc) is installed cargo check -p fe-workbench --tests for it is clean (if no Windows target is installed, the finish notes say so); the GH CI windows Test result for the 11 run_process tests that failed at 8ba6f008c is verified after landing by the Mac and is not a gate for this card"
    required: true
    manual: false
  - id: "C3"
    text: "Where Windows behaviour differs by design (process_group::terminate kills the tree at once, with no SIGTERM grace), the test asserts the Windows behaviour under cfg(windows) instead of being gated cfg(unix)"
    required: true
    manual: false
  - id: "C4"
    text: "macOS and Linux runs of tests::run_process are unchanged"
    required: true
    manual: false
---

# fe-workbench run_process tests on Windows: scripts run as /bin/sh -c and pids are checked with ps; run them with the PATH sh (Git for Windows) and add a shared POSIX-sh test helper (Mac T1966)

# run_process tests on Windows

GH CI 44405849017 on 8ba6f008c, windows Test: 11 tests::run_process tests fail. Their `sh()` RunSpec runs `program: "/bin/sh"`, which doesn't exist on Windows. The run ends `Failed { code: None }` (the spawn failed), or "the child sleep isn't running" / "gave up waiting: the child's pid".

Fix:
- Add a shared helper in src/tests/mod.rs giving the POSIX shell's path: /bin/sh on unix, and on Windows the `sh.exe` on PATH. Git for Windows' sh is on the runner's PATH: language_pack_per_pack runs `Command::new("sh")` and packaged both tarballs on Windows. Cards for the fake cargo, chat/terminal and path tests reuse it.
- `gone(pid)` uses `ps -o stat= -p`. The pid comes from the script's `echo $!`, which under MSYS is an MSYS pid, not a Windows one. On Windows, check it through the same sh (`kill -0 <pid>`, or MSYS `ps -p`).
- Production on Windows (process_group.rs) has no SIGTERM: `terminate` and `kill` both run `kill_tree`. Assertions such as "killed before its grace" and `trap '' TERM` semantics need cfg(windows) expectations that match process_group.rs's documented behaviour.

Tests only. If a Windows failure turns out to be in run_process.rs or process_group.rs, stop and report it for a bugfix card.

## Implementation notes

- `tests::posix_sh()` (src/tests/mod.rs): `/bin/sh` on unix; on Windows the first `sh.exe` in `PATH`'s directories, as a full path, panicking "no sh.exe on PATH" when there's none.
- run_process.rs: `sh()` runs `posix_sh()`. `gone(pid)` keeps `ps -o stat=` on unix; on Windows it runs `kill -0 "$1"` through the same sh, since `$$`/`$!` are MSYS pids.
- `a_cancel_kills_on_time_while_the_sink_is_blocked`: unix still asserts the child outlives GRACE (SIGTERM ignored); Windows asserts it is gone before GRACE (`terminate` runs `kill_tree` at once). The other cancel tests' bounds hold on both.
- Not checked locally on Windows: only the x86_64-unknown-linux-gnu target is installed on this host, so the Windows cfg code is checked only by GH CI's windows Clippy and Test.
