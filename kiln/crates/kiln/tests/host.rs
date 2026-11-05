// SPDX-License-Identifier: Apache-2.0

//! The host example, Kiln Chat, headless through `kiln::testing`: it sets
//! up cleanly on every platform; a message typed and sent shows in its
//! conversation; its command starts a conversation by key, from the palette
//! and from the menus, the menus drawn in the window too; a pane splits,
//! and the panel opens a conversation; the status bar follows the active
//! conversation; a conversation with unsent text asks before it closes;
//! its layout, conversations and all, comes back in a new window, leaving
//! the first's be, as does one saved before `kiln::app`; and its colour
//! comes from the user's theme file, or its default.

#[path = "../examples/host/app.rs"]
mod app;
#[path = "../examples/host/conversation.rs"]
mod conversation;
#[path = "support/platform_keymaps.rs"]
mod platform_keymaps;
#[path = "../examples/host/threads.rs"]
mod threads;

use std::{fs, path::Path};

use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln::{
    Config,
    command::Value,
    core::{AppId, AppPaths, Os, PathStyle, layout::LayoutStore},
    platform::config::{BatchSender, batch_channel},
    prelude::*,
    shell::{
        DockPosition, DockView as _, ItemPane, PaneItem as _, PaneView, Workspace,
        command_palette::COMMAND_PALETTE,
    },
    testing::{TestApp, assert_clean_config},
    ui::{Color, List, ThemeMode, hsla},
};

use conversation::{Conversation, ConversationState};
use threads::{ConversationList, Rows, Threads};

/// The chat, opened by `kiln::testing` as `main` opens it, but with no user
/// file or saved layout and its window's chrome as on Linux: its first
/// conversation shown and focused.
fn chat(cx: &mut TestAppContext) -> TestApp {
    TestApp::open(app::chat(), cx)
}

/// The chat's config as `kiln::app` makes it: its own keymap and menus over
/// kiln's keys.
fn config() -> Config {
    let config = Config::new(app::APP_ID, "Kiln Chat");
    app::config(config.default_keys(true).default_menus(true))
}

fn active(app: &mut TestApp) -> Entity<Conversation> {
    app.active_item::<Conversation>()
        .expect("a conversation is shown")
}

fn active_pane(app: &mut TestApp) -> Entity<ItemPane> {
    let workspace = app.workspace();
    workspace.read_with(app.cx(), |workspace, _| workspace.active_pane().clone())
}

fn panes(app: &mut TestApp) -> Vec<Entity<ItemPane>> {
    let workspace = app.workspace();
    workspace.read_with(app.cx(), |workspace, _| {
        workspace.group().panes().into_iter().cloned().collect()
    })
}

fn title(conversation: &Entity<Conversation>, cx: &mut VisualTestContext) -> String {
    conversation.read_with(cx, |conversation, cx| {
        let thread = Threads::get(conversation.state.thread, cx);
        thread.unwrap().title.to_string()
    })
}

fn messages(conversation: &Entity<Conversation>, cx: &mut VisualTestContext) -> Vec<String> {
    conversation.read_with(cx, |conversation, cx| {
        Threads::get(conversation.state.thread, cx)
            .unwrap()
            .messages
            .clone()
    })
}

fn draft(conversation: &Entity<Conversation>, cx: &mut VisualTestContext) -> String {
    conversation.read_with(cx, |conversation, _| conversation.state.draft.clone())
}

fn titles(pane: &Entity<ItemPane>, cx: &mut VisualTestContext) -> Vec<String> {
    pane.read_with(cx, |pane, cx| {
        let items = pane.pane().items().iter();
        items.map(|item| item.tab_title(cx).to_string()).collect()
    })
}

/// What the status bar shows.
fn status(app: &mut TestApp) -> Vec<String> {
    let workspace = app.workspace();
    workspace.read_with(app.cx(), |workspace, cx| {
        let items = workspace.status_items(cx).into_iter();
        items.map(|item| item.text.to_string()).collect()
    })
}

/// The conversations panel's list.
fn list(app: &mut TestApp) -> Entity<List<Rows>> {
    let workspace = app.workspace();
    app.cx().read(|cx| {
        let dock = workspace.read(cx).dock(DockPosition::Left).unwrap();
        let panel = dock.read(cx).panel::<ConversationList>().unwrap();
        panel.read(cx).0.clone()
    })
}

