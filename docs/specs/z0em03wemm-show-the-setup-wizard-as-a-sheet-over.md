---
task_uid: "01m6hxekgxq3tfvyz0em03wemm"
task: "T1341"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "setup_wizard_tests::shows_over_the_workbench: show_setup puts the sheet on Appearance with focus inside it, is_setup_showing() is true, and a click on the scrim reaches no pane"
    required: true
    manual: false
  - id: "C2"
    text: "setup_wizard_tests::skip_and_escape_record_skipped: the Skip setup button and Escape each close the sheet, return focus to the active item, and leave \u003cstate\u003e/setup.toml with outcome skipped; choices made before stay in settings.toml"
    required: true
    manual: false
  - id: "C3"
    text: "setup_wizard_tests::finish_records_completed: Continue through every page and Start Editing closes the sheet and records completed"
    required: true
    manual: false
  - id: "C4"
    text: "setup_wizard_tests::keyboard_only_walkthrough: with simulate_keystrokes only (tab, shift-tab, arrows, space, enter), choosing Vim, LazyVim, relative line numbers and format on save writes vim.enabled = true, vim.preset = \"lazyvim\", editor.line_numbers = \"relative\" and editor.format_on_save = true, each before the next page"
    required: true
    manual: false
  - id: "C5"
    text: "setup_wizard_tests::workbench_commands_wait: while the sheet shows, cmd-p, cmd-shift-p and cmd-w and a RunCommand for workbench.quick_open open no palette and close no tab; secondary-q still runs app.quit"
    required: true
    manual: false
  - id: "C6"
    text: "setup_wizard_tests::write_failure_toasts: a settings.toml that can't be written gives an error toast naming the key, and the page stays"
    required: true
    manual: false
  - id: "C7"
    text: "setup_wizard_tests::labels_and_roles: each page's (role, label) list has a Dialog labelled 'Set Up Tenmoku, step N of 5, \u003ctitle\u003e', a radio group per choice group with radio buttons labelled from the model, switches and buttons with non-empty labels"
    required: true
    manual: false
  - id: "C8"
    text: "setup_wizard_tests::ready_counts_and_shortcuts: the Ready page says how many settings the run changed and shows Settings, Command Palette, Go to File and Open Folder with the active keymap's labels"
    required: true
    manual: false
  - id: "C9"
    text: "rg -n 'rgb\\(|rgba\\(|hsla\\(|gpui::(white|black)\\(' crates/fe-workbench/src/setup_wizard.rs finds nothing: colours come from theme tokens"
    required: true
    manual: false
  - id: "C10"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Show the setup wizard as a sheet over the window: five pages with Skip setup on each, choices written through to settings.toml and applied live, keyboard-only, with VoiceOver roles and labels

## Problem

The first-run setup wizard's model (steps, choices, writes) and its first-run marker exist; nothing shows
them. This card draws the wizard as a sheet over a workbench window. Nothing opens it yet except
`Workbench::show_setup` (the launch and the command are a later card), so it ships dark.

## Design (docs/design-language.md)

- **A sheet in the window, not a tab and not a palette.** T1307 removed the welcome tab at the user's request;
  this must not bring a tab back. kiln's `ModalLayer` dismisses its modal when focus leaves or the window
  deactivates, which a five-page flow can't survive, and a separate window can fall behind the workbench. So
  the `Workbench` owns an `Option<Entity<SetupWizard>>` and renders it last in `Workbench::render`, over the
  panes and toasts.
