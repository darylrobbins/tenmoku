//! The setup wizard's sheet on its Appearance page, over the demo workbench:
//! dark as the defaults have it, and light by `theme.mode`.

use std::{fs, sync::Arc};

use fe_config::{EditorPalette, SettingsStore, themes};
use fe_ui::{Theme, ThemeData};
use gpui::VisualTestAppContext;
use kiln_platform::{
    config::{ConfigService, batch_channel},
    screenshots::Result,
};

use crate::scenes::{SCENE_SIZE, capture_with, goldens, write_project};

/// Each scene: its golden, the bundled theme the settings come to, and the
/// settings the sheet reads its choices from.
const SCENES: [(&str, &str, &str); 2] = [
    ("setup_wizard_dark", "tenmoku-dark", ""),
    (
        "setup_wizard_light",
        "tenmoku-light",
        "[theme]\nmode = \"light\"\n",
    ),
];

/// Renders each scene and compares it with its golden, or rewrites the golden.
/// Returns whether they all matched.
pub(crate) fn check(cx: &mut VisualTestAppContext) -> Result<bool> {
    let goldens = goldens();
    let mut matched = true;
    for (scene, theme, settings) in SCENES {
        let parent = tempfile::tempdir()?;
        let root = write_project(parent.path())?;
        let user = parent.path().join("settings.toml");
        fs::write(&user, settings)?;

        let theme = themes::find(theme).ok_or("a bundled theme")?.file();
        let data = ThemeData::from_file(&theme);
        let (palette, warnings) = EditorPalette::for_theme(&data);
        assert_eq!(warnings, [] as [String; 0], "the bundled theme's tables");
        let store = SettingsStore::load(Some(user), None).into_inner();
        // Nothing changes the file, so nothing need send a batch.
        let (_, batches) = batch_channel();
        cx.update(|cx| {
            ConfigService::new()
                .settings(store)
                .start_with(batches, cx)
                .detach();
            fe_ui::set_theme(cx, Theme::new(data));
            fe_gpui::set_editor_palette(cx, Arc::new(palette.clone()));
        });

        let image = capture_with(
            cx,
            &root,
            SCENE_SIZE,
            true,
            &palette,
            |workbench, window, cx| workbench.show_setup(window, cx),
        )?;
        matched &= goldens.check(scene, &image)?;
    }
    Ok(matched)
}
