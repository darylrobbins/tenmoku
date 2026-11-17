---
task_uid: "01m7j36n0d25f9bq09ehb5d0pe"
task: "T1903"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The fixture in crates/fe-workbench/src/tests/debug_session.rs writes fe_config::project_settings_path(root) with [lsp.servers.rust-analyzer] enabled = false before the workbench opens a .rs file in a trusted root, as tests/test_runner.rs and tests/debug_gutter.rs do"
    required: true
    manual: false
  - id: "C2"
    text: "After cargo nextest run -p fe-workbench -E 'test(/debug_session/)', no rust-analyzer process carries that run's NEXTEST_RUN_ID (ps -E), and every debug_session test still passes"
    required: true
    manual: false
---

# tests::debug_session starts a real rust-analyzer that outlives the test process (a_new_stop_retires_the_last_stops_place leaked one carrying a pipeline unit mark), as T1883 fixed for debug_gutter
