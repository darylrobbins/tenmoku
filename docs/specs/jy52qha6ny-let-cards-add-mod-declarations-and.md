---
task_uid: "01m6sb0w6rj8kadxjy52qha6ny"
task: "T1423"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "regatta.toml [scopes] shared_append_only lists \"crates/*/src/lib.rs\" and \"crates/fe-workbench/src/workbench_commands.rs\" alongside every entry it had before"
    required: true
    manual: false
  - id: "C2"
    text: "rgt config check passes"
    required: true
    manual: false
---

# Let cards add mod declarations and command registrations without claiming the file: crates/*/src/lib.rs and fe-workbench workbench_commands.rs in scopes.shared_append_only

## Problem
Cards that each add a module or a command serialise on whole-file claims of the files where those lines go. On helium all 8 ready lent cards pairwise conflict because every one claims crates/fe-workbench/src/lib.rs (a `mod` line), and three also claim crates/fe-workbench/src/workbench_commands.rs (command registration). The daemon then runs one lane at a time on a 6-lane host.

## Change (user-approved 2026-11-06)
In regatta.toml, add two entries to `[scopes] shared_append_only`, keeping every existing entry (setting the key replaces Regatta's defaults):
- `"crates/*/src/lib.rs"`
- `"crates/fe-workbench/src/workbench_commands.rs"`
Extend the comment above the key by one line: module declarations and command registrations are added lines, so cards that add a module or a command run in parallel; editing an existing line is still a scope expansion.
Do not add crates/fe-config/src/settings.rs, settings.schema.json or settings-template.toml: adding a setting there usually edits existing lines (struct defaults, JSON commas).

No code changes. Scope is regatta.toml only.
