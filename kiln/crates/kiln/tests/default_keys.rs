// SPDX-License-Identifier: LicenseRef-Proprietary

//! kiln's bundled keymap and menus, which `Config::default_keys` and
//! `Config::default_menus` turn on: the keys an app on kiln-shell needs,
//! under the app's keymap and the user's; the menus an app without a
//! table gets; and neither when they're off, as by default.

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use gpui::TestAppContext;
use kiln::{
    Config,
    command::{CommandId, ContextFrame, ContextStack, Value},
    core::{Keymap, Keystroke, Os},
    platform::{keys, menus},
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
}

/// A file under the system's temporary directory with `text`, removed
/// when dropped.
struct TempFile(PathBuf);

impl TempFile {
    fn new(text: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "kiln-default-keys-{}-{}.toml",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(name);
        fs::write(&path, text).unwrap();
        Self(path)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        fs::remove_file(&self.0).ok();
    }
}

/// The command `keys` alone runs in the frames `context`, as on `os`.
fn lookup(keymap: &Keymap, keys: &str, context: &[&str], os: Os) -> Option<String> {
    let keystroke = Keystroke::parse_for(keys, os).unwrap();
    let stack: ContextStack = context
        .iter()
        .map(|&name| ContextFrame::new(name))
        .collect();
    let invocation = keymap.lookup(&keystroke, &stack)?;
    Some(invocation.command.to_string())
}

/// Registers the commands with `kiln::init`, for the catalog.
fn catalog(cx: &mut TestAppContext) -> std::rc::Rc<dyn kiln::core::CommandCatalog> {
    cx.update(|cx| {
        kiln::init(cx, config());
        kiln::catalog(cx)
    })
}

#[gpui::test]
fn the_bundled_keys_resolve_under_an_app_keymap_without_them(cx: &mut TestAppContext) {
    let catalog = catalog(cx);
    let config = config().default_keys(true);
    for os in [Os::MacOs, Os::Linux] {
        let (keymap, diagnostics) = config.keymap_for(os, Some(&*catalog));
        assert_eq!(diagnostics, [], "on {os:?}");
        let expected = [
            ("up", "picker", "picker.select_prev"),
            ("down", "picker", "picker.select_next"),
            ("enter", "picker", "picker.confirm"),
            ("escape", "picker", "picker.dismiss"),
            ("backspace", "text_input", "text_input.backspace"),
        ];
        for (keys, context, command) in expected {
            let found = lookup(&keymap, keys, &[context], os);
            assert_eq!(found.as_deref(), Some(command), "{keys} on {os:?}");
        }
        let word = if os == Os::MacOs {
            "alt-backspace"
        } else {
            "ctrl-backspace"
        };
        let found = lookup(&keymap, word, &["text_input"], os);
        assert_eq!(
            found.as_deref(),
            Some("text_input.delete_word_left"),
            "on {os:?}"
        );
        let found = lookup(&keymap, "secondary-shift-p", &[], os);
        assert_eq!(
            found.as_deref(),
            Some("command_palette.toggle"),
            "anywhere, on {os:?}"
        );
        let found = lookup(&keymap, "secondary-q", &["main", "editor"], os);
        assert_eq!(found.as_deref(), Some("app.quit"), "on {os:?}");
        let found = lookup(&keymap, "secondary-n", &[], os);
        assert_eq!(found.as_deref(), Some("notes.new_note"), "the app's too");
        let f10 = lookup(&keymap, "f10", &[], os);
        if os == Os::MacOs {
            assert_eq!(f10, None, "macOS's menus are native");
        } else {
            assert_eq!(f10.as_deref(), Some("menu_bar.open"), "on {os:?}");
        }
    }
}

#[gpui::test]
fn init_installs_the_bundled_keys(cx: &mut TestAppContext) {
    let diagnostics = cx.update(|cx| kiln::init(cx, config().default_keys(true)));
    assert_eq!(diagnostics, []);
    cx.update(|cx| {
        let keymap = keys::current_keymap(cx);
        let found = lookup(&keymap, "secondary-shift-p", &[], Os::current());
        assert_eq!(found.as_deref(), Some("command_palette.toggle"));
    });
}

