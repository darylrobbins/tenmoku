// SPDX-License-Identifier: Apache-2.0

//! A running app's menu bar follows a locale change (`ActiveLocale`): the
//! installed menus are resolved again and installed in the new language,
//! and back, for items from a menu table and from an extension's menus.
//! The process catalogue is global: this binary's one test switches it
//! and switches back.

use std::fs;

use gpui::TestAppContext;
use kiln::{
    Config,
    core::{ApiVersion, extension::MANIFEST_FILE},
    platform::{config::batch_channel, menus},
};
use kiln_l10n::{LanguageIdentifier, Registered, ResourceFile, Resources};

mod notes {
    kiln::commands!(
        notes,
        [
            /// Tidy
            Tidy,
        ]
    );
}

const MANIFEST: &str = "\
id = \"acme.tidy\"
name = \"Tidy\"
version = \"1.0.0\"
license = \"MIT\"

[engine]
notes = \"1.0\"

[contributes]
menus = [\"menus.toml\"]
";

const EXTENSION_MENUS: &str = r#"
[[menus]]
name = "File"

[[menus.items]]
title_key = "locale-change-tidy"
command = "notes.tidy"
"#;

/// A fake locale, `qps-ploc`, for this binary: kiln's Close Window, which
/// the menu table has, and the extension's Tidy get labels.
static FAKE_LOCALE: Resources = Resources {
    crate_name: "kiln-locale-change-fake-locale",
    manifest_dir: env!("CARGO_MANIFEST_DIR"),
    declared_in: file!(),
    files: &[
        ResourceFile {
            locale: "en-US",
            path: "en-US/locale_change.ftl",
            source: "locale-change-tidy = Tidy\n",
        },
        ResourceFile {
            locale: "qps-ploc",
            path: "qps-ploc/locale_change.ftl",
            source: "command-window-close = ⟦Clösé Wïndöw⟧\n\
                     locale-change-tidy = ⟦Tïdy⟧\n",
        },
    ],
    dynamic_prefixes: &[],
};

inventory::submit! { Registered(&FAKE_LOCALE) }

fn set(tag: &str, cx: &mut TestAppContext) {
    let tag: LanguageIdentifier = tag.parse().unwrap();
    cx.update(|cx| kiln::platform::locale::set_locale(cx, &[tag]));
    cx.run_until_parked();
}

/// The installed File menu's command items, by the title shown in the
/// active locale.
fn file_menu(cx: &mut TestAppContext) -> Vec<String> {
    cx.read(|cx| {
        let bar = menus::installed(cx).expect("menus installed");
        let file = bar.menu("File").expect("a File menu");
        (file.items.iter())
            .filter_map(|item| {
                item.command()
                    .map(|command| command.label().resolve().to_string())
            })
            .collect()
    })
}

#[gpui::test]
fn l10n_locale_change_reinstalls_the_menu_bar(cx: &mut TestAppContext) {
    set("en-US", cx);
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    let extension = extensions.join("tidy");
    fs::create_dir_all(&extension).unwrap();
    fs::write(extension.join(MANIFEST_FILE), MANIFEST).unwrap();
    fs::write(extension.join("menus.toml"), EXTENSION_MENUS).unwrap();
    let (_sender, batches) = batch_channel();
    let config = Config::new("com.example.Notes", "Notes")
        .commands(notes::register)
        .fail_fast(true)
        .extensions(&extensions, "notes", ApiVersion { major: 1, minor: 0 })
        .watch_with(batches);
    cx.update(|cx| {
        let table = kiln::menus! { "File" => [kiln::window::Close] };
        kiln::Contributions::set_menu_table(cx, table);
        kiln::init(cx, config)
    });
    cx.run_until_parked();

    let english = ["Close Window", "Tidy"];
    assert_eq!(file_menu(cx), english);
    set("qps-ploc", cx);
    assert_eq!(file_menu(cx), ["⟦Clösé Wïndöw⟧", "⟦Tïdy⟧"]);
    set("en-US", cx);
    assert_eq!(file_menu(cx), english);
}
