//! A Markdown file in the editor, dark (Yohen) and light (Hakuji): headings
//! and strong text in the bold face, emphasis in the italic, struck text
//! struck through, the `**`, `*` and `~~` around them in the punctuation
//! colour, a quote and list markers in their colours (T1924). Markup in
//! markup takes both faces: a heading's emphasis, struck strong text and
//! `***both***` are bold and italic or struck.

use std::fs;

use fe_config::themes;
use fe_editor::EditorModel;
use gpui::{Pixels, Size, VisualTestAppContext, px, size};
use kiln_platform::screenshots::Result;

use crate::scenes::{capture_with, goldens, use_theme};

/// Wide and tall enough for every row; the tree is beside it.
const MARKDOWN_SIZE: Size<Pixels> = size(px(900.), px(520.));

const README: &str = "# Markdown styles

**Strong**, *emphasis* and ~~struck~~ text.

> A quote with **bold** inside.

- A list item, a [link](https://x.dev)
  - [x] A task, done
1. Numbered

## A second *heading*

Use `code`, ***both***, ~~**gone**~~ and [a label][docs].

[docs]: https://x.dev/docs
";

/// Each scene: its golden and its bundled theme.
const SCENES: [(&str, &str); 2] = [
    ("markdown_dark", themes::DEFAULT_DARK),
    ("markdown_light", themes::DEFAULT_LIGHT),
];

/// Renders each scene and compares it with its golden, or rewrites the golden.
/// Returns whether they all matched.
pub(crate) fn check(cx: &mut VisualTestAppContext) -> Result<bool> {
    let goldens = goldens();
    let mut matched = true;
    for (scene, theme) in SCENES {
        let parent = tempfile::tempdir()?;
        let root = parent.path().join("demo");
        fs::create_dir_all(&root)?;
        let readme = root.join("README.md");
        fs::write(&readme, README)?;
        let palette = use_theme(cx, theme)?;
        let model = EditorModel::open(&readme).map_err(|error| format!("README.md: {error}"))?;
        let image = capture_with(
            cx,
            &root,
            MARKDOWN_SIZE,
            false,
            &palette,
            move |workbench, window, cx| {
                let pane = workbench.pane().clone();
                pane.update(cx, |pane, cx| {
                    pane.add(model, cx);
                    pane.activate(0, window, cx);
                });
                let panel = workbench.panel().clone();
                panel.update(cx, |panel, cx| panel.reveal(readme, cx));
            },
        )?;
        matched &= goldens.check(scene, &image)?;
    }
    // Leaves the default dark theme in use, as the scenes after it expect.
    use_theme(cx, themes::DEFAULT_DARK)?;
    Ok(matched)
}
