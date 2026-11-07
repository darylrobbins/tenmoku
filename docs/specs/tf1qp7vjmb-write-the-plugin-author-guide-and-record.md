---
task_uid: "01m6j04wx8atshdxtf1qp7vjmb"
task: "T1352"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "guide_matches_the_code: docs/plugins/authoring.md names extensions.plugins_preview, states the package ids and versions the WIT package lines have, gives limits equal to kiln_plugin::limits, lists only capabilities that parse, points at a template path that exists, and the decision record carries the 2026-11-03 decisions (crates/tenmoku-plugin/tests/guide.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p tenmoku-plugin and cargo clippy -p tenmoku-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Write the plugin author guide and record the 2026-11-03 decisions: plugins as a preview behind extensions.plugins_preview, unstable WIT at 1.0.0, local unsigned installs, no Lua layer

## Problem

There is no author documentation, and the decision record predates the user's 2026-11-03 decisions
for the first release.

## Change

- `docs/plugins/authoring.md`, the author guide:
  - **Status.** A preview: off by default behind `extensions.plugins_preview`; declarative
    extensions (themes, keymaps, menus, language servers, grammars, snippets) are the supported path.
    The WIT is unstable: `kiln:extension@1.0.0` and `fastedit:editor@1.0.0` may change incompatibly
    in any release until declared stable; rebuild against each release. The package keeps its
    pre-rename name `fastedit:editor` during the preview.
  - **Quick start** from the template (`crates/tenmoku-plugin/template/`): copy it, rename the id,
    `build.zsh`, then install with Extensions: Install Folder for Development….
  - **Manifest**: `[contributes.plugin]` (`wasm`, `capabilities`, the reserved unverified
    `signature`), `[engine] tenmoku = "1.1"`, user directory only, never a workspace's.
  - **Capabilities and prompts**: each capability in plain words, the combination warnings, the
    full-access wildcards and their second acceptance, escalation on update, revoking and
    uninstalling (secrets deleted).
  - **Trust**: local, unsigned installs; the grant prompt is the gate; no registry or signing yet.
  - **Limits**: the call deadline, memory, event queue and read limit, and that an overrun unloads
    the plugin.
  - **API tour** by interface, the SDK's `Plugin` trait and `export_plugin!`, logs (Extensions: Show
    Plugin Log), and installing from an archive.
- `docs/decisions/vmtnmkay62-extensions-and-plugins.md`: a "Stage 2 decisions (user, 2026-11-03)"
  subsection (plugins ship as a preview behind `extensions.plugins_preview`; the WIT is unstable at
  @1.0.0 until language-feature and UI imports exist; trust for the preview is local unsigned
  installs plus the grant prompts; no separate Lua or scripting layer, which settles the mlua
  option), a preview note under "Versioning", and §10 brought up to date.
- `crates/tenmoku-plugin/tests/guide.rs`: the guide names `extensions.plugins_preview`; the package
  ids and versions it states match the `package` lines of the WIT files; its limits equal
  `kiln_plugin::limits` (`CALL_DEADLINE`, `MEMORY_BYTES`, `EVENT_QUEUE`, `MAX_READ_BYTES`); each
  capability it lists parses; the template path exists; the record carries the 2026-11-03 decisions.

## Notes

Later cards (diagnostics, formatting, the wider command allowlist) each add their section to the
guide. Host-independent.

Project rules: keep tests lean and fast; new tests go in new files; the scope lists exact files.
