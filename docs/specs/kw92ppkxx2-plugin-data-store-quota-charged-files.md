---
task_uid: "01m71sz9rq25ybrtkw92ppkxx2"
task: "T1702"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "`data` interface over `\u003cstate dir\u003e/plugin-data/\u003cid\u003e/`: names match a fixed pattern; one locked byte and name counter per plugin shared by every store (interactive and worker) and charged BEFORE each write grows anything."
    required: true
    manual: false
  - id: "C2"
    text: "Declared `background.data` quota with host ceiling, prompt line and escalation; counter rebuilt from disk at load (a crash-left file is counted)."
    required: true
    manual: false
  - id: "C3"
    text: "New tests in kiln/crates/kiln-plugin/tests/data_store.rs: `data_write_past_quota_is_refused_whole_and_leaves_counter`, `data_job_writing_in_a_loop_stops_at_quota`, `data_two_workers_and_interactive_never_pass_quota_together`, `data_replace_is_charged_before_the_rename`, `data_crash_left_file_counted_at_load`, `data_lowered_quota_allows_reads_and_deletes_but_not_growth`, `data_names_outside_pattern_refused`, `data_other_plugins_files_unreachable`, `data_clear_during_a_write`, `data_deleted_on_uninstall`."
    required: true
    manual: false
  - id: "C4"
    text: "Clear Data command and deletion on uninstall are wired in crates/fe-workbench/src/plugin_manage.rs with tests in plugin_manage_tests.rs."
    required: true
    manual: false
  - id: "C5"
    text: "Writes use `platform::replace_file` from P9."
    required: true
    manual: false
---

# Plugin data store: quota-charged files under plugin-data/<id>/ (B7, 3.2)

# Plugin data store: quota-charged files under plugin-data/<id>/ (B7, 3.2)

Draft id B7 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Section 3.2 data store. Codex rule: quota before growth (charge, write, refund on failure), never write then count.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.
