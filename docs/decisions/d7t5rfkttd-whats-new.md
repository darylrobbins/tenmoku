# What's New tab (T420, Mac T1946): decisions and the Linux check

## Design calls

- **One background decision for both offers.** `begin_setup`'s task now decides the wizard
  and What's New together (`setup_launch::Decision`): `first_run::decide` once, then the
  wizard's outcome, then What's New's. `offer_setup` takes the task, shares it
  (`futures::future::Shared`), shows the wizard from one clone and leaves the other in a
  Global (`Offered`) for `offer_whats_new`. Called alone, `offer_whats_new` makes its own
  decision that reads the setup marker but never writes it.
- **New machine = `FirstRun::Show`.** That is the launch that offers the wizard (or a pending
  marker's launch, which offers it now). It records the running version and shows no tab.
  `FirstRun::Existing` (files in use, no setup marker: an upgrade from before the wizard) and
  `FirstRun::Done` with no `whats_new.toml` (an upgrade from before this card) both show the tab.
- **Recorded on the tab's first render.** `show_whats_new_once` hands the item the state
  directory and the version. Its `render` takes them and records them on a background task.
  A window that closes, or a launch that quits, before the frame records nothing, so the next
  launch offers the tab again.
- **The offer waits for the launch to settle (review F1).** The decision can land before the
  first window has its files. Its restored layout's items arrive later
  (`PaneView::add_items` makes their active tab active), and so do the launch's own files.
  Either would push What's New aside, before it draws or after it has been recorded.
  `offer_whats_new` therefore takes a `settled` future, which `main.rs` resolves once
  `landed` (the launch file's read and the launch files) is done, and then waits for
  `Workbench::restoring()` to go false (`setup_launch::restored`, observing the Workbench).
  Only then does it open the tab, active and focused. The decision itself still runs in the
  background, off the first frame. `tests::whats_new::the_offer_waits_for_the_launch_to_settle`
  holds the restore (a test item kind whose restore waits on a channel) and the launch's
  settle, with the decision already landed, in both orders.
  Limit: macOS's extra cold-launch documents (`open_documents(&handed)`, after the other
  windows restore) aren't waited on. They open in a window on their own root, which is
  usually not this one.
- **The tab reads the editor theme from its opener.** `WhatsNew::new` runs inside a Workbench
  update, so it can't read the Workbench. `open_whats_new` passes the active pane's
  `EditorTheme`, and a theme reload reads it again from the Workbench (`restyle`).
- **The no-notes case gets a heading.** A running version with no file gets its own
  `<app> <version>` heading with the no-notes line under it, above the earlier releases, so
  the line says which version it means.
- **Localized strings are escaped.** The tab's heading and no-notes line are Fluent text put
  into Markdown, so ASCII punctuation is backslash-escaped. A translation can't turn into
  Markdown syntax.
- **build.rs skips hidden files** (`.DS_Store` on the Mac) and fails on every other name that
  isn't `X.Y.Z.md`. It repeats `parse_version` (also in fe-config's `whats_new`) because a
  build script would need a build dependency to use fe-config's.
- **The raw-HTML rule is checked on the source.** fe-markdown draws some inline tags (`<b>`)
  as formatting, so the parsed tree can't see them. The notes test also scans the source for
  `<` starting a tag, comment or declaration, outside code spans. A URL autolink in brackets
  is allowed.
- **Release notes 0.2.3**, from `git log v0.2.2..v0.2.3`. Kept: Prisma highlighting (T1900),
  the extra file-name mappings (T406), the Check for Updates fix after the repository move
  (T409) and the debug gutter's verification fix (T667). Left out as internal: the flake fix,
  the heap script, the a11y recorder seam, the test isolation fix, the fuzz lockfile, the
  golden regeneration and the version bump.

## Out-of-scope test pins

Two tests outside the card's scope pin the menu bar's length, and Help changes both. Only
the pins changed.

- `crates/fe-gpui/src/menu_native_tests.rs:102` counts the installed menus: 6 → 7. C10's
  `cargo nextest run -p fe-gpui menu` runs it.
- `crates/fe-workbench/src/tests/menu_bar.rs:106` (`f10_and_the_arrows_choose_an_item`):
  Left from File wraps to the last menu, now Help (index 5), not Window (4). The test's next
  step, Right Right to Edit, holds as before.

The fe-config literal lint (`fe-config::l10n ui_text_is_in_the_catalogue`) flagged
`whats_new::record`'s two format strings. `l10n-allow.toml` is out of scope, so they went
instead: the marker is written with `toml::to_string`, which also escapes, and the temp name
takes its extension from a const.

## C8: e2e dry run

`scripts/e2e-macos.sh --dry-run` (run on Linux with a stand-in `uname` that answers Darwin)
prints the scratch-set line with the new marker:

```
+ for n in 001 .. 003: mkdir -p …/scratch/<stamp>/$n/{project,config,state,home}; : >$n/project/notes.txt; echo 'outcome = "skipped"' >$n/state/setup.toml; echo 'seen = "0.2.3"' >$n/state/whats_new.toml
```

The version is the `[workspace.package]` one, which `bundle-macos.sh` stamps on the bundle
when no `--version` is passed (the e2e script passes none).

## C9: the real Linux app (run-fastedit-linux)

Debug build of this lane's head, under Xvfb (`FE_DISPLAY=:91`), launched on a file. The
state directory held `setup.toml` (`outcome = "completed"`) and `whats_new.toml`
(`seen = "0.2.2"`). The screenshots are in the session scratchpad
(`…/scratchpad/shots/`, not committed):

1. `01-launch-upgrade.png`: the launch opens a What's New tab, active and after `main.rs`.
   It shows "Tenmoku 0.2.3" with its New and Fixed sections, and the repository link in the
   link colour. The menu bar ends `Window Help`. `whats_new.toml` then read `seen = "0.2.3"`.
2. `02-relaunch-no-tab.png`: quit (ctrl+q) and relaunch. Only `main.rs`, no What's New tab;
   the marker is unchanged.
3. `03-help-menu.png`: the Help menu, holding What's New.
4. `04-help-whats-new.png`: choosing it opens the tab again with 0.2.3's notes.
5. `05-palette.png`: the command palette lists "What's New", with no key.

The app log had no panic.