#[gpui::test]
fn keymap_and_menus_load_cleanly_on_every_platform(cx: &mut TestAppContext) {
    let _app = chat(cx);
    cx.update(|cx| {
        let config = config();
        assert_clean_config(&config, cx);
        let catalog = kiln::catalog(cx);
        platform_keymaps::assert_keymaps_agree(&config, &*catalog, platform_keymaps::KILN);
        for os in [Os::MacOs, Os::Linux, Os::Windows] {
            let (menus, diagnostics) = config.menu_bar_for(os, Some(&*catalog));
            assert_eq!(diagnostics, [], "menus on {os:?}");
            assert_eq!(menus.menus.len(), 4, "menus on {os:?}");
        }
    });
}

#[gpui::test]
fn a_message_typed_and_sent_shows_in_the_conversation(cx: &mut TestAppContext) {
    let mut app = chat(cx);
    let general = active(&mut app);
    assert_eq!(title(&general, app.cx()), "General");
    assert_eq!(messages(&general, app.cx()), ["Welcome to Kiln Chat."]);

    // Enter in the field sends it, and clears the field.
    app.cx().simulate_input("Hello there");
    app.press("enter");
    assert_eq!(
        messages(&general, app.cx()),
        ["Welcome to Kiln Chat.", "Hello there"]
    );
    assert_eq!(draft(&general, app.cx()), "");

    // So does the command, by its key; an empty field sends nothing.
    app.cx().simulate_input("Second");
    app.press("secondary-enter");
    app.press("secondary-enter");
    assert_eq!(messages(&general, app.cx()).len(), 3);
    assert!(app.cx().debug_bounds("messages").is_some(), "drawn");
}

#[gpui::test]
fn the_host_command_runs_by_key_from_the_palette_and_the_menus(cx: &mut TestAppContext) {
    let mut app = chat(cx);
    app.press("secondary-n");
    assert_eq!(title(&active(&mut app), app.cx()), "Conversation 3");
    let pane = active_pane(&mut app);
    assert_eq!(titles(&pane, app.cx()), ["General", "Conversation 3"]);

    // The palette lists it where the conversation has focus, and runs it.
    app.press("secondary-shift-p");
    let workspace = app.workspace();
    let shell = workspace.read_with(app.cx(), |workspace, _| workspace.shell().clone());
    let modal = shell.read_with(app.cx(), |shell, cx| shell.modal(cx));
    assert_eq!(modal, Some(COMMAND_PALETTE));
    app.cx().simulate_input("new conv");
    app.press("enter");
    assert_eq!(shell.read_with(app.cx(), |shell, cx| shell.modal(cx)), None);
    assert_eq!(title(&active(&mut app), app.cx()), "Conversation 4");
    assert_eq!(app.cx().read(|cx| Threads::save(cx).0.len()), 4);
    assert_eq!(titles(&pane, app.cx()).len(), 3);

    // And from its menu.
    app.menu("Conversation", "New Conversation");
    assert_eq!(title(&active(&mut app), app.cx()), "Conversation 5");
}

/// The menus drawn in the window, as on Linux: F10 opens them, and
/// Conversation, New Conversation runs the command.
#[gpui::test]
fn f10_opens_the_menus_in_the_window(cx: &mut TestAppContext) {
    let mut app = chat(cx);
    // kiln binds F10 everywhere but macOS: load the keymap and menus as on
    // Linux, as the window draws its chrome.
    app.cx().update(|_, cx| {
        let config = config();
        let catalog = kiln::catalog(cx);
        let (keymap, diagnostics) = config.keymap_for(Os::Linux, Some(&*catalog));
        assert_eq!(diagnostics, []);
        let (menus, diagnostics) = config.menu_bar_for(Os::Linux, Some(&*catalog));
        assert_eq!(diagnostics, []);
        kiln::platform::keys::set_keymap(cx, keymap);
        kiln::platform::menus::install(cx, menus);
    });
    app.draw();
    assert!(app.cx().debug_bounds("menu:Conversation").is_some());
    app.press("f10 right");
    let workspace = app.workspace();
    let shell = workspace.read_with(app.cx(), |workspace, _| workspace.shell().clone());
    let bar = shell.read_with(app.cx(), |shell, _| shell.menu_bar().cloned().unwrap());
    let open = bar.read_with(app.cx(), |bar, _| bar.open_menu());
    assert_eq!(open, Some((1, Some(0))), "Conversation, New Conversation");
    app.press("enter");
    assert_eq!(title(&active(&mut app), app.cx()), "Conversation 3");
}

