---
task_uid: "01m7ap1ny790f34d0rhwvdbwz6"
task: "T347"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "cargo nextest run -p fe-syntax compiles with the csharp feature on"
    required: true
    manual: false
---

# Enable fe-syntax csharp feature for workspace tests (unblocks T339)

In crates/fe-syntax/Cargo.toml, the self dev-dependency line
`fe-syntax = { path = ".", default-features = false, features = ["no-query-budget"] }`
gets `"csharp"` added, with the comment "`csharp` is on so the gate's workspace test run compiles the csharp tests." Run `cargo metadata` so Cargo.lock stays current. Nothing else changes. Needed first so T339's regression check base has the feature.
