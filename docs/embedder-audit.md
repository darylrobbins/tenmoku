# Embedder audit: what a host can drive today

Spike T282 (Mac T1286). No production code changed;
`crates/fe-workbench/tests/embedder_command_surface.rs` pins the command
surface this audit names for hosts. Companion to `docs/ai-host.md` (the AI
seams) and the accessibility audit (`docs/accessibility-audit.md`), which
this one follows in shape: a baseline, then ordered follow-up cards.

"Today" is `e7142b5` with gpui-pre 0.3.9. "Proved by" names the test (or
example) that exercises the claim; "unverified" means no test does.

## Summary

- **The editor view alone embeds today.** `crates/fe-gpui/examples/ai_demo.rs`
  opens an `EditorView` in a plain GPUI app (no kiln app, no workbench) and
  drives every AI seam through public API; `crates/fe-gpui/tests/ai_demo_host.rs`
  pins its host glue headless.
- **Named commands are a real host API, with limits.** A host runs a typed
  command, or a registry command whose view is in the focus chain, by ID
  with an actor (`Invocation` + `kiln_platform::command_actions::dispatch_invocation`),
  observes dispatches (`observe_invocations`) and adds its own
  (`register_app_commands`, `#[kiln::command]`). Not every ID is reachable:
  the chat's `chat.review_changes` and `chat.propose_edit` run through the
  chat panel's private registries, which the panel dispatches itself as
  `Actor::Ai`; a host reaches review through the typed
  `chat.review_latest_changes`. Typed commands' metadata is listed only from
  a running app (`typed_commands(cx)`), and without args schemas.
- **The whole workbench embeds only by copying the binary's composition.**
  `fe_workbench::init` and `Workbench::new` are public, but the app around them
  (settings, theme, keymap, menus, extensions, windows; about 550 lines in
  `crates/fastedit/src/main.rs` plus `config.rs` and `theme.rs`) lives in the
  bin-only `fastedit` crate.
- **Three things force a fork for a workbench host**: the app's identity
  (`fe_config::APP_NAME`, `app_id()` and the `tenmoku` config and state
  directories) is read from inside library crates; the chat panel hard-codes
  the Anthropic provider; and the right dock's panel set is closed.
- **Nothing is consumable from outside yet**: every crate is
  `LicenseRef-Proprietary` and `publish = false` (relicensing is T1267 and
  T1268), all at 0.1.0, and an embedder must use the exact gpui-pre snapshot
  (`=0.3.9`) the workspace pins.

## Who embeds and how

Two shapes:

| Shape | What the host writes | What it gets | Entry points |
|---|---|---|---|
| **(a) The whole workbench in a host app** | A kiln app (`kiln::application()`, `kiln::init(cx, config)`) that registers fastedit's commands and opens `Workbench` windows, as `crates/fastedit/src/main.rs` does | Tabs, splits, the dock (Explorer, Search, Source Control, References, Problems, History, Chat, Call Hierarchy, AI Changes), the terminal, LSP, git, vim, the palette, settings and shortcuts editors, extensions | `fe_gpui::init_with_keymap`, `fe_workbench::init`, `fe_workbench::command_catalog`, `Workbench::new(Launch, EditorTheme, EditorSettings, ..)` |
| **(b) The editor view alone** | Its own GPUI app (kiln or not) with one or more `EditorView`s in its own layout | Text, syntax, multi-cursor, vim, folds, diagnostics, completion, hover and the AI seams (ghost text, streamed edits, proposals, decorations, AI undo) | `fe_gpui::init`, `EditorView::new(EditorModel, EditorTheme, cx)`, the hosts' provider traits (`CompletionProvider`, `HoverProvider`, `SignatureHelpProvider`, `InlayHintHost`, `LensProvider`) |

Which AI-tool use case needs which:

