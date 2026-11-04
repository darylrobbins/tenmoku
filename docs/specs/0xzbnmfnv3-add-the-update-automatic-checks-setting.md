---
task_uid: "01m6j99zfggp99fs0xzbnmfnv3"
task: "T441"
rev: 3
type: "feature"
complexity: "xs"
criteria:
  - id: "C1"
    text: "update_settings::update_defaults_to_automatic_checks passes"
    required: true
    manual: false
  - id: "C2"
    text: "update_settings::update_automatic_checks_false_parses passes"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-config passes and cargo clippy -p fe-config --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Add the update.automatic_checks setting (default true) for background update checks (Mac T1371)

## Problem

Tenmoku's automatic update check needs a user setting. Today nothing in `fe-config` mentions updates.

## Change

In `crates/fe-config/src/settings.rs` add `pub update: UpdateSettings` to `Settings` (`#[serde(default)]`, JsonSchema, documented) with `automatic_checks: bool`, default true: 'Check for a newer Tenmoku in the background once a day and offer to install it. Check for Updates… always works.' Default is true because Sparkle also asks the user on first launch before enabling its own schedule; this key is the user's off switch and the editor applies it (a later card). Keep the shipped settings schema/docs generation and any settings-key listing test green by updating whatever generated listing exists (find it with `rg -n 'zen_mode' crates --glob '!*.rs'`).

## Tests

New `crates/fe-config/tests/update_settings.rs`: `update_defaults_to_automatic_checks`, `update_automatic_checks_false_parses`, `unknown_update_key_is_reported` (follow how `zen_mode_settings.rs` asserts unknown keys, if it does).
