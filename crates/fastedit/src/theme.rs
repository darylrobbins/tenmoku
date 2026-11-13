//! The editor's colours from kiln's active theme. kiln picks, loads and
//! reloads the theme (the mode setting, the installed themes the
//! `theme.dark` and `theme.light` settings choose, the user's theme file
//! with fastedit's tables); this installs fastedit's bundled themes
//! ([`fe_config::themes`]) for `theme.name` to choose by title, passes the
//! choice on, and turns each active theme into the editor's palette
//! ([`fe_gpui::set_editor_palette`]).

use kiln_l10n::tr;

// The catalogue, for every test build that includes this file: see cli.rs.
#[cfg(test)]
#[path = "l10n.rs"]
mod l10n;

use std::sync::Arc;

use fe_config::{
    Diagnostic, EditorSettings, Settings, Severity, ThemeModeSetting, ThemeSettings,
    theme::EditorPalette, themes,
};
use fe_gpui::EditorTheme;
use gpui::App;
use kiln::{
    GlobalSettings,
    core::theme::{Appearance, ThemeFile},
    ui::{ActiveTheme as _, GlobalTheme, ThemeMode},
};
use toml::de::{DeTable, DeValue};

/// kiln's theme mode for the `theme.mode` setting.
pub(crate) fn mode(setting: ThemeModeSetting) -> ThemeMode {
    match setting {
        ThemeModeSetting::Dark => ThemeMode::Dark,
        ThemeModeSetting::Light => ThemeMode::Light,
        ThemeModeSetting::System => ThemeMode::System,
    }
}