#[gpui::test]
fn a_split_shows_the_conversation_twice_and_the_panel_opens_another(cx: &mut TestAppContext) {
    let mut app = chat(cx);
    let general = active(&mut app);
    app.press("secondary-\\");
    let panes = panes(&mut app);
    assert_eq!(panes.len(), 2);
    let copy = active(&mut app);
    assert_ne!(copy, general, "a second view");
    assert_eq!(active_pane(&mut app), panes[1]);

    // Both views show what either sends.
    app.cx().simulate_input("To both");
    app.press("enter");
    assert_eq!(messages(&general, app.cx()).last().unwrap(), "To both");

    // The panel's row opens Random in the active pane, as a click would.
    let list = list(&mut app);
    list.update(app.cx(), |list, cx| list.confirm(1, cx));
    app.draw();
    assert_eq!(title(&active(&mut app), app.cx()), "Random");
    assert_eq!(titles(&panes[1], app.cx()), ["General", "Random"]);
    assert_eq!(titles(&panes[0], app.cx()), ["General"]);
    // The panel selects the conversation shown.
    app.press("secondary-w");
    assert_eq!(list.read_with(app.cx(), |list, _| list.selected()), Some(0));

    // Closing the split's last tab closes the pane.
    app.press("secondary-w");
    assert_eq!(self::panes(&mut app).len(), 1);
    assert_eq!(active(&mut app), general);

    // Focus given to the panel goes on to its list: the arrow keys move the selection.
    let workspace = app.workspace();
    app.cx().update(|window, cx| {
        let dock = workspace.read(cx).dock(DockPosition::Left).unwrap().clone();
        let panel = dock.read(cx).panel::<ConversationList>().unwrap();
        window.focus(&gpui::Focusable::focus_handle(&panel, cx), cx);
    });
    app.draw();
    app.press("down");
    assert_eq!(list.read_with(app.cx(), |list, _| list.selected()), Some(1));
}

#[gpui::test]
fn the_status_bar_follows_the_active_conversation(cx: &mut TestAppContext) {
    let mut app = chat(cx);
    assert_eq!(status(&mut app), ["2 conversations", "1 message"]);
    app.cx().simulate_input("One more");
    app.press("enter");
    assert_eq!(status(&mut app), ["2 conversations", "2 messages"]);
    assert!(app.cx().debug_bounds("status-item:chat.messages").is_some());

    app.press("secondary-n");
    assert_eq!(status(&mut app), ["3 conversations", "0 messages"]);
    // The previous tab, by its click.
    let pane = active_pane(&mut app);
    (app.cx()).update(|window, cx| pane.update(cx, |pane, cx| pane.activate_item(0, window, cx)));
    app.draw();
    assert_eq!(status(&mut app), ["3 conversations", "2 messages"]);
}

#[gpui::test]
fn a_conversation_with_unsent_text_asks_before_it_closes(cx: &mut TestAppContext) {
    let mut app = chat(cx);
    let general = active(&mut app);
    let pane = active_pane(&mut app);
    app.cx().simulate_input("Not sent yet");
    app.draw();
    assert!(general.read_with(app.cx(), |general, cx| general.is_dirty(cx)));

    // Cancel keeps it, unsent text and all.
    app.press("secondary-w");
    assert!(app.cx().has_pending_prompt());
    app.cx().simulate_prompt_answer("Cancel");
    app.draw();
    assert_eq!(titles(&pane, app.cx()), ["General"]);
    assert_eq!(draft(&general, app.cx()), "Not sent yet");

    // Discard closes it.
    app.press("secondary-w");
    app.cx().simulate_prompt_answer("Discard");
    app.draw();
    assert_eq!(titles(&pane, app.cx()), Vec::<String>::new());

    // With nothing unsent, it closes without asking.
    let workspace = app.workspace();
    app.cx().update(|window, cx| {
        let random = ConversationState::new(1);
        workspace.update(cx, |workspace, cx| {
            workspace.open::<Conversation>(random, window, cx)
        })
    });
    app.press("secondary-w");
    assert!(!app.cx().has_pending_prompt());
    assert_eq!(titles(&pane, app.cx()), Vec::<String>::new());
}

