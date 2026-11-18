---
task_uid: "01m7p9g1m416f18nd7t5rfkttd"
task: "T420"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::whats_new::the_command_opens_one_tab_newest_first passes: with fixture notes for 0.2.2, 0.2.3 and 0.2.4 and the running version 0.2.3 (the test seam), app.show_whats_new opens a What's New tab in the active pane, focused, drawing the 0.2.3 heading and notes before 0.2.2's and nothing of 0.2.4's; running it again activates that tab with no second one; with no file for the running version, the no-notes line shows above the earlier versions"
    required: true
    manual: false
  - id: "C2"
    text: "tests::whats_new::an_upgrade_opens_it_once passes: with a state dir holding setup.toml and whats_new.toml seen = \"0.2.2\" and the running version 0.2.3, offer_whats_new opens the tab in the window; whats_new.toml still reads 0.2.2 until the tab has drawn and 0.2.3 after; a second workbench offered from the same state dir opens nothing"
    required: true
    manual: false
  - id: "C3"
    text: "tests::whats_new::no_tab_on_a_new_machine_a_quiet_launch_or_a_downgrade passes: fresh state and config dirs (the wizard is offered): no tab, and whats_new.toml records the running version; SetupLaunch::Never: no tab and nothing written; no state dir: no tab; seen = \"0.3.0\": no tab, the file unchanged; a setup.toml with no whats_new.toml (an upgrade from a build before this card): the tab opens"
    required: true
    manual: false
  - id: "C4"
    text: "tests::whats_new::the_shipped_notes_are_well_formed passes: every embedded release-notes file is named X.Y.Z.md and holds only ## New, ## Improved and ## Fixed bullet sections, in that order, at least one, with no H1, image, raw HTML or ]]\u003e; release-notes/0.2.3.md is among them, curated from v0.2.2..v0.2.3"
    required: true
    manual: false
  - id: "C5"
    text: "crates/fe-config/tests/whats_new.rs passes: decide for a missing, unreadable, older, equal and newer seen version, and record writing whats_new.toml atomically and replacing an earlier one"
    required: true
    manual: false
  - id: "C6"
    text: "crates/fe-keymap/tests/menu_whats_new.rs passes: on macOS and Linux the last menu is Help, after Window, holding What's New, which runs app.show_whats_new; tests/menu.rs, tests/l10n.rs, fe-gpui's menu_tests and crates/fastedit/tests/menus.rs default_menus_know_every_command pass with Help added"
    required: true
    manual: false
  - id: "C7"
    text: "app.show_whats_new is in the palette as What's New with no default key; command_catalog_tests and typed_command_ids_tests pass with the id, and fe-workbench::l10n.ui_text_is_in_the_catalogue and fe-workbench::l10n.keys_used_here_exist pass with the new strings"
    required: true
    manual: false
  - id: "C8"
    text: "scripts/e2e-macos.sh --dry-run shows each scratch state folder getting a whats_new.toml with the bundle's version beside setup.toml"
    required: true
    manual: false
  - id: "C9"
    text: "Checked in the real Linux app (run-fastedit-linux) with a state dir whose whats_new.toml says seen = \"0.2.2\": the launch opens What's New showing 0.2.3's notes; quit and relaunch: no tab; Help \u003e What's New opens it again (screenshots in the decision file)"
    required: true
    manual: false
  - id: "C10"
    text: "cargo nextest run -p fe-workbench whats_new setup_launch command_catalog typed_command_ids passes, cargo nextest run -p fe-workbench --test l10n passes, cargo nextest run -p fe-config --test whats_new passes, cargo nextest run -p fe-keymap passes, cargo nextest run -p fe-gpui menu passes, cargo nextest run -p fastedit --test menus passes; cargo clippy -p fe-workbench -p fe-config -p fe-keymap -p fe-gpui -p fastedit --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# What's New tab: curated release notes (release-notes/<version>.md) built into the app, opened once on the first launch of a new version (not on a new install, written only after it has shown) and from Help > What's New and the palette (Mac T1946)

