// SPDX-License-Identifier: LicenseRef-Proprietary

//! The host example, Kiln Chat, headless: it sets up cleanly on every
//! platform; a message typed and sent shows in its conversation; its
//! command starts a conversation by key, from the palette and from the
//! menus drawn in the window; a pane splits, and the panel opens a conversation; the status
//! bar follows the active conversation; a conversation with unsent text
//! asks before it closes; its layout comes back in a new window; and its
//! colour comes from the user's theme file, or its default.

#[path = "../examples/host/app.rs"]
mod app;
#[path = "../examples/host/commands.rs"]
mod commands;
#[path = "../examples/host/conversation.rs"]
mod conversation;
#[path = "../examples/host/panel.rs"]
mod panel;
#[path = "../examples/host/setup.rs"]
mod setup;
#[path = "../examples/host/threads.rs"]
mod threads;

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln::{
    Config,
    core::Os,
    platform::{
        Capabilities,
        config::{BatchSender, batch_channel},
    },
    prelude::*,
    shell::{
        ItemPane, PaneItem as _, PaneView, StatusItem, command_palette::COMMAND_PALETTE, layout,
    },
    ui::{Color, ThemeMode, hsla},
};

use app::ChatWindow;
use conversation::Conversation;
use threads::Threads;

/// Sets the app up with `config`, as `main` does (but for the user's files
/// and a saved layout), and opens its window as on Linux, the chat's first
/// conversation focused.
fn open(config: Config, cx: &mut TestAppContext) -> (Entity<ChatWindow>, &mut VisualTestContext) {
    cx.update(|cx| {
        let diagnostics = kiln::init(cx, config);
        assert_eq!(diagnostics, []);
        // The test platform answers prompts (`simulate_prompt_answer`), not
        // the in-window prompt `init` sets up where there are no native
        // ones.
        cx.reset_prompt_builder();
        layout::register_item::<Conversation>(cx);
        Threads::load(None, cx);
    });
    let (chat, cx) = cx.add_window_view(|window, cx| {
        let mut chat = ChatWindow::new(Capabilities::for_os(Os::Linux), window, cx);
        chat.start(None, window, cx);
        chat
    });
    draw(cx);
    (chat, cx)
}

