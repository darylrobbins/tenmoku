//! The scenes and their capture, with the Metal renderer: macOS only.

use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
};

use fe_config::{SettingsStore, themes};
use fe_editor::EditorModel;
use fe_gpui::{EditorPalette, EditorSettings, EditorTheme};
use fe_ui::{Theme, ThemeData};
use fe_workbench::{Launch, Workbench};
use gpui::{
    AppContext as _, Context, Focusable as _, Pixels, Size, VisualTestAppContext, Window,
    WindowHandle, px, size,
};
use kiln_platform::{
    config::{ConfigService, batch_channel},
    screenshots::{Goldens, Result, RgbaImage, Settle, exit_code, render_args},
};

/// The window size of the scene, in logical pixels.
pub(crate) const SCENE_SIZE: Size<Pixels> = size(px(1000.), px(600.));

/// Pinned so goldens don't depend on which fonts are installed (TESTING.md).
const FONT_FAMILY: &str = "Menlo";

pub(crate) const SCENE: &str = "workbench";

const MAIN_RS: &str = r#"mod util;

use std::collections::HashMap;

/// Counts the words in each line of the input.
fn main() {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for line in std::env::args().skip(1) {
        *counts.entry(line.clone()).or_default() += line.split_whitespace().count();
    }
    println!("{counts:?}");
}
"#;

/// Writes the demo project into `<parent>/demo`, so the root's name is fixed,
/// and returns its path.
pub(crate) fn write_project(parent: &Path) -> Result<PathBuf> {
    let root = parent.join("demo");
    fs::create_dir_all(root.join("src/util"))?;
    fs::create_dir_all(root.join("target"))?;
    for (rel, text) in [
        ("src/main.rs", MAIN_RS),
        ("src/lib.rs", "pub mod util;\n"),
        ("src/util/mod.rs", "pub fn util() {}\n"),
        (
            "Cargo.toml",
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        ),
        ("README.md", "# demo\n\nA small project to look at.\n"),
        (".gitignore", "target/\n"),
        ("target/out", ""),
    ] {
        fs::write(root.join(rel), text)?;
    }
    Ok(root)
}

/// Starts kiln's config service on a user settings file `parent/settings.toml`
/// holding `settings`, and puts the bundled theme `theme` in use. Returns that
/// theme's palette for [`capture_with`].
pub(crate) fn use_settings(
    cx: &mut VisualTestAppContext,
    parent: &Path,
    theme: &str,
    settings: &str,
) -> Result<EditorPalette> {
    let user = parent.join("settings.toml");
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
    Ok(palette)
}

/// Opens `root` in a workbench, off screen, with `src/` expanded and
/// `README.md` and `src/main.rs` open, then renders it once the background
/// work its frames start (the listing, loading languages, parsing) has landed.
/// `palette` gives the editors their colours.
pub(crate) fn capture(
    cx: &mut VisualTestAppContext,
    root: &Path,
    size: Size<Pixels>,
    open_demo: bool,
    palette: &EditorPalette,
) -> Result<RgbaImage> {
    capture_with(cx, root, size, open_demo, palette, |_, _, _| {})
}

