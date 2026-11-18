---
task_uid: "01m7r0xr7ye17nkvsnsabzqna4"
task: "T1959"
rev: 1
type: "chore"
complexity: ""
criteria:
  - id: "C1"
    text: "On a non-unix target none of the listed unused-import or dead-code lints fire: each item is gated to match its only users' cfg, or carries a targeted allow with a reason; no test is deleted"
    required: true
    manual: false
  - id: "C2"
    text: "cargo clippy -p fe-dap -p fe-workbench --all-targets -- -D warnings passes on macOS, and cargo nextest run -p fe-dap and the touched fe-workbench test modules pass"
    required: true
    manual: false
---

# Windows CI Clippy: unix-only test helpers unused off unix in fe-dap tests/registry.rs (15) and fe-workbench debug_session/launch.rs, git_askpass.rs, tests/debug_session.rs (6) fail -D warnings

GitHub CI's windows "Clippy" job (`cargo clippy --workspace --all-targets -- -D warnings`) on main 023b28b42 (run 44359952603, job 134998251501), after T1954, fails in two crates. All are unix-only test or helper code that is unused off unix:

fe-dap (lib test), crates/fe-dap/src/tests/registry.rs (15 errors):
- :15 unused import `DebugTransport`; :17 unused `kiln_l10n::tr`; :21 and :24 unused imports (`JAVA_PORT`, `JAVA`, `Lookup`, `Status`, `XCRUN`, `lldb_dap_lookups`, `python_interpreter`, `rust_init_commands`, ...)
- never used off unix: struct `Fixture` (:30) and its `new`, `on_path`, `login`, `login_with`, `registry`, `context` (:38); `write_script` (:92); `program` (:103); struct `FakeRunner` (:111) and its `answering`, `calls` (:117); `settings` (:150); type aliases `OrderRow` (:161), `CollisionRow` (:165), `PythonRow` (:173); `strings` (:175).
  Likely the tests that use them are `#[cfg(unix)]`; gate the helpers (or the whole module, if every test in it is unix-only) the same way.

fe-workbench (lib test), 6 errors:
- crates/fe-workbench/src/debug_session/launch.rs:93 `session_options` never used
- crates/fe-workbench/src/git_askpass.rs:379 `shown_text`, `is_masked`, `typed` never used; :400 `prompts` never used
- crates/fe-workbench/src/tests/debug_session.rs:1445 `click_rerun`, :1572 `config_named`, :1596 `resolve` never used

Fix each by matching the cfg of its only users (`#[cfg(unix)]` or `#[cfg_attr(not(unix), allow(dead_code))]` with a reason), not by deleting tests. Do not change behaviour on unix.

The lints fire only on a Windows target, so prove the fix by reading the cfgs, plus unix clippy and tests (or `cargo clippy --target x86_64-pc-windows-msvc` if that target's std is already installed; do not install toolchains). Crates after these in the build order (e.g. fastedit) may surface further Windows lints once this lands; those are a follow-up card, not this one.
