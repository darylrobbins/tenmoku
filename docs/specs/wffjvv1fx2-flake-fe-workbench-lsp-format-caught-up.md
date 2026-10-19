---
task_uid: "01m5ax5t32qzs81rwffjvv1fx2"
task: "T1011"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "caught_up_options_and_one_step passes 30 consecutive runs (cargo nextest --stress-count 30) with 8 busy-loop processes competing for CPU"
    required: true
    manual: false
  - id: "C2"
    text: "It never ends with a task or pending reply still holding the EditorView: the test drains or awaits the last lsp.format_selection request (the RANGE reply) before it returns, so gpui reports no leaked handles"
    required: true
    manual: false
---

# Flake: fe-workbench lsp_format caught_up_options_and_one_step panics with a leaked EditorView handle (gpui entity_map) on a loaded host

Main a65a5fe2 fast CI run 01m5atq3m0dz9ytxzz6eq4m2tm: fe-workbench lsp_format::tests::caught_up_options_and_one_step panicked at gpui entity_map.rs:1146 "Exited with leaked handles: Leaked handle for entity fe_gpui::view::EditorView". Host load 40-60. The test is old (T475) and unchanged by stack 39; it passed 30/30 on main under 8 busy loops, so this is a timing race.

Likely cause: the last step runs lsp.format_selection, waits only for the RANGE request to arrive (s.until "the range asked"), asserts, and returns while the format task (holding an EditorView clone) is still in flight. Under load the task has not finished when the test drops. Fix in the test: wait for the format to finish (reply applied / no pending task) before returning. Do not touch product code unless the wait shows a real leak.
