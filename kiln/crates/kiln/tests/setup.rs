// SPDX-License-Identifier: LicenseRef-Proprietary

//! Setting an app up with `kiln::init` and a `Config`, and the extension
//! traits in `kiln::prelude`, headless.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    Context, FocusHandle, InteractiveElement as _, IntoElement, Render, TestAppContext,
    VisualTestContext, Window, WindowAppearance, div,
};
use kiln::{
    Config, WindowSpec, app, binding_label_for,
    command::{CommandId, Invocation, Source},
    core::{Os, Severity},
    platform::{commands::observe_invocations, keys, menus},
    prelude::*,
    ui::{Appearance, GlobalTheme, Theme, ThemeMode, set_themes, system_appearance_changed},
    window,
};

mod notes {
    kiln::commands!(
        notes,
        [
            /// New Note
            NewNote,
        ]
    );
}

const KEYMAP: &str = r#"
[[bindings]]
keys = "secondary-n"
command = "notes.new_note"

[[bindings]]
keys = "secondary-q"
command = "app.quit"
"#;

const MENUS: &str = r#"
[[menus]]
name = "File"

[[menus.items]]
title = "New Note"
command = "notes.new_note"
"#;

fn config() -> Config {
    Config::new("com.example.Notes", "Notes")
        .commands(notes::register)
        .keymap("keymap.toml", KEYMAP)
        .menus("menus.toml", MENUS)
}

/// What the view's handlers saw, with the invocation each ran under.
type Log = Rc<RefCell<Vec<(&'static str, Option<Invocation>)>>>;

struct Notes {
    focus: FocusHandle,
    log: Log,
}

impl Render for Notes {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &notes::NewNote, _, cx| {
                let current = kiln::current_invocation(cx);
                this.log.borrow_mut().push(("new", current));
            }))
            .on_action(cx.listener(|this, _: &app::Quit, _, cx| {
                let current = kiln::current_invocation(cx);
                this.log.borrow_mut().push(("quit", current));
            }))
    }
}

/// A focused Notes window, after `kiln::init`, and what observers see.
fn notes_window(
    cx: &mut TestAppContext,
) -> (Log, Rc<RefCell<Vec<Invocation>>>, &mut VisualTestContext) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        kiln::init(cx, config());
        let sink = seen.clone();
        observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone())
        });
    });
    let log = Log::default();
    let (view, cx) = cx.add_window_view({
        let log = log.clone();
        move |_, cx| Notes {
            focus: cx.focus_handle(),
            log,
        }
    });
    cx.update(|window, cx| {
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    (log, seen, cx)
}

fn run(command: &str, cx: &mut VisualTestContext) {
    let invocation = Invocation::of(command).unwrap().source(Source::Script);
    cx.update(|window, cx| window.run_command(invocation, cx).unwrap());
    cx.run_until_parked();
}

fn id(id: &str) -> CommandId {
    CommandId::parse(id).unwrap()
}

#[gpui::test]
fn init_registers_the_apps_commands_before_checking_the_keymap(cx: &mut TestAppContext) {
    let diagnostics = cx.update(|cx| kiln::init(cx, config()));
    assert_eq!(diagnostics, [], "every command the keymap names is known");
    cx.update(|cx| {
        let keymap = keys::current_keymap(cx);
        assert_eq!(keymap.bindings().len(), 2);
        assert_eq!(keymap.bindings_for(&notes::NewNote::id()).len(), 1);
        let installed = menus::installed(cx).expect("the menus are installed");
        assert_eq!(installed.menus.len(), 1);
        let info = cx.global::<kiln::AppInfo>();
        assert_eq!(info.id, "com.example.Notes");
        assert_eq!(info.name, "Notes");
        assert_eq!(
            cx.global::<GlobalTheme>().mode(),
            Some(ThemeMode::System),
            "the theme follows the system unless the config says otherwise"
        );
    });
}

#[gpui::test]
fn init_returns_what_it_dropped(cx: &mut TestAppContext) {
    let keymap =
        format!("{KEYMAP}\n[[bindings]]\nkeys = \"secondary-m\"\ncommand = \"notes.missing\"\n");
    let config = Config::new("com.example.Notes", "Notes")
        .commands(notes::register)
        .keymap("keymap.toml", keymap)
        .theme_mode(ThemeMode::Light);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert!(
        diagnostics[0].message.contains("notes.missing"),
        "{diagnostics:?}"
    );
    cx.update(|cx| {
        assert_eq!(
            keys::current_keymap(cx).bindings().len(),
            2,
            "the rest load"
        );
        assert_eq!(cx.theme().appearance, Appearance::Light);
    });
}

#[gpui::test]
fn the_user_keymap_loads_over_the_apps(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("kiln-setup-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let user = dir.join("keymap.toml");
    std::fs::write(
        &user,
        "[[bindings]]\nkeys = \"secondary-shift-n\"\ncommand = \"notes.new_note\"\n",
    )
    .unwrap();
    // Not watched: the watcher's thread would wake the service's task.
    let (_, closed) = kiln::platform::config::batch_channel();
    let config = config().user_keymap(&user).watch_with(closed);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(diagnostics, []);
    cx.update(|cx| {
        let keymap = keys::current_keymap(cx);
        assert_eq!(keymap.bindings_for(&notes::NewNote::id()).len(), 2);
    });
}

#[gpui::test]
fn set_theme_mode_keeps_the_pair(cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_theme_mode(ThemeMode::Light);
        assert_eq!(cx.theme().appearance, Appearance::Light, "kiln's pair");
        // A pair whose "dark" is light shows which pair is kept.
        set_themes(cx, ThemeMode::Light, Theme::light(), Theme::dark());
        assert_eq!(cx.theme().appearance, Appearance::Dark);
        cx.set_theme_mode(ThemeMode::Dark);
        assert_eq!(cx.theme().appearance, Appearance::Light, "the pair set");
        assert_eq!(cx.global::<GlobalTheme>().mode(), Some(ThemeMode::Dark));
    });
}

