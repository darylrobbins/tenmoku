//! The editor's colours from kiln's active theme. kiln picks, loads and
//! reloads the theme (the mode setting, the user's theme file with
//! fastedit's tables); this turns each active theme into the editor's
//! palette ([`fe_gpui::set_editor_palette`]).

use std::sync::Arc;

use fe_config::{EditorSettings, Settings, ThemeModeSetting, theme::EditorPalette};
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

/// Sets the editor's palette from kiln's active theme, now and on every
/// change. On a change, editors restored from a saved layout from then on
/// open in the new colours ([`fe_gpui::set_restore_style`]). Call it after
/// [`kiln::init`].
pub(crate) fn init(cx: &mut App) {
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
    use std::path::Path;

    use fe_config::theme::EXTENSIONS;
    use gpui::TestAppContext;
    use kiln::{
        core::theme::{ThemeData, ThemeFile},
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
}
