---
task_uid: "01m72f648gr272kxd3s8et72bd"
task: "T562"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "New test `capability_portable_spawn_names_refuse_separators` in kiln/crates/kiln-command/tests/capability_portable.rs: `process.spawn:tools/rg`, `process.spawn:tools\\rg` and `process.spawn:C:rg` fail to parse on every OS (the test is not cfg-gated), and `process.spawn:rg` still parses."
    required: true
    manual: false
  - id: "C2"
    text: "New test `capability_portable_pathext_name_is_a_warning` in the same file: `process.spawn:rg.exe` parses but yields the warning text 'write `rg`; the extension is found on Windows', and a name like `rg.sh` does not warn."
    required: true
    manual: false
  - id: "C3"
    text: "New test `extension_portable_paths_refuse_backslash` in kiln/crates/kiln-core/tests/extension_portable_paths.rs: a contributed file path containing `\\` is refused on every OS with the message 'use / in extension paths'; `a/b.wasm` is accepted."
    required: true
    manual: false
  - id: "C4"
    text: "A test in kiln/crates/kiln-plugin/tests/manifest.rs (`manifest_wasm_path_with_backslash_is_refused`): `wasm = \"components\\\\p.wasm\"` is refused with the same message on every OS."
    required: true
    manual: false
  - id: "C5"
    text: "`fs.read:workspace` and `net:\u003chost\u003e` capabilities parse exactly as before (existing capability tests unchanged and passing)."
    required: true
    manual: false
---

# Portable plugin names and paths: refuse backslashes and bad spawn names on every OS (P1, W-e) (Mac T1686)

# Portable plugin names and paths: refuse backslashes and bad spawn names on every OS (P1, W-e)

Draft id P1 of the plugins stage 3 plan (docs/decisions/dqsjfernkt-plugins-stage-3.md, section 6). The design is approved by the user (2026-11-09); questions Q1 to Q7 follow the design's proposals.

Section 2.2 row W-e of the design. Contributed paths (`wasm_file` in manifest.rs, `contributed_file` in kiln-core extension.rs) refuse `\` on every OS, so one manifest works everywhere. `parse_program` in kiln-command refuses `/`, `\` and `:` in `process.spawn:` names on every OS, and a name ending in a PATHEXT extension is a warning, not a refusal. Do not add cfg(windows) to these checks: they are pure string rules proven on Linux.

Read the design sections named above before starting. Do not widen --scope; if a file outside it is needed, stop and say so.