#[test]
fn binding_labels_follow_each_platform() {
    let config = config();
    for (os, label) in [
        (Os::MacOs, "⌘N"),
        (Os::Linux, "Ctrl+N"),
        (Os::Windows, "Ctrl+N"),
    ] {
        let (keymap, _) = config.keymap_for(os, None);
        assert_eq!(
            binding_label_for(&keymap, &notes::NewNote::id(), os).as_deref(),
            Some(label),
            "on {os:?}"
        );
        assert_eq!(binding_label_for(&keymap, &id("notes.unbound"), os), None);
    }
}

#[gpui::test]
fn binding_label_reads_the_installed_keymap(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kiln::init(cx, config());
        let (keymap, _) = config().keymap_for(Os::current(), None);
        let expected = binding_label_for(&keymap, &app::Quit::id(), Os::current());
        assert!(expected.is_some());
        assert_eq!(cx.binding_label(&app::Quit::id()), expected);
    });
}

#[gpui::test]
fn run_command_dispatches_from_the_focused_element(cx: &mut TestAppContext) {
    let (log, seen, cx) = notes_window(cx);
    run("notes.new_note", cx);
    let log = log.borrow();
    assert_eq!(log.len(), 1);
    let (what, current) = &log[0];
    assert_eq!(*what, "new");
    assert_eq!(current.as_ref().unwrap().source, Source::Script);
    assert_eq!(seen.borrow().len(), 1);
}

#[gpui::test]
fn app_commands_are_typed_actions(cx: &mut TestAppContext) {
    assert_eq!(app::Quit::id(), id("app.quit"));
    assert_eq!(app::HideOthers::id(), id("app.hide_others"));
    assert_eq!(
        window::ToggleFullscreen::id(),
        id("window.toggle_fullscreen")
    );
    assert_eq!(window::Close::id(), id("window.close"));
    let (log, seen, cx) = notes_window(cx);

    // A view can handle one, as any action.
    run("app.quit", cx);
    assert_eq!(log.borrow().len(), 1);
    assert_eq!(log.borrow()[0].0, "quit");
    let current = log.borrow()[0].1.clone().unwrap();
    assert_eq!(current.command, id("app.quit"));

    // Otherwise it reaches the app's registry, and observers hear of it
    // once.
    seen.borrow_mut().clear();
    cx.update(|window, _| window.activate_window());
    run("window.toggle_fullscreen", cx);
    let seen: Vec<_> = seen
        .borrow()
        .iter()
        .map(|i| i.command.to_string())
        .collect();
    assert_eq!(seen, ["window.toggle_fullscreen"]);
    assert!(cx.update(|window, _| window.is_fullscreen()));
}

/// A Notes view in a new window, which follows the system's appearance if
/// `follow`.
fn open(follow: bool, cx: &mut TestAppContext) {
    cx.add_window(|window, cx| {
        if follow {
            window.follow_system_appearance(cx);
            // Twice is the same as once.
            window.follow_system_appearance(cx);
        }
        Notes {
            focus: cx.focus_handle(),
            log: Log::default(),
        }
    });
    cx.run_until_parked();
}

#[gpui::test]
fn the_theme_follows_the_system_appearance(cx: &mut TestAppContext) {
    // The test platform is light, so as if the system had been dark until
    // now. A window tells its observers its appearance once it opens.
    cx.update(|cx| {
        kiln::init(cx, config());
        system_appearance_changed(cx, WindowAppearance::Dark);
    });
    assert_eq!(cx.read(|cx| cx.theme().appearance), Appearance::Dark);
    open(false, cx);
    assert_eq!(
        cx.read(|cx| cx.theme().appearance),
        Appearance::Dark,
        "no window follows yet"
    );
    open(true, cx);
    assert_eq!(cx.read(|cx| cx.theme().appearance), Appearance::Light);
}

#[gpui::test]
fn window_options_carry_the_app_id(cx: &mut TestAppContext) {
    cx.update(|cx| {
        kiln::init(cx, config());
        let options = kiln::window_options(&WindowSpec::main("Notes"), cx);
        assert_eq!(options.app_id.as_deref(), Some("com.example.Notes"));
        let spec = WindowSpec::main("Notes").app_id("com.example.Other");
        let options = kiln::window_options(&spec, cx);
        assert_eq!(options.app_id.as_deref(), Some("com.example.Other"));
    });
}
