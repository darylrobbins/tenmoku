// SPDX-License-Identifier: Apache-2.0

//! Extensions' keymaps and menus through `kiln::init`, headless: laid
//! between the app's and the user's keymap, added to the app's menus but
//! never taking from them, and resolved again when their files change.
//! Changes are sent as batches, as the watchers would send them.

use std::{
    fs,
    path::{Path, PathBuf},
};

use gpui::{App, TestAppContext};
use kiln::{
    Config,
    command::{ContextFrame, ContextStack},
    core::{
        ApiVersion, Diagnostic, KeySequence, KeymapIssue, Keystroke, Severity,
        extension::MANIFEST_FILE,
    },
    platform::{
        config::{self, BatchSender, batch_channel},
        keys, menus,
    },
};

mod notes {
    kiln::commands!(
        notes,
        [
            /// New Note
            NewNote,
            /// Archive
            Archive,
            /// Find
            Find,
            /// Replace
            Replace,
            /// Tidy
            Tidy,
            /// Show All
            ShowAll,
        ]
    );
}

const KEYMAP: &str = r#"
[[bindings]]
keys = "secondary-n"
command = "notes.new_note"

[[bindings]]
keys = "secondary-f"
context = "editor"
command = "notes.find"
"#;

const MENUS: &str = r#"
[[menus]]
name = "File"

[[menus.items]]
title = "New Note"
command = "notes.new_note"

[[menus.items]]
title = "Archive"
command = "notes.archive"

[[menus]]
name = "Edit"

[[menus.items]]
title = "Find"
command = "notes.find"

[[menus]]
name = "Window"

[[menus.items]]
title = "Show All"
command = "notes.show_all"
"#;

const MANIFEST: &str = "\
id = \"acme.tidy\"
name = \"Tidy\"
version = \"1.0.0\"
license = \"MIT\"

[engine]
notes = \"1.0\"

