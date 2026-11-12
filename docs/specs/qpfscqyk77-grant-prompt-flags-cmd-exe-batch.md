---
task_uid: "01m71syv5a0w6xncqpfscqyk77"
task: "T1689"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The grant prompt flags a program whose `ProgramIdentity.interpreter` is set ('runs through cmd.exe, which reads its arguments again') the way `sh` is flagged; test `prompt_flags_cmd_exe_interpreter` in crates/fe-workbench/src/plugin_prompt_tests.rs, and a plain `.exe` is not flagged."
    required: true
    manual: false
  - id: "C2"
    text: "`programs()` in crates/fe-workbench/src/plugin_host.rs falls back to `kiln_core::platform::system_path()` when PATH is unset; test `programs_fall_back_to_system_path_when_path_unset`."
    required: true
    manual: false
  - id: "C3"
    text: "The prompt text goes through the l10n catalogue the neighbouring prompt strings use (T1560 has moved them); the l10n lint passes."
    required: true
    manual: false
---

# Grant prompt flags cmd.exe batch programs; programs() falls back to system_path() (P3b, W-a, W-b)

# Grant prompt flags cmd.exe batch programs; programs() falls back to system_path() (P3b, W-a, W-b)

Draft id P3b of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Split from the design's P3 so the Windows kiln-level cards do not wait for the i18n card T1560, which owns plugin_host.rs and plugin_prompt.rs. Section 2.2 row W-b.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.