fn chat(cx: &mut TestAppContext) -> (Entity<ChatWindow>, &mut VisualTestContext) {
    open(setup::config(), cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Presses `keys`, with this platform's `secondary`, then draws a frame.
fn press(keys: &str, cx: &mut VisualTestContext) {
    let secondary = if Os::current() == Os::MacOs {
        "cmd"
    } else {
        "ctrl"
    };
    cx.simulate_keystrokes(&keys.replace("secondary", secondary));
    draw(cx);
}

fn active(chat: &Entity<ChatWindow>, cx: &mut VisualTestContext) -> Entity<Conversation> {
    chat.read_with(cx, |chat, cx| chat.active_conversation(cx))
        .expect("a conversation is shown")
}

fn title(conversation: &Entity<Conversation>, cx: &mut VisualTestContext) -> String {
    conversation.read_with(cx, |conversation, cx| {
        conversation.thread().read(cx).title.to_string()
    })
}

fn messages(conversation: &Entity<Conversation>, cx: &mut VisualTestContext) -> Vec<String> {
    conversation.read_with(cx, |conversation, cx| {
        let thread = conversation.thread().read(cx);
        thread.messages.iter().map(ToString::to_string).collect()
    })
}

fn titles(pane: &Entity<ItemPane>, cx: &mut VisualTestContext) -> Vec<String> {
    pane.read_with(cx, |pane, cx| {
        let items = pane.pane().items().iter();
        items.map(|item| item.tab_title(cx).to_string()).collect()
    })
}

fn status(chat: &Entity<ChatWindow>, cx: &mut VisualTestContext) -> Vec<String> {
    chat.read_with(cx, |chat, cx| {
        let items = chat.status_items(cx).into_iter();
        items
            .map(|item: StatusItem| item.text.to_string())
            .collect()
    })
}

#[gpui::test]
fn keymap_and_menus_load_cleanly_on_every_platform(cx: &mut TestAppContext) {
    let _ = chat(cx);
    cx.update(|cx| {
        let catalog = kiln::catalog(cx);
        let config = setup::config();
        for os in [Os::MacOs, Os::Linux, Os::Windows] {
            let (keymap, diagnostics) = config.keymap_for(os, Some(&*catalog));
            assert_eq!(diagnostics, [], "keymap on {os:?}");
            // F10 opens the menus drawn in the window, which macOS hasn't.
            let bindings = if os == Os::MacOs { 14 } else { 15 };
            assert_eq!(keymap.bindings().len(), bindings, "keymap on {os:?}");
            let (menus, diagnostics) = config.menu_bar_for(os, Some(&*catalog));
            assert_eq!(diagnostics, [], "menus on {os:?}");
            assert_eq!(menus.menus.len(), 4, "menus on {os:?}");
        }
    });
}

#[gpui::test]
fn a_message_typed_and_sent_shows_in_the_conversation(cx: &mut TestAppContext) {
    let (chat, cx) = chat(cx);
    let general = active(&chat, cx);
    assert_eq!(title(&general, cx), "General");
    assert_eq!(messages(&general, cx), ["Welcome to Kiln Chat."]);

    // Enter in the field sends it, and clears the field.
    cx.simulate_input("Hello there");
    press("enter", cx);
    assert_eq!(
        messages(&general, cx),
        ["Welcome to Kiln Chat.", "Hello there"]
    );
    assert_eq!(general.read_with(cx, |c, cx| c.draft(cx).to_owned()), "");

    // So does the command, by its key; an empty field sends nothing.
    cx.simulate_input("Second");
    press("secondary-enter", cx);
    press("secondary-enter", cx);
    assert_eq!(messages(&general, cx).len(), 3);
    assert!(cx.debug_bounds("messages").is_some(), "drawn");
}

#[gpui::test]
fn the_host_command_runs_by_key_from_the_palette_and_the_menus(cx: &mut TestAppContext) {
    let (chat, cx) = chat(cx);
    press("secondary-n", cx);
    assert_eq!(title(&active(&chat, cx), cx), "Conversation 3");
    let pane = chat.read_with(cx, |chat, _| chat.active_pane().clone());
    assert_eq!(titles(&pane, cx), ["General", "Conversation 3"]);

    // The palette lists it where the conversation has focus, and runs it.
    press("secondary-shift-p", cx);
    let shell = chat.read_with(cx, |chat, _| chat.shell().clone());
    let modal = shell.read_with(cx, |shell, cx| shell.modal(cx));
    assert_eq!(modal, Some(COMMAND_PALETTE));
    cx.simulate_input("new conv");
    press("enter", cx);
    assert_eq!(shell.read_with(cx, |shell, cx| shell.modal(cx)), None);
    assert_eq!(title(&active(&chat, cx), cx), "Conversation 4");
    assert_eq!(cx.read(|cx| Threads::all(cx).len()), 4);
    assert_eq!(titles(&pane, cx).len(), 3);
}

/// The menus drawn in the window, as on Linux: F10 opens them, and
/// Conversation, New Conversation runs the command.
#[gpui::test]
fn f10_opens_the_menus_in_the_window(cx: &mut TestAppContext) {
    let (chat, cx) = chat(cx);
    // `init` loads the keymap and menus for the host, and the chat binds
    // F10 everywhere but macOS: load them as on Linux, as the window draws.
    cx.update(|_, cx| {
        let config = setup::config();
        let catalog = kiln::catalog(cx);
        let (keymap, diagnostics) = config.keymap_for(Os::Linux, Some(&*catalog));
        assert_eq!(diagnostics, []);
        let (menus, diagnostics) = config.menu_bar_for(Os::Linux, Some(&*catalog));
        assert_eq!(diagnostics, []);
        kiln::platform::keys::set_keymap(cx, keymap);
        kiln::platform::menus::install(cx, menus);
    });
    draw(cx);
    assert!(cx.debug_bounds("menu:Conversation").is_some());
    cx.simulate_keystrokes("f10 right");
    draw(cx);
    let shell = chat.read_with(cx, |chat, _| chat.shell().clone());
    let bar = shell.read_with(cx, |shell, _| shell.menu_bar().cloned().unwrap());
    let open = bar.read_with(cx, |bar, _| bar.open_menu());
    assert_eq!(open, Some((1, Some(0))), "Conversation, New Conversation");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(title(&active(&chat, cx), cx), "Conversation 3");
}

#[gpui::test]
fn a_split_shows_the_conversation_twice_and_the_panel_opens_another(cx: &mut TestAppContext) {
    let (chat, cx) = chat(cx);
    let general = active(&chat, cx);
    press("secondary-\\", cx);
    let panes: Vec<_> = chat.read_with(cx, |chat, _| {
        chat.group().panes().into_iter().cloned().collect()
    });
    assert_eq!(panes.len(), 2);
    let copy = active(&chat, cx);
    assert_ne!(copy, general, "a second view");
    assert_eq!(
        chat.read_with(cx, |chat, _| chat.active_pane().clone()),
        panes[1]
    );

    // Both views show what either sends.
    cx.simulate_input("To both");
    press("enter", cx);
    assert_eq!(messages(&general, cx).last().unwrap(), "To both");

    // The panel's row opens Random in the active pane, as a click would.
    let list = chat.read_with(cx, |chat, cx| chat.panel().read(cx).list().clone());
    list.update(cx, |list, cx| list.confirm(1, cx));
    draw(cx);
    assert_eq!(title(&active(&chat, cx), cx), "Random");
    assert_eq!(titles(&panes[1], cx), ["General", "Random"]);
    assert_eq!(titles(&panes[0], cx), ["General"]);
    // The panel selects the conversation shown.
    press("secondary-w", cx);
    assert_eq!(list.read_with(cx, |list, _| list.selected()), Some(0));

    // Closing the split's last tab closes the pane.
    press("secondary-w", cx);
    let count = chat.read_with(cx, |chat, _| chat.group().panes().len());
    assert_eq!(count, 1);
    assert_eq!(active(&chat, cx), general);
}

#[gpui::test]
fn the_status_bar_follows_the_active_conversation(cx: &mut TestAppContext) {
    let (chat, cx) = chat(cx);
    assert_eq!(status(&chat, cx), ["2 conversations", "1 message"]);
    cx.simulate_input("One more");
    press("enter", cx);
    assert_eq!(status(&chat, cx), ["2 conversations", "2 messages"]);
    assert!(cx.debug_bounds("status-item:chat.messages").is_some());

    press("secondary-n", cx);
    assert_eq!(status(&chat, cx), ["3 conversations", "0 messages"]);
    // The previous tab, by its click.
    let pane = chat.read_with(cx, |chat, _| chat.active_pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate_item(0, window, cx)));
    draw(cx);
    assert_eq!(status(&chat, cx), ["3 conversations", "2 messages"]);
}

#[gpui::test]
fn a_conversation_with_unsent_text_asks_before_it_closes(cx: &mut TestAppContext) {
    let (chat, cx) = chat(cx);
    let general = active(&chat, cx);
    let pane = chat.read_with(cx, |chat, _| chat.active_pane().clone());
    cx.simulate_input("Not sent yet");
    draw(cx);
    assert!(general.read_with(cx, |general, cx| general.is_dirty(cx)));

    // Cancel keeps it, unsent text and all.
    press("secondary-w", cx);
    assert!(cx.has_pending_prompt());
    cx.simulate_prompt_answer("Cancel");
    draw(cx);
    assert_eq!(titles(&pane, cx), ["General"]);
    assert_eq!(
        general.read_with(cx, |c, cx| c.draft(cx).to_owned()),
        "Not sent yet"
    );

    // Discard closes it.
    press("secondary-w", cx);
    cx.simulate_prompt_answer("Discard");
    draw(cx);
    assert_eq!(titles(&pane, cx), Vec::<String>::new());

    // With nothing unsent, it closes without asking.
    let random = cx.read(|cx| Threads::all(cx)[1].clone());
    cx.update(|window, cx| chat.update(cx, |chat, cx| chat.open(random, window, cx)));
    press("secondary-w", cx);
    assert!(!cx.has_pending_prompt());
    assert_eq!(titles(&pane, cx), Vec::<String>::new());
}

#[gpui::test]
fn the_layout_comes_back_in_a_new_window(cx: &mut TestAppContext) {
    let (chat, cx) = chat(cx);
    press("secondary-\\", cx);
    cx.simulate_input("Unsent");
    draw(cx);
    let entry = chat.read_with(cx, |chat, cx| chat.entry(cx));
    assert_eq!(entry["threads"].as_array().map(Vec::len), Some(2));

    let mut app = cx.cx.clone();
    let (restored, cx) = app.add_window_view(|window, cx| {
        let mut chat = ChatWindow::new(Capabilities::for_os(Os::Linux), window, cx);
        chat.start(Some(&entry), window, cx);
        chat
    });
    draw(cx);
    let panes: Vec<_> = restored.read_with(cx, |chat, _| {
        chat.group().panes().into_iter().cloned().collect()
    });
    assert_eq!(panes.len(), 2);
    assert_eq!(titles(&panes[0], cx), ["General"]);
    assert_eq!(titles(&panes[1], cx), ["General"]);
    let active = restored.read_with(cx, |chat, _| chat.active_pane().clone());
    assert_eq!(active, panes[1]);
    let draft = active_draft(&restored, cx);
    assert_eq!(draft, "Unsent", "the split's own field");
}

fn active_draft(chat: &Entity<ChatWindow>, cx: &mut VisualTestContext) -> String {
    let conversation = active(chat, cx);
    conversation.read_with(cx, |conversation, cx| conversation.draft(cx).to_owned())
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "kiln-host-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(name);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn save(path: &PathBuf, text: &str, sender: &BatchSender, cx: &mut VisualTestContext) {
    fs::write(path, text).unwrap();
    sender.send(vec![path.clone()]);
    draw(cx);
}

#[gpui::test]
fn the_bubble_colour_comes_from_the_users_theme_or_its_default(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let path = dir.0.join("theme.toml");
    let theme = |bubble: &str| {
        format!("kiln_theme = 1\nappearance = \"dark\"\n[chat]\nbubble = \"{bubble}\"\n")
    };
    fs::write(&path, theme("#123456")).unwrap();
    let (sender, batches) = batch_channel();
    let config = setup::config()
        .theme_mode(ThemeMode::Dark)
        .user_theme(&path)
        .watch_with(batches);
    let (_, cx) = open(config, cx);
    let bubble = |cx: &mut VisualTestContext| cx.read(conversation::bubble_color);
    assert_eq!(bubble(cx), hsla(Color::rgb(0x123456)));

    // It follows the file as it's saved.
    save(&path, &theme("#abcdef"), &sender, cx);
    assert_eq!(bubble(cx), hsla(Color::rgb(0xabcdef)));

    // kiln's light theme has no `chat` table: the accent's tint instead.
    cx.update(|_, cx| cx.set_theme_mode(ThemeMode::Light));
    let subtle = cx.read(|cx| cx.theme().colors.accent.subtle);
    assert_eq!(bubble(cx), subtle);
}