[contributes]
keymaps = [\"keymap.toml\"]
menus = [\"menus.toml\"]
";

const EXTENSION_KEYMAP: &str = r#"
[[bindings]]
keys = "secondary-alt-t"
command = "notes.tidy"

[[bindings]]
keys = "secondary-f"
context = "editor"
command = "notes.replace"
"#;

const EXTENSION_MENUS: &str = r#"
[[menus]]
name = "File"

[[menus.items]]
title = "Tidy"
command = "notes.tidy"
after = "notes.new_note"

[[menus]]
name = "Acme"

[[menus.items]]
title = "Tidy"
command = "notes.tidy"
"#;

/// An app with the extension installed under `state`, its keymap and
/// menus as given, the user's keymap in `state/config`.
struct Setup {
    extensions: PathBuf,
    extension: PathBuf,
    user_keymap: PathBuf,
    sender: BatchSender,
}

impl Setup {
    fn new(state: &Path, keymap: &str, menus: &str) -> (Self, Config) {
        let extensions = state.join("extensions");
        let extension = extensions.join("tidy");
        fs::create_dir_all(&extension).unwrap();
        fs::write(extension.join(MANIFEST_FILE), MANIFEST).unwrap();
        fs::write(extension.join("keymap.toml"), keymap).unwrap();
        fs::write(extension.join("menus.toml"), menus).unwrap();
        let user_keymap = state.join("config/keymap.toml");
        let (sender, batches) = batch_channel();
        let config = Config::new("com.example.Notes", "Notes")
            .commands(notes::register)
            .keymap("keymap.toml", KEYMAP)
            .menus("menus.toml", MENUS)
            .fail_fast(true)
            .extensions(&extensions, "notes", ApiVersion { major: 1, minor: 0 })
            .user_keymap(&user_keymap)
            .watch_with(batches);
        let setup = Self {
            extensions,
            extension,
            user_keymap,
            sender,
        };
        (setup, config)
    }

    /// Writes the extension's `file` and sends the batch the tree watcher
    /// would.
    fn save(&self, file: &str, text: &str, cx: &mut TestAppContext) {
        fs::write(self.extension.join(file), text).unwrap();
        self.sender.send(vec![self.extensions.clone()]);
        cx.run_until_parked();
    }

    fn save_user_keymap(&self, text: &str, cx: &mut TestAppContext) {
        fs::create_dir_all(self.user_keymap.parent().unwrap()).unwrap();
        fs::write(&self.user_keymap, text).unwrap();
        self.sender.send(vec![self.user_keymap.clone()]);
        cx.run_until_parked();
    }
}

/// Sets the app up with the extension's `keymap` and `menus`, returning
/// what `init` found.
fn init(
    state: &Path,
    keymap: &str,
    menus: &str,
    cx: &mut TestAppContext,
) -> (Setup, Vec<Diagnostic>) {
    let (setup, config) = Setup::new(state, keymap, menus);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();
    (setup, diagnostics)
}

/// The command `keys` alone runs in the frames `context`.
fn runs(keys: &str, context: &[&str], cx: &mut TestAppContext) -> Option<String> {
    let keystroke = Keystroke::parse(keys).unwrap();
    let stack: ContextStack = context
        .iter()
        .map(|&name| ContextFrame::new(name))
        .collect();
    cx.update(|cx| keys::lookup(&keystroke, &stack, cx))
        .map(|invocation| invocation.command.to_string())
}

/// Each menu's name and its items' IDs, as installed.
fn installed(cx: &App) -> Vec<(String, Vec<String>)> {
    let bar = menus::installed(cx).expect("menus are installed");
    bar.menus
        .iter()
        .map(|menu| {
            let ids = menu.items.iter().filter_map(|item| item.id());
            (menu.name.clone(), ids.map(str::to_owned).collect())
        })
        .collect()
}

/// The shortcut the installed `menu`'s item `id` shows.
fn shortcut(menu: &str, id: &str, cx: &App) -> Option<KeySequence> {
    let bar = menus::installed(cx).expect("menus are installed");
    let item = bar.menu(menu)?.item(id)?.command()?;
    item.shortcut(&keys::current_keymap(cx))
}

/// The diagnostics published for `file`, as severity and message.
fn published(file: &Path, cx: &mut TestAppContext) -> Vec<(Severity, String)> {
    cx.update(|cx| {
        config::diagnostics(cx)
            .iter()
            .filter(|diagnostic| diagnostic.file == file)
            .map(|diagnostic| (diagnostic.severity, diagnostic.message.clone()))
            .collect()
    })
}

#[gpui::test]
fn extension_bindings_sit_between_app_and_user(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let (setup, diagnostics) = init(state.path(), EXTENSION_KEYMAP, EXTENSION_MENUS, cx);
    assert_eq!(diagnostics, []);

    // New keys run the extension's command; the app's stay.
    assert_eq!(
        runs("secondary-alt-t", &[], cx).as_deref(),
        Some("notes.tidy")
    );
    assert_eq!(
        runs("secondary-n", &[], cx).as_deref(),
        Some("notes.new_note")
    );
    // The app's keys and context: the extension's, a later layer, wins,
    // and the report names the app's as hidden.
    assert_eq!(
        runs("secondary-f", &["editor"], cx).as_deref(),
        Some("notes.replace")
    );
    let hidden = cx.update(|cx| {
        keys::current_keymap(cx)
            .report()
            .into_iter()
            .filter_map(|issue| match issue {
                KeymapIssue::Overridden { winner, loser } => {
                    Some((winner.command.to_string(), loser.command.to_string()))
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    });
    assert_eq!(hidden, [("notes.replace".into(), "notes.find".into())]);
    assert_eq!(cx.update(|cx| shortcut("Edit", "notes.find", cx)), None);

    // The user's keymap wins over both.
    setup.save_user_keymap(
        "[[bindings]]\nkeys = \"secondary-f\"\ncontext = \"editor\"\ncommand = \"notes.archive\"\n",
        cx,
    );
    assert_eq!(
        runs("secondary-f", &["editor"], cx).as_deref(),
        Some("notes.archive")
    );
    assert_eq!(
        runs("secondary-alt-t", &[], cx).as_deref(),
        Some("notes.tidy")
    );
    cx.update(|cx| assert_eq!(config::diagnostics(cx), []));
}

#[gpui::test]
fn extensions_cannot_unbind_or_remove(cx: &mut TestAppContext) {
    let keymap = r#"
[[bindings]]
keys = "secondary-n"
command = ""

[[bindings]]
keys = "secondary-alt-u"
command = "notes.unknown"

[[bindings]]
keys = "secondary-alt-t"
command = "notes.tidy"
"#;
    let menus = r#"
[[menus]]
name = "File"

[[menus.items]]
id = "notes.archive"
remove = true

[[menus.items]]
title = "Brand New Note"
command = "notes.new_note"

[[menus.items]]
title = "Gone"
command = "notes.unknown"

[[menus.items]]
title = "Tidy"
command = "notes.tidy"
"#;
    let state = tempfile::tempdir().unwrap();
    // fail_fast is on: the extension's problems are its own, not the app's.
    let (setup, diagnostics) = init(state.path(), keymap, menus, cx);

    let keymap_file = setup.extension.join("keymap.toml");
    let menus_file = setup.extension.join("menus.toml");
    let found: Vec<(PathBuf, Severity, usize, String)> = diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic.file.clone(),
                diagnostic.severity,
                diagnostic.line,
                diagnostic.message.clone(),
            )
        })
        .collect();
    assert_eq!(found.len(), 5, "{found:#?}");
    assert!(
        found
            .iter()
            .all(|(_, severity, _, _)| *severity == Severity::Warning),
        "{found:#?}"
    );
    let at = |file: &Path, line: usize, needle: &str| {
        found
            .iter()
            .any(|(f, _, l, message)| f == file && *l == line && message.contains(needle))
    };
    assert!(at(&keymap_file, 2, "can't unbind"), "{found:#?}");
    assert!(at(&keymap_file, 8, "unknown command"), "{found:#?}");
    assert!(at(&menus_file, 5, "can't remove"), "{found:#?}");
    assert!(at(&menus_file, 9, "already has an item"), "{found:#?}");
    assert!(at(&menus_file, 15, "notes.unknown"), "{found:#?}");
    // Published as the extension's.
    cx.update(|cx| assert_eq!(config::diagnostics(cx), diagnostics));

    // The app's binding stays; the extension's other one loads.
    assert_eq!(
        runs("secondary-n", &[], cx).as_deref(),
        Some("notes.new_note")
    );
    assert_eq!(runs("secondary-alt-u", &[], cx), None);
    assert_eq!(
        runs("secondary-alt-t", &[], cx).as_deref(),
        Some("notes.tidy")
    );
    // The app's items stay, as the app titles them; the new one is added.
    cx.update(|cx| {
        assert_eq!(
            installed(cx)[0],
            (
                "File".into(),
                vec![
                    "notes.new_note".into(),
                    "notes.archive".into(),
                    "notes.tidy".into()
                ]
            )
        );
        let bar = menus::installed(cx).unwrap();
        let new_note = bar.menu("File").unwrap().item("notes.new_note").unwrap();
        assert_eq!(new_note.command().unwrap().title, "New Note");
    });
}

#[gpui::test]
fn extension_menu_items_are_added(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let (_setup, diagnostics) = init(state.path(), EXTENSION_KEYMAP, EXTENSION_MENUS, cx);
    assert_eq!(diagnostics, []);
    cx.update(|cx| {
        assert_eq!(
            installed(cx),
            [
                (
                    "File".into(),
                    vec![
                        "notes.new_note".into(),
                        "notes.tidy".into(),
                        "notes.archive".into()
                    ]
                ),
                ("Edit".into(), vec!["notes.find".into()]),
                ("Acme".into(), vec!["notes.tidy".into()]),
                ("Window".into(), vec!["notes.show_all".into()]),
            ]
        );
        let tidy = KeySequence::parse("secondary-alt-t").unwrap();
        assert_eq!(shortcut("File", "notes.tidy", cx), Some(tidy.clone()));
        assert_eq!(shortcut("Acme", "notes.tidy", cx), Some(tidy));
    });
}

#[gpui::test]
fn extension_keys_reload_and_never_fail_fast(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let (setup, diagnostics) = init(state.path(), EXTENSION_KEYMAP, EXTENSION_MENUS, cx);
    assert_eq!(diagnostics, []);
    let keymap_file = setup.extension.join("keymap.toml");
    let menus_file = setup.extension.join("menus.toml");

    // An edit swaps the binding, and the menu's shortcut with it.
    let edited = EXTENSION_KEYMAP.replace("secondary-alt-t", "secondary-alt-y");
    setup.save("keymap.toml", &edited, cx);
    assert_eq!(runs("secondary-alt-t", &[], cx), None);
    assert_eq!(
        runs("secondary-alt-y", &[], cx).as_deref(),
        Some("notes.tidy")
    );
    let tidy = KeySequence::parse("secondary-alt-y").unwrap();
    cx.update(|cx| assert_eq!(shortcut("Acme", "notes.tidy", cx), Some(tidy.clone())));

    // A file with errors keeps its last good layer, and they're published;
    // fail_fast is on, and nothing panics.
    setup.save("keymap.toml", "[[bindings]\n", cx);
    assert_eq!(
        runs("secondary-alt-y", &[], cx).as_deref(),
        Some("notes.tidy")
    );
    let errors = published(&keymap_file, cx);
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].0, Severity::Error);

    setup.save("menus.toml", "[[menus]]\nname = 1\n", cx);
    cx.update(|cx| assert_eq!(installed(cx)[2].0, "Acme"));
    assert_eq!(published(&menus_file, cx).len(), 1);

    // A warning comes and goes with the file.
    let unknown = format!(
        "{edited}\n[[bindings]]\nkeys = \"secondary-alt-u\"\ncommand = \"notes.unknown\"\n"
    );
    setup.save("keymap.toml", &unknown, cx);
    let warnings = published(&keymap_file, cx);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0].0, Severity::Warning);
    setup.save("keymap.toml", &edited, cx);
    setup.save("menus.toml", EXTENSION_MENUS, cx);
    cx.update(|cx| assert_eq!(config::diagnostics(cx), []));

    // Dropping the extension drops its layers.
    fs::remove_dir_all(&setup.extension).unwrap();
    setup.sender.send(vec![setup.extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(runs("secondary-alt-y", &[], cx), None);
    assert_eq!(
        runs("secondary-f", &["editor"], cx).as_deref(),
        Some("notes.find")
    );
    cx.update(|cx| {
        let names: Vec<String> = installed(cx).into_iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["File", "Edit", "Window"]);
    });
}

