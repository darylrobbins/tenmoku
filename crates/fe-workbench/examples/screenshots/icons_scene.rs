//! The project tree's file and folder icons as a user's first window has
//! them, `workbench.icon_theme` at its default: dark and light, with a Rust,
//! a TOML, a Makefile, a justfile and a Markdown file, `src/` expanded and
//! `node_modules/` closed.

use std::fs;

use gpui::{Pixels, Size, VisualTestAppContext, px, size};
use kiln_platform::screenshots::Result;

use crate::scenes::{capture_with, goldens, use_settings};

/// Tall enough for all of the tree's rows; the rest of the window is empty.
const ICONS_SIZE: Size<Pixels> = size(px(480.), px(360.));

/// Each scene: its golden, its bundled theme, and the settings that choose it.
/// The Start page is off: it would fill the empty pane, and its recent folders
/// list the scene's temporary directory.
const SCENES: [(&str, &str, &str); 2] = [
    (
        "file_icons_dark",
        "tenmoku-dark",
        "[workbench]\nstart_page = false\n",
    ),
    (
        "file_icons_light",
        "tenmoku-light",
        "[theme]\nmode = \"light\"\n\n[workbench]\nstart_page = false\n",
    ),
];

/// Renders each scene and compares it with its golden, or rewrites the golden.
/// Returns whether they all matched.
pub(crate) fn check(cx: &mut VisualTestAppContext) -> Result<bool> {
    let goldens = goldens();
    let mut matched = true;
    for (scene, theme, settings) in SCENES {
        let parent = tempfile::tempdir()?;
        let root = parent.path().join("demo");
        fs::create_dir_all(root.join("src"))?;
        fs::create_dir_all(root.join("node_modules"))?;
        for rel in [
            "src/main.rs",
            "node_modules/package.json",
            "config.toml",
            "Makefile",
            "justfile",
            "README.md",
        ] {
            fs::write(root.join(rel), "")?;
        }
        let palette = use_settings(cx, parent.path(), theme, settings)?;

        let main_rs = root.join("src/main.rs");
        let image = capture_with(
            cx,
            &root,
            ICONS_SIZE,
            false,
            &palette,
            |workbench, _, cx| {
                let panel = workbench.panel().clone();
                panel.update(cx, |panel, cx| panel.reveal(main_rs, cx));
            },
        )?;
        matched &= goldens.check(scene, &image)?;
    }
    Ok(matched)
}
