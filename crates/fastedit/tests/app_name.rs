//! The app is Tenmoku where people see it: its binary, its app menu, the
//! text it ships and its bundled themes.

use std::path::Path;

use fe_config::themes::{bundled_themes, find};
use fe_gpui::fe_keymap::{self, DEFAULT_LINUX_MENUS, DEFAULT_MACOS_MENUS, KEYMAP_HEADER, MENUS};
use gpui::TestAppContext;
use kiln::platform::{command_actions::typed_commands, commands::app_command_list};

#[test]
fn the_binary_is_named_tenmoku() {
    let binary = Path::new(env!("CARGO_BIN_EXE_tenmoku"));
    assert!(binary.is_file(), "{}", binary.display());
    assert_eq!(
        binary.file_stem().and_then(|stem| stem.to_str()),
        Some("tenmoku")
    );
}

#[test]
fn app_menu_and_display_name_say_tenmoku() {
    assert_eq!(fe_config::paths::app_id().display, "Tenmoku");
    assert_eq!(fe_config::APP_NAME, "Tenmoku");

    let (bar, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    let bar = bar.unwrap();
    let app = bar.menus.first().unwrap();
    assert_eq!(app.name, "Tenmoku");
    let titles: Vec<_> = (app.items.iter())
        .filter_map(|item| item.command())
        .map(|item| item.title.as_str())
        .collect();
    assert!(titles.contains(&"Hide Tenmoku"), "{titles:?}");
    assert!(titles.contains(&"Quit Tenmoku"), "{titles:?}");
}

/// Whether `text` names fastedit, but for the project directory
/// `.fastedit/`, which T1258 moves.
fn names_fastedit(text: &str) -> bool {
    text.to_lowercase()
        .replace(".fastedit/", "")
        .contains("fastedit")
}

#[gpui::test]
fn no_shipped_text_names_fastedit(cx: &mut TestAppContext) {
    let shipped = [
        ("menus", MENUS),
        ("macOS menus", DEFAULT_MACOS_MENUS),
        ("Linux menus", DEFAULT_LINUX_MENUS),
        ("keymap header", KEYMAP_HEADER),
        ("settings template", fe_config::USER_SETTINGS_TEMPLATE),
        (
            "settings schema",
            include_str!("../../fe-config/settings.schema.json"),
        ),
        ("welcome", fe_workbench::WELCOME),
    ];
    for (what, text) in shipped {
        let lines: Vec<_> = text.lines().filter(|line| names_fastedit(line)).collect();
        assert_eq!(lines, [""; 0], "{what}");
    }

    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let commands = cx.update(|cx| {
        let typed = typed_commands(cx)
            .into_iter()
            .map(|typed| (typed.id.as_str().to_owned(), typed.title, String::new()));
        let metas = (app_command_list(cx).into_iter())
            .chain(fe_workbench::command_list())
            .map(|meta| (meta.id.as_str().to_owned(), meta.title, meta.description));
        typed.chain(metas).collect::<Vec<_>>()
    });
    assert!(commands.len() > 100, "{}", commands.len());
    let named: Vec<_> = (commands.iter())
        .filter(|(_, title, description)| names_fastedit(title) || names_fastedit(description))
        .collect();
    assert_eq!(named, [] as [&(String, String, String); 0]);
}

#[test]
fn bundled_themes_are_tenmoku() {
    let ours: Vec<_> = (bundled_themes().iter())
        .filter(|theme| theme.id.starts_with("tenmoku-"))
        .map(|theme| (theme.title, theme.id))
        .collect();
    assert_eq!(
        ours,
        [
            ("Tenmoku Dark", "tenmoku-dark"),
            ("Tenmoku Light", "tenmoku-light"),
            ("Tenmoku Ember", "tenmoku-ember"),
        ]
    );
    let ember = find("Tenmoku Ember").unwrap();
    assert_eq!(ember.id, "tenmoku-ember");
    assert_eq!(find("Ember"), Some(ember));
    assert_eq!(find("tenmoku-ember"), Some(ember));
    assert!(
        (bundled_themes().iter())
            .all(|theme| !names_fastedit(theme.title) && !names_fastedit(theme.id))
    );
}