#[gpui::test]
fn a_missing_keymap_file_leaves_the_rest_loaded(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let (setup, config) = Setup::new(state.path(), EXTENSION_KEYMAP, EXTENSION_MENUS);
    let keymap_file = setup.extension.join("keymap.toml");
    fs::remove_file(&keymap_file).unwrap();
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].file, keymap_file);
    assert!(diagnostics[0].is_error());
    // Its menus load.
    cx.update(|cx| assert_eq!(installed(cx)[2].0, "Acme"));
    assert_eq!(runs("secondary-alt-t", &[], cx), None);

    setup.save("keymap.toml", EXTENSION_KEYMAP, cx);
    assert_eq!(
        runs("secondary-alt-t", &[], cx).as_deref(),
        Some("notes.tidy")
    );
    cx.update(|cx| assert_eq!(config::diagnostics(cx), []));
}

/// A keymap file replaced by a link out of the extension's directory
/// isn't read: its last good layer stays.
#[cfg(unix)]
#[gpui::test]
fn a_keymap_file_leading_outside_is_not_read(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let (setup, diagnostics) = init(state.path(), EXTENSION_KEYMAP, EXTENSION_MENUS, cx);
    assert_eq!(diagnostics, []);
    let outside = state.path().join("outside.toml");
    fs::write(
        &outside,
        "[[bindings]]\nkeys = \"secondary-alt-o\"\ncommand = \"notes.tidy\"\n",
    )
    .unwrap();
    let keymap_file = setup.extension.join("keymap.toml");
    fs::remove_file(&keymap_file).unwrap();
    std::os::unix::fs::symlink(&outside, &keymap_file).unwrap();
    setup.sender.send(vec![setup.extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(runs("secondary-alt-o", &[], cx), None);
    assert_eq!(
        runs("secondary-alt-t", &[], cx).as_deref(),
        Some("notes.tidy")
    );
    let errors = published(&keymap_file, cx);
    assert!(
        errors
            .iter()
            .any(|(severity, message)| *severity == Severity::Error
                && message.contains("outside acme.tidy's directory")),
        "{errors:?}"
    );
}

/// A menu is known by its en-US name, its ID, however its file names it: an
/// extension's menu that names the app's Window menu by its catalogue key
/// still can't replace the app's items there, and the warning points at
/// the item. A file can't give a menu an ID of its own.
#[gpui::test]
fn a_keyed_extension_menu_cannot_replace_the_apps_items(cx: &mut TestAppContext) {
    let menus = r#"
[[menus]]
title_key = "kiln-core-menu-window"

[[menus.items]]
title = "Show Everything"
command = "notes.show_all"

[[menus.items]]
title = "Tidy"
command = "notes.tidy"
"#;
    let state = tempfile::tempdir().unwrap();
    let (setup, diagnostics) = init(state.path(), EXTENSION_KEYMAP, menus, cx);
    let menus_file = setup.extension.join("menus.toml");
    let found: Vec<_> = (diagnostics.iter())
        .map(|d| (d.file.clone(), d.line, d.key.clone(), d.message.clone()))
        .collect();
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].0, menus_file);
    assert_eq!(
        (found[0].1, found[0].2.as_deref()),
        (5, Some("menus[0].items[0]"))
    );
    assert!(found[0].3.contains("already has an item"), "{found:#?}");
    cx.update(|cx| {
        assert_eq!(
            installed(cx)[2],
            (
                "Window".into(),
                vec!["notes.show_all".into(), "notes.tidy".into()]
            )
        );
        let bar = menus::installed(cx).unwrap();
        let show_all = bar.menu("Window").unwrap().item("notes.show_all").unwrap();
        assert_eq!(show_all.command().unwrap().title, "Show All");
    });

    // An ID of its own, to merge into a menu it isn't named for, is an
    // unknown field: the file doesn't load.
    let renamed = r#"
[[menus]]
id = "Window"
name = "Other"

[[menus.items]]
title = "Show Everything"
command = "notes.show_all"
"#;
    setup.save("menus.toml", renamed, cx);
    let published = published(&menus_file, cx);
    assert!(
        published
            .iter()
            .any(|(severity, message)| *severity == Severity::Error && message == "unknown field"),
        "{published:?}"
    );
    cx.update(|cx| {
        let bar = menus::installed(cx).unwrap();
        let show_all = bar.menu("Window").unwrap().item("notes.show_all").unwrap();
        assert_eq!(show_all.command().unwrap().title, "Show All");
        assert!(bar.menu("Other").is_none());
    });
}

/// `$app` in a keyed menu title names the app when the app's menus are a
/// contributed table, which isn't a menu file: an extension's item titled
/// `kiln-core-menu-quit-app` over it.
#[gpui::test]
fn an_extension_item_over_a_menu_table_names_the_app(cx: &mut TestAppContext) {
    let menus = r#"
[[menus]]
name = "File"

[[menus.items]]
title_key = "kiln-core-menu-quit-app"
command = "notes.tidy"
"#;
    let state = tempfile::tempdir().unwrap();
    let (_setup, config) = Setup::new(state.path(), EXTENSION_KEYMAP, menus);
    cx.update(|cx| {
        kiln::Contributions::set_menu_table(cx, kiln::menus! { "File" => [notes::NewNote] });
    });
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();
    assert_eq!(diagnostics, []);
    cx.update(|cx| {
        let bar = menus::installed(cx).unwrap();
        let tidy = bar.menu("File").unwrap().item("notes.tidy").unwrap();
        let tidy = tidy.command().unwrap();
        assert_eq!(tidy.title, "Quit Notes");
        assert_eq!(tidy.label().resolve(), "Quit Notes");
    });
}
