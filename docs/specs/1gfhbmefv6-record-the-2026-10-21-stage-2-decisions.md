---
task_uid: "01m5gdw9s6p111vk1gfhbmefv6"
task: "T273"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "rg -c \"^### Stage 2 decisions \\(user, 2026-10-21\\)\" docs/decisions/vmtnmkay62-extensions-and-plugins.md prints 1, and the subsection sits inside section 7 (before the next ## heading)"
    required: true
    manual: false
  - id: "C2"
    text: "the subsection states: uninstalling deletes per-plugin secrets and the confirmation says so; macOS and Linux only; AI chat stays built in; panels deferred; Q9 and Q10 deferred with a reserved signature slot; the P8 allowlist with default-deny; the four default constants (100 ms, 64 MB, 256 events, 16 MB); guest fixtures checked in, CI never builds guests (rg -c on \"secrets\", \"Windows\", \"signature\", \"256\", \"never builds\" each \u003e= 1 in section 7)"
    required: true
    manual: false
  - id: "C3"
    text: "section 10 lists the stage-2 series by subject and no longer says stage 2 is drafted after sign-off (rg -c \"drafted after sign-off\" is 0)"
    required: true
    manual: false
  - id: "C4"
    text: "git diff --stat shows only docs/decisions/vmtnmkay62-extensions-and-plugins.md"
    required: true
    manual: false
---

# Record the 2026-10-21 stage-2 decisions in the extensions decision record and list the stage-2 card series in section 10 (Mac T1044)

## Problem

The user approved stage 2 of the extensions record (`docs/decisions/vmtnmkay62-extensions-and-plugins.md`
§7: code plugins as WebAssembly components on wasmtime) on 2026-10-21, with decisions the record does not
yet state, and §10 still says stage 2 is to be drafted. The card series that follows (P1 onwards) cites
these decisions, so they belong in the record first.

## Change

Only `docs/decisions/vmtnmkay62-extensions-and-plugins.md`, after T1043 (which edits the same file):

- In §7, after the "Security decisions" subsection and before "VS Code extension risks", add
  `### Stage 2 decisions (user, 2026-10-21)` with these bullets:
  - Uninstalling a plugin deletes its per-plugin secrets (decision 5's store), and the uninstall
    confirmation says so.
  - Targets: macOS and Linux only; Windows is not a target now.
  - The AI chat stays built in; moving it to a plugin is out of the stage-2 series. Panels are deferred.
  - Signing, a registry and auto-update (Q9) and the admin `allowed_licenses` lock (Q10) are deferred; the
    manifest reserves a `signature` slot that nothing verifies yet.
  - Command authority: a small allowlist of existing commands is annotated plugin-callable (read-only and
    navigation commands, plus `editor.insert` and `editor.newline` with `buffer.write`); every other command
    is refused to plugins by default.
  - Defaults, as named constants that settings may override later: 100 ms epoch deadline per call, 64 MB of
    memory per plugin, 256 queued events per plugin, 16 MB for one buffer read.
  - Guest fixtures are prebuilt `.wasm` files checked in beside their Rust or WAT source and a build
    script; CI never builds guests, and `.wat` fixtures (compiled by wasmtime's `wat` feature in tests) are
    preferred wherever they suffice.
  - Plugins run on their own wasmtime `Engine` (tree-sitter's lives inside its C API), sharing the one
    wasmtime crate version through feature unification.
- In §10, replace the stage-2 sentence with the stage-2 card series, by subject and in order: crate and
  dependency; WIT worlds; instance runner with limits; hostile-plugin suite; plugin manifest kind; grant store
  and escalation; install prompt and combination warnings; host-side dispatch authorization; commands;
  events; editor read and edit; decorations, gutter and status items; filesystem and process spawn;
  network; per-plugin secrets; versioning adapters; compiled-component cache; running granted plugins in the
  app; management surface (list, uninstall, revoke). Do not write task handles; they are added when filed.

## Notes

Docs only: no code, no `Cargo.lock`.
