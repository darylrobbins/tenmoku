# Audit capture for B3: per-surface scenes, hooks and probes (T1519)

## Status

Done on the Mac (Metal), 2026-11-08. Folds in the UI polish audit's held cards
P15 (hooks for the left-out surfaces, the golden run isolated) and P26 (token
probe over the audit scenes) of `docs/decisions/25w38k498b-ui-polish-audit.md`,
for the surface cards to use in their screenshot criteria. Peek and the
completion menu, also under P15 there, aren't in this card's scope and have
no scene yet.

## The command

```text
cargo run -p fe-workbench --example screenshots -- audit target/ui-audit
```

It writes 49 PNGs in about 100 s once built: T1429's 41, plus
`problems_rows`, `terminal_session` and `git_log_card`, each `_dark` (Yohen,
no settings) and `_light` (Hakuji, `[theme] mode = "light"`), and checks each
capture's probe points (below). It exits 1 if a probe fails, a surface has
no probes, or a scene's step couldn't do its part.

## Layout

- `examples/screenshots/audit_scenes.rs` is the runner: the `Surface` type and
  its `DEFAULT`, the appearances, the step helpers (`run`, `select`, `idle`,
  `chain`, `fail`), the Git history, the coverage check and the two lists,
  `SURFACES` and `LAST` (surfaces whose setup nothing undoes, captured after
  the bundled themes: the trust prompt). The spec called it `audit/mod.rs`;
  the card's scope named `audit_scenes.rs` and not `audit/mod.rs`, so it
  stays and declares the surface files with `#[path = "audit/<name>.rs"]`.
- `examples/screenshots/audit/<name>.rs` is one surface: its `SURFACE`, its
  steps, its setup if any and its probes. All 18 have a file: T1429's 15
  (the trust prompt included, whose setup moved into `audit/prompt.rs`) and
  the three new ones. A surface card adds its file, its `mod` line and its
  entry in `SURFACES`. Of these files the scope listed `chrome.rs` and the
  three new ones; the other 14 are the split the spec asks for (review
  round 1, F3).

## Hooks

Each is `#[doc(hidden)]` and only the audit calls it.

| Hook | Where | What it does |
| --- | --- | --- |
| `Workbench::set_diagnostics_for_audit(path, diagnostics, cx)` | `crates/fe-workbench/src/problems_panel.rs` | Applies `(byte range, Diagnostic)`s to every open editor of `path` under an owner no server or plugin gets (`DiagnosticOwner::Plugin(u64::MAX)`; plugin keys count up from 1), then relists the Problems panel. So the scene has rows of every severity, a related row, squiggles and the status bar counts, with no language server. |
| `TerminalPanel::open_with_session(bytes, window, cx)` | `crates/fe-workbench/src/terminal_panel.rs` | Adds a session running `/bin/sh -c 'printf %s "$1"; exec cat >/dev/null'` with `bytes` as `$1`: fixed output, no user shell, profile or `terminal.*` setting. `cat` keeps the process (the "running" command) alive until the session closes. Shares `add`'s new `add_session`. |
| `Workbench::git_log_panel(cx)` | `crates/fe-workbench/src/git_log.rs` | The dock's Git Log panel. `GitLogPanel` became `pub` (still unexported) so the example can hold it. |
| `GitLogPanel::show_card(ix, cx)` | `crates/fe-workbench/src/git_log.rs` | Builds commit `ix`'s `CommitCard` as its row draws and draws it in a `deferred(anchored())` child of the row, one pixel past the row's start, where GPUI puts a tooltip past the pointer and with the same flip to fit the window. A new list read, or a filter, forgets it. |

The terminal scene's process runs beside the app, so no frame or task waits
for it: its `wait` step polls `Terminal::output_text` for the last line, up to
10 s. Its output is 10 rows so it fits the default bottom dock: `cargo build`
(exit 0), `cargo test` with a failing test (exit 1) and `cargo run -- serve`
still running, each marked with OSC 133 A, B, C and D as a shell integration
marks them, with ANSI bold, green, red and blue.

A step whose part doesn't happen fails the run (`audit_scenes::fail`): the
terminal's output not there after 10 s, a Problems file not open, no Git Log
panel, no editor to select or hover in. The runner turns it into an error
naming the capture, and the run exits 1 without writing that PNG:

```text
$ # wait() looking for text the session never writes
$ SCREENSHOTS_ALLOW_OTHER_SCALE=1 cargo run -p fe-workbench --example screenshots -- audit target/ui-audit-noterm
…
ok      problems_rows_light probes (2)
error: terminal_session_dark: the terminal session didn't write its output in 10 s
$ echo $?
1
```

## Isolation (every mode)

The isolation T1429 gave the audit now covers every mode of the example,
the golden run included, and lives in `main.rs`: `isolate` re-executes the
example with its arguments and `TENMOKU_CONFIG_DIR`, `TENMOKU_STATE_DIR` and
`XDG_CACHE_HOME` on a fresh temporary folder (`TENMOKU_SCREENSHOTS_ROOT`,
formerly `TENMOKU_AUDIT_ROOT`), and the child refuses to start unless those
and `fe_config::paths::app_paths()` point there (`isolated`):

```text
$ TENMOKU_SCREENSHOTS_ROOT=/tmp/nowhere target/debug/examples/screenshots
screenshots: TENMOKU_CONFIG_DIR isn't /tmp/nowhere/config: not starting
```

Before this, every golden run recorded its demo folders in the real
`~/.local/state/tenmoku/recent.toml`; on this Mac it held 70 temporary-folder
entries. A golden run now leaves it as it was (same mtime and size before
and after, 2026-11-08), and still matches its goldens:

