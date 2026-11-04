---
task_uid: "01m6enxsqrz3xcgyp0cnw3dafp"
task: "T1313"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "shell_command::tests::install_links_a_bundle_binary_path_with_spaces_and_creates_the_directory (temp HOME, target Contents/MacOS/tenmoku)"
    required: true
    manual: false
  - id: "C2"
    text: "shell_command::tests::install_replaces_our_own_symlink_but_refuses_any_other_file_and_names_it"
    required: true
    manual: false
  - id: "C3"
    text: "shell_command::tests::uninstall_removes_only_a_symlink_that_points_at_a_tenmoku_binary"
    required: true
    manual: false
  - id: "C4"
    text: "shell_command::tests::path_hint_names_the_export_line_only_when_the_directory_is_not_on_path"
    required: true
    manual: false
  - id: "C5"
    text: "shell_command_menu::macos_app_menu_has_the_install_item_after_keyboard_shortcuts_json_and_linux_file_menu_beside_settings"
    required: true
    manual: false
  - id: "C6"
    text: "cargo clippy --workspace --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Add 'Install tenmoku Command in PATH' and an uninstall item that link ~/.local/bin/tenmoku to the running binary

## Problem
The `tenmoku` binary is `~/Applications/Tenmoku.app/Contents/MacOS/tenmoku` (`scripts/bundle-macos.sh` copies it there); nothing puts it on PATH, so the launcher cards are unreachable for a bundle user. VS Code's "Install 'code' command in PATH" is the model; the user approved a Tenmoku equivalent on 2026-11-02.

## Change
Add commands `app.install_shell_command` ("Install 'tenmoku' Command in PATH") and `app.uninstall_shell_command` ("Uninstall 'tenmoku' Command from PATH"), as macOS items in the Tenmoku app menu after Keyboard Shortcuts (JSON) in `crates/fe-keymap/menus/fastedit.toml`; on Linux they go in File beside Settings… (the build script's `platform` handling; update `crates/fe-keymap/tests/menu.rs`, which lists these exactly). Logic in new `crates/fastedit/src/shell_command.rs`: the target is `std::env::current_exe()` canonicalised (inside the bundle this is `Contents/MacOS/tenmoku`, wherever the .app lives); the link is `~/.local/bin/tenmoku` (no admin prompt; created with its parent dir). Replace an existing symlink that points at a Tenmoku binary silently; refuse and report any other existing file; uninstall removes only a symlink that points at a tenmoku binary. Afterwards a notification names the link and target, and warns with the exact `export PATH` line when `~/.local/bin` is not on `$PATH`. Run off the main thread. Behind `cfg(unix)`; Windows is out of scope.
