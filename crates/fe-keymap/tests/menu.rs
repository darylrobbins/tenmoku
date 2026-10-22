//! The menu table: the default bar, shortcuts resolved through the keymap,
//! per-context alternatives, and hosts extending or replacing the bar.

use std::{collections::HashSet, path::Path};

use fe_command::{CommandId, ContextFrame, ContextStack, Source, Value};
use fe_config::Severity;
use fe_keymap::{
    CatalogError, CommandCatalog, DEFAULT_KEYMAP_PATH, DEFAULT_MACOS, ItemKind, KeySequence,
    Keymap, KeymapLayer, MenuBar, OsAction, SystemMenu, shortcuts,
};

fn stack(names: &[&str]) -> ContextStack {
    names.iter().copied().map(ContextFrame::new).collect()
}

/// The default keymap, with `user` over it if given.
fn keymap(user: Option<&str>) -> Keymap {
    let (default, _) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), DEFAULT_MACOS, None);
    let mut layers = vec![default.expect("the default keymap loads")];
    if let Some(user) = user {
        let (user, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), user, None);
        assert_eq!(diagnostics, [], "{user:?}");
        layers.push(user.unwrap());
    }
    Keymap::from_layers(layers)
}

fn default_bar() -> MenuBar {
    let (bar, diagnostics) = fe_keymap::default_menus_macos(None);
    assert_eq!(diagnostics, []);
    bar.expect("the default menus load")
}

/// The command item of `menu` with `id`.
fn item<'a>(bar: &'a MenuBar, menu: &str, id: &str) -> &'a fe_keymap::CommandItem {
    bar.menu(menu)
        .and_then(|menu| menu.item(id))
        .and_then(|item| item.command())
        .unwrap_or_else(|| panic!("{menu} > {id}"))
}

fn keys(source: &str) -> KeySequence {
    KeySequence::parse(source).unwrap()
}

/// Item IDs, `-` for a separator.
fn ids(bar: &MenuBar, menu: &str) -> Vec<String> {
    let menu = bar.menu(menu).unwrap();
    menu.items
        .iter()
        .map(|item| item.id().unwrap_or("-").to_owned())
        .collect()
}

#[test]
fn default_menus_parse() {
    let bar = default_bar();
    let names: Vec<_> = bar.menus.iter().map(|menu| menu.name.as_str()).collect();
    assert_eq!(names, ["fastedit", "File", "Edit", "View", "Go", "Window"]);

    let edit = bar.menu("Edit").unwrap();
    let os_actions: Vec<_> = edit
        .items
        .iter()
        .filter_map(|item| item.command())
        .filter_map(|item| Some((item.title.as_str(), item.os_action?)))
        .collect();
    assert_eq!(
        os_actions,
        [
            ("Undo", OsAction::Undo),
            ("Redo", OsAction::Redo),
            ("Cut", OsAction::Cut),
            ("Copy", OsAction::Copy),
            ("Paste", OsAction::Paste),
            ("Select All", OsAction::SelectAll),
        ]
    );

    let app = bar.menu("fastedit").unwrap();
    let services = app.items.iter().find_map(|item| match &item.kind {
        ItemKind::System(system) => Some(system),
        _ => None,
    });
    assert_eq!(
        services.map(|system| (system.title.as_str(), system.menu)),
        Some(("Services", SystemMenu::Services))
    );

    for menu in &bar.menus {
        let ids: Vec<_> = menu.items.iter().filter_map(|item| item.id()).collect();
        let unique: HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "{}", menu.name);
        // The default table is tidy, and places nothing.
        assert!(menu.items.iter().all(|item| item.place.is_none()));
        assert_ne!(menu.items.first().unwrap().kind, ItemKind::Separator);
        assert_ne!(menu.items.last().unwrap().kind, ItemKind::Separator);
    }
}

