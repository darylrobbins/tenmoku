---
task_uid: "01m6rmd9f9wyamx74t2j4vk3dv"
task: "T475"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test start_page_tests::model_lists_start_actions_and_existing_recents: from a RecentList with 7 folders (one deleted on disk) and 7 files, the model has Start, Recent folders (max 5, none missing), Recent files (max 5), Settings sections, each row carrying a command invocation"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test start_page_tests::vim_adds_the_dashboard_keys: with vim on the model carries the letter hints and pressing one runs its row's command; with vim off there are no letter keys and typing is not captured"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test start_page_tests::shows_only_with_no_tabs: the page shows in a rootless window with no tabs, disappears when a tab opens, returns when the last closes, and never shows while a tab exists"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test start_page_tests::setup_wizard_keeps_the_page_inert: while Workbench::setup() is Some the page takes no focus and handles no keys; after the wizard closes focus is on the page (T1343 first-launch path)"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test start_page_tests::setting_off_keeps_the_plain_empty_state: workbench.start_page = false draws the existing 'No open files' message"
    required: true
    manual: false
  - id: "C6"
    text: "fe-config test (tests/start_page_setting.rs) start_page_defaults_true_and_validates: default true, false parses, a string is rejected by the schema; template and schema both list the key"
    required: true
    manual: false
  - id: "C7"
    text: "cargo clippy --workspace --all-targets -- -D warnings passes"
    required: true
    manual: false
---

# Show a Start page in the empty pane when nothing is open: New File, Open File, Open Folder, recent folders and files, settings, and LazyVim-dashboard keys when vim is on, behind workbench.start_page (Mac T1398)

Lent from Mac T1398 (lend y42, 2026-11-06). Cut it back as-is with no Mac numbers in a helium batch.

## Problem

Since T1307, a launch with nothing to open shows an empty pane ("No open files"). VS Code's Welcome page and LazyVim's dashboard (snacks/alpha) fill that gap with recent projects, recent files and the common first actions. The data exists (`fe_config::recent`, `<state_dir>/recent.toml`, T513/T528; `RecentPicker` in `recent_picker.rs`).

Linux-OK for everything but one optional visual check.

## Change

A Start page, drawn in the pane's empty state (not a tab), shown when the active pane has no tabs and the window has no root, or a root with no tabs. It goes away when a tab opens and returns when the last closes.

- `crates/fe-workbench/src/start_page_model.rs` (new, headless): `StartPage::from(recent: &RecentList, vim: bool, now)` yields sections: Start (New File, Open File, Open Folder, Clone Repository only if a clone command exists, otherwise omit), Recent folders (max 5), Recent files (max 5), Settings (Open Settings, Keyboard Shortcuts), and when vim is on a key hint column (`n`, `f`, `r`, `g`, `p`, `s`, `q` as in LazyVim's dashboard). Entries whose path is gone are filtered with `RecentList::existing`. Rows carry the command invocation they run.
- `crates/fe-workbench/src/start_page.rs` (new): the element: centred column using design-language tokens (`docs/design-language.md`), keyboard navigable (up/down, Enter), mouse clickable. With vim on, the letter keys run the row (only while the page has focus and no tab exists, so they cannot type into anything); first letter in the hint is underlined.
- New setting `workbench.start_page` (bool, default true) in `crates/fe-config/src/settings.rs`, `settings-template.toml`, `settings.schema.json`; off keeps today's plain empty state.
- Setup wizard (T1343): the wizard is a sheet over the window shown on a machine's first launch; the Start page sits under it. While `Workbench::setup()` is `Some`, the page takes no focus and handles no keys; when the wizard closes ("Start Editing" or "Skip") focus moves to the Start page. No wizard step changes.
- Not shown for `--wait` launches or `--perf`; those open a file anyway.

## Out of scope

Tips, release notes, walkthroughs, a project-specific page, remote workspaces.

## Conflicts

Touches `workbench.rs` and `lib.rs` (small), the settings triple also touched by the format-on-type and word-wrap-column cards: additive keys. Does not overlap the harpoon card's commands.


## Helium notes
- A setting goes live on helium only through a line in crates/fe-workbench/src/workbench_root.rs `reload_settings` (Pane.settings is private; GlobalSettings misses per-root settings). If the new keys must change behaviour without a restart, respec workbench_root.rs into scope before the first finish (`rgt task respec --scope`, which replaces the list).

## Implementation notes

- The empty state is drawn in `pane.rs`, which has no hook for it, so the scope takes in `pane.rs`
  (`Pane::set_start_page`, `Pane::shown_start_page`, the empty branch of `Pane::render`),
  `workbench_root.rs` (one line in `reload_settings`, so `workbench.start_page` applies live) and
  `recent_picker.rs` (`load` made `pub(crate)`, so the page reads the list through the same queue).
- The page tracks the pane's own focus handle in its place while it shows, so whatever focuses the
  empty pane (kiln-shell after the last tab closes, a click, `Workbench::focus_handle`, a dock or the
  setup wizard giving focus back) focuses the page, and its keys and commands are on the focus path.
- `RecentList` carries no times, so `StartPage::from` takes no `now`; it takes the vim flag, whether
  `git.clone` is registered (it isn't on helium, so Clone Repository is omitted) and the home
  directory for `~`. It's built off the UI thread, as `RecentList::existing` stats each path.
- Recent rows run `start_page.open_recent_folder` and `start_page.open_recent_file` with a `path`
  arg: typed commands on the page's view, not in the palette.
- Dashboard keys: `n` New File, `f` Open File, `p` Open Folder, `s` Open Settings; with vim on the page
  also lists Find Text (`g`), Recent Files (`r`) and Quit (`q`). `j`/`k` move as up/down do.