# What's New tab: curated release notes built in, opened once after an upgrade and from Help > What's New

User request (2026-11-17): "Let's add a card to implement Release Notes / What's New in the
app. Shall we use git-cliff? Or do we want more control over what's included? I'd like a
What's New pane in the app with the release notes. Pops up automatically when we first open a
new version and visible from the menu."

Decision: not git-cliff. The commits aren't conventional-commit style and many are internal
(flakes, lane plumbing, test seams), so the notes are curated by hand, one Markdown file per
release, `release-notes/<X.Y.Z>.md` at the repo root. That one file feeds the app (this card),
and the GitHub release body and Sparkle appcast (the follow-up card).

## Notes files

- `release-notes/<X.Y.Z>.md`, the version exactly as `[workspace.package] version` writes it.
  No H1: the tab adds the version heading. Only `## New`, `## Improved` and `## Fixed`
  sections, in that order, each optional but at least one, each a bullet list of short
  user-facing lines. No images, no raw HTML, no `]]>` (the appcast wraps the text in CDATA).
  Write these rules in the doc comment of the module that embeds the files.
- Seed `release-notes/0.2.3.md`: what 0.2.3 shipped to users, curated from
  `git log v0.2.2..v0.2.3` (most of its 11 commits are internal; keep only what a user would
  notice, e.g. the repository move's Check for Updates fix and the Prisma grammar).

## Built in

- A new `crates/fe-workbench/build.rs` reads `release-notes/*.md` (`CARGO_MANIFEST_DIR/../..`),
  fails the build with a clear message on a file not named `X.Y.Z.md`, and writes
  `$OUT_DIR/release_notes.rs`: `&[(version, include_str!(<absolute path>))]`, newest first.
  `cargo:rerun-if-changed` on the directory, so adding a file rebuilds.
- The tab shows the versions up to the running one (`env!("CARGO_PKG_VERSION")`), newest first;
  a file for a version above it (a draft for the next release) is left out. If the running
  version has no file (a dev build after a bump), a one-line "no notes for this version" shows
  above the earlier ones. A `cfg(test)` seam (a Global) replaces the embedded list and the
  running version for tests.
- The notes text is English only, as written; the tab's own strings (title, version heading
  `{ $app } { $version }`, the empty line) are Fluent keys.

## The tab

- New `crates/fe-workbench/src/whats_new.rs`: a read-only pane item, `PaneItem` KIND
  `fastedit.whats_new`, tab title "What's New", an existing line icon. It draws one
  `fe_markdown::Document` built from the files (a `# <app> <version>` heading before each) with
  `fe_markdown_view::MarkdownView` (`crates/fe-markdown-view/src/view.rs`), styled as
  `crates/fe-workbench/src/markdown_preview.rs` styles its view (`MarkdownStyle` from the active
  theme, restyled on a theme reload). A web link opens in the browser; other links do nothing.
  It isn't kept in saved layouts.
- `Workbench::open_whats_new`: as `open_keyboard_shortcuts` does
  (`crates/fe-workbench/src/shortcuts_editor.rs:1855-1871`), the open one in any pane is
  activated, else a new tab in the active pane, focused.
- Command `app.show_whats_new` "What's New" in `crates/fe-workbench/src/workbench_commands.rs`
  next to `app.set_up` (:639), title in `crates/fe-workbench/l10n/en-US/commands.ftl`, no
  default key, in the palette.
- Menu: a new Help menu, last, after Window, on macOS and Linux, holding What's New, in
  `crates/fe-keymap/menus/fastedit.toml` with keys `keymap-menu-help` and
  `keymap-menu-whats-new` in `crates/fe-keymap/l10n/en-US/keymap.ftl`. kiln-platform registers
  only AppKit's Windows menu (`kiln/crates/kiln-platform/src/menus.rs:199`), not a Help menu
  role, so macOS's Help search field is out of scope.

## Opened once after an upgrade

