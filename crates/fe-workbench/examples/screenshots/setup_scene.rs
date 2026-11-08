//! The setup wizard's sheet on its Appearance page, over the demo workbench:
//! dark as the defaults have it, and light by `theme.mode`.

use fe_config::themes;
use gpui::VisualTestAppContext;
use kiln_platform::screenshots::Result;

use crate::scenes::{SCENE_SIZE, capture_with, goldens, use_settings, write_project};

/// Each scene: its golden, the bundled theme the settings come to, and the
/// settings the sheet reads its choices from.
const SCENES: [(&str, &str, &str); 2] = [
    ("setup_wizard_dark", themes::DEFAULT_DARK, ""),
    (
        "setup_wizard_light",
        themes::DEFAULT_LIGHT,
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
        let palette = use_settings(cx, parent.path(), theme, settings)?;

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
