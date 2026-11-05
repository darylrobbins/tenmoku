---
task_uid: "01m6j8zm7jez2xxbpe8wxcn7td"
task: "T1379"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T1009

Findings of the review of T1009 that don't block the merge (below merge.block_severity):

- F2 medium kiln/crates/kiln/tests/support/platform_keymaps.rs:111: Platform-specific exceptions still discard context, arguments and layer when comparing unmatched bindings. F10-to-F9 is now detected, but restricting F10 to context="picker" still passes, despite disabling the window-wide shortcut. The substituted delete-word bindings likewise pass with mismatched contexts between platforms, so the requested complete-identity comparison remains incomplete.
  fix: Normalize the explicitly permitted key substitutions while retaining all other identity fields, then compare complete bindings. Assert F10's complete expected identity separately and add mutation checks for incorrect exception contexts.
  instances: kiln/crates/kiln/tests/support/platform_keymaps.rs:62, kiln/crates/kiln/tests/support/platform_keymaps.rs:67, kiln/crates/kiln/tests/support/platform_keymaps.rs:111, kiln/crates/kiln/tests/default_keys.rs:403, kiln/crates/kiln/tests/gallery.rs:75, kiln/crates/kiln/tests/host.rs:126
