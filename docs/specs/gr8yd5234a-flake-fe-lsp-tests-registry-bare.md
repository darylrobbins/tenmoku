---
task_uid: "01m6rnk6q535yvncgr8yd5234a"
task: "T1417"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "tests::registry::bare_commands_use_the_login_shell_path passes 30 consecutive runs with CPU burners running (2x ncpu busy loops, stopped afterwards)"
    required: true
    manual: false
  - id: "C2"
    text: "only crates/fe-lsp/src/tests/registry.rs changes; LOGIN_SHELL_TIMEOUT and launcher.rs are untouched"
    required: true
    manual: false
---

# Flake: fe-lsp tests::registry::bare_commands_use_the_login_shell_path fails under load

Flake: fe-lsp `tests::registry::bare_commands_use_the_login_shell_path` fails under heavy machine load.

Evidence: main post-merge check run 01m6rjhjt8z7jp8hkyy49wecmp (commit 5c7c3e7b, a Sparkle-only change) failed only this test, at crates/fe-lsp/src/tests/registry.rs:1496 (the `logs.last()...contains(why)` assert), at load ~200-280. The failing shell logged the right reason, but the `chatty-shell` case logged "took over 2 s to print its PATH" where the test expects "ended without writing its PATH". The chatty stand-in is `printf '/bin\nbye'` and exits 0 at once, so the only way to hit the 2 s deadline is the `/bin/sh` that runs it being starved of CPU for 2 s. The test is correct; production (LOGIN_SHELL_TIMEOUT = 2 s in crates/fe-lsp/src/launcher.rs) behaved as designed: on a timeout it falls back to Tenmoku's own PATH and logs why. The same load that starves a stand-in sh would make a real `zsh -l` slow too, but that costs only a fallback at load >200, so production is left alone.

Fix (test only, no production change, no regression test): in crates/fe-lsp/src/tests/registry.rs, make the failing-shell and chatty-shell cases of the `for (shell, why)` loop not depend on wall time. Those two cases must reach their result well inside 2 s; if the log says "took over 2 s" for a case whose expected reason is not the timeout, rebuild a fresh `LoginPath::new(&shell)` and retry (at most 3 attempts), then run the existing assertions on the attempt that did not time out. Keep the slow-shell case as is. Do not change LOGIN_SHELL_TIMEOUT or any non-test file.

Scope: crates/fe-lsp/src/tests/registry.rs only.

Proof (chore, no regression test): run `cargo nextest run -p fe-lsp tests::registry::bare_commands_use_the_login_shell_path` repeatedly (e.g. 30 runs, several in parallel with a CPU burner such as `yes > /dev/null` x ncpu*2 running, stopped afterwards) with zero failures, plus the normal fe-lsp gate.
