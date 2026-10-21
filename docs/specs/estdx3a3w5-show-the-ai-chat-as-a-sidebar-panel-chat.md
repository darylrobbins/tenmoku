---
task_uid: "01m5e7nze904ecwzestdx3a3w5"
task: "T243"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test chat_panel::chat_panel_tests::chat_open_makes_and_shows_the_panel: before chat.open the rail has no Chat button; after it the dock shows DockPanel::Chat with the composer focused and Chat on the rail after Git Log's place; a second chat.open reuses the panel"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test chat_panel::chat_panel_tests::the_key_comes_from_the_environment_then_the_keychain: a KeySource env value is used and the header says Key from ANTHROPIC_API_KEY; with none, a MemoryStore entry dev.fastedit.fastedit / chat.anthropic is used; with neither the view is in its no-key state; reload_key picks up a changed store"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test chat_panel::chat_panel_tests::new_chats_take_settings_and_grants: a new chat takes model, effort, show_thinking, max_tokens and fallbacks from [chat] in the settings global and keeps them after the settings change; CHAT_CAPABILITIES parse to Grants allowing exactly buffer.read and net:api.anthropic.com"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test chat_panel::chat_panel_tests::the_notice_shows_until_dismissed: the first panel shows the record's privacy notice; Got It writes \u003cstate_dir\u003e/chats/notice-seen and a new window's panel doesn't show it"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench's sidebar_panels_tests pass with Chat in PANELS; message-square.svg is Lucide's at the commit THIRD_PARTY.md names and both THIRD_PARTY.md icon lists include it; workbench.rs is unchanged"
    required: true
    manual: false
---

# Show the AI chat as a sidebar panel (chat.open), wired to Claude through fe-net with the chat's grants and the key from ANTHROPIC_API_KEY or the Keychain (Mac T968)