| Use case | Shape | Why |
|---|---|---|
| An AI IDE (a VS Code fork's job: files, terminal, git, LSP and an agent panel) | (a) | Needs the whole workbench, plus its own chat provider and panels, which force a fork today. |
| An agent's review or diff surface (show proposed hunks, accept or reject) | (b) | `EditorView::set_proposals` and `ProposalEvent` are all it needs. |
| Inline completion in another product's editor pane | (b) | Ghost text and `GhostTextEvent`. |
| A notebook, prompt or spec editor inside an AI app | (b) | An `EditorView` per cell, the host's own layout. |
| Driving the editor from an agent process (tool calls) | (a) or (b) | Commands by ID with `Actor::Ai`; `CommandMeta::args_schema` is the tool schema. |

Note for (b): `fe-gpui` still depends on `kiln-platform`, `kiln-shell` and
`kiln-ui` (`crates/fe-gpui/Cargo.toml`), so an editor-only host compiles
kiln's shell even if it never opens a kiln window.

## What a host can drive today

Each row: what, where (file and symbol), and the test or example that proves
it.

### Named commands by ID with an actor

| Claim | Where | Proved by |
|---|---|---|
| A command is an ID plus JSON args, an actor (`User`, `Ai { agent, session }`, `Lsp`, `Format`, `Host(name)`), a source and a target, serialized one JSON object per line. | `kiln/crates/kiln-command/src/invocation.rs`: `Invocation`, `Actor`, `Source`, `Target` | `kiln/crates/kiln-command/src/tests/registry.rs`: `invocations_round_trip` |
| A host runs a command in a window by ID from the focused element: a typed (`#[kiln::command]`) one as its action; any other as `RunCommand`, which runs only if a view in the focus chain has a registry that knows the ID (the editor's, the search panel's, the project panel's, the status bar's), else the app registry's (`app.*`, `window.*`, the host's own). | `kiln/crates/kiln-platform/src/command_actions.rs`: `dispatch_invocation`; `kiln/crates/kiln-platform/src/commands.rs`: `RunCommand`, `dispatch_app` | `kiln/crates/kiln-platform/tests/run_command_typed.rs`: `a_raw_run_command_naming_a_typed_command_runs_its_view_method`, `a_run_command_naming_no_typed_command_is_left_to_the_caller`; `kiln/crates/kiln-platform/tests/commands.rs`: `unhandled_run_command_reaches_the_app_registry`; `crates/fe-gpui/src/command_tests.rs`: `keystrokes_dispatch_through_the_registry` |
| Some command IDs are **not** reachable by `dispatch_invocation`: `chat.review_changes` and `chat.propose_edit` live in the chat panel's private registries, which the panel dispatches itself (`run_review_changes`, `run_propose_edit`) and reports to observers with `notify_observers`. A host asks for a review with the typed `chat.review_latest_changes` instead (latest turn only; no `turn` or `chat` args). | `crates/fe-workbench/src/chat_changeset.rs`: `ChatPanel::run_review_changes`; `crates/fe-workbench/src/chat_apply.rs`: `PROPOSE_EDIT`, `ChatPanel::run_propose_edit`; `crates/fe-workbench/src/workbench_commands.rs`: `review_latest_chat_changes` | `crates/fe-workbench/src/chat_changeset_tests.rs`: `the_typed_command_reviews_the_latest_turn` (the typed path); the private ones: n/a (a limit, card 3) |
| An editor command runs as its invocation's actor: its edit carries that actor's `Origin`, and `editor.undo` by an actor undoes only its own transactions. | `crates/fe-editor/src/commands.rs`: `registry`, `origin`, `EditorModel::run_as`; `crates/fe-gpui/src/commands.rs` dispatches the view's invocation into it | Headless: `crates/fe-editor/src/tests/commands.rs`: `actors_stamp_and_undo_their_own_transactions`, `crates/fe-editor/src/rows_tests.rs`: `row_commands_run_as_the_actor`. Through an `EditorView`: unverified (card 8). |
| A typed command's handler sees its invocation (who asked, from where, in which window). | `kiln/crates/kiln-platform/src/command_actions.rs`: `current_invocation`, `dispatch_window` | `kiln/crates/kiln/tests/macro_command.rs`: `a_handler_sees_its_invocation_and_window` (checks `source`; the `actor` field rides in the same `Invocation`, unasserted) |
| A host observes every successful dispatch (for a log, a recorder or an agent bridge). | `kiln/crates/kiln-platform/src/commands.rs`: `observe_invocations`, `notify_observers` | `kiln/crates/kiln-platform/tests/commands.rs`: `observers_see_app_commands`; `kiln/crates/kiln/tests/macro_panel.rs`: `observers_hear_of_a_command_once_whatever_focus_does_before_its_handler_runs` |
| A host adds its own app commands beside `app.*` and `window.*`. | `kiln/crates/kiln-platform/src/commands.rs`: `register_app_commands` | `kiln/crates/kiln-platform/tests/commands.rs`: `hosts_add_app_commands` |
| A kiln app declares commands, panels, items and status with attribute macros, and they reach its keys, palette and menus. | `kiln/crates/kiln-macros/src/lib.rs` (`command`, `panel`, `status`, `item`, `settings`, `model`); `kiln/crates/kiln/src/app_builder.rs`: `app`, `AppBuilder` | `kiln/crates/kiln/tests/host.rs`: `the_host_command_runs_by_key_from_the_palette_and_the_menus`; `kiln/crates/kiln/examples/host/` |
| Validating an ID and its args with no app: `command_catalog()` checks the workbench's typed commands, the chat view's and every registry's. It only checks; it can't list. `kiln::recorded_catalog` (inside it) knows the typed IDs and accepts them with null args only. | `crates/fe-workbench/src/lib.rs`: `command_catalog`; `kiln/crates/kiln/src/contributions.rs`: `recorded_catalog` | `crates/fe-workbench/src/command_catalog_tests.rs`: `the_default_keymaps_commands_check_with_no_app`, `the_default_menus_commands_check_with_no_app` |
| Listing the registry commands with no app: `command_list()` gives the search, commit box, project panel, status bar and editor registries' `CommandMeta` (id, title, description, context, and `args_schema`, the JSON schema of a command with args). It holds no typed command. | `crates/fe-workbench/src/lib.rs`: `command_list`; `kiln/crates/kiln-command/src/registry.rs`: `CommandMeta` | `crates/fe-workbench/src/command_catalog_tests.rs`: `the_palette_list_holds_no_typed_command`; `crates/fe-workbench/tests/embedder_command_surface.rs`: `listed_commands_are_unique_and_titled` |
| Listing the typed commands needs a running app, after `fe_workbench::init`: `typed_commands(cx)` gives each one's id, title and context, but **no args schema** (`TypedCommand` holds an action builder; `build(args)` checks a call). `kiln::Contributions::commands(cx)` gives the same plus keys and menu placement. So a host building an agent's tool list from the typed commands has no schema to give it. | `kiln/crates/kiln-platform/src/command_actions.rs`: `typed_commands`, `TypedCommand`; `kiln/crates/kiln/src/contributions.rs`: `Contributions::commands`, `CommandSpec` | `crates/fastedit/tests/app_name.rs`: `no_shipped_text_names_fastedit` (lists them with `typed_commands` and `command_list`); the missing schema: n/a (a gap, card 4) |
| The commands this audit names for hosts all check against the catalog with the args shown: `editor.insert {"text"}`, `editor.undo`, `editor.redo`, `ghost_text.accept`/`accept_word`/`accept_line`/`dismiss`, `proposal.accept_hunk`/`reject_hunk`/`accept_all`/`reject_all`/`next_hunk`/`previous_hunk`, `chat.open`, `chat.send`, `workbench.close_tab`, `command_palette.toggle`. | as above | `crates/fe-workbench/tests/embedder_command_surface.rs`: `commands_named_for_hosts_are_in_the_catalog` |
| The chat's own AI actions are invocations by `Actor::Ai { agent: "fastedit.chat", session: <chat id> }` that the app's observers hear: Review Changes (`chat.review_changes`) and Apply (`chat.propose_edit`). They go through the panel's private registries, not `dispatch_invocation`, so a host observes and records them but can't replay them by ID (row above). | `crates/fe-workbench/src/chat_changeset.rs`: `ChatPanel::review_turn`, `run_review_changes`; `crates/fe-workbench/src/chat_apply.rs`: `run_propose_edit` | `crates/fe-workbench/src/chat_changeset_tests.rs`: `review_is_dispatched_as_ai_actor`; `crates/fe-workbench/src/chat_apply_tests.rs`: `apply_is_an_ai_invocation` |
| A plugin's dispatch (`Actor::Host("<extension id>")`) is authorized against its grants. | `kiln/crates/kiln-command/src/authorize.rs`: `PluginPolicy`; `kiln/crates/kiln-command/src/registry.rs` | `kiln/crates/kiln-command/tests/plugin_authorize.rs` |

Note: `command_list()` holds one entry per ID **and context**: `editor.save`
is listed for the editor and again for a Markdown preview shown in its place
(`crates/fe-workbench/src/markdown_preview.rs`: `palette_commands`). A host
building a tool list from it must key on the ID.

### The AI seams (`docs/ai-host.md`)

