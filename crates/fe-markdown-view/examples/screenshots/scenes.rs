//! The scenes and their capture, with the Metal renderer: macOS only.

use std::{process::ExitCode, sync::Arc};

use fe_config::themes::{DEFAULT_DARK, DEFAULT_LIGHT};
use fe_gpui::{EditorPalette, EditorSettings, EditorTheme};
use fe_markdown_view::{MarkdownStyle, MarkdownView};
use gpui::{AppContext as _, Pixels, Size, VisualTestAppContext, px, size};
use kiln_platform::screenshots::{Goldens, Result, RgbaImage, Settle, exit_code};
use kiln_ui::{Theme, ThemeData, UiTheme};

/// The window size of every scene, in logical pixels. The spec's 800×600
/// cuts the fixture off below the Rust fence, so the scenes are as tall as
/// it needs to show every construct.
const SCENE_SIZE: Size<Pixels> = size(px(800.), px(820.));

/// Pinned so goldens don't depend on which fonts are installed (TESTING.md):
/// the bundled code family, which [`main`] registers.
const FONT_FAMILY: &str = fe_fonts::CODE;

/// Every construct the preview draws, in one document.
const FIXTURE: &str = r#"# Markdown preview

Text with *emphasis*, **strong**, ~~struck~~, `inline code` and [a link](https://example.com).

## Lists

- A bullet
  - Nested under it
1. First
2. Second
- [x] A done task
- [ ] An open task

### Tables and code

| Left | Centre | Right |
|:-----|:------:|------:|
| a | b | 1 |
| longer cell | c | 22 |

```rust
pub fn double(x: u32) -> u32 { x * 2 } // twice
```

```py3
def greet(name): return f"hello, {name}"
```

> A quote, with a bar beside it.

---

![a local picture](picture.svg) ![a remote picture](https://example.com/picture.png)
"#;

/// A small picture for the fixture's local image.
const PICTURE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="40" viewBox="0 0 64 40"><rect width="64" height="40" rx="6" fill="#4f7cc8"/><circle cx="20" cy="20" r="10" fill="#f0c674"/></svg>"##;

struct Scene {
    name: &'static str,
    palette: &'static EditorPalette,
    ui: UiTheme,
}

/// The UI colours of the bundled theme `id`: the scenes use Tenmoku's
/// defaults, Yohen and Hakuji, as the editor palettes do.
fn ui_of(id: &str) -> UiTheme {
    let theme = fe_config::themes::find(id).expect("a bundled theme");
    Theme::new(ThemeData::from_file(&theme.file())).ui_theme()
}

fn scenes() -> Vec<Scene> {
    vec![
        Scene {
            name: "markdown_preview_dark",
            palette: EditorPalette::fastedit_dark(),
            ui: ui_of(DEFAULT_DARK),
        },
        Scene {
            name: "markdown_preview_light",
            palette: EditorPalette::fastedit_light(),
            ui: ui_of(DEFAULT_LIGHT),
        },
    ]
}

/// Renders the fixture in `scene`'s colours, off screen, once its images
/// and highlights have landed.
fn capture(cx: &mut VisualTestAppContext, scene: &Scene) -> Result<RgbaImage> {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("picture.svg"), PICTURE)?;
    let doc_path = dir.path().join("README.md");
    let document = Arc::new(fe_markdown::parse(FIXTURE));
    let window = cx.open_offscreen_window(SCENE_SIZE, |_, cx| {
        let editor = EditorTheme {
            font_family: FONT_FAMILY.into(),
            ..EditorTheme::from_palette(scene.palette, &EditorSettings::default(), cx)
        };
        let style = MarkdownStyle::new(&editor, &scene.ui);
        let root = Some(dir.path().to_owned());
        cx.new(|cx| MarkdownView::new(document, doc_path, root, style, cx))
    })?;
    let view = cx.update_window(window.into(), |view, _, _| {
        view.downcast::<MarkdownView>().expect("a preview")
    })?;
    let mut settle = Settle::new();
    settle.observe(&view, cx);
    Ok(settle.capture(cx, window.into())?.0)
}

/// Renders every scene and compares it with its golden, or rewrites the
/// goldens. Returns whether all scenes matched.
fn check_scenes(cx: &mut VisualTestAppContext) -> Result<bool> {
    let goldens = Goldens::new(env!("CARGO_MANIFEST_DIR"), "examples/screenshots/golden");
    let mut passed = true;
    for scene in scenes() {
        let image = capture(cx, &scene)?;
        passed &= goldens.check(scene.name, &image)?;
    }
    Ok(passed)
}

pub(crate) fn main() -> ExitCode {
    let mut cx = VisualTestAppContext::new(gpui_platform::current_platform(false));
    if let Err(error) = cx.update(|cx| cx.text_system().add_fonts(fe_fonts::all())) {
        eprintln!("the bundled fonts weren't registered: {error:#}");
        return ExitCode::FAILURE;
    }
    exit_code(check_scenes(&mut cx))
}
