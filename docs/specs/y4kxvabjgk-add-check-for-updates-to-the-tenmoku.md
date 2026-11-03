---
task_uid: "01m6enx70gbw7kn8y4kxvabjgk"
task: "T1309"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "update_check_tests::newer_release_is_reported: with a fake fetcher returning tag v0.2.0 and running 0.1.0, the toast says 0.2.0 is available and its action opens https://github.com/darylrobbins/fastedit/releases/tag/v0.2.0"
    required: true
    manual: false
  - id: "C2"
    text: "update_check_tests::same_or_older_is_up_to_date: tag v0.1.0 or v0.0.9 at running 0.1.0 toasts 'up to date'"
    required: true
    manual: false
  - id: "C3"
    text: "update_check_tests::pre_release_and_bad_tags: a draft or prerelease, or a tag that is not vMAJOR.MINOR.PATCH, is ignored and the toast says no release was found, never a panic"
    required: true
    manual: false
  - id: "C4"
    text: "update_check_tests::failure_is_a_toast: a fetcher error, a non-200 status (including 403 rate limit) and invalid JSON each toast a one-line reason with the HTTP status"
    required: true
    manual: false
  - id: "C5"
    text: "update_check_tests::fetch_runs_off_main_thread: the fetcher is called from cx.background_spawn, and a second invocation while one is in flight does not start another"
    required: true
    manual: false
  - id: "C6"
    text: "menu_check_for_updates::tenmoku_menu_has_item: the macOS default Tenmoku menu has 'Check for Updates…' bound to app.check_for_updates, and default_menus_parse still passes"
    required: true
    manual: false
  - id: "C7"
    text: "No test touches the network: the real fetcher is built only in the app, and every test injects a fake"
    required: true
    manual: false
  - id: "C8"
    text: "Cargo.lock gains no new [[package]]: fe-net (already a dependency) is the HTTP client"
    required: true
    manual: false
  - id: "C9"
    text: "cargo deny check licenses bans sources passes"
    required: true
    manual: false
---

# Add Check for Updates to the Tenmoku menu: compare the running version with the latest GitHub release and toast the result with a download link

## Problem
There is no way to tell from the editor whether a newer Tenmoku exists. Releases are published by `.github/workflows/release.yml` (tag `vX.Y.Z`, a GitHub Release in darylrobbins/fastedit with a zip plus SHA-256; see `docs/release/macos-signing.md`, "Releasing"). The user approved a minimal manual check on 2026-11-02 ("Yes please").

## Change
Add `crates/fe-workbench/src/update_check.rs` with a command `app.check_for_updates`, titled "Check for Updates…", registered in `workbench_commands.rs`.

1. Fetch `https://api.github.com/repos/darylrobbins/fastedit/releases/latest` with `fe-net::Client` (already a fe-workbench dependency): grant `net:api.github.com`; send a `User-Agent` and `Accept: application/vnd.github+json`; parse with `serde_json`; run in `cx.background_spawn`; one check at a time. fe-net does not follow redirects; this URL answers 200 directly.
2. Take `tag_name` and `html_url`. Strip the `v` and compare MAJOR.MINOR.PATCH numerically with `env!("CARGO_PKG_VERSION")` using a small hand-written parser (no new crate). Ignore drafts, prereleases and malformed tags.
3. Toast the outcome: newer → "Tenmoku X is available (you have Y)" with a "Download" action opening `html_url` via `cx.open_url`; same or older → up to date; no release found or any error → one readable line.
4. Hide the network behind a small trait or function value: the app wires the real fetcher, tests inject fakes.
5. Add the item to the macOS Tenmoku menu in `crates/fe-keymap/menus/fastedit.toml`, after Hide/Show All and before Settings… (or after the separator).

No auto-download, no background polling, no persisted state. Tests go in the new `update_check_tests.rs` and `crates/fe-keymap/tests/menu_check_for_updates.rs`; the catalog and id lists get one row each.
