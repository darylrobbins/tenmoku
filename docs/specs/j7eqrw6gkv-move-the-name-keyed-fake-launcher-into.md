---
task_uid: "01m77zy63dyy45afj7eqrw6gkv"
task: "T330"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "`tests::fake_launchers::routes_each_spec_name_to_its_own_launcher` and `tests::fake_launchers::each_server_keeps_its_own_records` pass"
    required: true
    manual: false
  - id: "C2"
    text: "kill.rs no longer defines `Named`; its existing tests pass unchanged"
    required: true
    manual: false
---

# Move the name-keyed fake launcher into the fake LSP server as FakeLaunchers so tests can run two servers (LSP add-ons A2, T1645) (Mac T1732)

# Move the name-keyed fake launcher into the fake LSP server as FakeLaunchers so tests can run two servers (LSP add-ons A2, T1645)

Spec: §7 (Two fake servers for one document), §8 row A2.

Row: `FakeLaunchers` (name-keyed launcher) moved from `kill.rs` (the `Named` launcher at kill.rs:540) into the fake. `FakeLaunchers::new([("css", primary.launcher()), ("vars", add_on.launcher())])`; each `FakeServer` keeps its own script, records, replicas and launches.

Test file: crates/fe-lsp/src/tests/fake_launchers.rs, declared from tests/mod.rs. kill.rs keeps passing using the moved type.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.
