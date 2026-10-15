---
task_uid: "01m4yd6tmm55n8hx4p4gfy1s4s"
task: "T718"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln tests/testing_isolation.rs: with a settings file in a fake user config dir that a Config hook would load, TestApp::open never reads it (the app sees its defaults), and the test fails if isolation is removed"
    required: true
    manual: false
  - id: "C2"
    text: "testing_isolation.rs: on a macOS host, TestApp::open builds its window options and both the workspace and a custom root with Linux capabilities from startup (no native chrome, Linux keys), asserted on the options and the drawn chrome"
    required: true
    manual: false
  - id: "C3"
    text: "kiln's existing tests (tests/testing.rs, tests/host.rs, kiln-shell and kiln-platform suites) pass; a real app's startup still uses Capabilities::current(); cargo clippy -p kiln -p kiln-shell -p kiln-platform --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Isolate kiln::testing from user settings loaded by config hooks, and thread its Linux capabilities through window options and every root at startup (T674 review F1, F2)

## Problem

T674 review findings F1 and F2 (review dy6crm, candidate gjz7sa), deferred out of T674's scope by the lead:
`kiln::testing::TestApp::open` remaps the explicit keymap, theme and settings paths into temp dirs, but

- **F1, user-file isolation:** settings that `Config` hooks load from framework-managed paths
  (`kiln/crates/kiln-platform/src/config.rs`, `kiln/crates/kiln/src/setup.rs`) are still read from the real
  user config dir before the test's temp dirs apply, so a test can see the developer's own settings;
- **F2, Linux capabilities:** `kiln_platform::window_options` and kiln-shell's `Root` call
  `Capabilities::current()` directly (`kiln/crates/kiln-platform/src/window_options.rs`,
  `kiln/crates/kiln-shell/src/root.rs`, `workspace.rs`), so the Linux capabilities TestApp promises apply only
  partially and after startup; window options and the custom-root chrome stay host-dependent.

## Change

- Apply TestApp's temp-dir isolation before any config hook can load framework-managed settings (defer or remap
  those loads to the test's config dir).
- Thread a startup capability override from `AppBuilder::start` (`kiln/crates/kiln/src/app_builder.rs`) through
  window-option construction and both the workspace and custom roots, and have `TestApp::open` pass Linux's.
- New tests in `kiln/crates/kiln/tests/testing_isolation.rs`.

## Implementation notes

- **F2.** `kiln_platform::Capabilities::of(cx)` is the app's capabilities: those `Capabilities::install` set as
  a GPUI global, else `Capabilities::current()`. `kiln::window_options`, `Root::new`, `Workspace::new` and
  `kiln_shell::init`'s prompt choice read it, so a window opened at any point follows the override.
  `Config::capabilities(caps)` has `init` install it first and use its OS for the keymap, menus,
  contributions and `TextField`'s keys. `TestApp::open` passes Linux's through its last config hook, so the
  post-open `linux_chrome` patch is gone, and `TestApp::press` maps `secondary` to the app's OS (`ctrl`).
  Without `Config::capabilities` nothing is installed and a real app gets `Capabilities::current()`
  (`an_app_started_without_test_app_is_this_platform_s`).
- **F1.** `Config::settings` now loads its store when `init` runs, not when called, and
  `Config::user_files_in(dir)` has `init` read every user file (settings, keymap, theme) from `dir` by its
  file name, whatever path a hook gave it. `TestApp::open` points it at its empty temp config dir. A store
  the app loaded itself (`Config::settings_store`) keeps the files it read, as documented.
- Left as they are: `kiln_core::shortcut_label` and `kiln::binding_label_for` still format labels for the
  host OS (kiln-core and ext.rs are outside this card's scope). The carried T673 F4 is in `config.rs`, which
  this change doesn't touch.
- Mutations checked locally: dropping `user_files_in`, the capabilities override, or `Capabilities::of` in
  window options, `Root`, `Workspace` or `init`'s OS each fails a test in `testing_isolation.rs`.
