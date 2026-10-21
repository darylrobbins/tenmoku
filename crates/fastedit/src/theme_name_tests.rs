//! `theme.name`: a bundled theme by title, chosen for the slot of its
//! appearance at start and again on each settings reload, a warning for a
//! name nothing bundled has, and `theme.dark` winning its slot.

use std::{fs, path::Path};

use fe_config::{Severity, theme::EXTENSIONS, themes::find};
use gpui::TestAppContext;
use kiln::{
    Config,
    core::theme::ThemeData,
    platform::config::{BatchSender, batch_channel},
    prelude::AppExt as _,
};

use super::*;

/// The editor palette the bundled theme `title` gives its slot.
fn palette_of(title: &str) -> EditorPalette {
    let data = ThemeData::from_file(&find(title).expect("bundled").file());
    let (palette, warnings) = EditorPalette::for_theme(&data);
    assert_eq!(warnings, [] as [String; 0]);
    palette
}

fn editor_palette(cx: &mut TestAppContext) -> EditorPalette {
    cx.update(|cx| (*fe_gpui::editor_palette(cx)).clone())
}

/// The warnings on `theme.name`: their line and message.
fn name_warnings(cx: &mut TestAppContext) -> Vec<(usize, String)> {
    cx.update(|cx| {
        (kiln::platform::config::diagnostics(cx).iter())
            .filter(|d| d.key.as_deref() == Some("theme.name"))
            .inspect(|d| assert_eq!(d.severity, Severity::Warning))
            .map(|d| (d.line, d.message.clone()))
            .collect()
    })
}

/// Rewrites the user's settings file, and has kiln's service reload it.
fn write(user: &Path, sender: &BatchSender, text: &str, cx: &mut TestAppContext) {
    fs::write(user, text).unwrap();
    sender.send(vec![user.to_owned()]);
    cx.run_until_parked();
}

#[gpui::test]
fn name_selects_a_bundled_theme_live(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let user = state.path().join("settings.toml");
    fs::write(&user, "[theme]\nname = \"Dark Modern\"\n").unwrap();
    let store = fe_config::SettingsStore::load(Some(user.clone()), None);
    let (sender, batches) = batch_channel();
    let config = Config::new("dev.fastedit.test", "fastedit")
        .settings_store(store.into_inner())
        .theme_mode(ThemeMode::Dark)
        .theme_extensions(EXTENSIONS)
        .watch_with(batches);
    let diagnostics = cx.update(|cx| kiln::init(cx, config));
    assert_eq!(diagnostics, []);
    cx.update(init);
    cx.run_until_parked();

    let dark_modern = palette_of("Dark Modern");
    assert_ne!(dark_modern, *EditorPalette::fastedit_dark());
    assert_eq!(editor_palette(cx), dark_modern);
    assert_eq!(
        cx.update(|cx| cx.theme().name.clone()),
        "VS Code Dark Modern"
    );
    assert_eq!(name_warnings(cx), []);
    cx.update(|cx| assert_eq!(kiln::platform::config::diagnostics(cx), []));

    // A reload re-selects, with no restart.
    write(&user, &sender, "[theme]\nname = \"Fastedit Ember\"\n", cx);
    let ember = palette_of("Fastedit Ember");
    assert_ne!(ember, dark_modern);
    assert_eq!(editor_palette(cx), ember);

    // A name nothing bundled has leaves the built-in, with a warning at it,
    // which a reload of something else keeps.
    let text = "[theme]\nmode = \"dark\"\nname = \"Dark Moderrn\"\n";
    write(&user, &sender, text, cx);
    assert_eq!(editor_palette(cx), *EditorPalette::fastedit_dark());
    let warnings = name_warnings(cx);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(warnings[0].0, 3);
    assert!(
        (warnings[0].1).starts_with("theme Dark Moderrn isn't bundled"),
        "{warnings:?}"
    );
    write(
        &user,
        &sender,
        &format!("{text}\n[editor]\ntab_size = 2\n"),
        cx,
    );
    assert_eq!(name_warnings(cx), warnings);

    // theme.dark, set too, wins the dark slot, with a warning on the name.
    let text = "[theme]\ndark = \"builtin/fastedit-ember\"\nname = \"Dark Modern\"\n";
    write(&user, &sender, text, cx);
    assert_eq!(editor_palette(cx), ember);
    let warnings = name_warnings(cx);
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert_eq!(
        warnings[0],
        (
            3,
            "theme.name Dark Modern is ignored: theme.dark chooses the dark theme".to_owned()
        )
    );

    // A light theme's name leaves the dark slot to theme.dark.
    let text = "[theme]\ndark = \"builtin/fastedit-ember\"\nname = \"light\"\n";
    write(&user, &sender, text, cx);
    assert_eq!(editor_palette(cx), ember);
    assert_eq!(name_warnings(cx), []);
    cx.update(|cx| cx.set_theme_mode(ThemeMode::Light));
    assert_eq!(editor_palette(cx), palette_of("Fastedit Light"));
    cx.update(|cx| cx.set_theme_mode(ThemeMode::Dark));

    // No name, no choice: the built-in again.
    write(&user, &sender, "", cx);
    assert_eq!(editor_palette(cx), *EditorPalette::fastedit_dark());
    cx.update(|cx| assert_eq!(kiln::platform::config::diagnostics(cx), []));
}
