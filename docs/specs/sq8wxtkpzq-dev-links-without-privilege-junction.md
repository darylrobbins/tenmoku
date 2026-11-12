---
task_uid: "01m76wrjmfre20cjsq8wxtkpzq"
task: "T589"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "`plugin_dev.rs` falls back to a directory junction when `symlink_dir` fails with error 1314 (ERROR_PRIVILEGE_NOT_HELD): dev links fall back to `cmd /c mklink /J` with quoted arguments; a directory name containing %, ^ or ! is refused with a clear message (fail closed, no cmd expansion). Known limit: making the junction through Win32 so those names work is Mac follow-up T1727."
    required: true
    manual: false
  - id: "C2"
    text: "A seam forces the fallback in tests; new test `dev_link_falls_back_to_a_junction` in plugin_dev_tests.rs runs on the admin CI runner and on Linux (the seam stands in for the junction on Unix)."
    required: true
    manual: false
  - id: "C3"
    text: "`linked_from`, `links_to` and uninstall's remove-only-the-link accept a junction: tests `uninstall_removes_a_junction_link_only` and `links_to_recognises_a_junction` in plugin_manage_tests.rs; the link target's files survive uninstall."
    required: true
    manual: false
  - id: "C4"
    text: "Existing plugin_dev and plugin_manage tests pass unchanged."
    required: true
    manual: false
---

# Dev links without privilege: junction fallback on Windows (P8, W-f) (Mac T1692)

# Dev links without privilege: junction fallback on Windows (P8, W-f)

Draft id P8 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Section 2.2 row W-f. Junctions work on local volumes only; say so in the failure message when the target is on a network volume.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.

## Implementation notes

- The junction is made by `cmd /D /C mklink /J` with raw, quoted arguments; a path holding `%`, `"`, `^` or `!` is refused (`cmd_unsafe`). A Win32 reparse-point call would need extra `windows-sys` features in `crates/fe-workbench/Cargo.toml`, outside this scope; a follow-up can swap it in behind `make_junction`.
- A network target (plain or verbatim UNC) gets the local-volumes-only message when the junction fails.