#[gpui::test]
fn the_apps_keymap_overrides_the_bundled_and_the_users_overrides_both(cx: &mut TestAppContext) {
    let catalog = catalog(cx);
    let app = format!(
        "{KEYMAP}\n[[bindings]]\nkeys = \"secondary-q\"\ncommand = \"notes.new_note\"\n\
         \n[[bindings]]\nkeys = \"escape\"\ncommand = \"app.quit\"\ncontext = \"picker\"\n"
    );
    let user = TempFile::new(
        "[[bindings]]\nkeys = \"escape\"\ncommand = \"window.close\"\ncontext = \"picker\"\n",
    );
    for os in [Os::MacOs, Os::Linux] {
        let config = config()
            .keymap("keymap.toml", app.clone())
            .default_keys(true);
        let (keymap, diagnostics) = config.keymap_for(os, Some(&*catalog));
        assert_eq!(diagnostics, []);
        let quit = lookup(&keymap, "secondary-q", &[], os);
        assert_eq!(
            quit.as_deref(),
            Some("notes.new_note"),
            "the app's, on {os:?}"
        );
        let escape = lookup(&keymap, "escape", &["picker"], os);
        assert_eq!(escape.as_deref(), Some("app.quit"), "the app's, on {os:?}");
        let enter = lookup(&keymap, "enter", &["picker"], os);
        assert_eq!(enter.as_deref(), Some("picker.confirm"), "the rest stay");

        let (keymap, diagnostics) = config.user_keymap(&user.0).keymap_for(os, Some(&*catalog));
        assert_eq!(diagnostics, []);
        let escape = lookup(&keymap, "escape", &["picker"], os);
        assert_eq!(
            escape.as_deref(),
            Some("window.close"),
            "the user's, on {os:?}"
        );
        let quit = lookup(&keymap, "secondary-q", &[], os);
        assert_eq!(quit.as_deref(), Some("notes.new_note"));
    }
}

#[gpui::test]
fn by_default_the_keymap_is_the_apps_and_the_users_alone(cx: &mut TestAppContext) {
    let user =
        TempFile::new("[[bindings]]\nkeys = \"secondary-shift-n\"\ncommand = \"notes.new_note\"\n");
    let (_, closed) = kiln::platform::config::batch_channel();
    let config = config().user_keymap(&user.0).watch_with(closed);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    assert_eq!(diagnostics, []);
    cx.update(|cx| {
        let keymap = keys::current_keymap(cx);
        let commands: Vec<String> = keymap
            .bindings()
            .iter()
            .map(|binding| binding.command.to_string())
            .collect();
        assert_eq!(commands, ["notes.new_note", "notes.new_note"]);
        assert_eq!(menus::installed(cx).map(|bar| bar.menus.len()), Some(0));
        let catalog = kiln::catalog(cx);
        for os in [Os::MacOs, Os::Linux, Os::Windows] {
            let (keymap, _) = Config::new("com.example.Notes", "Notes")
                .keymap("keymap.toml", KEYMAP)
                .keymap_for(os, Some(&*catalog));
            assert_eq!(keymap.bindings().len(), 1, "on {os:?}");
        }
    });
}

#[gpui::test]
fn init_installs_the_bundled_menus_named_for_the_app(cx: &mut TestAppContext) {
    let diagnostics = cx.update(|cx| kiln::init(cx, config().default_menus(true)));
    assert_eq!(diagnostics, []);
    cx.update(|cx| {
        let bar = menus::installed(cx).expect("the menus are installed");
        let names: Vec<&str> = bar.menus.iter().map(|menu| menu.name.as_str()).collect();
        assert_eq!(names, ["Notes", "View", "Window"]);
    });
}

/// Each menu's command items as `title: command`, as on `os`.
fn items(config: &Config, os: Os, cx: &mut TestAppContext) -> Vec<(String, Vec<String>)> {
    cx.update(|cx| {
        let catalog = kiln::catalog(cx);
        let (bar, diagnostics) = config.menu_bar_for(os, Some(&*catalog));
        assert_eq!(diagnostics, [], "on {os:?}");
        bar.menus
            .iter()
            .map(|menu| {
                let items = menu
                    .items
                    .iter()
                    .filter_map(|item| item.command())
                    .map(|item| {
                        let known = catalog.check(&item.command, &Value::Null);
                        assert_eq!(known, Ok(()), "{} on {os:?}", item.command);
                        format!("{}: {}", item.title, item.command)
                    })
                    .collect();
                (menu.name.clone(), items)
            })
            .collect()
    })
}

#[gpui::test]
fn the_bundled_menus_on_each_platform(cx: &mut TestAppContext) {
    let _ = catalog(cx);
    let config = Config::new("com.example.Notes", "Notes").default_menus(true);
    let view = (
        "View".to_owned(),
        vec![
            "Command Palette: command_palette.toggle".to_owned(),
            "Toggle Full Screen: window.toggle_fullscreen".to_owned(),
        ],
    );
    let window = (
        "Window".to_owned(),
        vec![
            "Minimize: window.minimize".to_owned(),
            "Zoom: window.zoom".to_owned(),
            "Close Window: window.close".to_owned(),
        ],
    );
    let macos = (
        "Notes".to_owned(),
        vec![
            "Hide Notes: app.hide".to_owned(),
            "Hide Others: app.hide_others".to_owned(),
            "Show All: app.show_all".to_owned(),
            "Quit Notes: app.quit".to_owned(),
        ],
    );
    assert_eq!(
        items(&config, Os::MacOs, cx),
        [macos, view.clone(), window.clone()]
    );
    let other = ("Notes".to_owned(), vec!["Quit Notes: app.quit".to_owned()]);
    for os in [Os::Linux, Os::Windows] {
        let expected = [other.clone(), view.clone(), window.clone()];
        assert_eq!(items(&config, os, cx), expected, "on {os:?}");
    }
}

