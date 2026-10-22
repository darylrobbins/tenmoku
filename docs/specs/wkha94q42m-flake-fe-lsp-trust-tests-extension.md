---
task_uid: "01m5fv76qvkc3vg5wkha94q42m"
task: "T213"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "In crates/fe-lsp/src/extension_servers_tests.rs, user_settings_apply_over_an_extension_server no longer depends on how many launches happened before pyright's: before re-enabling zls with update_settings(\u0026LspSettings::default()) it closes the zig buffer (manager.close(zig.id())) so no waiting document makes zls relaunch as launch 2, and the pyright open_synced uses the launch number of the pyright launch (fake.launched() position) rather than a hard-coded 2. No production code and no other test file changes."
    required: true
    manual: false
  - id: "C2"
    text: "trust_tests::extension_servers::user_settings_apply_over_an_extension_server passes 30 consecutive runs with 0 retries under load: loop 'cargo nextest run -p fe-lsp trust_tests::extension_servers::user_settings_apply_over_an_extension_server --retries 0' 30 times while 'yes \u003e /dev/null' runs on 3x the core count of processes."
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-lsp passes, including trust_tests::extension_servers::an_extension_server_routes_its_language and trust_tests::extension_servers::changing_extension_specs_restarts_only_changed_servers."
    required: true
    manual: false
---

# Flake: fe-lsp trust_tests::extension_servers::user_settings_apply_over_an_extension_server waits for the python buffer at launch 2 but a transient zls launch can take that index (60 s timeout under load) (Mac T1064)

Flake: post-merge runs 01m5fqtkxgzm1w1g1137qwhfmv and 01m5frdxrc9kq9jm1w1vd44k8k (T1023, load ~50) both failed only trust_tests::extension_servers::user_settings_apply_over_an_extension_server, panicking at crates/fe-lsp/src/tests/fake.rs:759 "timed out waiting for the buffer's text" after 60 s; the dump shows the python replica open with the right text but launch: 3, while the test waits for launch 2 (wait_synced in trust_tests.rs:99 requires r.launch == launch). T1023 touches no fe-lsp; the revert's run passed.

Cause (test-only): in the test, after zls is turned off the zig buffer stays waiting in the manager. update_settings(&LspSettings::default()) re-enables the extension zls, so reconcile can place that waiting zig document and launch zls (launch 2); set_extension_specs(pyright evil) then drops zls again. Whether that transient zls launch reaches the launcher before it is stopped depends on thread timing, so pyright is launch 2 on a quiet machine and launch 3 under load. The assertion of launch 2 at extensions_servers_tests.rs open_synced(..., "python", 2) is a timing assumption, not a manager race.

Fix: close the zig buffer before the update_settings(default) step, and take pyright's launch number from the fake rather than hard-coding 2. Related flake tasks (T582, T622, T712 style) are other tests; no duplicate.
