---
task_uid: "01m72f65drv1g65djesc1efg5w"
task: "T564"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "`kiln_core::platform::system_path()` and `minimal_environment(os_env)` exist with one rule set per OS; the Windows set is exactly SystemRoot, windir, SystemDrive, USERPROFILE, APPDATA, LOCALAPPDATA, TEMP, TMP, PATHEXT, ProgramData, NUMBER_OF_PROCESSORS and PROCESSOR_ARCHITECTURE, with PATH = %SystemRoot%\\System32;%SystemRoot%;%SystemRoot%\\System32\\Wbem, names matched case-insensitively (`Path` is `PATH`)."
    required: true
    manual: false
  - id: "C2"
    text: "New tests in kiln/crates/kiln-plugin/tests/environment.rs, run on every OS by passing a given environment to each OS's rule function: `environment_unix_passes_path_home_lang_tmpdir_term_only`, `environment_windows_passes_systemroot_set_case_insensitively`, `environment_windows_path_is_system32_form`."
    required: true
    manual: false
  - id: "C3"
    text: "`ProgramIdentity` gains `interpreter: Option\u003cString\u003e`, set to `cmd.exe` for `.bat` and `.cmd`; `.exe` and `.com` stay None. Test `environment_bat_and_cmd_have_cmd_interpreter` in the same file."
    required: true
    manual: false
  - id: "C4"
    text: "kiln/crates/kiln-plugin/wit/process.wit's doc comment lists both environments and says 10 s, not 30 s, matching PROGRAM_DEADLINE."
    required: true
    manual: false
---

# Spawn environment per OS and batch-file interpreter flag for plugin programs (P3, W-a, W-b) (Mac T1688)

# Spawn environment per OS and batch-file interpreter flag for plugin programs (P3, W-a, W-b)

Draft id P3 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Section 2.2 rows W-a and W-b, kiln side. Q6 (batch files) follows the design's proposal: allowed but flagged as cmd.exe interpreters; the prompt wording is the next card (P3b), which waits for the i18n card T1560 that owns the same fe-workbench files.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.
