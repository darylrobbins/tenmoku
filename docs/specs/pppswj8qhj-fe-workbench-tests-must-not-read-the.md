---
task_uid: "01m6j81n8gz703hnpppswj8qhj"
task: "T1378"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "Regression test tests::user_config_hermetic::a_user_settings_file_in_the_environment_starts_no_server fails at base (a bare assert or assert_eq, no custom message) and passes after the fix"
    required: true
    manual: false
  - id: "C2"
    text: "With a user settings.toml that configures a gopls server in the developer config dir, the 5 chat_panel_tests (trusted_roots_attach_the_workspace_context, automatic_context_is_vetted_again_as_its_file_changes, automatic_context_skips_links_out_of_the_folder_and_to_secrets, revoking_trust_cuts_the_context_before_a_pending_vetting_ends, what_was_asked_under_an_earlier_context_is_dropped) pass"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench passes"
    required: true
    manual: false
  - id: "C4"
    text: "cargo clippy -p fe-workbench --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# fe-workbench tests must not read the real user config (settings.toml, keymap.toml, user dictionary): a developer config that configures a language server made 5 chat_panel_tests spawn it and panic

## Bug

fe-workbench unit tests read the developer's REAL ~/.config/tenmoku/settings.toml. A user-level '[lsp.servers.gopls] command = "gopls" languages = ["go"] root_markers = ["go.work","go.mod"]' made five chat_panel tests panic at gpui test_scheduler.rs:194 ("Detected activity on thread fe-lsp gopls ... not deterministic"), because a real gopls was spawned: chat_panel_tests context::trusted_roots_attach_the_workspace_context, context::automatic_context_is_vetted_again_as_its_file_changes, context::automatic_context_skips_links_out_of_the_folder_and_to_secrets, context::revoking_trust_cuts_the_context_before_a_pending_vetting_ends, apply::what_was_asked_under_an_earlier_context_is_dropped. It broke T1340's gate and would break main's post-check on any machine whose user config configures a server.

## Cause

Read path: Workbench (lsp.rs, new_lsp_manager and start_lsp_manager) sets lsp.user from settings_path(cx); crates/fe-workbench/src/open.rs settings_path falls through to fe_config::user_settings_path() (fe-config paths.rs: TENMOKU_CONFIG_DIR, then XDG_CONFIG_HOME/tenmoku, then HOME/.config/tenmoku) unless a test set the cfg(test) ConfigDirOverride global. The shared helper tests::open (src/tests/mod.rs) and most one-off helpers never set it, so the chat, layout and most other tests use the real file. shortcuts_editor.rs keymap_path has the same fall-through for the user keymap.toml. With no Launcher global installed the workbench uses LocalLauncher, so a configured server really starts.

## Fix

Make the test default hermetic by construction: under cfg(test), settings_path and keymap_path with no ConfigDirOverride global return None (no user config), instead of falling through to the process environment. Tests that want a user file already set ConfigDirOverride (or TENMOKU_CONFIG_DIR in a re-exec'd child); keep them working. Audit the other cfg(test) reads of the real dir in fe-workbench (settings_editor.rs files(), settings_editor_controls.rs Change::write, spell Dictionary::user) and route them the same way if a test can reach them without an override; do not widen beyond fe-workbench. Do not add a launcher guard: tests rely on lsp.local.

## Regression test

New file crates/fe-workbench/src/tests/user_config_hermetic.rs, declared from src/tests/mod.rs, base API only (tests::open_root, set_server_launcher, ServerLauncher, Launch, Workbench). One #[gpui::test]: create a temp config dir with settings.toml holding '[lsp.servers.bogus] command = "/nonexistent-should-not-run" languages = ["go"] root_markers = ["go.mod"]'; set TENMOKU_CONFIG_DIR to it (nextest runs each test in its own process, so setting the env var is safe; SAFETY comment as in tests/grammar_prompt.rs); install a recording ServerLauncher via set_server_launcher that records every spec.command and returns Err(io::ErrorKind::Other); open a temp project with go.mod and main.go via open_root, trust the root (set_root_trust(true)), open main.go, draw and run_until_parked; assert!(recorded.lock().unwrap().is_empty()) and assert_eq! on the recorded commands being an empty Vec. No custom assert messages (a base panic must start with "assertion"). At base the bogus server is launched, so it fails; after the fix nothing is.

## Also: the landing race this fix exposes (lead respec, rev 2)

With the real settings read gone, lsp::links::landing_tests::a_later_open_of_the_same_file_cancels_the_landing (crates/fe-workbench/src/lsp_links_landing_tests.rs) fails at gpui seeds 0 and 7: two follow() calls with no park between each spawn a background is_file check in open_link_file_from (crates/fe-workbench/src/lsp_links.rs), and the open whose check completes last bumps the ticket, so the order of two background tasks decides which open wins. That is a production bug: the later open must win however the checks finish. Fix it in lsp_links.rs by taking the ticket when the open starts (or otherwise ordering by start, not completion), so an earlier open whose check completes late cannot claim the landing. Do not change the test to park between follows. The test must pass at SEED=0 through SEED=9 (run it with each seed and say so in the finish note).