#[test]
fn shortcuts_follow_the_keymap() {
    let bar = default_bar();
    let keymap = keymap(None);
    let shortcut = |menu: &str, id: &str, keymap: &Keymap| item(&bar, menu, id).shortcut(keymap);
    assert_eq!(
        shortcut("View", "search.show", &keymap),
        Some(keys("cmd-shift-f"))
    );
    assert_eq!(
        shortcut("File", "editor.save", &keymap),
        Some(keys("cmd-s"))
    );
    assert_eq!(
        shortcut("Go", "workbench.next_tab", &keymap),
        Some(keys("cmd-shift-]"))
    );
    assert_eq!(
        shortcut("fastedit", "app.hide", &keymap),
        Some(keys("cmd-h"))
    );

    // A chord shows no key, though it runs the command.
    assert_eq!(shortcut("File", "pane.close", &keymap), None);
    let close = CommandId::parse("pane.close").unwrap();
    let workbench_editor = stack(&["workbench", "editor"]);
    assert_eq!(
        shortcuts(&keymap, &close, &Value::Null, &workbench_editor),
        [keys("cmd-k cmd-w")]
    );

    // A user binding moves it.
    let moved = self::keymap(Some(
        r#"
        [[bindings]]
        keys = "alt-cmd-s"
        command = "editor.save"
        context = "editor"
        "#,
    ));
    let save = CommandId::parse("editor.save").unwrap();
    assert_eq!(
        shortcuts(&moved, &save, &Value::Null, &workbench_editor)[0],
        keys("alt-cmd-s")
    );
    assert_eq!(
        shortcut("File", "editor.save", &moved),
        Some(keys("alt-cmd-s"))
    );

    // An unbinding removes it.
    let unbound = self::keymap(Some(
        r#"
        [[bindings]]
        keys = "cmd-s"
        command = ""
        context = "editor"
        "#,
    ));
    assert_eq!(shortcut("File", "editor.save", &unbound), None);

    // A deeper binding for the same keys hides it.
    let hidden = self::keymap(Some(
        r#"
        [[bindings]]
        keys = "cmd-b"
        command = "editor.select_all"
        context = "editor"
        "#,
    ));
    assert_eq!(
        shortcut("View", "dock.toggle", &keymap),
        Some(keys("cmd-b"))
    );
    assert_eq!(shortcut("View", "dock.toggle", &hidden), None);
    // A chord whose context doesn't hold isn't listed, though a binding
    // for its first key runs the command.
    let prefixed = self::keymap(Some(
        r#"
        [[bindings]]
        keys = "ctrl-x"
        command = "editor.save"
        context = "editor"

        [[bindings]]
        keys = "ctrl-x ctrl-s"
        command = "editor.save"
        context = "terminal"
        "#,
    ));
    assert_eq!(
        shortcuts(&prefixed, &save, &Value::Null, &workbench_editor),
        [keys("ctrl-x"), keys("cmd-s")]
    );

    let toggle = CommandId::parse("dock.toggle").unwrap();
    assert_eq!(
        shortcuts(&hidden, &toggle, &Value::Null, &workbench_editor),
        []
    );
}

#[test]
fn alternatives_follow_the_context() {
    let bar = default_bar();
    let paste = item(&bar, "Edit", "editor.paste");
    for (frames, command) in [
        (
            &["workbench", "search_panel", "text_input"][..],
            "text_input.paste",
        ),
        (&["workbench", "editor"][..], "editor.paste"),
    ] {
        let stack = stack(frames);
        let invocation = paste.invocation(&stack);
        assert_eq!(invocation.command.as_str(), command);
        assert_eq!(invocation.source, Source::Menu);
        assert_eq!(invocation.context, stack);
        assert_eq!(invocation.args, Value::Null);
    }

    // Args, the item's or an alternative's, reach the invocation as given.
    let (bar, diagnostics) = MenuBar::parse(
        Path::new("menus.toml"),
        r#"
        [[menus]]
        name = "Edit"

        [[menus.items]]
        title = "Select Word Left"
        command = "editor.move"
        args = { motion = "word_left", extend = true }
        alternatives = [{ command = "text_input.move", args = { by = "word" }, when = "text_input" }]
        "#,
        None,
    );
    assert_eq!(diagnostics, []);
    let bar = bar.unwrap();
    let item = item(&bar, "Edit", "editor.move");
    let editor = item.invocation(&stack(&["editor"]));
    assert_eq!(
        editor.args,
        serde_json::json!({ "motion": "word_left", "extend": true })
    );
    let input = item.invocation(&stack(&["text_input"]));
    assert_eq!(input.command.as_str(), "text_input.move");
    assert_eq!(input.args, serde_json::json!({ "by": "word" }));
}

