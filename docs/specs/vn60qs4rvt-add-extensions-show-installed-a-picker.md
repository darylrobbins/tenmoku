---
task_uid: "01m5ffwsh4vrffncvn60qs4rvt"
task: "T1039"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "extensions_picker_tests::rows_show_name_kinds_licence_source_and_state: with two user extensions loaded (one contributing a theme and an lsp server, one grammars), the picker has two rows with their name, id, version, kinds summary, licence expression and class, source User and state Active"
    required: true
    manual: false
  - id: "C2"
    text: "extensions_picker_tests::an_extension_waiting_on_consent_shows_it: an extension whose server is pending shows NeedsConsent with the server's effective invocation in its detail, a declined one shows Declined"
    required: true
    manual: false
  - id: "C3"
    text: "extensions_picker_tests::enter_on_a_waiting_row_opens_its_consent_prompt: Enter on a NeedsConsent row shows the consent prompt for that extension; Enter on an Active row shows none"
    required: true
    manual: false
  - id: "C4"
    text: "extensions_picker_tests::the_command_opens_the_picker: dispatching workbench.show_extensions opens it, and the id is in typed_command_ids_tests's list (the only edit to that file)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Add Extensions: Show Installed, a picker of installed extensions with their kinds, licence, source and state

## Problem

Nothing shows which extensions are installed, what they contribute, or why one's language server isn't
running. Extensions record §1: "shown at install and in the extensions list as its effective
invocation", §6: "The extensions list shows [the licence]".

## Change

New command `workbench.show_extensions` ("Extensions: Show Installed", palette only) opening a
`Picker` like `crates/fe-workbench/src/theme_picker.rs` (new file
`crates/fe-workbench/src/extensions_picker.rs`, with `#[cfg(test)] mod extensions_picker_tests;` and
the new file `crates/fe-workbench/src/extensions_picker_tests.rs`), registered in `workbench.rs`,
`workbench_commands.rs` (as `theme.select` is, :620) and `lib.rs`; add the id to the list in
`crates/fe-workbench/src/typed_command_ids_tests.rs`.

- Rows come from `kiln::extensions::loaded(cx)` (kiln's loaded manifests) and the gate's
  `pending_extension_consents`: name, id, version, a kinds summary (themes, keymaps, menus, lsp,
  grammars, snippets with counts), the licence expression with its class (permissive, copyleft,
  unknown), and the source. `enum ExtensionSource { User, Workspace }` and `enum ExtensionState {
  Active, NeedsConsent { servers }, Declined { servers }, Disabled, SkippedUntrusted }` exist from
  this card; this card produces only `User`, `Active`, `NeedsConsent` and `Declined`. The cards for
  workspace extensions and for disabling produce the others.
- Detail (secondary line or the picker's detail pane, as `ThemePicker` shows its own): each lsp
  server with its effective invocation and granted/pending/declined status.
- Enter on a row with `NeedsConsent` or `Declined` shows that extension's consent prompt (the
  prompt card's function). Enter on any other row does nothing. An extension that failed to load isn't
  a row; its diagnostics are in the config problems list (`workbench.show_config_problems`).

## Out of scope

Installing, removing, updating or toggling an extension from the list; a menu item or key; a rail
view.