/// [`capture`], with `before` run in the workbench once it's open, before the
/// first frame: to put something over it.
pub(crate) fn capture_with(
    cx: &mut VisualTestAppContext,
    root: &Path,
    size: Size<Pixels>,
    open_demo: bool,
    palette: &EditorPalette,
    before: impl FnOnce(&mut Workbench, &mut Window, &mut Context<Workbench>),
) -> Result<RgbaImage> {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.open_offscreen_window(size, |window, cx| {
        let theme = EditorTheme {
            font_family: FONT_FAMILY.into(),
            ..EditorTheme::from_palette(palette, &EditorSettings::default(), cx)
        };
        cx.new(|cx| {
            let mut workbench =
                Workbench::new(launch, theme, EditorSettings::default(), window, cx);
            workbench.set_title_bar(true, cx);
            workbench
        })
    })?;
    let workbench = cx.update_window(window.into(), |view, window, cx| {
        let workbench = view.downcast::<Workbench>().expect("a workbench");
        window.focus(&workbench.focus_handle(cx), cx);
        workbench
    })?;
    if open_demo {
        open_files(cx, window, &workbench, root)?;
    }
    cx.update_window(window.into(), |_, window, cx| {
        workbench.update(cx, |workbench, cx| before(workbench, window, cx));
    })?;
    // Views notify themselves when background work lands. Draw until a frame
    // and everything it started leave the workbench, its panel and its
    // editors quiet: that frame shows the result.
    let mut settle = Settle::new();
    settle.observe(&workbench, cx);
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    settle.observe(&panel, cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    settle.observe(&pane, cx);
    let editors: Vec<_> = pane.read_with(cx, |pane, _| {
        pane.tabs()
            .iter()
            .map(|tab| tab.editor().unwrap().clone())
            .collect()
    });
    for editor in editors {
        settle.observe(&editor, cx);
    }
    Ok(settle.capture(cx, window.into())?.0)
}

/// Opens `README.md`, then `src/main.rs` (active, focused, caret at the
/// start), and selects `main.rs` in the tree, which expands `src/`.
fn open_files(
    cx: &mut VisualTestAppContext,
    window: WindowHandle<Workbench>,
    workbench: &gpui::Entity<Workbench>,
    root: &Path,
) -> Result<()> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    for rel in ["README.md", "src/main.rs"] {
        let path = root.join(rel);
        let model = EditorModel::open(&path).map_err(|error| format!("{rel}: {error}"))?;
        pane.update(cx, |pane, cx| pane.add(model, cx));
    }
    let main_rs = root.join("src/main.rs");
    workbench
        .read_with(cx, |workbench, _| workbench.panel().clone())
        .update(cx, |panel, cx| panel.reveal(main_rs, cx));
    cx.update_window(window.into(), |_, window, cx| {
        let ix = pane.read(cx).tabs().len() - 1;
        pane.update(cx, |pane, cx| pane.activate(ix, window, cx));
    })?;
    cx.run_until_parked();
    Ok(())
}

/// This example's goldens.
pub(crate) fn goldens() -> Goldens {
    Goldens::new(env!("CARGO_MANIFEST_DIR"), "examples/screenshots/golden")
}

/// Renders the scene and compares it with its golden, or rewrites the golden.
/// Returns whether it matched. Failures go to the output directory fe-gpui's
/// scenes share, as `workbench.actual.png` and `workbench.diff.png`.
fn check_scene(cx: &mut VisualTestAppContext) -> Result<bool> {
    let goldens = goldens();
    let parent = tempfile::tempdir()?;
    let root = write_project(parent.path())?;
    let image = capture(cx, &root, SCENE_SIZE, true, EditorPalette::fastedit_dark())?;
    goldens.check(SCENE, &image)
}

/// `render <out.png> <dir> [--size WxH]`: the folder in a workbench, with
/// nothing opened.
fn render(cx: &mut VisualTestAppContext, args: &[String]) -> Result<()> {
    let usage = "usage: screenshots render <out.png> <dir> [--size WxH]";
    let (out, dir, size) = render_args(args, SCENE_SIZE, usage)?;
    let root = std::path::absolute(dir)?;
    capture(cx, &root, size, false, EditorPalette::fastedit_dark())?.save(out)?;
    println!("wrote {out}");
    Ok(())
}

pub(crate) fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        Arc::new(fe_ui::Assets),
    );
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    exit_code(match args.first().map(String::as_str) {
        Some("render") => render(&mut cx, &args[1..]).map(|()| true),
        // The setup and icon scenes last: they leave their settings, theme
        // and icon set in use.
        None => check_scene(&mut cx)
            .and_then(|matched| Ok(crate::token_probe::check(&mut cx)? && matched))
            .and_then(|matched| Ok(crate::setup_scene::check(&mut cx)? && matched))
            .and_then(|matched| Ok(crate::icons_scene::check(&mut cx)? && matched)),
        Some(_) => Err("usage: screenshots [render <out.png> <dir> [--size WxH]]".into()),
    })
}