```text
ok      workbench
token probe: 0 identical pixels, allowed 0
ok      workbench tokens
ok      setup_wizard_dark
ok      setup_wizard_light
ok      file_icons_dark
ok      file_icons_light
```

The audit decision's Captures section still says the golden run isn't
isolated and points at `audit_scenes.rs:isolated`; both are now as above.

## Token probe over the audit scenes

`token_probe.rs` has `Probe { at, token, under }` (a point in logical pixels
on a flat fill, a kiln token's `group.name` and, for a translucent one, the
token under it) and `check_points`, which reads
the capture at its own scale (width over the scene's 1000) and fails a point
whose pixel isn't the token's colour, from the theme the capture was drawn
in, within `CHANNEL_TOLERANCE`. It fails outright on a token that doesn't
exist, a translucent one without an opaque one under it, or a point outside
the capture. Points are logical
and read at the capture's scale, so 1× and 2× captures are judged alike and
`SCREENSHOTS_ALLOW_OTHER_SCALE` (which CI's 1× Mac sets) doesn't excuse
anything. Probes run on every capture of their surface; `chrome`'s also run on
every bundled theme's chrome capture.

Every surface has probes, and the run refuses one without before it
captures anything (a surface's `PROBES` emptied, temporarily, here
`hover`'s):

```text
$ SCREENSHOTS_ALLOW_OTHER_SCALE=1 cargo run -p fe-workbench --example screenshots -- audit target/ui-audit-noprobes
error: audit surface hover has no probes
$ echo $?
1
```

A probe names an opaque token, or (`probe_over`) a translucent one and the
opaque token under it, checked as the one drawn over the other by its alpha:
the trust prompt's scrim is `surface.overlay` over `surface.background`.

| Scene | Points (logical) and tokens |
| --- | --- |
| `chrome` and the 13 theme captures | title bar, tab strip, project tree, rail, status bar: `surface.panel`; editor: `surface.background` |
| `project_tree_git` | `main.rs`'s selected row, tree unfocused: `list.row_selected_inactive`; the tree below its rows: `surface.panel` |
| `start_page` | New File, selected: `list.row_selected`; the page: `surface.background` |
| `settings_editor` | the selected page, unfocused: `list.row_selected_inactive`; a toggle that's on: `accent.base`; the page list: `surface.background` |
| `shortcuts_editor` | the search box, the table's header gap and the table: `surface.background` |
| `command_palette` | the selected row: `list.row_selected`; the palette and its input: `surface.panel` |
| `file_finder` | the selected file: `list.row_selected`; the picker: `surface.panel` |
| `find_bar` | the bar, twice: `surface.panel`; the editor below: `surface.background` |
| `search_panel` | the query row and the panel below the results: `surface.panel` |
| `toast` | its warning dot: `status.warning`; the editor beside it: `surface.background` |
| `git_panel` | the commit message box: `surface.background`; the disabled Commit button: `state.disabled`; the panel: `surface.panel` |
| `git_log` | the selected commit: `list.row_selected`; the panel: `surface.panel` |
| `problems_panel` | the panel: `surface.panel`; the editor: `surface.background` |
| `hover` | the popover: `surface.raised`; its code block: `surface.panel` |
| `problems_rows` | the panel below its rows: `surface.panel`; the editor: `surface.background` |
| `terminal_session` | the terminal right of its output: `surface.background`; the panel's header: `surface.panel` |
| `git_log_card` | the card over the editor and the Source Control panel: `surface.panel` |
| `prompt` | the dialog: `surface.panel`; Trust Folder: `accent.base`; the scrim over the editor: `surface.overlay` over `surface.background` |

The toast's fill has no probe: it's translucent `surface.overlay` (the
audit's finding) over the toast's own shadow, so no token, or pair of them,
gives the colour there; in Hakuji it reads #909090.

A finding from the first run: `surface.chrome` is drawn nowhere. Probing
the title bar and rail for it failed in every theme where it differs from
`surface.panel` (Yohen's chrome is #120e0b, its panel #17120f); the title
bar, tab strip, rail and status bar all draw `surface.panel`
(`kiln_ui::activity_rail`, `status_bar`, `tab_bar`), which the token
defaults to.

### Passing, with `SCREENSHOTS_ALLOW_OTHER_SCALE` set

`SCREENSHOTS_ALLOW_OTHER_SCALE=1 cargo run -p fe-workbench --example
screenshots -- audit target/ui-audit` at the round-1 commit: exit 0, 49
captures, 49 `ok` lines and no `FAILED` line:

```text
ok      chrome_dark probes (6)
ok      chrome_light probes (6)
ok      project_tree_git_dark probes (2)
…       (every surface, dark and light)
ok      git_log_card_light probes (2)
ok      vscode-dark-modern probes (6)
…       (every bundled theme: probes (6))
ok      hakuji probes (6)
ok      prompt_dark probes (3)
ok      prompt_light probes (3)
wrote 49 captures to target/ui-audit
```

### An injected wrong colour fails it

The Problems panel's background hard-coded, temporarily, to Tenmoku Dark's
panel colour in `ProblemsPanel::render`:

```diff
-            .bg(ui.panel_background)
+            .bg(gpui::rgb(0x121418))
```

Then the same command, with `SCREENSHOTS_ALLOW_OTHER_SCALE=1` set, exits 1:

```text
FAILED  problems_panel_dark probe (850, 400): #121418, but surface.panel is #17120f
FAILED  problems_panel_light probe (850, 400): #121418, but surface.panel is #f7f7f9
FAILED  problems_rows_dark probe (850, 400): #121418, but surface.panel is #17120f
FAILED  problems_rows_light probe (850, 400): #121418, but surface.panel is #f7f7f9
wrote 49 captures to target/ui-audit-injected
FAILED  4 captures' token probes
```

Each injected edit was reverted (`git checkout`) before the passing run
above.
