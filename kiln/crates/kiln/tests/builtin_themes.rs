// SPDX-License-Identifier: Apache-2.0

//! The app's own themes through `kiln::init`, headless: installed as
//! `builtin/<id>`, listed beside the extensions' and chosen like them, and
//! kept when a rescan of the extensions directory replaces the
//! extensions' themes. Changes are sent as batches, as the watchers would
//! send them.

use std::{fs, path::Path};

use gpui::{App, TestAppContext};
use kiln::{
    Config,
    core::{
        ApiVersion, Diagnostic, Severity,
        extension::MANIFEST_FILE,
        theme::{Appearance, ThemeData, ThemeFile},
    },
    extensions::{
        ThemeInfo, builtin_theme_id, install_builtin_themes, select_themes, select_themes_noting,
        themes,
    },
    platform::config::{self, batch_channel},
    ui::GlobalTheme,
};

const MANIFEST: &str = "\
id = \"acme.solarized\"
name = \"Solarized\"
version = \"1.0.0\"
license = \"MIT\"

[engine]
notes = \"1.0\"

[contributes]
themes = [\"themes/solarized-dark.toml\"]
";

const SOLARIZED: &str = "\
name = \"Solarized Dark\"
appearance = \"dark\"

[surface]
background = \"#002b36\"
";

const DUSK: &str = "\
name = \"Notes Dusk\"
appearance = \"dark\"

[surface]
background = \"#202040\"
";

const DAWN: &str = "\
appearance = \"light\"

[surface]
background = \"#fff4e0\"
";

fn parse(name: &str, source: &str) -> ThemeFile {
    let (file, diagnostics) = ThemeFile::parse(Path::new(name), source, &[]);
    assert_eq!(diagnostics, []);
    file.unwrap()
}

fn background(data: &ThemeData) -> String {
    data.tokens.get("surface.background").unwrap().to_string()
}

/// The dark and light pair's surface backgrounds now.
fn backgrounds(cx: &App) -> (String, String) {
    let (dark, light) = cx.global::<GlobalTheme>().themes().expect("a pair");
    (background(dark.data()), background(light.data()))
}

#[gpui::test]
fn installed_builtin_themes_survive_a_rescan(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    let solarized = extensions.join("solarized");
    fs::create_dir_all(solarized.join("themes")).unwrap();
    fs::write(solarized.join(MANIFEST_FILE), MANIFEST).unwrap();
    fs::write(solarized.join("themes/solarized-dark.toml"), SOLARIZED).unwrap();
    let (sender, batches) = batch_channel();
    let config = Config::new("com.example.Notes", "Notes")
        .fail_fast(true)
        .extensions(&extensions, "notes", ApiVersion { major: 1, minor: 0 })
        .watch_with(batches);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    assert_eq!(diagnostics, []);
    cx.run_until_parked();

    let dusk = parse("notes-dusk.toml", DUSK);
    let dawn = parse("notes-dawn.toml", DAWN);
    cx.update(|cx| install_builtin_themes(cx, &[("notes-dusk", &dusk), ("notes-dawn", &dawn)]));
    assert_eq!(builtin_theme_id("notes-dusk"), "builtin/notes-dusk");
    let expected = [
        ThemeInfo {
            id: "acme.solarized/solarized-dark".into(),
            title: "Solarized Dark".into(),
            appearance: Appearance::Dark,
        },
        ThemeInfo {
            id: "builtin/notes-dawn".into(),
            title: "notes-dawn".into(),
            appearance: Appearance::Light,
        },
        ThemeInfo {
            id: "builtin/notes-dusk".into(),
            title: "Notes Dusk".into(),
            appearance: Appearance::Dark,
        },
    ];
    assert_eq!(cx.update(|cx| themes(cx)), expected);
    // Installed, not chosen: the pair is kiln's.
    let kiln = (
        background(ThemeData::dark()),
        background(ThemeData::light()),
    );
    assert_eq!(cx.update(|cx| backgrounds(cx)), kiln);

    cx.update(|cx| select_themes(cx, Some("builtin/notes-dusk"), Some("builtin/notes-dawn")));
    assert_eq!(
        cx.update(|cx| backgrounds(cx)),
        ("#202040".to_owned(), "#fff4e0".to_owned())
    );
    cx.update(|cx| assert_eq!(config::diagnostics(cx), []));

    // A rescan that changes the extensions' themes keeps the app's.
    fs::write(
        solarized.join("themes/solarized-dark.toml"),
        SOLARIZED.replace("#002b36", "#003040"),
    )
    .unwrap();
    sender.send(vec![extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| themes(cx)), expected);
    assert_eq!(
        cx.update(|cx| backgrounds(cx)),
        ("#202040".to_owned(), "#fff4e0".to_owned())
    );
    // So does one that removes the extension.
    fs::remove_dir_all(&solarized).unwrap();
    sender.send(vec![extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| themes(cx)), expected[1..]);
    assert_eq!(
        cx.update(|cx| backgrounds(cx)),
        ("#202040".to_owned(), "#fff4e0".to_owned())
    );

    // The app's notes on its choice are published with the choice's own,
    // and stay through a batch, until the next choice.
    let note = Diagnostic {
        severity: Severity::Warning,
        file: state.path().join("settings.toml"),
        key: Some("theme.name".into()),
        line: 2,
        column: 1,
        message: "theme Dusc isn't bundled".into(),
    };
    cx.update(|cx| select_themes_noting(cx, None, Some("builtin/notes-dusk"), vec![note.clone()]));
    let found = cx.update(|cx| config::diagnostics(cx).to_vec());
    assert_eq!(found.len(), 2, "{found:#?}");
    assert_eq!(found[0], note);
    assert_eq!(
        found[1].message,
        "theme builtin/notes-dusk is dark, so it can't be the light theme"
    );
    assert_eq!(cx.update(|cx| backgrounds(cx)), kiln);
    sender.send(vec![extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| config::diagnostics(cx).to_vec()), found);
    cx.update(|cx| select_themes(cx, None, None));
    cx.update(|cx| assert_eq!(config::diagnostics(cx), []));
}
