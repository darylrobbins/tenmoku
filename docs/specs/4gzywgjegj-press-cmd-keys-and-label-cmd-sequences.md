---
task_uid: "01m6kjmv2n0nqt704gzywgjegj"
task: "T1380"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "setup_wizard_tests::workbench_commands_wait passes on macOS and Linux: it presses cmd-p cmd-shift-p cmd-w while the sheet shows (no palette, no tab closed), secondary-q still runs app.quit, and cmd-p after escape opens the palette"
    required: true
    manual: false
  - id: "C2"
    text: "setup_wizard_tests::ready_counts_and_shortcuts passes on macOS and Linux: the Settings, Command Palette, Go to File and Open Folder hints are expected as label(\"cmd-,\"), label(\"cmd-shift-p\"), label(\"cmd-p\") and label(\"cmd-o\")"
    required: true
    manual: false
  - id: "C3"
    text: "rg -n 'secondary-[^q]' crates/fe-workbench/src/setup_wizard_tests.rs prints nothing; the label() helper's doc says the tests' keymap is fe_keymap::DEFAULT_MACOS on every host"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-workbench --lib setup_wizard_tests passes (all 9)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Press cmd- keys and label cmd- sequences in the setup wizard tests, which run on the macOS keymap on every host, so workbench_commands_wait and ready_counts_and_shortcuts pass on Linux (neon)

## Problem

On Linux (neon, at mac/main d28a66b1), setup_wizard_tests::workbench_commands_wait fails at :431
(`assert!(s.palette_shows())` after `escape`, `secondary-p`) and ready_counts_and_shortcuts fails at :579
(the "Change any of them in Settings (...)" hint). The tests' keymap is `fe_gpui::default_keymap()`, which is
`fe_keymap::DEFAULT_MACOS` on every host. On Linux, though, `secondary-` means ctrl to both
`simulate_keystrokes` and `KeySequence::parse`. So ctrl-p is unbound there, and the production hint (from the
bound `cmd-,`, labelled "Super+,") differs from the test's expected `label("secondary-,")` ("Ctrl+,").
Production is right: the app loads the keymap for the platform it runs on.

## Change

Test-only, in crates/fe-workbench/src/setup_wizard_tests.rs, as other fe-workbench tests write keys
(`cmd-p`, `cmd-shift-p`, `cmd-,`):
- line 390: `secondary-p secondary-shift-p secondary-w` becomes `cmd-p cmd-shift-p cmd-w`
- line 430: `secondary-p` becomes `cmd-p`
- lines 578-583: `label("secondary-…")` becomes `label("cmd-…")`
- keep `secondary-q` (line 422): the sheet's own on_key checks `modifiers.secondary()`, not the keymap
- update the `label()` doc comment to say the keymap is DEFAULT_MACOS on every host

No production change, no regression test.
