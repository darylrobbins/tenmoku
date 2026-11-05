// SPDX-License-Identifier: Apache-2.0

//! Extensions' themes through `kiln::init`, headless: listed by ID,
//! chosen for a slot of the dark and light pair with the user's theme file
//! laid over the choice, and re-applied when their files change. Changes
//! are sent as batches, as the watchers would send them.

use std::{fs, path::Path};

use gpui::{App, TestAppContext};
use kiln::{
    Config,
    core::{
        ApiVersion, Severity,
        extension::MANIFEST_FILE,
        theme::{Appearance, TOKEN_KEYS, ThemeData, Tokens},
    },
    extensions::{ThemeInfo, select_themes, themes},
    platform::config::{self, BatchSender, batch_channel},
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
themes = [\"themes/solarized-dark.toml\", \"themes/solarized-light.toml\"]
";

const DARK: &str = "\
name = \"Solarized Dark\"
appearance = \"dark\"

[surface]
background = \"#002b36\"

[accent]
base = \"#268bd2\"
";

const LIGHT: &str = "\
appearance = \"light\"

[surface]
background = \"#fdf6e3\"
";

/// An app with the extension installed under `state`, its user files in
/// `state/config`, and the sender of its batches.
struct Setup {
    extensions: std::path::PathBuf,
    user_theme: std::path::PathBuf,
    sender: BatchSender,
}

fn setup(state: &Path, cx: &mut TestAppContext) -> Setup {
    let extensions = state.join("extensions");
    let themes = extensions.join("solarized/themes");
    fs::create_dir_all(&themes).unwrap();
    fs::write(extensions.join("solarized").join(MANIFEST_FILE), MANIFEST).unwrap();
    fs::write(themes.join("solarized-dark.toml"), DARK).unwrap();
    fs::write(themes.join("solarized-light.toml"), LIGHT).unwrap();
    let user_theme = state.join("config/theme.toml");
    let (sender, batches) = batch_channel();
    let config = Config::new("com.example.Notes", "Notes")
        .fail_fast(true)
        .extensions(&extensions, "notes", ApiVersion { major: 1, minor: 0 })
        .user_theme(&user_theme)
        .watch_with(batches);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    assert_eq!(diagnostics, []);
    cx.run_until_parked();
    Setup {
        extensions,
        user_theme,
        sender,
    }
}

/// The dark and light pair's tokens now.
fn pair(cx: &App) -> (ThemeData, ThemeData) {
    let (dark, light) = cx.global::<GlobalTheme>().themes().expect("a pair");
    (dark.data().clone(), light.data().clone())
}

fn color(tokens: &Tokens, key: &str) -> String {
    tokens.get(key).unwrap().to_string()
}

#[gpui::test]
fn extension_themes_are_listed_and_selected(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let _setup = setup(state.path(), cx);
    let listed = cx.update(|cx| themes(cx));
    assert_eq!(
        listed,
        [
            ThemeInfo {
                id: "acme.solarized/solarized-dark".into(),
                title: "Solarized Dark".into(),
                appearance: Appearance::Dark,
            },
            ThemeInfo {
                id: "acme.solarized/solarized-light".into(),
                title: "solarized-light".into(),
                appearance: Appearance::Light,
            },
        ]
    );
    // Installed, not chosen: the pair is kiln's.
    let (dark, light) = cx.update(|cx| pair(cx));
    assert_eq!(dark, *ThemeData::dark());
    assert_eq!(light, *ThemeData::light());

    cx.update(|cx| select_themes(cx, Some("acme.solarized/solarized-dark"), None));
    let (dark, light) = cx.update(|cx| pair(cx));
    assert_eq!(dark.name, "Solarized Dark");
    assert_eq!(dark.appearance, Appearance::Dark);
    assert_eq!(color(&dark.tokens, "surface.background"), "#002b36");
    assert_eq!(color(&dark.tokens, "accent.base"), "#268bd2");
    // Derived from the accent it set.
    assert_eq!(color(&dark.tokens, "border.focused"), "#268bd2");
    // What it leaves out is the bundled base's.
    let base = ThemeData::dark();
    assert_eq!(
        dark.tokens.get("text.primary"),
        base.tokens.get("text.primary")
    );
    assert_eq!(
        dark.tokens.get("status.error"),
        base.tokens.get("status.error")
    );
    assert_eq!(light, *ThemeData::light());
    cx.update(|cx| assert_eq!(config::diagnostics(cx), []));

    cx.update(|cx| select_themes(cx, None, Some("acme.solarized/solarized-light")));
    let (dark, light) = cx.update(|cx| pair(cx));
    assert_eq!(dark, *ThemeData::dark(), "the dark slot's choice is undone");
    assert_eq!(color(&light.tokens, "surface.background"), "#fdf6e3");
}

#[gpui::test]
fn the_user_theme_lays_over_the_selected_theme(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let setup = setup(state.path(), cx);
    cx.update(|cx| select_themes(cx, Some("acme.solarized/solarized-dark"), None));
    let (solarized, _) = cx.update(|cx| pair(cx));

    fs::write(
        &setup.user_theme,
        "appearance = \"dark\"\n\n[status]\nerror = \"#ff0000\"\n",
    )
    .unwrap();
    setup.sender.send(vec![setup.user_theme.clone()]);
    cx.run_until_parked();
    let (dark, light) = cx.update(|cx| pair(cx));
    assert_eq!(color(&dark.tokens, "status.error"), "#ff0000");
    for key in TOKEN_KEYS.iter().filter(|key| **key != "status.error") {
        assert_eq!(
            dark.tokens.get(key),
            solarized.tokens.get(key),
            "{key} is the selected theme's"
        );
    }
    assert_eq!(color(&dark.tokens, "surface.background"), "#002b36");
    assert_eq!(light, *ThemeData::light());

    // A new choice keeps the user's file over it.
    cx.update(|cx| select_themes(cx, None, None));
    let (dark, _) = cx.update(|cx| pair(cx));
    assert_eq!(color(&dark.tokens, "status.error"), "#ff0000");
    assert_eq!(
        dark.tokens.get("surface.background"),
        ThemeData::dark().tokens.get("surface.background")
    );

    // Without the file, the choice alone.
    cx.update(|cx| select_themes(cx, Some("acme.solarized/solarized-dark"), None));
    fs::remove_file(&setup.user_theme).unwrap();
    setup.sender.send(vec![setup.user_theme.clone()]);
    cx.run_until_parked();
    let (dark, _) = cx.update(|cx| pair(cx));
    assert_eq!(dark, solarized);
}

#[gpui::test]
fn a_missing_or_mismatched_selection_falls_back(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let setup = setup(state.path(), cx);
    let warnings = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            config::diagnostics(cx)
                .iter()
                .map(|diagnostic| (diagnostic.severity, diagnostic.message.clone()))
                .collect::<Vec<_>>()
        })
    };

    cx.update(|cx| select_themes(cx, Some("acme.solarized/solarized-darker"), None));
    let (dark, _) = cx.update(|cx| pair(cx));
    assert_eq!(dark, *ThemeData::dark());
    assert_eq!(
        warnings(cx),
        [(
            Severity::Warning,
            "theme acme.solarized/solarized-darker isn't installed".to_owned()
        )]
    );

    cx.update(|cx| select_themes(cx, Some("acme.solarized/solarized-light"), None));
    let (dark, light) = cx.update(|cx| pair(cx));
    assert_eq!(dark, *ThemeData::dark());
    assert_eq!(light, *ThemeData::light(), "nor is it the light slot's");
    assert_eq!(
        warnings(cx),
        [(
            Severity::Warning,
            "theme acme.solarized/solarized-light is light, so it can't be the dark theme"
                .to_owned()
        )]
    );

    cx.update(|cx| select_themes(cx, Some("acme.solarized/solarized-dark"), None));
    assert_eq!(warnings(cx), []);
    // The warning stays gone through a batch, which publishes every
    // source's diagnostics.
    setup.sender.send(vec![setup.extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(warnings(cx), []);

    // Editing the selected theme's file re-applies it.
    let file = setup
        .extensions
        .join("solarized/themes/solarized-dark.toml");
    fs::write(&file, DARK.replace("#002b36", "#073642")).unwrap();
    setup.sender.send(vec![setup.extensions.clone()]);
    cx.run_until_parked();
    let (dark, _) = cx.update(|cx| pair(cx));
    assert_eq!(color(&dark.tokens, "surface.background"), "#073642");

    // A file with errors is the extension's error, and keeps its last good
    // version.
    fs::write(&file, "appearance = \"dark\"\n[surface\n").unwrap();
    setup.sender.send(vec![setup.extensions.clone()]);
    cx.run_until_parked();
    let found = warnings(cx);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, Severity::Error);
    let (dark, _) = cx.update(|cx| pair(cx));
    assert_eq!(color(&dark.tokens, "surface.background"), "#073642");

    // Uninstalled, the choice falls back, with a warning.
    fs::remove_dir_all(setup.extensions.join("solarized")).unwrap();
    setup.sender.send(vec![setup.extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| themes(cx)), []);
    let (dark, _) = cx.update(|cx| pair(cx));
    assert_eq!(dark, *ThemeData::dark());
    assert_eq!(
        warnings(cx),
        [(
            Severity::Warning,
            "theme acme.solarized/solarized-dark isn't installed".to_owned()
        )]
    );
}

