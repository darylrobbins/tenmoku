//! The editor's colours from kiln's active theme. kiln picks, loads and
//! reloads the theme (the mode setting, the installed themes the
//! `theme.dark` and `theme.light` settings choose, the user's theme file
//! with fastedit's tables); this passes the choice on, and turns each
//! active theme into the editor's palette ([`fe_gpui::set_editor_palette`]).

use std::sync::Arc;

use fe_config::{EditorSettings, Settings, ThemeModeSetting, ThemeSettings, theme::EditorPalette};
use fe_gpui::EditorTheme;
use gpui::App;
use kiln::{
    GlobalSettings,
    ui::{ActiveTheme as _, GlobalTheme, ThemeMode},
};

/// kiln's theme mode for the `theme.mode` setting.
pub(crate) fn mode(setting: ThemeModeSetting) -> ThemeMode {
    match setting {
        ThemeModeSetting::Dark => ThemeMode::Dark,
        ThemeModeSetting::Light => ThemeMode::Light,
        ThemeModeSetting::System => ThemeMode::System,
    }
}

/// Chooses the installed themes the settings name
/// ([`kiln::extensions::select_themes`]), now and whenever a reload
/// changes them, and sets the editor's palette from kiln's active theme,
/// now and on every change. On a change, editors restored from a saved
/// layout from then on open in the new colours
/// ([`fe_gpui::set_restore_style`]). Call it after [`kiln::init`].
pub(crate) fn init(cx: &mut App) {
    if let Some(global) = cx.try_global::<GlobalSettings<Settings>>() {
        let theme = global.settings().theme.clone();
        select(&theme, cx);
    }
    cx.observe_global::<GlobalSettings<Settings>>(|cx| {
        let global = cx.global::<GlobalSettings<Settings>>();
        let (now, before) = (&global.settings().theme, &global.previous().theme);
        if (&now.dark, &now.light) != (&before.dark, &before.light) {
            let now = now.clone();
            select(&now, cx);
        }
    })
    .detach();
    apply(cx);
    cx.observe_global::<GlobalTheme>(|cx| {
        apply(cx);
        let settings = cx
            .try_global::<GlobalSettings<Settings>>()
            .map(|global| global.settings().editor.clone())
            .unwrap_or_default();
        let theme = editor_theme(&settings, cx);
        fe_gpui::set_restore_style(cx, theme, settings);
    })
    .detach();
}

/// Chooses the installed themes `theme.dark` and `theme.light` name.
fn select(theme: &ThemeSettings, cx: &mut App) {
    kiln::extensions::select_themes(cx, theme.dark.as_deref(), theme.light.as_deref());
}

/// Sets the editor's palette from the active theme, logging what's wrong
/// with its tables.
fn apply(cx: &mut App) {
    let theme = cx.theme();
    let (palette, warnings) = EditorPalette::for_theme(theme.data());
    for warning in warnings {
        log::warn!(target: "fastedit", "theme {}: {warning}", theme.name);
    }
    fe_gpui::set_editor_palette(cx, Arc::new(palette));
}