| Claim | Where | Proved by |
|---|---|---|
| Ghost text at the caret, shown and cleared by the host; the user takes it with `ghost_text.*`; the host hears `Accepted { transaction, text, whole }` or `Dismissed`. | `crates/fe-editor/src/ghost_text.rs`: `GhostText`; `crates/fe-gpui/src/view.rs`: `EditorView::set_ghost_text`, `GhostTextEvent` | `crates/fe-gpui/src/view/ghost_text_tests.rs`: `typing_consumes_and_stale_drops`; `crates/fe-gpui/src/view/ghost_text_key_tests.rs`: `tab_accepts_one_undo_step`; `crates/fe-gpui/tests/ai_demo_host.rs`: `full_accept_then_undo_is_not_reoffered`; `crates/fe-workbench/tests/palette_ghost_text.rs`: `palette_accepts_the_suggestion` |
| A streamed edit: one AI-origin transaction however many chunks, its region moving with the user's typing. | `crates/fe-buffer/src/stream.rs`: `EditStream::begin`, `push`, `finish` | `crates/fe-buffer/tests/ai_stream.rs`: `typing_during_a_stream`, `conflicts_and_undone` |
| Undo AI: `Buffer::undo(Origin::is_ai)` reverts the AI's transactions only. | `crates/fe-buffer/src/buffer.rs`: `Buffer::undo` | `crates/fe-gpui/tests/ai_demo_host.rs`: `hand_undo_redo_updates_touched_rows` |
| A proposed change as diff blocks with Accept and Reject; the host resolves hunks or hears `ProposalEvent`. | `crates/fe-buffer/src/proposed.rs`: `ProposedChange::from_text`; `EditorView::set_proposals`, `accept_hunk`, `reject_hunk`, `ProposalEvent` | `crates/fe-gpui/src/view/proposal_accept_tests.rs`: `accept_button_applies_hunk`, `cmd_z_undoes_an_accepted_hunk`; `crates/fe-gpui/src/view/proposal_command_tests.rs`: `host_and_failures` |
| Marking what the AI touched with row backgrounds that follow edits. | `crates/fe-editor/src/decorations.rs`: `DecorationSet`; `EditorModel::add_decoration_provider` | `crates/fe-editor/src/decorations_tests.rs`: `follows_edits_before_inside_and_over`; `crates/fe-gpui/tests/ai_demo_host.rs`: `hand_accepted_hunk_still_marked` |
| A changeset across files, reviewed in the AI Changes panel (workbench only). | `crates/fe-editor/src/changeset.rs`; `crates/fe-workbench/src/ai_changes_panel.rs` | `crates/fe-editor/tests/changeset.rs`; `crates/fe-workbench/src/chat_changeset_tests.rs`: `review_builds_changeset_for_all_blocks` |

### The editor view as a component

