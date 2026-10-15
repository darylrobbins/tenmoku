---
task_uid: "01m4vbj528mda814gvsdnyy1e8"
task: "T674"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln tests/testing.rs: TestApp::open starts a kiln::app headless with temp config and state dirs (its explicit keymap, theme and settings paths remapped there) and Linux capabilities as far as testing.rs can set them, and press(secondary-shift-p) opens the command palette. Settings that Config hooks load from framework-managed paths, and a Linux capability override threaded through window options and every root at startup, are out of scope here: T718"
    required: true
    manual: false
  - id: "C2"
    text: "testing.rs: run_command and menu(menu, item) run an app command, workspace() returns the window's Workspace and active_item::\u003cT\u003e() the active item downcast"
    required: true
    manual: false
  - id: "C3"
    text: "testing.rs: assert_clean_config passes for a clean Config and panics listing each diagnostic with its OS and file for a binding that is wrong on one platform only"
    required: true
    manual: false
  - id: "C4"
    text: "kiln::testing exists only with the test-support feature; cargo check -p kiln without it succeeds"
    required: true
    manual: false
---

# Add kiln::testing: open a kiln::app headless, press keys, run commands and menus, reach the workspace typed, and check a config on every platform

## Problem

Part of the Kiln app layer the owner approved on 2026-10-13 (phase 1; design record
`docs/decisions/kiln-app-layer.md`). There is no framework-level test handle, so the host example exposes its
internals to its test through accessors marked `#[allow(dead_code, reason = "for tests/host.rs")]`
(`examples/host/app.rs:158-176`, `panel.rs:59`, `conversation.rs:81`), and `tests/host.rs:52-80` re-implements
setup, prompt-builder reset, window opening and drawing. Checking a keymap and menu table on every platform is
hand-written per app.

## Change

Add `kiln::testing` in a new `kiln/crates/kiln/src/testing.rs`, compiled only with kiln's existing
`test-support` feature (`kiln/crates/kiln/Cargo.toml`; add gpui's `test-support` to it if needed):

- `TestApp::open(builder: AppBuilder, cx: &mut TestAppContext) -> TestApp`: starts the app headless with temp
  config and state dirs (so no user file is read), Linux capabilities, the test platform's prompts (as
  `tests/host.rs:52-62` does), asserts `init` returned no diagnostics, and draws.
- On `TestApp`: `press("secondary-shift-p")` (keystrokes, then run until parked and draw), `draw()`,
  `run_command("chat.new_conversation")`, `menu("Conversation", "New Conversation")` (through
  `kiln_platform::menus::run`), `workspace() -> Entity<Workspace<ItemPane>>`, `active_item::<T>() ->
  Option<Entity<T>>`, `window()` and `cx()` for the `VisualTestContext`, as escape hatches.
- `assert_clean_config(&Config)`: parses its keymap and menus as on macOS, Linux and Windows
  (`Config::keymap_for`, `Config::menu_bar_for`) against `kiln::catalog` and panics listing every diagnostic
  with its OS and file. A `TestApp`-free form, for an app that keeps `kiln::init`.

New tests go in `kiln/crates/kiln/tests/testing.rs`.

## Scope note (lead, rev 2)

Review dy6crm's F1 (settings loaded by Config hooks still read user files) and F2 (window options and Root read Capabilities::current() directly) need changes in kiln-platform and kiln-shell, outside this card's four files. They are deferred to T718; this card delivers the isolation and capabilities testing.rs itself can apply.

## Implementation notes

- `assert_clean_config(&Config, &App)` takes the `App`: `kiln::catalog` reads the commands registered in it, so call it once `init` (or `TestApp::open`) has registered them.
- `TestApp::menu` finds the item by title or ID (`menus::run` itself takes the ID, which defaults to the command ID).
- `tests/testing.rs` has `required-features = ["test-support"]`.