- New `crates/fe-config/src/whats_new.rs`, modelled on `first_run.rs`: the marker
  `<state_dir>/whats_new.toml` with `seen = "X.Y.Z"`, a read-only `decide` and an atomic
  `record` (temp file made with `create_new`, then rename, as `first_run::record` does).
  Versions compare as `MAJOR.MINOR.PATCH` numbers (the rules of `parse_version`,
  `crates/fe-workbench/src/update_check.rs:302`).
- The offer, `fe_workbench::offer_whats_new`, works with the setup wizard's launch decision
  (`crates/fe-workbench/src/setup_launch.rs`; restructure what `offer_setup` hands on as
  needed, keeping one background decision and nothing on the first frame's path):
  - the wizard is offered this launch (a brand-new install): record the running version, show
    nothing;
  - `SetupLaunch::Never` (`--wait`, `--perf`): show nothing, write nothing;
  - no state directory: show nothing (it could never be recorded);
  - `seen` equal to or above the running version: nothing, the marker kept as it is;
  - `seen` below it, or no or unreadable marker on a machine that isn't new (an upgrade from a
    build before this card): open the tab in the launch's first window.
- The running version is written only after the tab has been shown: once it has drawn in the
  window (its first render, or the frame after opening it). A launch that quits, or whose
  window closes, before then shows it again next time. Once per upgrade, never per window: last-quit
  restore's other windows and IPC-opened windows never offer it.
- Only `crates/fastedit/src/main.rs` calls the offer, once, for the launch's first window, after
  `offer_setup` (:1245). Nothing in `Workbench::new`, window restore, the screenshot examples
  or audit scenes calls it, so workbench tests and screenshot runs never see the tab unless a
  test calls the offer itself. `scripts/e2e-macos.sh` writes `whats_new.toml` with the bundle's
  version next to the `setup.toml` it writes (:362-369), so the XCUITest runs don't get the tab
  either.

## Not in this card

The release workflow, the appcast and GitHub release body, the drafting script and the check
that a release has its notes file: the follow-up card. Velopack's own release notes. Localized
notes. A "show on upgrade" setting.

## Pins

`crates/fe-keymap/tests/menu.rs` (:60, :309, :474), `crates/fe-keymap/tests/l10n.rs`
(`MACOS_TITLES`, `LINUX_TITLES`) and `crates/fe-gpui/src/menu_tests.rs` (:85) list the menus
and gain Help. `command_catalog_tests.rs` and `typed_command_ids_tests.rs` gain the id. No
screenshot golden should move; if one does, block for a Mac regeneration rather than editing
goldens. New workbench tests go in the new `crates/fe-workbench/src/tests/whats_new.rs`,
declared from `crates/fe-workbench/src/tests/mod.rs`; the marker's in the new
`crates/fe-config/tests/whats_new.rs`; the menu's in the new
`crates/fe-keymap/tests/menu_whats_new.rs`.

Overlaps (rebase on whichever lands first): T1933 (running) and T1935, T1834 (ready) on
`workbench_commands.rs` and `commands.ftl`; T1834 on the menus file, `keymap.ftl` and the
fe-keymap menu and l10n tests; T1682 (running) on `scripts/e2e-macos.sh`; many cards on
`fe-workbench/src/lib.rs` and `tests/mod.rs`. Do not widen --scope; if a file outside it is
needed, stop and say so.

## Implementation notes

- The design calls, the 0.2.3 curation, the C8 dry run and the C9 Linux check (with its
  screenshots) are in `docs/decisions/d7t5rfkttd-whats-new.md`.
- `offer_setup` and `offer_whats_new` share one background decision (`setup_launch::Decision`).
  `offer_whats_new` called without `offer_setup` decides by itself and leaves the wizard's
  marker alone.
- The running version is recorded from the tab's first `render`, on a background task.
- Out of scope, forced by the Help menu: `crates/fe-gpui/src/menu_native_tests.rs:102`
  counts 7 menus (run by C10's `cargo nextest run -p fe-gpui menu`), and
  `crates/fe-workbench/src/tests/menu_bar.rs:106` wraps Left from File to Help (index 5).