- **Scrim:** the whole window in `surface.overlay`, occluding the mouse. **Card:** 720 wide (the window's
  width less 48 if narrower), a fixed height so pages don't jump, `surface.raised`, 1 px `border.default`,
  `rounded_md`, `shadow_lg`, 24 padding, gaps from the 4/8/12/16/24 scale. Header: the step's title at 15
  semibold (the settings heading's role) and a one-line subtitle at 12 in `text.muted`. Footer: "Skip setup"
  (ghost button) at the left on every page; progress dots in the middle (6 px, `rounded_full`, `accent.base`
  for the current step, `border.default` otherwise); Back (not on the first page) and Continue (primary,
  kiln `ButtonSize` large) at the right; the last page's primary button is "Start Editing". kiln's
  `Button`, `Switch` and controls throughout; choice groups as segmented buttons, the chosen one in
  `accent.subtle` with a `border.focused` edge. Colours only from tokens: no colour literals.
- **No motion** (the design language has none): a page changes in one frame.
- Copy: sentence case, short, no exclamation marks. Ready page: "You changed N settings" (or "Tenmoku's
  defaults are set" with none), "Change any of them in Settings (<shortcut>)", and three hints with the
  active keymap's labels (`fe_keymap::shortcut_label`): Command Palette, Go to File, Open Folder.

## Change

New `crates/fe-workbench/src/setup_wizard.rs` (`SetupWizard`, a view over the model's `Choices`), declared in
`crates/fe-workbench/src/lib.rs` (drop the model's `cfg_attr(... dead_code)` once it's used). In
`crates/fe-workbench/src/workbench.rs`: the field, the overlay in `render`, `pub fn show_setup(&mut self,
window, cx)`, `pub fn is_setup_showing(&self) -> bool`.

- All five pages render here from the model's controls: segmented choices, switches, and the font size as a
  stepper (minus, value, plus). Appearance's themes show as a plain list of titles and Font's families as a
  list here; the next card replaces those two pages' bodies with theme tiles and a live code sample.
- **Write-through.** Each choice is written at once with `fe_config::write_settings` (the model's
  `choose`), off the main thread, each write after the one before (this view's own chain), so the config
  watcher's reload applies it to the whole window, the sheet included, within the M5 200 ms. Skip keeps what
  was already chosen; the Skip button's tooltip says so ("Keeps what you've chosen so far"). A failed write shows an error toast, as the Settings editor's
  does, and the page stays.
- The paths: the user's `settings.toml` (`fe_config::user_settings_path`) and state dir
  (`fe_config::paths::state_dir`), unless a test global sets them; under `cfg(test)` none unless set (the
  `ChatStateDir` pattern in `chat_panel.rs`), so no test touches the user's files.
- **Finish and Skip** both close the sheet, return focus to what had it, and record the outcome with
  `fe_config::first_run::record` (`completed` / `skipped`), off the main thread.
- **Keys, keyboard only:** the sheet's focus carries its own key context, `setup_wizard`, with
  `kiln::platform::keys::HOLDS_ALL_KEYS`, and reads keys in its own `on_key_down` (no new keymap rows, so none
  of fe-gpui's pinned binding-context lists change). Tab / shift-Tab move between controls with a
  `state.focus_ring` ring; arrows move within a choice group and choose; Space toggles a switch or presses the
  focused button; Enter presses the focused button, else Continue; Escape is Skip setup. secondary-q still
  quits; other platform-modifier keys do nothing while the sheet shows. While it shows, `on_run_command`
  ignores every command but `app.*` and `window.*`, so a menu item can't open a palette under the sheet.
- **VoiceOver** (docs/accessibility-audit.md: nothing has roles yet; this surface should): the card is
  `.id()` + `.role(Dialog)` labelled "Set Up Tenmoku, step N of 5, <title>"; each choice group a radio group
  of radio buttons with `aria_selected`; switches with `aria_toggled`; buttons with their label; every label
  from the model's text. The headless test window builds no a11y tree, so tests check the labels the view
  passes (a `#[cfg(test)]` list of (role, label) per page is fine); the human check card covers VoiceOver.

Tests in new `crates/fe-workbench/src/setup_wizard_tests.rs`, declared `#[cfg(test)] mod setup_wizard_tests;`
in `lib.rs`.

## Notes

- Reading back a write in tests: `SettingsStore::load(Some(user), None)` then `store.reload(&[user])` gives
  the event for `Workbench::reload_settings` (the auto-save tests do this).
- Focus events fire only at a draw in an active window: `window.activate_window()` first.

## Implementation notes

- **Menu picks skip `on_run_command`.** `dispatch_invocation` sends a typed command as its own action, so the
  gate in `on_run_command` alone doesn't stop a menu pick of `quick_open.toggle`. While the sheet shows,
  `Workbench::render` also leaves off `Self::attach`'s handlers. The typed `app.*` and `window.*` commands
  on the workbench (`window.new`, `app.check_for_updates`, the shell command pair) therefore do nothing
  from a menu until the sheet closes. `app.quit`, `window.close` and the other registry ones still run.
- **`workbench.quick_open` isn't a command.** Go to File is `quick_open.toggle`. The C5 test runs both names,
  as a `RunCommand`, and the real one as a menu pick does too.
- **A failed write goes back.** Beyond the toast, the control returns to what the file holds
  (`Choices::adopt`, from a second `Choices` of what has landed), unless a later write of that key is
  waiting, so the Ready page's count stays true.
- **Focus stays in the sheet.** Focus taken from outside (an open from another process) comes back at the
  next draw, unless a prompt in the window has it, and `Workbench`'s `Focusable` answers with the sheet's
  handle while it shows.
- **Toasts sit under the scrim**, as the design asks: an error toast is dimmed, and can't be dismissed until
  the sheet closes. It stays until dismissed, so it's still there then.
- `Choices::summary` is now test-only: the Ready page has its own copy, with the shortcut, and
  `setup_wizard_model_tests.rs`, which uses it, is outside this card's scope.