/// The editor's colours in use with the font and metrics of `settings`.
pub(crate) fn editor_theme(settings: &EditorSettings, cx: &App) -> EditorTheme {
    EditorTheme::from_palette(&fe_gpui::editor_palette(cx), settings, cx)
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use fe_config::theme::EXTENSIONS;
    use gpui::TestAppContext;
    use kiln::{
        Config,
        core::theme::{ThemeData, ThemeFile},
        platform::config::batch_channel,
        prelude::AppExt as _,
        ui::{Theme, set_theme, set_themes},
    };

    use super::*;

    #[gpui::test]
    fn palette_follows_kiln(cx: &mut TestAppContext) {
        assert_eq!(mode(ThemeModeSetting::Dark), ThemeMode::Dark);
        assert_eq!(mode(ThemeModeSetting::Light), ThemeMode::Light);
        assert_eq!(mode(ThemeModeSetting::System), ThemeMode::System);
        cx.update(|cx| {
            init(cx);
            set_themes(cx, ThemeMode::Light, Theme::dark(), Theme::light());
        });
        let palette = cx.update(|cx| fe_gpui::editor_palette(cx));
        assert_eq!(*palette, *EditorPalette::fastedit_light());

        cx.update(|cx| cx.set_theme_mode(ThemeMode::Dark));
        let palette = cx.update(|cx| fe_gpui::editor_palette(cx));
        assert_eq!(*palette, *EditorPalette::fastedit_dark());

        let source = "appearance = \"dark\"\n\n[editor]\nbackground = \"#000000\"\n";
        let (file, diagnostics) = ThemeFile::parse(Path::new("theme.toml"), source, EXTENSIONS);
        assert_eq!(diagnostics, []);
        let data = ThemeData::from_file(&file.expect("a theme"));
        cx.update(|cx| set_theme(cx, Theme::new(data)));
        let palette = cx.update(|cx| fe_gpui::editor_palette(cx));
        let dark = EditorPalette::fastedit_dark();
        assert_eq!(palette.editor.background.to_rgba_u32(), 0x000000ff);
        assert_ne!(palette.editor.background, dark.editor.background);
        let mut expected = dark.clone();
        expected.editor.background = palette.editor.background;
        assert_eq!(*palette, expected);
    }

    #[gpui::test]
    fn theme_names_follow_settings(cx: &mut TestAppContext) {
        let state = tempfile::tempdir().unwrap();
        let extensions = state.path().join("extensions");
        let themes = extensions.join("solarized/themes");
        fs::create_dir_all(&themes).unwrap();
        fs::write(
            extensions.join("solarized/extension.toml"),
            "id = \"acme.solarized\"\nname = \"Solarized\"\nversion = \"1.0.0\"\n\
             license = \"MIT\"\n\n[engine]\nfastedit = \"1.0\"\n\n\
             [contributes]\nthemes = [\"themes/solarized-dark.toml\"]\n",
        )
        .unwrap();
        fs::write(
            themes.join("solarized-dark.toml"),
            "appearance = \"dark\"\n\n[surface]\nbackground = \"#002b36\"\n\n\
             [editor]\nbackground = \"#002b36\"\n",
        )
        .unwrap();
        let user = state.path().join("settings.toml");
        fs::write(&user, "").unwrap();
        let store = fe_config::SettingsStore::load(Some(user.clone()), None);
        let (sender, batches) = batch_channel();
        let config = Config::new("dev.fastedit.test", "fastedit")
            .settings_store(store.into_inner())
            .theme_mode(ThemeMode::Dark)
            .theme_extensions(EXTENSIONS)
            .watch_with(batches);
        let config = crate::config::with_extensions(config, extensions);
        let diagnostics = cx.update(|cx| kiln::init(cx, config));
        assert_eq!(diagnostics, []);
        cx.update(init);
        cx.run_until_parked();
        let background = |cx: &mut TestAppContext| {
            cx.update(|cx| {
                let theme = cx.theme();
                let palette = fe_gpui::editor_palette(cx);
                (
                    theme
                        .data()
                        .tokens
                        .get("surface.background")
                        .unwrap()
                        .to_string(),
                    palette.editor.background.to_rgba_u32(),
                )
            })
        };
        let kiln_dark = ThemeData::dark()
            .tokens
            .get("surface.background")
            .unwrap()
            .to_string();
        let fastedit_dark = EditorPalette::fastedit_dark()
            .editor
            .background
            .to_rgba_u32();
        assert_eq!(background(cx), (kiln_dark.clone(), fastedit_dark));

        fs::write(&user, "[theme]\ndark = \"acme.solarized/solarized-dark\"\n").unwrap();
        sender.send(vec![user.clone()]);
        cx.run_until_parked();
        assert_eq!(background(cx), ("#002b36".to_owned(), 0x002b36ff));

        fs::write(&user, "[theme]\nmode = \"dark\"\n").unwrap();
        sender.send(vec![user.clone()]);
        cx.run_until_parked();
        assert_eq!(background(cx), (kiln_dark, fastedit_dark));
    }
}