/// Installs the bundled themes, as `builtin/<id>`
/// ([`kiln::extensions::install_builtin_themes`]), chooses the themes the
/// settings name ([`kiln::extensions::select_themes`]), now and whenever a
/// reload changes them, and sets the editor's palette from kiln's active
/// theme, now and on every change. On a change, editors restored from a
/// saved layout from then on open in the new colours
/// ([`fe_gpui::set_restore_style`]). Call it after [`kiln::init`].
pub(crate) fn init(cx: &mut App) {
    let files: Vec<(&str, ThemeFile)> = (themes::bundled_themes().iter())
        .map(|theme| (theme.id, theme.file()))
        .collect();
    let files: Vec<(&str, &ThemeFile)> = files.iter().map(|(id, file)| (*id, file)).collect();
    kiln::extensions::install_builtin_themes(cx, &files);
    if let Some(global) = cx.try_global::<GlobalSettings<Settings>>() {
        let theme = global.settings().theme.clone();
        select(&theme, cx);
    }
    cx.observe_global::<GlobalSettings<Settings>>(|cx| {
        let global = cx.global::<GlobalSettings<Settings>>();
        let (now, before) = (&global.settings().theme, &global.previous().theme);
        if (&now.name, &now.dark, &now.light) != (&before.name, &before.dark, &before.light) {
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

/// Chooses the installed themes `theme.dark` and `theme.light` name, and
/// the bundled one `theme.name` does for the slot of its appearance,
/// publishing what's wrong with `theme.name`.
fn select(theme: &ThemeSettings, cx: &mut App) {
    let source = kiln::platform::config::accepted_sources::<Settings>(cx);
    let (dark, light, notes) = choice(theme, source.first().map(String::as_str));
    kiln::extensions::select_themes_noting(cx, dark.as_deref(), light.as_deref(), notes);
}

/// The installed themes the dark and light slots start from, by ID: each
/// slot's `theme.dark` or `theme.light`, else the bundled theme
/// `theme.name` names if it's of the slot's appearance, else Tenmoku's
/// default for the slot, Yohen or Hakuji ([`themes::DEFAULT_DARK`],
/// [`themes::DEFAULT_LIGHT`]). Then what's wrong with `theme.name`, at it
/// in `source`, the user's settings file: a name no bundled theme has, or
/// one that a slot's own setting overrides.
fn choice(
    theme: &ThemeSettings,
    source: Option<&str>,
) -> (Option<String>, Option<String>, Vec<Diagnostic>) {
    let (dark, light, notes) = named(theme, source);
    let default = |id| Some(kiln::extensions::builtin_theme_id(id));
    (
        dark.or_else(|| default(themes::DEFAULT_DARK)),
        light.or_else(|| default(themes::DEFAULT_LIGHT)),
        notes,
    )
}

/// [`choice`] before the defaults: the slots the settings choose, if any.
fn named(
    theme: &ThemeSettings,
    source: Option<&str>,
) -> (Option<String>, Option<String>, Vec<Diagnostic>) {
    let (mut dark, mut light) = (theme.dark.clone(), theme.light.clone());
    let mut notes = Vec::new();
    let Some(name) = theme.name.as_deref() else {
        return (dark, light, notes);
    };
    let message = match themes::find(name) {
        None => {
            let titles: Vec<&str> = (themes::bundled_themes().iter())
                .map(|theme| theme.title)
                .collect();
            tr!(
                "fastedit-theme-not-bundled",
                name = name,
                themes = titles.join(", ")
            )
            .to_string()
        }
        Some(found) => {
            let (slot, appearance) = match found.appearance {
                Appearance::Dark => (&mut dark, "dark"),
                Appearance::Light => (&mut light, "light"),
            };
            if slot.is_none() {
                *slot = Some(kiln::extensions::builtin_theme_id(found.id));
                return (dark, light, notes);
            }
            tr!(
                "fastedit-theme-name-ignored",
                title = found.title,
                appearance = appearance
            )
            .to_string()
        }
    };
    let (line, column) = source.map_or((1, 1), name_position);
    notes.push(Diagnostic {
        severity: Severity::Warning,
        file: fe_config::user_settings_path().unwrap_or_default(),
        key: Some("theme.name".to_owned()),
        line,
        column,
        message,
    });
    (dark, light, notes)
}

/// The 1-based line and column of `theme.name` in `source`, else 1, 1.
fn name_position(source: &str) -> (usize, usize) {
    let offset = DeTable::parse(source).ok().and_then(|document| {
        let (_, theme) = (document.get_ref().iter()).find(|(key, _)| key.get_ref() == "theme")?;
        let DeValue::Table(theme) = theme.get_ref() else {
            return None;
        };
        let (key, _) = theme.iter().find(|(key, _)| key.get_ref() == "name")?;
        Some(key.span().start)
    });
    let Some(offset) = offset else {
        return (1, 1);
    };
    let before = &source[..offset.min(source.len())];
    let line_start = before.rfind('\n').map_or(0, |at| at + 1);
    (
        before.matches('\n').count() + 1,
        before[line_start..].chars().count() + 1,
    )
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
#[path = "theme_name_tests.rs"]
mod theme_name_tests;

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

    /// The bundled theme `id`'s data and editor palette.
    fn bundled(id: &str) -> (ThemeData, EditorPalette) {
        let data = ThemeData::from_file(&themes::find(id).expect("bundled").file());
        let (palette, warnings) = EditorPalette::for_theme(&data);
        assert_eq!(warnings, [] as [String; 0], "{id}");
        (data, palette)
    }

    #[gpui::test]
    fn unset_slots_are_yohen_and_hakuji(cx: &mut TestAppContext) {
        let (yohen, yohen_palette) = bundled("yohen");
        let (hakuji, hakuji_palette) = bundled("hakuji");
        // The bundled palettes are theirs, so a theme leaving keys out
        // takes Yohen's or Hakuji's.
        assert_eq!(*EditorPalette::fastedit_dark(), yohen_palette);
        assert_eq!(*EditorPalette::fastedit_light(), hakuji_palette);
        assert_ne!(yohen.tokens, ThemeData::dark().tokens);
        assert_ne!(hakuji.tokens, ThemeData::light().tokens);

        // No theme settings: Yohen's slot and Hakuji's, the name aside.
        let default = |id| Some(kiln::extensions::builtin_theme_id(id));
        let (dark, light, notes) = choice(&ThemeSettings::default(), None);
        assert_eq!((dark, light), (default("yohen"), default("hakuji")));
        assert_eq!(notes, []);
        let named = ThemeSettings {
            name: Some("Dark Modern".to_owned()),
            ..ThemeSettings::default()
        };
        let (dark, light, notes) = choice(&named, None);
        let dark_modern = default("vscode-dark-modern");
        assert_eq!((dark, light), (dark_modern, default("hakuji")));
        assert_eq!(notes, []);

        let state = tempfile::tempdir().unwrap();
        let user = state.path().join("settings.toml");
        fs::write(&user, "").unwrap();
        let store = fe_config::SettingsStore::load(Some(user), None);
        // The test's own batches, not a watcher thread, which the test
        // scheduler refuses to hear from.
        let (_sender, batches) = batch_channel();
        let config = Config::new("dev.fastedit.test", "fastedit")
            .settings_store(store.into_inner())
            .theme_mode(ThemeMode::Dark)
            .theme_extensions(EXTENSIONS)
            .watch_with(batches);
        let diagnostics = cx.update(|cx| kiln::init(cx, config));
        assert_eq!(diagnostics, []);
        cx.update(init);
        cx.run_until_parked();
        let showing = |cx: &mut TestAppContext| {
            cx.update(|cx| {
                let theme = cx.theme();
                let palette = (*fe_gpui::editor_palette(cx)).clone();
                (theme.name.to_string(), theme.data().tokens, palette)
            })
        };
        let (name, tokens, palette) = showing(cx);
        assert_eq!(name, "Yohen");
        assert_eq!(tokens, yohen.tokens);
        assert_eq!(palette, yohen_palette);

        cx.update(|cx| cx.set_theme_mode(ThemeMode::Light));
        let (name, tokens, palette) = showing(cx);
        assert_eq!(name, "Hakuji");
        assert_eq!(tokens, hakuji.tokens);
        assert_eq!(palette, hakuji_palette);
        cx.update(|cx| assert_eq!(kiln::platform::config::diagnostics(cx), []));
    }

    #[gpui::test]
    fn palette_follows_kiln(cx: &mut TestAppContext) {
        assert_eq!(mode(ThemeModeSetting::Dark), ThemeMode::Dark);
        assert_eq!(mode(ThemeModeSetting::Light), ThemeMode::Light);
        assert_eq!(mode(ThemeModeSetting::System), ThemeMode::System);
        // Kiln's own bases, with no editor tables: the bundled palettes,
        // Yohen's and Hakuji's.
        let (_, yohen) = bundled("yohen");
        let (_, hakuji) = bundled("hakuji");
        cx.update(|cx| {
            init(cx);
            set_themes(cx, ThemeMode::Light, Theme::dark(), Theme::light());
        });
        let palette = cx.update(|cx| fe_gpui::editor_palette(cx));
        assert_eq!(*palette, hakuji);

        cx.update(|cx| cx.set_theme_mode(ThemeMode::Dark));
        let palette = cx.update(|cx| fe_gpui::editor_palette(cx));
        assert_eq!(*palette, yohen);

        // A bundled theme of its own colours keeps them: Tenmoku Dark.
        let (tenmoku_dark, tenmoku_palette) = bundled("tenmoku-dark");
        assert_ne!(tenmoku_palette, yohen);
        cx.update(|cx| set_theme(cx, Theme::new(tenmoku_dark)));
        let palette = cx.update(|cx| fe_gpui::editor_palette(cx));
        assert_eq!(*palette, tenmoku_palette);

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
             license = \"MIT\"\n\n[engine]\ntenmoku = \"1.0\"\n\n\
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
        let (yohen, yohen_palette) = bundled("yohen");
        let yohen = yohen.tokens.get("surface.background").unwrap().to_string();
        let yohen_palette = yohen_palette.editor.background.to_rgba_u32();
        assert_eq!(background(cx), (yohen.clone(), yohen_palette));

        fs::write(&user, "[theme]\ndark = \"acme.solarized/solarized-dark\"\n").unwrap();
        sender.send(vec![user.clone()]);
        cx.run_until_parked();
        assert_eq!(background(cx), ("#002b36".to_owned(), 0x002b36ff));

        fs::write(&user, "[theme]\nmode = \"dark\"\n").unwrap();
        sender.send(vec![user.clone()]);
        cx.run_until_parked();
        assert_eq!(background(cx), (yohen, yohen_palette));
    }
}
