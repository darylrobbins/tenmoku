---
task_uid: "01m73zz8m7ssn3q3my4y32w0f4"
task: "T1722"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "tests::registry::bare_commands_use_the_login_shell_path passes 30 consecutive runs with CPU burners running (2x ncpu), every shell the test expects to succeed retried on a timeout like the failing ones"
    required: true
    manual: false
  - id: "C2"
    text: "only crates/fe-lsp/src/tests/registry.rs changes; LOGIN_SHELL_TIMEOUT and launcher.rs are untouched"
    required: true
    manual: false
---

# Flake: fe-lsp tests::registry::bare_commands_use_the_login_shell_path still fails under load (shells meant to succeed hit the 2 s LOGIN_SHELL_TIMEOUT; T1417 only retried the failing shells)

Flake: fe-lsp `tests::registry::bare_commands_use_the_login_shell_path` still fails under heavy machine load, after T1417.

Evidence: main post-merge check run 01m73yftp72rg893c5v2g992fm (commit 85245333d, stack 124: kiln-core/kiln-plugin + an fe-registry test, nothing in fe-lsp) failed only this test, 1 of 8901: crates/fe-lsp/src/tests/registry.rs:1626 `assert_eq!(login.path(), Some("bin"))`, left: None. Log: ~/.local/state/regatta/runs/01m73yftp72rg893c5v2g992fm/test.fastedit.log lines 3806-3825. Load 18-26 at the time; the test took 6.4 s against 3.0 s idle. The same tree passed a full macOS gate (fail=0), and the test passes alone at 85245333d.

Cause: `LoginPath` falls back to None when the stand-in login shell takes longer than LOGIN_SHELL_TIMEOUT (2 s). T1417 (893da19a9) added retries only for the failing, slow and chatty shells; the shells that are meant to succeed (e.g. `relative_only`, line 1626) still have no retry, so a starved run still reads None.

Fix (test only, no production change, no regression test): in crates/fe-lsp/src/tests/registry.rs, give every shell the test expects to succeed the same retry T1417 gave the failing ones: retry while the resolution timed out (its log/outcome says it took over the timeout), up to a small bound, and assert on the first non-timeout result. Keep each assertion's meaning unchanged.

Scope: crates/fe-lsp/src/tests/registry.rs only.

Proof (chore, no regression test): run `cargo nextest run -p fe-lsp tests::registry::bare_commands_use_the_login_shell_path` 30 consecutive times with CPU burners running (2x ncpu `yes > /dev/null`), all passing; note the command and result in the finish notes.