/// Every command but one.
struct Without(&'static str);

impl CommandCatalog for Without {
    fn check(&self, command: &CommandId, _: &Value) -> Result<(), CatalogError> {
        match command.as_str() == self.0 {
            true => Err(CatalogError::UnknownCommand),
            false => Ok(()),
        }
    }
}

#[test]
fn hosts_extend_and_replace() {
    let mut bar = default_bar();
    let (layer, diagnostics) = MenuBar::parse(
        Path::new("host-menus.toml"),
        r#"
        [[menus]]
        name = "Tools"

        [[menus.items]]
        title = "Run Tests"
        command = "tools.run_tests"

        [[menus]]
        name = "View"

        [[menus.items]]
        title = "Terminal"
        command = "terminal.toggle"
        after = "dock.toggle"

        [[menus]]
        name = "Edit"

        [[menus.items]]
        id = "search.show"
        remove = true

        [[menus.items]]
        id = "search.show_replace"
        remove = true

        [[menus]]
        name = "File"

        [[menus.items]]
        title = "Save File"
        command = "editor.save"
        "#,
        None,
    );
    assert_eq!(diagnostics, []);
    bar.extend(layer.unwrap());
    let names: Vec<_> = bar.menus.iter().map(|menu| menu.name.as_str()).collect();
    assert_eq!(
        names,
        ["fastedit", "File", "Edit", "View", "Go", "Tools", "Window"]
    );
    assert_eq!(ids(&bar, "Tools"), ["tools.run_tests"]);
    assert_eq!(
        &ids(&bar, "View")[..9],
        [
            "dock.show_project",
            "search.show",
            "outline.show",
            "problems.show",
            "dock.toggle",
            "terminal.toggle",
            "chat.open",
            "view.toggle_breadcrumbs",
            "view.toggle_status_bar",
        ]
    );
    // An item with a taken ID replaces it where it was.
    assert_eq!(
        ids(&bar, "File")[..10],
        [
            "workbench.new_file",
            "-",
            "workbench.open_folder",
            "workbench.open_file",
            "workbench.open_recent",
            "workbench.trust_folder",
            "workbench.untrust_folder",
            "-",
            "editor.save",
            "workbench.save_as"
        ]
    );
    assert_eq!(item(&bar, "File", "editor.save").title, "Save File");
    // The separator before Find in Files and Replace in Files went with
    // them.
    assert_eq!(
        ids(&bar, "Edit"),
        [
            "editor.undo",
            "editor.redo",
            "-",
            "editor.cut",
            "editor.copy",
            "editor.paste",
            "editor.select_all",
            "-",
            "editor.indent",
            "editor.outdent",
            "editor.move_rows_up",
            "editor.move_rows_down",
            "editor.duplicate_rows",
            "editor.delete_rows",
            "editor.toggle_comment",
            "-",
            "lsp.rename",
            "lsp.code_actions",
            "lsp.format_document",
            "lsp.format_selection",
            "-",
            "find.show",
            "find.show_replace"
        ]
    );

    // A catalog that lacks a command drops its item, warning once.
    let (bar, diagnostics) = fe_keymap::default_menus_macos(Some(&Without("outline.show")));
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, Severity::Warning);
    assert!(diagnostics[0].message.contains("outline.show"));
    let bar = bar.unwrap();
    assert!(bar.menu("View").unwrap().item("outline.show").is_none());
    assert!(bar.menu("View").unwrap().item("dock.toggle").is_some());

    // A menu whose items all drop goes too.
    let (bar, diagnostics) = MenuBar::parse(
        Path::new("menus.toml"),
        r#"
        [[menus]]
        name = "Outline"

        [[menus.items]]
        title = "Outline"
        command = "outline.show"
        "#,
        Some(&Without("outline.show")),
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(bar, Some(MenuBar::default()));

    // An unknown field is an error.
    let (bar, diagnostics) = MenuBar::parse(
        Path::new("menus.toml"),
        r#"
        [[menus]]
        name = "File"

        [[menus.items]]
        title = "Save"
        command = "editor.save"
        shortcut = "cmd-s"
        "#,
        None,
    );
    assert_eq!(bar, None);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics[0].is_error());
    assert_eq!(
        diagnostics[0].key.as_deref(),
        Some("menus[0].items[0].shortcut")
    );

    // A table parsed alone is the whole bar.
    let (bar, diagnostics) = MenuBar::parse(
        Path::new("menus.toml"),
        r#"
        [[menus]]
        name = "Host"

        [[menus.items]]
        title = "Quit"
        command = "app.quit"
        "#,
        None,
    );
    assert_eq!(diagnostics, []);
    let bar = bar.unwrap();
    let names: Vec<_> = bar.menus.iter().map(|menu| menu.name.as_str()).collect();
    assert_eq!(names, ["Host"]);
    assert_eq!(ids(&bar, "Host"), ["app.quit"]);
}

