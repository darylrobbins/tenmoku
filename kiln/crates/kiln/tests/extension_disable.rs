// SPDX-License-Identifier: Apache-2.0

//! `kiln::extensions::set_disabled`, headless: a disabled extension stays
//! loaded but contributes nothing (extensions record §10), and enabled
//! again it contributes all it did, with no restart.

use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
};

use gpui::{App, TestAppContext};
use kiln::{
    Config,
    command::ContextStack,
    core::{ApiVersion, ExtensionSet, Keystroke, Severity, extension::MANIFEST_FILE},
    extensions::{self, select_themes, set_disabled},
    platform::{
        config::{self, BatchSender, ConfigSource, batch_channel},
        keys, menus,
    },
    ui::GlobalTheme,
};
use serde_json::json;

mod notes {
    kiln::commands!(
        notes,
        [
            /// New Note
            NewNote,
            /// Tidy
            Tidy,
        ]
    );
}

const MENUS: &str = r#"
[[menus]]
name = "File"

[[menus.items]]
title = "New Note"
command = "notes.new_note"
"#;

const SOLARIZED: &str = "\
id = \"acme.solarized\"
name = \"Solarized\"
version = \"1.0.0\"
license = \"MIT\"

[engine]
notes = \"1.0\"

[contributes]
themes = [\"solarized-dark.toml\"]
keymaps = [\"keymap.toml\"]
menus = [\"menus.toml\"]

[contributes.lsp]
command = \"solarized-ls\"
";

const OTHER: &str = "\
id = \"acme.other\"
name = \"Other\"
version = \"1.0.0\"
license = \"MIT\"

[engine]
notes = \"1.0\"

[contributes.lsp]
command = \"other-ls\"
";

const DARK: &str = "\
name = \"Solarized Dark\"
appearance = \"dark\"

[surface]
background = \"#002b36\"
";

const KEYMAP: &str = r#"
[[bindings]]
keys = "secondary-alt-t"
command = "notes.tidy"
"#;

const EXTENSION_MENUS: &str = r#"
[[menus]]
name = "Acme"

[[menus.items]]
title = "Tidy"
command = "notes.tidy"
"#;

fn install(root: &Path, dir: &str, files: &[(&str, &str)]) {
    let dir = root.join(dir);
    fs::create_dir_all(&dir).unwrap();
    for (name, text) in files {
        fs::write(dir.join(name), text).unwrap();
    }
}

/// What a disabled extension takes away, as seen now: its theme listed,
/// the dark slot laid from it, its key bound, its menu installed.
#[derive(Debug, PartialEq)]
struct Contributed {
    theme: bool,
    dark_from_it: bool,
    key: bool,
    menu: bool,
}

fn contributed(cx: &App) -> Contributed {
    let (dark, _) = cx.global::<GlobalTheme>().themes().expect("a pair");
    let background = dark.data().tokens.get("surface.background").unwrap();
    let keystroke = Keystroke::parse("secondary-alt-t").unwrap();
    let bar = menus::installed(cx).expect("menus are installed");
    Contributed {
        theme: (extensions::themes(cx).iter())
            .any(|theme| theme.id == "acme.solarized/solarized-dark"),
        dark_from_it: background.to_string() == "#002b36",
        key: keys::lookup(&keystroke, &ContextStack::new(), cx).is_some(),
        menu: bar.menu("Acme").is_some(),
    }
}

const ALL: Contributed = Contributed {
    theme: true,
    dark_from_it: true,
    key: true,
    menu: true,
};

const NONE: Contributed = Contributed {
    theme: false,
    dark_from_it: false,
    key: false,
    menu: false,
};

fn warnings(cx: &App) -> Vec<(Severity, String)> {
    (config::diagnostics(cx).iter())
        .map(|diagnostic| (diagnostic.severity, diagnostic.message.clone()))
        .collect()
}

/// The IDs each call to on_extensions had.
type Seen = Rc<RefCell<Vec<Vec<String>>>>;

/// Solarized (a theme, keys and a menu) and another extension installed
/// under `state`, and the app's config over them, recording what
/// on_extensions has.
fn app(state: &Path) -> (Config, BatchSender, Seen, PathBuf) {
    let root = state.join("extensions");
    install(
        &root,
        "solarized",
        &[
            (MANIFEST_FILE, SOLARIZED),
            ("solarized-dark.toml", DARK),
            ("keymap.toml", KEYMAP),
            ("menus.toml", EXTENSION_MENUS),
        ],
    );
    install(&root, "other", &[(MANIFEST_FILE, OTHER)]);
    let seen: Seen = Rc::default();
    let (sender, batches) = batch_channel();
    let config = Config::new("com.example.Notes", "Notes")
        .commands(notes::register)
        .menus("menus.toml", MENUS)
        .fail_fast(true)
        .extensions(&root, "notes", ApiVersion { major: 1, minor: 0 })
        .extension_kind("lsp", json!({ "type": "object" }))
        .on_extensions({
            let seen = seen.clone();
            move |set: &ExtensionSet, _| {
                let ids = set.extensions().iter().map(|m| m.id.clone()).collect();
                seen.borrow_mut().push(ids);
            }
        })
        .watch_with(batches);
    (config, sender, seen, root)
}

/// A config source that disables Solarized as the service starts, as an
/// app following its settings does.
struct DisableOnInstall;

impl ConfigSource for DisableOnInstall {
    fn paths(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn install(&mut self, cx: &mut App) {
        set_disabled(cx, ["acme.solarized"]);
    }

    fn reload(&mut self, _: &[PathBuf], _: &mut App) {}
}

#[gpui::test]
fn an_extension_disabled_from_the_start_never_contributes(
    before: &mut TestAppContext,
    installing: &mut TestAppContext,
) {
    for (cx, from_install) in [(before, false), (installing, true)] {
        let state = tempfile::tempdir().unwrap();
        let (config, sender, seen, root) = app(state.path());
        let config = if from_install {
            config.source(DisableOnInstall)
        } else {
            cx.update(|cx| set_disabled(cx, ["acme.solarized"]));
            config
        };
        assert_eq!(cx.update(|cx| kiln::init(cx, config)), []);
        cx.run_until_parked();
        cx.update(|cx| select_themes(cx, Some("acme.solarized/solarized-dark"), None));
        assert_eq!(cx.update(|cx| contributed(cx)), NONE, "{from_install}");
        assert_eq!(*seen.borrow(), [["acme.other"]], "{from_install}");

        // An unchanged rescan keeps it so.
        sender.send(vec![root.clone()]);
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| contributed(cx)), NONE, "{from_install}");

        cx.update(|cx| set_disabled(cx, Vec::<String>::new()));
        cx.run_until_parked();
        assert_eq!(cx.update(|cx| contributed(cx)), ALL, "{from_install}");
    }
}

#[gpui::test]
fn a_disabled_extension_contributes_nothing(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let (config, sender, seen, root) = app(state.path());
    assert_eq!(cx.update(|cx| kiln::init(cx, config)), []);
    cx.run_until_parked();
    cx.update(|cx| select_themes(cx, Some("acme.solarized/solarized-dark"), None));
    assert_eq!(cx.update(|cx| contributed(cx)), ALL);
    assert_eq!(*seen.borrow(), [["acme.other", "acme.solarized"]]);

    // Disabled, with an ID nothing has beside it: still loaded, but its
    // theme, keys, menu and app kinds are gone, and the selected theme
    // falls back with the usual warning.
    cx.update(|cx| set_disabled(cx, ["acme.solarized", "acme.uninstalled"]));
    cx.run_until_parked();
    let loaded: Vec<String> = (cx.update(|cx| extensions::loaded(cx)).into_iter())
        .map(|manifest| manifest.id)
        .collect();
    assert_eq!(loaded, ["acme.other", "acme.solarized"]);
    assert_eq!(cx.update(|cx| contributed(cx)), NONE);
    assert_eq!(seen.borrow().last().unwrap(), &["acme.other"]);
    let isnt_installed = (
        Severity::Warning,
        "theme acme.solarized/solarized-dark isn't installed".to_owned(),
    );
    assert_eq!(
        cx.update(|cx| warnings(cx)),
        std::slice::from_ref(&isnt_installed)
    );
    assert_eq!(
        cx.update(|cx| extensions::disabled(cx)),
        ["acme.solarized", "acme.uninstalled"]
    );

    // A rescan keeps it disabled, the warning kept.
    sender.send(vec![root.clone()]);
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| contributed(cx)), NONE);
    assert_eq!(cx.update(|cx| warnings(cx)), [isnt_installed]);

    // The same set again changes nothing.
    let calls = seen.borrow().len();
    cx.update(|cx| set_disabled(cx, ["acme.uninstalled", "acme.solarized"]));
    cx.run_until_parked();
    assert_eq!(seen.borrow().len(), calls);

    // Enabled again: all of it comes back, and the warning goes.
    cx.update(|cx| set_disabled(cx, Vec::<String>::new()));
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| contributed(cx)), ALL);
    assert_eq!(
        seen.borrow().last().unwrap(),
        &["acme.other", "acme.solarized"]
    );
    assert_eq!(cx.update(|cx| warnings(cx)), []);
}