{
  "mode": "inline",
  "body": "# Show the AI chat as a sidebar panel, wired to Claude with the key from the environment or the Keychain\n\n## Problem\n\nThe AI chat decision record (`docs/decisions/*-ai-chat.md`; signed off by the user) puts the\nchat in fastedit's workbench as a kiln sidebar panel (record §1). `fe-chat-view` (the card this\ndepends on) is the view; `fe-chat-anthropic` with `fe-net` the provider; `kiln_core::secrets`\nthe keychain; `[chat]` the settings. Nothing puts them in a window.\n\nThe sidebar's panels are each declared by their module as a `SidebarPanel` spec\n(`crates/fe-workbench/src/dock.rs`, since c09414be); History and Git Log are made on first\nuse and registered with `Dock::set_history` / `Dock::set_git_log`, the pattern to follow.\n\n## Change\n\nNew `crates/fe-workbench/src/chat_panel.rs` (`mod chat_panel;` in `lib.rs`, whose `init` also\ncalls `fe_chat_view::init`), and `fe-chat`, `fe-chat-view`, `fe-chat-anthropic`, `fe-net` in\n`crates/fe-workbench/Cargo.toml`.\n\n- `ChatPanel`: a dock item holding the `Entity<ChatView>`, its `SIDEBAR` spec:\n  `DockPanel::new(\"chat\")`, name \"Chat\", icon `Icon::MessageSquare`, show command `chat.open`,\n  `DockOp::ShowChat`, order 90 (after Git Log), `always_on_rail: false`, `focus_on_show: true`\n  (the composer takes focus). In `dock.rs`: `DockPanel::Chat`, the `PANELS` entry,\n  `DockOp::ShowChat`, a `chat: Option<Entity<ChatPanel>>` with `set_chat` as `set_history`\n  has, and `dock_item!(ChatPanel, …)`. The panel is made by the first `chat.open`, not at\n  window start (that would mean editing `workbench.rs`'s `Dock::new` call), and joins the rail\n  then, as History does.\n- The icon: Lucide's `message-square.svg` at the commit `THIRD_PARTY.md` already names\n  (`500620a2…`), added to `kiln/crates/kiln-ui/assets/icons/`, to `Icon` in\n  `kiln/crates/kiln-ui/src/file_icons.rs` and to the icon lists in `THIRD_PARTY.md` and\n  `kiln/THIRD_PARTY.md`, as T808 adds `git-graph.svg`.\n- `chat.open` (title \"Chat: Open\"): a `#[kiln::command(context = \"workbench\")] impl Workbench`\n  block in `chat_panel.rs`, making the panel if needed, then showing it through\n  `Workbench::dock()` and `Dock::show(DockPanel::Chat, …)`. `run_dock_op` is private to\n  `workbench.rs`; leave `workbench.rs` alone (several queued cards hold it).\n- **The chat's grants**: `CHAT_CAPABILITIES = [\"buffer.read\", \"net:api.anthropic.com\"]`,\n  parsed once into a `kiln_command::Grants` the panel holds (record §9); the provider is\n  `AnthropicProvider::with_client(&grants, key)`, so fe-net reaches only that host.\n- **The key** (record §7): `KeySource { env: Option<String>, store: Arc<dyn SecretStore> }`,\n  in the app `std::env::var(\"ANTHROPIC_API_KEY\")` and `KeychainStore`; service\n  `dev.fastedit.fastedit` (the bare app id, `fe_config::paths::app_id()`), account `chat.anthropic` (record, user decision 3: the general secret store's convention). The environment wins; the panel's\n  header then says \"Key from ANTHROPIC_API_KEY\". The keychain read runs on the background\n  executor (it may raise a system prompt) and the view stays in its no-key state until it\n  returns. `ChatPanel::reload_key` re-reads it (the commands card's set and clear call it). A\n  `SetKeyRequested` from the view runs `chat.set_api_key` once that command exists; until\n  then it's logged.\n- **New chats** take `model`, `effort`, `show_thinking`, `max_tokens`, `fallbacks` from\n  `[chat]` in the settings global (`kiln_platform::config` `GlobalSettings<fe_config::Settings>`;\n  defaults when there's none, as in tests). A settings change doesn't touch an open chat.\n  A \"New Chat\" button in the panel header starts one (`chat.new`, context `chat`).\n- **First-run notice** (record §10): shown until dismissed; dismissal writes\n  `<state_dir>/chats/notice-seen` (fe-config's `state_dir`).\n\nTests in `crates/fe-workbench/src/chat_panel_tests.rs`, declared from `chat_panel.rs` under\n`#[cfg(test)]` (later chat cards declare their test files from this one), on GPUI's test\nplatform with `fe_chat::ScriptedProvider` in place of the real provider (a `#[cfg(test)]`\nprovider factory on the panel) and `kiln_core::secrets::MemoryStore` for the keychain. No\nnetwork.",
  "ref": "",
  "ref_blob": ""
}

## Implementation notes

- **`chat.open` isn't on `impl Workbench`.** `#[kiln::command] impl V` generates
  `impl ViewCommands for V`, so a second block on `Workbench` beside
  `workbench_commands.rs`'s doesn't compile. `chat.open` (context `workbench`) and
  `chat.new` (context `chat`) are declared in `#[kiln::command(context = "chat")]
  impl ChatPanel`; `chat_panel::init` adds an app `on_action` listener for
  `chat.open` that runs `Workbench::open_chat` in the dispatching window, as
  `find_commands::init` does for `find.show_replace`. The panel's own handler
  propagates to it.
- `run_dock_op` is `pub(crate)`, so `open_chat` shows the panel through it, as
  `git.file_history` does; `workbench.rs` is unchanged.
- The key source and the state directory are app globals (`KeySource`,
  `ChatStateDir`); under test both default to nothing (no key, no notice file), so
  no test reads the developer's key or state. The app's keychain is opened on its
  first read, on the background executor.
- The view's Open Settings runs `workbench.open_settings`. Set API Key and Open
  Settings run after the view's event, deferred, as they may come back to the panel.
- `message-square` credits `colebemis` in its `icons/message-square.json` at the
  commit, so both THIRD_PARTY.md files list it as Feather-derived (MIT).
