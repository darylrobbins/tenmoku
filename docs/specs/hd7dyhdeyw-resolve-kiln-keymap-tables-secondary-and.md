---
task_uid: "01m519tmq80zezt3hd7dyhdeyw"
task: "T102"
rev: 4
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "typed_tables.rs: a new test resolves one kiln::keymap! table for macOS explicitly and asserts secondary binds cmd and the macos entry wins, and for Linux asserts ctrl and the linux entry; it runs on Linux"
    required: true
    manual: false
  - id: "C2"
    text: "keys_bind_by_platform_and_in_a_context_in_place_of_the_attribute_keys, menu_items_show_the_shortcuts_the_keys_table_gives and the_apps_keymap_goes_over_the_table keep their names and pass, with expectations derived from the platform the table resolves against instead of hard-coded Linux keys"
    required: true
    manual: false
  - id: "C3"
    text: "At runtime a keys table resolves against the platform the app runs on, the same way the #[kiln::command(keys = ..)] attribute path does"
    required: true
    manual: false
---

# Resolve kiln::keymap! tables' secondary and per-platform keys against an explicit platform, so they bind cmd on macOS, with a macOS resolution test that runs on Linux (Mac T681 macOS fix)

Filed on argon from the Mac lead's note zk (2026-10-15): Mac host stack 10 failed its macOS gate on batch 15. This fixes argon T91 (Mac T681) on macOS. Base: argon trunk 6a499d8 (= mac/main b39ea29 + batch 15). Title the host commit "... (Mac T681 macOS fix)".

## Problem

On macOS, three kiln tests in `kiln/crates/kiln/tests/typed_tables.rs` fail:
`keys_bind_by_platform_and_in_a_context_in_place_of_the_attribute_keys`,
`menu_items_show_the_shortcuts_the_keys_table_gives` and `the_apps_keymap_goes_over_the_table`.
At typed_tables.rs:120 the left side is `[("ctrl-shift-n", None)]` and the right is `[("cmd-shift-n", None)]`; another
assertion gets `None` where `"typed_tables.archive"` is expected. A `kiln::keymap!` table binds `secondary` as ctrl on
macOS instead of cmd. Something on the table path (the `kiln::keymap!`/`__keys!` expansion in
`kiln/crates/kiln/src/tables.rs`, `Keys`, or `AppBuilder::keys` in app_builder.rs) picks the platform wrongly, or
fixes it at the wrong time (a `cfg!` evaluated in the wrong crate, or no platform passed where the attribute-keys path
passes one). The tests' own Linux-only expectations (for example `f2` for `{ macos = .., linux = "f2" }`) must also hold
on every platform: where an expectation is platform-specific, derive it from the same platform the table resolves
against, don't hard-code Linux.

## Change

- Make a keys table resolve `secondary` and `{ macos = .., linux = .., default = .. }` keys against an explicit
  platform, the same way the `#[kiln::command(keys = ..)]` attribute path does, so the choice can be tested on Linux.
  At runtime it is the platform the app runs on.
- Add a test in typed_tables.rs that resolves the same table for macOS (explicitly) and asserts `secondary` gives cmd
  and the `macos` entry wins, and for Linux gives ctrl and the `linux` entry. It runs on Linux, so argon catches this.
- Make the three failing tests platform-correct (on macOS they expect cmd and the macos keys), keeping their names
  and what they check.

## Out of scope

The attribute keys path, unless it shares the bug. No change to kiln-command's key parsing beyond what the explicit
platform needs.