/// The themes' IDs listed now.
fn listed(cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| themes(cx).into_iter().map(|theme| theme.id).collect())
}

#[gpui::test]
fn a_missing_theme_leaves_its_sibling_loaded(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let extensions = state.path().join("extensions");
    let themes_dir = extensions.join("solarized/themes");
    fs::create_dir_all(&themes_dir).unwrap();
    fs::write(extensions.join("solarized").join(MANIFEST_FILE), MANIFEST).unwrap();
    fs::write(themes_dir.join("solarized-dark.toml"), DARK).unwrap();
    let (sender, batches) = batch_channel();
    let config = Config::new("com.example.Notes", "Notes")
        .fail_fast(true)
        .extensions(&extensions, "notes", ApiVersion { major: 1, minor: 0 })
        .watch_with(batches);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    cx.run_until_parked();
    let light = themes_dir.join("solarized-light.toml");
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].file, light);
    assert!(diagnostics[0].is_error());
    assert_eq!(listed(cx), ["acme.solarized/solarized-dark"]);

    fs::write(&light, LIGHT).unwrap();
    sender.send(vec![extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(
        listed(cx),
        [
            "acme.solarized/solarized-dark",
            "acme.solarized/solarized-light"
        ]
    );
    cx.update(|cx| assert_eq!(config::diagnostics(cx), []));

    // Gone again, it drops out alone.
    fs::remove_file(&light).unwrap();
    sender.send(vec![extensions.clone()]);
    cx.run_until_parked();
    assert_eq!(listed(cx), ["acme.solarized/solarized-dark"]);
}

/// A theme file replaced by a link out of the extension's directory isn't
/// read: the manifest's last good version stays, and so does the theme's.
#[cfg(unix)]
#[gpui::test]
fn a_theme_leading_outside_is_not_read(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let setup = setup(state.path(), cx);
    cx.update(|cx| select_themes(cx, Some("acme.solarized/solarized-dark"), None));
    let outside = state.path().join("outside.toml");
    fs::write(&outside, DARK.replace("#002b36", "#ff00ff")).unwrap();
    let file = setup
        .extensions
        .join("solarized/themes/solarized-dark.toml");
    fs::remove_file(&file).unwrap();
    std::os::unix::fs::symlink(&outside, &file).unwrap();
    setup.sender.send(vec![setup.extensions.clone()]);
    cx.run_until_parked();
    let (dark, _) = cx.update(|cx| pair(cx));
    assert_eq!(color(&dark.tokens, "surface.background"), "#002b36");
    assert_eq!(listed(cx).len(), 2);
    let errors: Vec<String> = cx.update(|cx| {
        config::diagnostics(cx)
            .iter()
            .filter(|diagnostic| diagnostic.file == file)
            .map(|diagnostic| diagnostic.message.clone())
            .collect()
    });
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].contains("outside acme.solarized's directory"),
        "{errors:?}"
    );
}

