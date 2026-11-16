---
task_uid: "01m7h5c5agkz6h6etc8t4a588x"
task: "T396"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "debug_with in crates/fe-workbench/src/tests/debug_gutter.rs writes fe_config::project_settings_path(root) with [lsp.servers.rust-analyzer] enabled = false before the workbench opens, as tests/test_runner.rs does"
    required: true
    manual: false
  - id: "C2"
    text: "After cargo nextest run -p fe-workbench -E 'test(/debug_gutter/)', no rust-analyzer process carries that run's NEXTEST_RUN_ID (ps -E), and every debug_gutter test still passes"
    required: true
    manual: false
---

# debug_gutter tests start a real rust-analyzer that outlives the test process and pins pipeline slots (Mac T1883)