#[gpui::test]
fn the_layout_comes_back_in_a_new_window(cx: &mut TestAppContext) {
    let mut app = chat(cx);
    app.press("secondary-\\");
    app.cx().simulate_input("Unsent");
    app.draw();
    let first = app.workspace();
    let entry = first.read_with(app.cx(), |workspace, cx| workspace.entry(cx));
    assert_eq!(entry["threads"].as_array().map(Vec::len), Some(2));
    // A conversation started since, with a message, isn't in the entry.
    app.press("secondary-n");
    app.cx().simulate_input("Later");
    app.press("enter");
    let later = active(&mut app);

    // A window as the chat's: its setup, then the restore.
    let mut cx = app.cx().cx.clone();
    let (restored, cx) = cx.add_window_view(|window, cx| {
        let mut workspace = Workspace::item_panes(app::KEY_CONTEXT, window, cx);
        app::setup(&mut workspace, window, cx);
        workspace.restore(&entry, window, cx);
        workspace
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    let panes: Vec<_> = restored.read_with(cx, |workspace, _| {
        workspace.group().panes().into_iter().cloned().collect()
    });
    assert_eq!(panes.len(), 2);
    assert_eq!(titles(&panes[0], cx), ["General"]);
    assert_eq!(titles(&panes[1], cx), ["General"]);
    let active = restored.read_with(cx, |workspace, _| workspace.active_pane().clone());
    assert_eq!(active, panes[1]);
    let conversation = restored.read_with(cx, |workspace, cx| {
        let item = workspace.active_item(cx).unwrap();
        item.downcast_ref::<Conversation>().unwrap().clone()
    });
    assert_eq!(draft(&conversation, cx), "Unsent", "the split's own field");

    // The first window's conversations are as they were: the app's list
    // still holds them, its panel lists them, and its entry keeps them.
    let threads = cx.read(Threads::save).0;
    assert_eq!(threads.len(), 3);
    assert_eq!(messages(&later, cx), ["Later"]);
    assert_eq!(later.read_with(cx, |later, _| later.state.thread), 2);
    let rows = list(&mut app).read_with(app.cx(), |list, _| list.delegate().0.clone());
    let titles: Vec<_> = threads.iter().map(|thread| thread.title.clone()).collect();
    assert_eq!(rows, titles);
    let entry = first.read_with(app.cx(), |workspace, cx| workspace.entry(cx));
    assert_eq!(entry["threads"][2]["messages"], Value::from(vec!["Later"]));
    let state = &entry["panes"]["tree"]["members"][1]["pane"]["items"][1]["state"];
    assert_eq!(state["thread"], 2, "the new conversation's tab is saved");
}

/// A layout as Kiln Chat saved it before `kiln::app`: its conversations
/// with their IDs, its tabs' states and its panel's dock as `dock`.
const BEFORE_KILN_APP: &str = r#"{
    "panes": {
        "tree": {
            "axis": "horizontal",
            "ratios": [0.5, 0.5],
            "members": [
                {"pane": {"items": [
                    {"kind": "chat.conversation", "document": 0, "state": {"thread": 0, "draft": ""}},
                    {"kind": "chat.conversation", "document": 1, "state": {"thread": 1, "draft": "Later"}}
                ], "active": 1}},
                {"pane": {"items": [
                    {"kind": "chat.conversation", "document": 2, "state": {"thread": 0, "draft": "Unsent"}}
                ], "active": 0}}
            ]
        },
        "active": [1]
    },
    "dock": {
        "visible": false, "active": "conversations", "joined": [],
        "size": 300.0, "side_size": 300.0, "bottom_size": 240.0
    },
    "threads": [
        {"id": 0, "title": "General", "messages": ["Welcome to Kiln Chat.", "Hi"]},
        {"id": 1, "title": "Random", "messages": []}
    ]
}"#;

#[gpui::test]
fn a_layout_saved_before_kiln_app_comes_back(cx: &mut TestAppContext) {
    let dir = tempfile::TempDir::new().unwrap();
    let paths = paths(&dir);
    let store = LayoutStore::new(paths.layouts_dir().unwrap(), paths.state_dir().unwrap());
    let claim = store.claim_new().unwrap();
    store
        .write(&claim, BEFORE_KILN_APP.parse().unwrap())
        .unwrap();
    claim.release(false);

    // No real watcher: its thread would race the test scheduler.
    let (_sender, batches) = batch_channel();
    let chat = (app::chat().paths(paths)).config(|config| config.watch_with(batches));
    let started = cx.update(|cx| chat.start(cx));
    assert_eq!(started.diagnostics, []);
    cx.run_until_parked();
    let workspace = started.window.root::<Workspace>(cx).unwrap();
    let cx = &mut VisualTestContext::from_window(started.window.0, cx);
    cx.run_until_parked();
    let threads = cx.read(Threads::save).0;
    assert_eq!(threads.len(), 2);
    assert_eq!(threads[0].messages, ["Welcome to Kiln Chat.", "Hi"]);

    let panes: Vec<_> = workspace.read_with(cx, |workspace, _| {
        workspace.group().panes().into_iter().cloned().collect()
    });
    assert_eq!(panes.len(), 2);
    assert_eq!(titles(&panes[0], cx), ["General", "Random"]);
    assert_eq!(titles(&panes[1], cx), ["General"]);
    let drafts = |pane: &Entity<ItemPane>, cx: &mut VisualTestContext| {
        let items = pane.read_with(cx, |pane, _| pane.pane().items().to_vec());
        (items.iter())
            .map(|item| draft(item.downcast_ref::<Conversation>().unwrap(), cx))
            .collect::<Vec<_>>()
    };
    assert_eq!(drafts(&panes[0], cx), ["", "Later"]);
    assert_eq!(drafts(&panes[1], cx), ["Unsent"]);

    let dock = workspace.read_with(cx, |workspace, cx| {
        let dock = workspace.dock(DockPosition::Left).unwrap().read(cx).dock();
        (dock.is_visible(), f32::from(dock.current_size()))
    });
    assert_eq!(dock, (false, 300.));
}

/// The chat's directories, as `kiln::app` makes them, under `dir`.
fn paths(dir: &tempfile::TempDir) -> AppPaths {
    let home = dir.path().as_os_str().to_owned();
    let id = AppId::new(app::APP_ID, "kiln-chat", "Kiln Chat");
    AppPaths::resolve(&id, PathStyle::Native, Os::current(), |name| {
        matches!(name, "HOME" | "APPDATA" | "LOCALAPPDATA").then(|| home.clone())
    })
}

fn save(path: &Path, text: &str, sender: &BatchSender, cx: &mut TestAppContext) {
    fs::write(path, text).unwrap();
    sender.send(vec![path.to_owned()]);
    cx.run_until_parked();
}

/// The user's theme file is one thing `kiln::testing` keeps from the app,
/// so this opens the chat by `kiln::app` itself, its directories in a
/// temporary one and the files' changes sent as the watcher would.
#[gpui::test]
fn the_bubble_colour_comes_from_the_users_theme_or_its_default(cx: &mut TestAppContext) {
    let dir = tempfile::TempDir::new().unwrap();
    let paths = paths(&dir);
    let path = paths.config_dir().unwrap().join("theme.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let theme = |bubble: &str| {
        format!("kiln_theme = 1\nappearance = \"dark\"\n[chat]\nbubble = \"{bubble}\"\n")
    };
    fs::write(&path, theme("#123456")).unwrap();
    let (sender, batches) = batch_channel();
    let chat = (app::chat().paths(paths))
        .theme_mode(ThemeMode::Dark)
        .config(|config| config.watch_with(batches));
    let started = cx.update(|cx| chat.start(cx));
    assert_eq!(started.diagnostics, []);
    cx.run_until_parked();
    let bubble = |cx: &mut TestAppContext| cx.read(conversation::bubble_color);
    assert_eq!(bubble(cx), hsla(Color::rgb(0x123456)));

    // It follows the file as it's saved.
    save(&path, &theme("#abcdef"), &sender, cx);
    assert_eq!(bubble(cx), hsla(Color::rgb(0xabcdef)));

    // kiln's light theme has no `chat` table: the accent's tint instead.
    cx.update(|cx| cx.set_theme_mode(ThemeMode::Light));
    let subtle = cx.read(|cx| cx.theme().colors.accent.subtle);
    assert_eq!(bubble(cx), subtle);
}