#[gpui::test]
fn the_app_name_is_quoted_into_the_bundled_menus(cx: &mut TestAppContext) {
    let _ = catalog(cx);
    let config = Config::new("com.example.Quotes", r#"Say "Hi" \o/"#).default_menus(true);
    let menus = items(&config, Os::Linux, cx);
    assert_eq!(menus[0].0, r#"Say "Hi" \o/"#);
    assert_eq!(menus[0].1, [r#"Quit Say "Hi" \o/: app.quit"#]);
}

/// The commands `menus` hold, sorted.
fn commands(menus: &[(String, Vec<String>)]) -> Vec<String> {
    let mut commands: Vec<String> = menus
        .iter()
        .flat_map(|(_, items)| items.iter())
        .map(|item| item.rsplit(": ").next().unwrap().to_owned())
        .collect();
    commands.sort();
    commands
}

/// Menus are known by name, so the bundled menu an app shares its name
/// with is folded into its app menu: every command stays, in a menu each.
#[gpui::test]
fn an_app_named_like_a_bundled_menu_keeps_every_menu(cx: &mut TestAppContext) {
    let _ = catalog(cx);
    let notes = Config::new("com.example.Notes", "Notes").default_menus(true);
    for name in ["View", "Window"] {
        let config = Config::new("com.example.Clash", name).default_menus(true);
        for os in [Os::MacOs, Os::Linux, Os::Windows] {
            let menus = items(&config, os, cx);
            let names: Vec<&str> = menus.iter().map(|(name, _)| name.as_str()).collect();
            let (other, folded) = if name == "View" {
                ("Window", "Toggle Full Screen: window.toggle_fullscreen")
            } else {
                ("View", "Close Window: window.close")
            };
            assert_eq!(names, [name, other], "{name} on {os:?}");
            let last = menus[0].1.last().map(String::as_str);
            assert_eq!(last, Some(folded), "{name} on {os:?}");
            assert_eq!(
                commands(&menus),
                commands(&items(&notes, os, cx)),
                "{name} on {os:?}"
            );
        }
    }
}

#[gpui::test]
fn init_installs_the_menus_of_an_app_named_view(cx: &mut TestAppContext) {
    init_named("View", false, cx);
}

#[gpui::test]
fn init_installs_the_menus_of_an_app_named_window_failing_fast(cx: &mut TestAppContext) {
    init_named("Window", true, cx);
}

#[gpui::test]
fn init_installs_the_menus_of_an_app_named_window(cx: &mut TestAppContext) {
    init_named("Window", false, cx);
}

#[gpui::test]
fn init_installs_the_menus_of_an_app_named_view_failing_fast(cx: &mut TestAppContext) {
    init_named("View", true, cx);
}

/// `kiln::init` for an app called `name` with kiln's menus, and
/// `fail_fast` on or off: it neither panics nor reports anything, and
/// installs two menus, the app's first.
fn init_named(name: &str, fail_fast: bool, cx: &mut TestAppContext) {
    let config = Config::new("com.example.Clash", name.to_owned())
        .default_keys(true)
        .default_menus(true)
        .fail_fast(fail_fast);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    assert_eq!(diagnostics, []);
    cx.update(|cx| {
        let bar = menus::installed(cx).expect("the menus are installed");
        assert_eq!(bar.menus.len(), 2);
        assert_eq!(bar.menus[0].name, name);
    });
}

#[gpui::test]
fn an_app_menu_table_replaces_the_bundled_menus(cx: &mut TestAppContext) {
    let config = config().menus("menus.toml", MENUS).default_menus(true);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    assert_eq!(diagnostics, []);
    cx.update(|cx| {
        let bar = menus::installed(cx).expect("the menus are installed");
        let names: Vec<&str> = bar.menus.iter().map(|menu| menu.name.as_str()).collect();
        assert_eq!(names, ["File"]);
    });
}

#[gpui::test]
fn the_bundled_keymap_and_menus_load_cleanly_on_every_platform(cx: &mut TestAppContext) {
    let catalog = catalog(cx);
    let config = Config::new("com.example.Notes", "Notes")
        .default_keys(true)
        .default_menus(true);
    for os in [Os::MacOs, Os::Linux, Os::Windows] {
        let (keymap, diagnostics) = config.keymap_for(os, Some(&*catalog));
        assert_eq!(diagnostics, [], "keymap on {os:?}");
        let bindings = if os == Os::MacOs { 8 } else { 9 };
        assert_eq!(keymap.bindings().len(), bindings, "on {os:?}");
        let (bar, diagnostics) = config.menu_bar_for(os, Some(&*catalog));
        assert_eq!(diagnostics, [], "menus on {os:?}");
        assert_eq!(bar.menus.len(), 3, "on {os:?}");
        let quit = CommandId::parse("app.quit").unwrap();
        assert!(
            bar.menus[0]
                .items
                .iter()
                .any(|item| item.command().is_some_and(|item| item.command == quit))
        );
    }
}