| Claim | Where | Proved by |
|---|---|---|
| `EditorView::new(model, theme, cx)` in any GPUI window, after `fe_gpui::init(cx)` (or `init_with_keymap` with the host's keymap). | `crates/fe-gpui/src/view.rs`: `EditorView::new`; `crates/fe-gpui/src/lib.rs`: `init`, `init_with_keymap` | `crates/fe-gpui/examples/ai_demo.rs`; `crates/fe-gpui/tests/ai_demo_host.rs` |
| `EditorTheme` is plain data: fonts, metrics and every colour; `default_dark`, `from_settings`, `from_palette`; the app-wide palette is `set_editor_palette`. | `crates/fe-gpui/src/lib.rs`: `EditorTheme`; `crates/fe-gpui/src/theme.rs`: `set_editor_palette` | `crates/fe-gpui/src/theme_tests.rs`: `from_settings_is_fastedit_dark`, `set_colors_keeps_metrics` |
| A host's own `EditorHost` drives an `EditorModel` headless through the editor registry (no GPUI). | `crates/fe-editor/src/commands.rs`: `EditorHost`, `EditorTarget`, `HeadlessHost`, `registry` | `crates/fe-editor/src/tests/commands.rs`: `headless_dispatch_matches_model_calls`, `recorded_invocations_replay`; `crates/fe-editor/tests/occurrence_carets.rs` (its own `EditorHost`) |
| Inside an `EditorView` the host is fe-gpui's own: a host can't substitute its `EditorHost` there. | `crates/fe-gpui/src/commands.rs:251`: `pub(crate) struct ViewHost` | n/a (a limit, listed under forks) |
| Language features come from the host's providers: completion, hover, signature help, inlay hints, lenses, gutter annotations, host blocks. | `crates/fe-gpui/src/lib.rs` re-exports: `CompletionProvider`, `HoverProvider`, `SignatureHelpProvider`, `InlayHintHost`, `LensProvider`, `GutterAnnotations`, `HostBlock` | `crates/fe-gpui/src/view/lens_tests.rs`, `crates/fe-gpui/src/signature_help_tests.rs`, `crates/fe-gpui/src/annotation_column_tests.rs`: `entries_paint_once_per_run_tinted_by_heat`; completion and hover: through the workbench's LSP (`crates/fe-workbench/src/tests/lsp.rs`) |
| Vim is an app-wide switch. | `crates/fe-gpui/src/vim.rs`: `set_vim_enabled` | `crates/fe-gpui/src/vim/tests.rs` |

### The workbench as a component

| Claim | Where | Proved by |
|---|---|---|
| `fe_workbench::init(cx)` registers the workbench's, the editor's and the chat view's commands (after `fe_gpui::init`). | `crates/fe-workbench/src/lib.rs`: `init` | Every workbench test app; `crates/fe-workbench/src/command_catalog_tests.rs`: `the_palette_lists_what_it_listed` |
| `Workbench::new(Launch { root, file }, EditorTheme, EditorSettings, window, cx)` opens on a folder, a file or nothing; `resolve_launch` turns a CLI arg into a `Launch`. | `crates/fe-workbench/src/workbench.rs`: `Workbench::new`, `Launch`, `resolve_launch` | `crates/fe-workbench/src/workspace_embed_tests.rs`: `the_pane_group_is_the_workspaces` |
| `set_window_opener`: the host decides how `window.new` and Open Folder in New Window open windows. | `crates/fe-workbench/src/open.rs`: `set_window_opener` | `crates/fe-workbench/src/windows_tests.rs`: `new_windows_come_from_the_host` |
| `set_server_launcher`: the host launches language servers (a container, a remote). | `crates/fe-workbench/src/lsp.rs`: `set_server_launcher`, `fe_lsp::ServerLauncher` | `crates/fe-workbench/src/tests/lsp.rs`: `host_launcher_replaces_local` |
| `set_lsp_settings`: the `[lsp]` settings workbenches start with. | `crates/fe-workbench/src/lsp.rs`: `set_lsp_settings` | Unverified: no test calls it (tests reach `[lsp]` through `Workbench::reload_settings`, `crates/fe-workbench/src/tests/lsp.rs`: `settings_and_shutdown`). |
| `set_trust_file`: where folder trust is kept. | `kiln/crates/kiln-shell/src/trust.rs`: `set_trust_file`, re-exported by `crates/fe-workbench/src/trust.rs` | `crates/fe-workbench/src/trust_tests.rs`: `first_open_prompts`, `writes_keep_other_decisions` |
| Extension servers and their consent file. | `crates/fe-workbench/src/lsp_consent.rs`: `set_extension_servers`, `set_extension_consent_file` | `crates/fe-workbench/src/lsp_consent_prompt_tests.rs`, `crates/fe-workbench/src/extensions_picker_tests.rs` |
| These setters are app globals: one opener, launcher, trust file and LSP default per process, shared by every workbench window. | `cx.set_global` in each setter above | by construction |

### Panels and docks

| Claim | Where | Proved by |
|---|---|---|
| The workbench's right dock shows fastedit's panels; a host can show, read and toggle them. | `crates/fe-workbench/src/dock.rs`: `Dock`, `Dock::show`, `DockPanel` constants; `Workbench::dock` (`crates/fe-workbench/src/workbench_chrome.rs`) | `crates/fe-workbench/src/sidebar_panels_tests.rs`, `crates/fe-workbench/src/dock_position_tests.rs` |
| A host can **not** add its own panel to the workbench's dock: the set is the static `PANELS` and `DockPanel::new` is `pub(crate)`. | `crates/fe-workbench/src/dock.rs:75` (`DockPanel::new`), `:133` (`PANELS`) | n/a (fork) |
| A kiln app (shape (a) without fastedit's workbench, or a host's own window) adds its own dock panels with `#[kiln::panel]` or `AppBuilder::panel`. | `kiln/crates/kiln-shell/src/dock.rs`: `DockItem`, `DockState`; `kiln/crates/kiln/src/app_builder.rs`: `AppBuilder::panel` | `kiln/crates/kiln/tests/macro_panel.rs`: `a_panel_shows_in_its_dock_toggled_by_its_key_the_palette_and_the_view_menu`; `kiln/crates/kiln-shell/tests/dock.rs` |
| Panes take a host's own items (`PaneItem`). | `kiln/crates/kiln-shell/src/item.rs`: `PaneItem`; `AppBuilder::item` | `kiln/crates/kiln/tests/macro_item.rs`; `kiln/crates/kiln/tests/host.rs`: `a_split_shows_the_conversation_twice_and_the_panel_opens_another`. In the workbench's panes: unverified. |
| Status bar items. | `crates/fe-workbench/src/status_bar.rs`: `StatusBar`, `StatusItem`; `AppBuilder::status` | `kiln/crates/kiln/tests/macro_panel.rs`: `a_status_fn_shows_on_the_bar_with_its_new_text_when_the_bar_next_draws` |

### Theming and branding

| Claim | Where | Proved by |
|---|---|---|
| A kiln app chooses its themes: dark and light mode, extension themes, the user's `theme.toml` over them. | `kiln/crates/kiln/src/app_builder.rs`: `theme_mode`, `theme_extensions`; `kiln/crates/kiln-ui/src/theme.rs` | `kiln/crates/kiln/tests/extension_themes.rs`: `extension_themes_are_listed_and_selected`, `the_user_theme_lays_over_the_selected_theme`; `kiln/crates/kiln/tests/host.rs`: `the_bubble_colour_comes_from_the_users_theme_or_its_default` |
| The editor's colours follow the palette a host sets. | `fe_gpui::set_editor_palette`, `EditorTheme::from_palette` | `crates/fe-workbench/src/theme_tests.rs`, `crates/fe-gpui/src/theme_tests.rs` |
| A kiln app's name, id and paths. | `kiln::app(id, name)`, `AppBuilder::short_name`, `paths`; `kiln_core::AppId` | `kiln/crates/kiln/tests/app_builder.rs`: `the_short_name_and_paths_follow_the_name_unless_given` |
| Menus and keys from the host's own tables. | `kiln/crates/kiln/src/setup.rs`: `Config::menus`, `Config::keymap`; `AppBuilder::menus`, `keys`, `menu_bar` | `kiln/crates/kiln/tests/host.rs`: `keymap_and_menus_load_cleanly_on_every_platform` |
| The workbench and editor brand themselves Tenmoku regardless: window and tab titles, "Hide Tenmoku", the bundled theme names, the welcome text. | See "What forces a fork" | n/a |
| The app icon is set by the binary only (macOS Dock icon). | `crates/fastedit/src/app_icon.rs`: `set` | n/a (bin-only) |
| The title bar is kiln-shell's per platform; the workbench sets its text. | `crates/fe-workbench/src/title_bar.rs`: `titlebar_options`, `NO_ROOT`; `kiln/crates/kiln-shell/src/title_bar.rs` | `crates/fe-workbench/src/tests/window_frame.rs` |

### Settings and keymaps

| Claim | Where | Proved by |
|---|---|---|
| Settings are typed and layered: compiled defaults, the user's file, the project's, with a project filter narrowing what a project may set. | `kiln/crates/kiln-core/src/settings.rs`: `SettingsStore`, `ProjectFilter`; `crates/fe-config/src/settings.rs`: `SettingsStore::load(user, project)` | `kiln/crates/kiln/tests/app_builder.rs`: `a_settings_override_replaces_the_conventional_store`; `crates/fe-config/src/tests` |
| A host ships its own default keymap and menus (the user's keymap layers over them). | `fe_gpui::init_with_keymap`; `kiln::Config::keymap`, `menus`; `kiln-platform`'s `SharedKeymap::defaults` | `kiln/crates/kiln-platform/tests/keymap_store_defaults.rs`; `kiln/crates/kiln/tests/default_keys.rs` |
| A host can't ship **default settings** other than `Settings::default()`: there is no host layer between the compiled defaults and the user's file. | `crates/fe-config/src/settings.rs`: `SettingsStore::load` | n/a (gap) |
| A host can't **lock** a setting or a binding: the user's file always wins; `ProjectFilter` narrows only the project layer, and the user's file is never narrowed. | `kiln/crates/kiln-core/src/settings.rs:80` | n/a (gap) |

### Chat

| Claim | Where | Proved by |
|---|---|---|
| The chat's model seam is a trait: a `ChatRequest` in, a stream of `ChatEvent`s out, dropped to cancel. | `crates/fe-chat/src/provider.rs`: `ChatProvider` | `crates/fe-chat/tests/conversation.rs`; `crates/fe-chat-anthropic/tests/provider.rs` (the Anthropic one) |
| `ChatView` runs over any provider a host gives it, and the host hears `ChatViewEvent`s (apply a block, open a path). | `crates/fe-chat-view/src/view.rs`: `ChatView::new(provider, ..)`, `set_provider`, `ChatViewEvent` | `crates/fe-chat-view/tests/view.rs`: `a_sent_message_streams_into_the_transcript`, `stop_cancels_the_stream` (with `fe_chat::ScriptedProvider`) |
| Inline edit from a selection and an instruction to a proposed change, over any provider. | `crates/fe-chat/src/inline.rs`: `inline_request`, `run_inline_edit`, `to_proposal` | `crates/fe-chat/tests/inline_edit.rs` |
| In the workbench, the chat panel is Claude only: `ChatPanel::provider` builds `AnthropicProvider` outside tests; the provider factory seam is `#[cfg(test)]`, and `ChatPanel` isn't exported. | `crates/fe-workbench/src/chat_panel.rs:1598-1602` (`provider`), `:1614-1617` (`set_provider_factory`, test only); `crates/fe-workbench/src/lib.rs:113` | n/a (fork) |

### Plugins and extensions

| Claim | Where | Proved by |
|---|---|---|
| Installed (user) extensions contribute all six kinds: themes, keymaps and menus (kiln's), and language servers, grammars and snippets (fastedit's own kinds), hot reloaded. | `kiln/crates/kiln-core/src/extension.rs`; `kiln/crates/kiln/src/extensions.rs`; `crates/fastedit/src/config.rs`: `with_extensions` | `kiln/crates/kiln-core/tests/extension_discovery.rs`; `kiln/crates/kiln/tests/extensions_reload.rs`: `extensions_load_and_reload_through_init`; `kiln/crates/kiln/tests/extension_themes.rs`: `extension_themes_are_listed_and_selected`; `kiln/crates/kiln/tests/extension_keys_menus.rs`: `extension_bindings_sit_between_app_and_user`; `crates/fastedit/tests/extension_servers.rs`: `extension_lsp_servers_reach_the_workbench`; `crates/fastedit/tests/extension_grammars.rs`: `extension_grammars_register`; `crates/fastedit/tests/extension_snippets.rs`: `a_manifest_declares_snippets` |
| A trusted workspace's own extensions (`<root>/.tenmoku/extensions`) contribute only the kinds the app allows: fastedit allows `grammars`, `lsp` and `snippets`; themes, keymaps, menus and any other kind are dropped with a warning. | `kiln/crates/kiln/src/setup.rs`: `Config::workspace_extensions`; `kiln/crates/kiln-core/src/extension.rs`: `ExtensionApi::only_kinds`; `crates/fastedit/src/config.rs:163`: `WORKSPACE_KINDS` | `kiln/crates/kiln-core/tests/extension_workspace.rs`: `a_workspace_manifest_keeps_only_language_kinds`; `kiln/crates/kiln/tests/extension_workspace.rs`: `trusting_loads_and_withdrawing_trust_drops_it`; `crates/fastedit/tests/extension_workspace.rs`: `a_workspace_grammar_goes_with_trust`, `a_trusted_workspace_server_needs_its_own_consent`. That fastedit's three are exactly the allowed set: unverified (no test reads `WORKSPACE_KINDS`). |
| A host declares its own extension kinds with a JSON schema. | `kiln/crates/kiln/src/setup.rs`: `Config::extension_kind` | `crates/fastedit/src/config.rs` registers `lsp`, `grammars`, `snippets`; `kiln/crates/kiln/tests/extension_workspace.rs` |
| Code plugins (WebAssembly components on wasmtime) run headless with grants, commands, events, editor read and edit, decorations, status, fs, process and network. | `kiln/crates/kiln-plugin/src/lib.rs` (`runner`, `grants`, `commands`, `events`); `crates/fe-plugin/src/lib.rs` (`fastedit:editor@1.0.0`) | `kiln/crates/kiln-plugin/tests/*.rs` (e.g. `hostile.rs`, `grants.rs`), `crates/fe-plugin/tests/editor.rs`, `status.rs` |
| Code plugins don't run in the app yet: the workbench depends on `kiln-plugin` (for its install prompt, `crates/fe-workbench/src/plugin_prompt.rs`) but not on `fe-plugin`, and the "running granted plugins in the app" card of `docs/decisions/vmtnmkay62-extensions-and-plugins.md` § 10 is open. | `crates/fe-workbench/Cargo.toml` | n/a |

## What forces a fork today

A host must patch source for each of these. Line numbers are at `e7142b5`.

**The app is a binary, not a library.**

- `crates/fastedit/Cargo.toml`: `fastedit` is `[[bin]] tenmoku` only. Its
  composition is what a workbench host needs and can't call:
  `crates/fastedit/src/main.rs:391-550` (`main`: settings load, `kiln::init`,
  theme, vim switches, the workbench setters, window layouts and restore),
  `crates/fastedit/src/config.rs:39` (`shipped`: commands, catalog, keymap,
  menus, theme extensions) and `:60` (`config`: the user's files and
  extensions), `crates/fastedit/src/theme.rs` (editor theme from kiln's).
  A host copies about 1,300 lines and keeps them in step.

**The app's identity is read inside library crates** (`fe_config::APP_NAME`,
`fe_config::paths::app_id()`, the `tenmoku` XDG directories):

- `crates/fe-config/src/paths.rs:21` `APP_NAME = "Tenmoku"`, `:25-26`
  `app_id()` = `dev.tenmoku.tenmoku`, and every path function after it
  (`config_dir`, `state_dir`, `user_settings_path`, `user_theme_path`,
  `user_vimrc_path`, `project_settings_path` = `<root>/.tenmoku/settings.toml`,
  `trust_path`).
- `crates/fe-config/src/recent.rs:37`: the recent list in `state_dir()`.
- `crates/fe-keymap/src/lib.rs:41-42`: `user_keymap_path` from `app_paths()`.
- `crates/fe-gpui/src/commands.rs:45`, `:164`, `:229`: the app commands are
  titled for `APP_NAME` ("Hide Tenmoku"), even for an editor-only host.
- Window titles: `crates/fe-gpui/src/pane_item.rs:848`,
  `crates/fe-gpui/src/diff_view.rs:1011`,
  `crates/fe-gpui/src/file_viewer/pane_item.rs:31`,
  `crates/fe-gpui/src/image_viewer/pane_item.rs:36`,
  `crates/fe-workbench/src/workbench.rs:487`, `crates/fe-workbench/src/pane.rs:539`,
  `crates/fe-workbench/src/title_bar.rs:15`,
  `crates/fe-workbench/src/settings_editor.rs:1158`,
  `crates/fe-workbench/src/shortcuts_editor.rs:1666`,
  `crates/fe-workbench/src/markdown_preview.rs:394`.
- Files the workbench writes: `crates/fe-workbench/src/settings_editor_controls.rs:210-211`
  and `crates/fe-workbench/src/settings_editor.rs:394`, `:422` (user and
  project settings), `crates/fe-workbench/src/open.rs:207-210`,
  `crates/fe-workbench/src/workbench_root.rs:36`, `crates/fe-workbench/src/lsp.rs:1051`.
- The chat's keychain service and state: `crates/fe-workbench/src/chat_panel.rs:134`
  (`key_service` = the app id), `:245` (`state_dir`; the override global
  `ChatStateDir` at `:230` is `pub(crate)`).
- Strings sent outside: `crates/fe-lsp/src/server.rs:2260` (LSP `clientInfo.name`
  "Tenmoku"), `crates/fe-chat-anthropic/src/wire.rs:91` (user agent
  `tenmoku/<version>`), `crates/fe-terminal/src/session.rs:294`
  (`TERM_PROGRAM=tenmoku`), `crates/fe-git/src/branch.rs:151` (stash message).
- Shown text: `crates/fe-workbench/src/workbench.rs:63` (`WELCOME`),
  `crates/fe-lsp/src/registration.rs:68`, `crates/fe-workbench/src/workspace_edit.rs:377`,
  `crates/fe-vim/src/vimrc.rs:35`, `crates/fe-workbench/src/lsp.rs:421`.
- Bundled themes and default menus: `crates/fe-config/src/themes.rs:58-60`
  and `:73` ("Tenmoku Dark", "Tenmoku Light", "Tenmoku Ember"),
  `crates/fe-config/src/theme.rs:195`, `:202`, `crates/fe-workbench/src/theme_picker.rs:200-201`,
  `crates/fe-keymap/menus/fastedit.toml:12`, `:16`, `:53` (replaceable by a
  host's own menus through `kiln::Config::menus`, so this one alone doesn't
  force a fork).

**Closed or `pub(crate)` seams:**

- The chat provider: `crates/fe-workbench/src/chat_panel.rs:1598-1602`
  (`AnthropicProvider` outside tests), `:1614-1617` (`set_provider_factory`,
  `#[cfg(test)]`); `ChatPanel` is private (`crates/fe-workbench/src/lib.rs:113`).
- The dock's panels: `crates/fe-workbench/src/dock.rs:75` (`DockPanel::new`,
  `pub(crate)`), `:133` (`PANELS`), and `Dock::new` takes fastedit's panel
  entities only (`:379`).
- The view's command host: `crates/fe-gpui/src/commands.rs:251`
  (`pub(crate) struct ViewHost`).
- The panels a host would extend or embed on their own are private:
  `HierarchyPanel`, `HistoryPanel`, `GitLogPanel` (`crates/fe-workbench/src/lib.rs:114-115`),
  `ProblemsPanel` and `AiChangesPanel` (`pub(crate)` in `dock.rs`).

**Globals.** `set_window_opener`, `set_server_launcher`, `set_lsp_settings`,
`set_trust_file`, `set_extension_servers`, `set_vim_enabled` and the editor
palette are one per GPUI `App`: a host can't run two differently configured
workbenches in one process. Not a fork for most hosts; listed so it's known.

**Settings and keymaps.** No host defaults layer and no locked keys (above):
a host that must ship different defaults or enforce a policy patches
`crates/fe-config/src/settings.rs` and `kiln/crates/kiln-core/src/settings.rs`.

## Public surface and stability

How the counts were made: "pub lines" is `^pub ` at the top level of
`src/lib.rs` (modules, re-exports and items; `pub mod` counts as one);
"names" counts each re-exported name in a `pub use {..}` and each `pub`
item or module. Neither counts what a `pub mod` holds. "GPUI" is from
`cargo metadata`: **direct** (depends on `gpui-pre`), **transitive** (through
another crate) or **none**. "src lines" is all `.rs` under `src/`, tests
included.

Every crate: licence `LicenseRef-Proprietary`, `publish = false`, version
0.1.0. The `crates/*` set them through `license.workspace` and
`publish.workspace` (root `Cargo.toml`); the `kiln/crates/*` set them
literally in their own `Cargo.toml`, so relicensing (T1267, T1268) touches
both.

| Crate | Kind | pub lines (mod, use) | names | GPUI | src lines | Workspace deps | Proposed tier |
|---|---|---|---|---|---|---|---|
| `crates/fastedit` | bin `tenmoku` | n/a | n/a | direct | 3,969 | fe-buffer, fe-config, fe-gpui, fe-lsp, fe-syntax, fe-vim, fe-workbench, kiln, kiln-platform | internal (its composition moves to a library: card 6) |
| `crates/fe-buffer` | lib | 9 (0, 9) | 45 | none | 7,149 | fe-text | **stable**: `Buffer`, `Origin`, `EditStream`, `ProposedChange`, transactions |
| `crates/fe-chat` | lib | 8 (0, 8) | 57 | none | 2,205 | fe-buffer | **stable**: `ChatProvider` and its types; the rest unstable |
| `crates/fe-chat-anthropic` | lib | 6 (0, 6) | 16 | none | 1,133 | fe-chat, fe-net, kiln-command | unstable |
| `crates/fe-chat-view` | lib | 5 (0, 2) | 14 | direct | 2,544 | fe-buffer, fe-chat, fe-editor, fe-gpui, fe-markdown, fe-markdown-view, kiln, kiln-ui | unstable |
| `crates/fe-config` | lib | 23 (10, 12) | 77 | none | 4,350 | kiln-core | unstable (paths become host-set: card 1) |
| `crates/fe-display` | lib | 12 (3, 6) | 25 | none | 12,297 | fe-buffer, fe-text | internal |
| `crates/fe-editor` | lib | 29 (15, 11) | 45 | none | 25,207 | fe-buffer, fe-display, fe-text, kiln-command | **stable** for `EditorModel`, `commands` (`EditorHost`, `registry`), `GhostText`, `decorations`, `changeset`; other modules unstable |
| `crates/fe-fileview` | lib | 10 (0, 3) | 18 | none | 1,926 | fe-text | internal |
| `crates/fe-git` | lib | 17 (3, 10) | 28 | none | 4,040 | (none) | internal |
| `crates/fe-gpui` | lib | 43 (10, 28) | 92 | direct | 69,182 | fe-buffer, fe-config, fe-display, fe-editor, fe-fileview, fe-keymap, fe-syntax, fe-text, fe-vim, kiln-command, kiln-platform, kiln-shell, kiln-ui | **stable** for `init`, `EditorView`, `EditorTheme`, the event and provider types; the rest unstable |
| `crates/fe-keymap` | lib | 17 (0, 5) | 67 | none | 1,562 | fe-config, kiln-command, kiln-core | unstable |
| `crates/fe-lsp` | lib | 39 (11, 28) | 90 | none | 37,083 | fe-buffer, fe-config, fe-nav, fe-text | unstable (`ServerLauncher` is a host seam) |
| `crates/fe-markdown` | lib | 10 (0, 1) | 14 | none | 2,090 | fe-buffer, fe-syntax | internal |
| `crates/fe-markdown-view` | lib | 6 (0, 5) | 14 | direct | 2,588 | fe-gpui, fe-markdown, fe-syntax, kiln-ui | internal |
| `crates/fe-nav` | lib | 9 (0, 9) | 43 | none | 8,158 | fe-buffer, fe-project, fe-syntax, fe-text | internal |
| `crates/fe-net` | lib | 6 (0, 2) | 6 | none | 692 | kiln-command | internal |
| `crates/fe-plugin` | lib | 6 (4, 0) | 6 | none | 1,464 | fe-buffer, fe-editor, fe-net, fe-text, kiln-command, kiln-plugin | unstable (its WIT world is the plugin contract, versioned on its own) |
| `crates/fe-project` | lib | 8 (2, 6) | 40 | none | 14,802 | (none) | internal |
| `crates/fe-syntax` | lib | 16 (0, 16) | 62 | none | 15,188 | fe-buffer | unstable (`Language` for a host's `set_language`) |
| `crates/fe-terminal` | lib | 6 (1, 5) | 24 | none | 3,933 | (none) | internal |
| `crates/fe-terminal-view` | lib | 7 (2, 5) | 15 | direct | 3,108 | fe-terminal | internal |
| `crates/fe-text` | lib | 8 (0, 3) | 10 | none | 1,842 | (none) | **stable** (the text source under `fe-buffer`) |
| `crates/fe-vim` | lib | 29 (7, 20) | 113 | none | 71,102 | fe-buffer, fe-editor, fe-text, kiln-command | internal |
| `crates/fe-workbench` | lib | 27 (1, 22) | 67 | direct | 183,651 | every `fe-*` but fe-plugin, fe-terminal; kiln, kiln-command, kiln-platform, kiln-plugin, kiln-shell, kiln-ui | unstable; `init`, `command_catalog`, `command_list`, `Workbench`, `Launch` and the `set_*` seams become stable after cards 1-6 |
| `kiln/crates/kiln` | lib (facade) | 38 (7, 31) | 72 (+2 `macro_export`) | direct | 8,449 | kiln-command, kiln-core, kiln-macros, kiln-platform, kiln-shell, kiln-ui | **stable** (the facade an app depends on; re-exports GPUI as `kiln::gpui`) |
| `kiln/crates/kiln-command` | lib | 8 (1, 7) | 30 | none | 2,075 | (none) | **stable**: `CommandId`, `Invocation`, `Actor`, `CommandMeta`, `Registry` |
| `kiln/crates/kiln-core` | lib | 34 (15, 19) | 88 | none | 13,707 | kiln-command | unstable (reached through `kiln`) |
| `kiln/crates/kiln-macros` | proc-macro | 6 (0, 0) | 6 | none | 1,803 | (none) | internal (reached through `kiln`'s re-exports) |
| `kiln/crates/kiln-platform` | lib | 15 (11, 4) | 16 | direct | 3,697 | kiln-command, kiln-core | unstable (its `commands` and `command_actions` modules are the dispatch API; candidates for `kiln`'s stable surface) |
| `kiln/crates/kiln-plugin` | lib | 15 (15, 0) | 15 | none | 4,601 | kiln-command, kiln-core | unstable |
| `kiln/crates/kiln-shell` | lib | 37 (18, 16) | 72 | direct | 10,421 | kiln-command, kiln-core, kiln-platform, kiln-ui | unstable |
| `kiln/crates/kiln-ui` | lib | 25 (1, 24) | 95 | direct | 10,308 | kiln-command, kiln-core, kiln-platform | unstable |

GPUI coupling an embedder inherits:

- **gpui-pre `=0.3.9`**, the crates.io snapshot of Zed at `93d7456`, with
  `gpui-pre-platform` and `gpui-pre-sum-tree` pinned to the same `=0.3.9`
  (root `Cargo.toml`, `[workspace.dependencies]`). A host's own GPUI code must
  use that exact version: GPUI types (`Entity`, `Window`, `App`, `Hsla`) cross
  every GPUI-coupled API above, and two GPUI versions in one binary don't
  share them. There is no vendored GPUI fork today (`vendor/` holds only
  `tree-sitter` and `tree-sitter-yaml`, `[patch.crates-io]`), so a host can
  match the pin from crates.io; a future move to a Zed git revision would
  need the host's `[patch]` too.
- Every crate marked **none** stays usable headless: `scripts/check-no-gpui.sh`
  fails any crate outside its `UI_CRATES` list that depends on GPUI, and
  kiln's README states `kiln-command` and `kiln-core` never will. An agent process can load commands,
  keymaps and settings, and run `EditorModel` commands, with no GPUI.
- The toolchain: edition 2024, `rust-version = "1.98"` (root `Cargo.toml`).
- Public structs with all-`pub` fields and no `#[non_exhaustive]` break a host
  on every added field: `EditorTheme` (`crates/fe-gpui/src/lib.rs:118`, 60
  fields), `Launch` (`crates/fe-workbench/src/workbench.rs:74`),
  `EditorTarget`, `CommandMeta`.

## Proposed follow-up cards

Ordered. Sizes are xs-xl. Card 1 unblocks the most; 8 is independent and
cheap. A "new" file doesn't exist yet; every other path does.

1. **Host-set app identity in the libraries** — type: feature; size: m.
   An `AppIdentity` (name, `AppId`, `AppPaths`) global set by the host,
   defaulting to Tenmoku's, read wherever `fe_config::APP_NAME`,
   `paths::app_id()` and the path functions are read today.
   Files: `crates/fe-config/src/paths.rs`, `crates/fe-config/src/lib.rs`,
   `crates/fe-config/src/recent.rs`, `crates/fe-keymap/src/lib.rs`,
   `crates/fe-gpui/src/commands.rs`, `crates/fe-gpui/src/pane_item.rs`,
   `crates/fe-gpui/src/diff_view.rs`, `crates/fe-gpui/src/file_viewer/pane_item.rs`,
   `crates/fe-gpui/src/image_viewer/pane_item.rs`, `crates/fe-workbench/src/workbench.rs`,
   `crates/fe-workbench/src/pane.rs`, `crates/fe-workbench/src/title_bar.rs`,
   `crates/fe-workbench/src/settings_editor.rs`, `crates/fe-workbench/src/settings_editor_controls.rs`,
   `crates/fe-workbench/src/shortcuts_editor.rs`, `crates/fe-workbench/src/markdown_preview.rs`,
   `crates/fe-workbench/src/open.rs`, `crates/fe-workbench/src/workbench_root.rs`,
   `crates/fe-workbench/src/lsp.rs`, `crates/fe-workbench/src/chat_panel.rs`,
   `crates/fastedit/src/main.rs`. Dependencies: none.
2. **A host's chat provider in the workbench** — feature; s.
   `fe_workbench::set_chat_provider(cx, factory)` replacing the
   `#[cfg(test)]` `set_provider_factory`, Anthropic the default; the chat's
   key UI hidden when the host's provider needs no key.
   Files: `crates/fe-workbench/src/chat_panel.rs`, `crates/fe-workbench/src/lib.rs`,
   `crates/fe-workbench/src/chat_panel_tests.rs`. Dependencies: none.
3. **The chat's AI actions as commands a host can run** — feature; s.
   `chat.review_changes { chat, turn }` and `chat.propose_edit` become typed
   commands (or the panel takes their `RunCommand`), so `dispatch_invocation`
   reaches them and a recorded invocation replays; the panel's own buttons go
   through the same path.
   Files: `crates/fe-workbench/src/chat_changeset.rs`, `crates/fe-workbench/src/chat_apply.rs`,
   `crates/fe-workbench/src/chat_panel.rs`, `crates/fe-workbench/src/workbench_commands.rs`,
   `crates/fe-workbench/src/chat_changeset_tests.rs`, `crates/fe-workbench/src/chat_apply_tests.rs`.
   Dependencies: none.
4. **Args schemas and a headless list for typed commands** — feature; m.
   `#[kiln::command]` records its args type's JSON schema; `TypedCommand`
   and a new `kiln::recorded_commands(krate) -> Vec<CommandMeta>` expose it
   with no app, and `fe_workbench::command_list` (or a sibling
   `host_commands()`) lists typed commands with schemas, so a host builds an
   agent's tool list from one call.
   Files: `kiln/crates/kiln-macros/src/command.rs`, `kiln/crates/kiln-platform/src/command_actions.rs`,
   `kiln/crates/kiln/src/contributions.rs`, `kiln/crates/kiln/src/lib.rs`,
   `kiln/crates/kiln/tests/recorded_catalog.rs`, `crates/fe-workbench/src/lib.rs`,
   `crates/fe-workbench/src/command_catalog_tests.rs`, `crates/fe-workbench/tests/embedder_command_surface.rs`.
   Dependencies: none.
5. **Host panels in the workbench's dock** — feature; m.
   `Workbench`/`Dock` take a host's `DockItem` with an id, icon, title and
   rail order; `DockPanel` constructible for it; saved dock state keeps it.
   Files: `crates/fe-workbench/src/dock.rs`, `crates/fe-workbench/src/dock_layout.rs`,
   `crates/fe-workbench/src/workbench_chrome.rs`, `crates/fe-workbench/src/lib.rs`,
   `crates/fe-workbench/src/sidebar_panels_tests.rs`. Dependencies: none.
6. **The app's composition as a library** — feature; l.
   Move `fastedit`'s setup (settings load, `shipped`/`config`, theme,
   extensions, window layouts) into `fe_workbench::app` as a builder a host
   calls with overrides (identity, keymap, menus, themes, extra commands,
   panels, provider); `crates/fastedit/src/main.rs` becomes a few lines over it.
   Files: `crates/fastedit/src/main.rs`, `crates/fastedit/src/config.rs`,
   `crates/fastedit/src/theme.rs`, `crates/fastedit/src/workspace_extensions.rs`,
   `crates/fe-workbench/src/lib.rs`, new `crates/fe-workbench/src/app.rs`,
   `crates/fe-workbench/Cargo.toml`. Dependencies: 1, 2, 5.
7. **Host default settings and locked keys** — feature; m.
   A host layer between `Settings::default()` and the user's file, and a
   locked-key list that drops user and project values with a diagnostic and
   shows them read-only in the settings editor; the same for keymap bindings.
   Files: `kiln/crates/kiln-core/src/settings.rs`, `kiln/crates/kiln-core/src/keymap.rs`,
   `crates/fe-config/src/settings.rs`, `crates/fe-config/src/settings_model.rs`,
   `crates/fe-workbench/src/settings_editor.rs`, `crates/fe-workbench/src/shortcuts_editor.rs`.
   Dependencies: none.
8. **Pin an AI actor's command through an `EditorView`** — chore; xs.
   A test that `dispatch_invocation` of `editor.insert` by `Actor::Ai` on a
   focused view makes an AI-origin transaction that `editor.undo` by the user
   leaves; the headless twin exists (`actors_stamp_and_undo_their_own_transactions`).
   Files: `crates/fe-gpui/src/command_tests.rs`. Dependencies: none.
9. **An embedding example and its test** — feature; m.
   A host app with its own name, a dock panel, a chat provider and a command
   dispatched as `Actor::Ai`, over card 6's builder, pinned headless as
   `ai_demo_host` pins `ai_demo`.
   Files: new `crates/fe-workbench/examples/embed_host.rs`, new
   `crates/fe-workbench/tests/embed_host.rs`, `docs/ai-host.md` (a
   "Whole workbench" section). Dependencies: 6.
10. **The stable surface, marked and checked** — chore; m.
    `#[non_exhaustive]` (with constructors) on `EditorTheme`, `Launch`,
    `CommandMeta` and the event enums; crate docs that name the stable items
    of the tier table; a public-API snapshot of each stable crate, checked by
    a new script run as a gate stage and in CI.
    Files: `crates/fe-gpui/src/lib.rs`, `crates/fe-gpui/src/view.rs`,
    `crates/fe-workbench/src/lib.rs`, `crates/fe-workbench/src/workbench.rs`,
    `crates/fe-buffer/src/lib.rs`, `crates/fe-editor/src/lib.rs`,
    `crates/fe-chat/src/lib.rs`, `crates/fe-text/src/lib.rs`,
    `kiln/crates/kiln-command/src/lib.rs`, `kiln/crates/kiln-command/src/registry.rs`,
    `kiln/crates/kiln/src/lib.rs`, new `scripts/check-public-api.sh`, new
    `docs/public-api/` (one `<crate>.txt` snapshot per stable crate:
    `kiln-command.txt`, `kiln.txt`, `fe-text.txt`, `fe-buffer.txt`,
    `fe-editor.txt`, `fe-gpui.txt`, `fe-chat.txt`), `regatta.toml` (a
    `public_api` stage beside `no_gpui`), `.github/workflows/ci.yml`.
    Dependencies: 4, 6.
11. **Publish the stable tier** — chore; l.
    Apache-2.0 licence fields, `publish = true`, versions, descriptions and a
    README for the seven stable crates and the eleven workspace crates they
    depend on (crates.io needs the whole closure, computed with
    `cargo metadata`); a GPUI-pin policy (when `=0.3.9` moves, and that it's
    a breaking change for every GPUI-coupled crate).
    Files: root `Cargo.toml`; the manifests `kiln/crates/kiln/Cargo.toml`,
    `kiln/crates/kiln-command/Cargo.toml`, `kiln/crates/kiln-core/Cargo.toml`,
    `kiln/crates/kiln-macros/Cargo.toml`, `kiln/crates/kiln-platform/Cargo.toml`,
    `kiln/crates/kiln-shell/Cargo.toml`, `kiln/crates/kiln-ui/Cargo.toml`,
    `crates/fe-text/Cargo.toml`, `crates/fe-buffer/Cargo.toml`,
    `crates/fe-display/Cargo.toml`, `crates/fe-editor/Cargo.toml`,
    `crates/fe-fileview/Cargo.toml`, `crates/fe-syntax/Cargo.toml`,
    `crates/fe-config/Cargo.toml`, `crates/fe-keymap/Cargo.toml`,
    `crates/fe-vim/Cargo.toml`, `crates/fe-gpui/Cargo.toml`, `crates/fe-chat/Cargo.toml`;
    a new `README.md` beside each of those 18 manifests (none exists today);
    new `docs/release/gpui-pin.md`. The workbench and its other dependencies
    publish later, after card 6. Dependencies: T1267, T1268, 10.

Code plugins in the app are already carded ("running granted plugins in the
app", `docs/decisions/vmtnmkay62-extensions-and-plugins.md` § 10); a host
gets them with the workbench once that lands, so no card here.
