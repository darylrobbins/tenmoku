---
task_uid: "01m5f775s37nqz66mqrk9rz9wd"
task: "T1022"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench test chat_panel::chat_panel_tests::rail::chat_is_on_the_rail_from_the_start: a fresh workbench's rail is Explorer, Search, Changes, Git Log, Chat (Icon::MessageSquare), then Terminal; Chat's index is unchanged after git.file_history and References show"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test chat_panel::chat_panel_tests::rail::clicking_chat_on_the_rail_shows_it_focused: a rail click shows Chat with the composer focused; a second click hides the dock"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test chat_panel::chat_panel_tests::rail::the_key_is_not_read_until_the_chat_shows: a counting SecretStore sees 0 reads across startup frames, exactly 1 after the first rail click, and key_origin() is Keychain"
    required: true
    manual: false
  - id: "C4"
    text: "Rail-pinning tests in sidebar_panels_tests.rs, chat_panel_tests.rs, git_panel_tests.rs, explorer_views_tests.rs, tests/activity_rail.rs, tests/references_panel.rs updated and passing; workbench.png regenerated on the Mac"
    required: true
    manual: false
---

# Put the AI chat on the activity rail from the start, under Git Log, with its message-square icon, reading the key only once it shows

# Put the AI chat on the activity rail from the start, under Git Log, with its message-square icon

## Problem

The user (2026-10-21, main 88bbda08): "Let's keep the AI chat on the rail permanently so I can just
click it." Today the Chat button joins the rail only after `chat.open` has run once:
`chat_panel::SIDEBAR` (crates/fe-workbench/src/chat_panel.rs:52) has `always_on_rail: false`, order 90,
and the panel is made lazily by `Workbench::open_chat` → `Dock::set_chat` (dock.rs:408). Until then the
only way in is the palette (T972, which adds keys and a View menu item, is queued on helium behind
T969-T971 and doesn't touch the rail).

## Change

Follow T808 (docs/specs/296s3j0y89-put-the-git-log-on-the-activity-rail.md, commit 50cad90c), which did
this for Git Log.

- **Placement.** VS Code shows its chat in the secondary side bar. fastedit has one sidebar dock (Right,
  movable) and no secondary side bar, so the chat goes on the one rail as an always-on panel, directly
  under the always-on group (Explorer, Search, Changes, Git Log, Chat), above the panels that join once
  shown (References, Problems, History), so its button never moves as those join. `SIDEBAR`:
  `always_on_rail: true`, `order: 47`, icon stays `Icon::MessageSquare`, name "Chat", show command
  `chat.open`. Move it after `git_log::SIDEBAR` in dock.rs `PANELS`, make `DockPanel::ALL`
  `[DockPanel; 5]`, and update the dock.rs module docs and chat_panel.rs's.
- **Made with the dock, key read on first show.** In `Workbench::new` (workbench.rs, next to the Git Log
  panel), make the `ChatPanel` and register it in `Dock::new` right after `git_log` (the rail lists
  panels in the order added), so `Dock::chat()` is always `Some`; `open_chat` keeps its make-if-missing
  branch harmlessly or drops it. `ChatPanel::new` must no longer call `reload_key`: reading the Keychain
  at launch can raise a macOS prompt before the user asked for the chat. Read the key the first time the
  panel is shown (its first render, deferred past the frame as Git Log's first listing is, or the
  `ShowChat` op), once; `reload_key` callers are unchanged. A restored session whose dock was left on
  Chat reads it when it draws.
- A rail click runs `DockOp::ShowChat`, which shows and focuses the composer, as `chat.open` does; a
  second click hides the dock.

Update the rail-pinning tests to the new rail: sidebar_panels_tests.rs (the always-on and order tests,
and the `chat.open` join test at :287), chat_panel_tests.rs (`chat_open_makes_and_shows_the_panel`
asserts the panel is absent before `chat.open`), tests/activity_rail.rs, tests/references_panel.rs,
git_panel_tests.rs and explorer_views_tests.rs where they pin the rail. Regenerate
crates/fe-workbench/examples/screenshots/golden/workbench.png (one more rail icon; Mac only).

New tests in crates/fe-workbench/src/chat_rail_tests.rs, declared at the end of
crates/fe-workbench/src/chat_panel_tests.rs by `#[path = "chat_rail_tests.rs"] mod rail;`, using the
parent's helpers (`rail`, `shown`, `chat`, `composer_focused`, `set_source`, `draw`),
`Dock::rail_button_bounds` and `VisualTestContext::simulate_click`:

- `chat_is_on_the_rail_from_the_start`: a fresh workbench, before any command, has the rail Explorer,
  Search, Changes, Git Log, Chat, then the Terminal action; Chat draws `Icon::MessageSquare`; after
  `git.file_history` and `references` show, Chat's index is unchanged.
- `clicking_chat_on_the_rail_shows_it_focused`: a click on its rail button shows the dock with Chat and
  the composer focused; a second click hides the dock.
- `the_key_is_not_read_until_the_chat_shows`: with a `SecretStore` (defined in the test file) counting
  `get` calls installed via `KeySource`, a fresh workbench drawn several frames reads 0 times; after a
  rail click and `run_until_parked`, once, and the panel's `key_origin()` is `Keychain`.

Not in this card: keys and a menu item for `chat.open` (T972); a secondary side bar.