/// Every command item's command, by menu, in order.
fn commands_by_menu(bar: &MenuBar) -> Vec<(String, Vec<String>)> {
    bar.menus
        .iter()
        .map(|menu| {
            let commands = menu
                .items
                .iter()
                .filter_map(|item| item.command())
                .map(|item| item.command.as_str().to_owned())
                .collect();
            (menu.name.clone(), commands)
        })
        .collect()
}

#[test]
fn linux_menus_are_the_macos_menus_without_the_app_menu() {
    let (linux, diagnostics) = fe_keymap::default_menus_linux(None);
    assert_eq!(diagnostics, []);
    let linux = linux.expect("the Linux menus load");
    let names: Vec<_> = linux.menus.iter().map(|menu| menu.name.as_str()).collect();
    assert_eq!(names, ["File", "Edit", "View", "Go", "Window"]);

    // macOS's, but for what only macOS has, with the app menu's Settings…
    // and Quit in File, before Close Tab and last.
    let macos_only = [
        "app.hide",
        "app.hide_others",
        "app.show_all",
        "window.minimize",
        "window.zoom",
    ];
    let mut expected = commands_by_menu(&default_bar());
    expected.remove(0);
    for (_, commands) in &mut expected {
        commands.retain(|command| !macos_only.contains(&command.as_str()));
    }
    let file = &mut expected[0].1;
    let close_tab = file
        .iter()
        .position(|c| c == "workbench.close_tab")
        .unwrap();
    file.insert(close_tab, "workbench.open_settings".to_owned());
    file.push("app.quit".to_owned());
    assert_eq!(commands_by_menu(&linux), expected);

    // No system item and no doubled or trailing separator.
    for menu in &linux.menus {
        assert!(!matches!(
            menu.items.last().unwrap().kind,
            ItemKind::Separator
        ));
        for pair in menu.items.windows(2) {
            let separators = pair
                .iter()
                .filter(|item| matches!(item.kind, ItemKind::Separator))
                .count();
            assert!(separators < 2, "{}", menu.name);
        }
        assert!(
            menu.items
                .iter()
                .all(|item| !matches!(item.kind, ItemKind::System(_))),
            "{}",
            menu.name
        );
    }
    // Edit > Paste keeps its alternative for the search box.
    let paste = item(&linux, "Edit", "editor.paste");
    assert_eq!(
        paste
            .invocation(&stack(&["workbench", "text_input"]))
            .command
            .as_str(),
        "text_input.paste"
    );
}