/// A loaded theme swapped for a link to a file outside the extension's
/// directory is never read, even though the manifest is kept from its
/// last good version: the outside colours aren't applied, the file gets an
/// error, and the extension's other theme still loads.
#[cfg(unix)]
#[gpui::test]
fn a_theme_swapped_for_an_outside_symlink_is_not_read(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let setup = setup(state.path(), cx);
    cx.update(|cx| {
        select_themes(
            cx,
            Some("acme.solarized/solarized-dark"),
            Some("acme.solarized/solarized-light"),
        )
    });
    let outside = state.path().join("outside.toml");
    fs::write(&outside, DARK.replace("#002b36", "#ff00ff")).unwrap();
    let themes_dir = setup.extensions.join("solarized/themes");
    let file = themes_dir.join("solarized-dark.toml");
    fs::remove_file(&file).unwrap();
    std::os::unix::fs::symlink(&outside, &file).unwrap();
    setup.sender.send(vec![setup.extensions.clone()]);
    cx.run_until_parked();

    let (dark, light) = cx.update(|cx| pair(cx));
    assert_eq!(color(&dark.tokens, "surface.background"), "#002b36");
    assert_eq!(color(&light.tokens, "surface.background"), "#fdf6e3");
    assert_eq!(
        listed(cx),
        [
            "acme.solarized/solarized-dark",
            "acme.solarized/solarized-light"
        ]
    );
    let errors: Vec<String> = cx.update(|cx| {
        config::diagnostics(cx)
            .iter()
            .filter(|diagnostic| diagnostic.file == file)
            .inspect(|diagnostic| assert_eq!(diagnostic.severity, Severity::Error))
            .map(|diagnostic| diagnostic.message.clone())
            .collect()
    });
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("outside"), "{errors:?}");
}
