//! Golden screenshot of the workbench: the project tree, the tab bar and the
//! editor, rendered off screen with the real Metal renderer. The harness is
//! fe-gpui's (`crates/fe-gpui/examples/screenshots`), whose comparison it
//! includes.
//!
//! ```text
//! cargo run -p fe-workbench --example screenshots                  # compare with golden/
//! UPDATE_SCREENSHOTS=1 cargo run -p fe-workbench --example screenshots
//! cargo run -p fe-workbench --example screenshots -- render out.png <dir> [--size WxH]
//! ```
//!
//! AppKit must run on the main thread, which `main` is but test threads
//! aren't, so `cargo test` only runs the comparison's unit tests.

// Under `cargo test` the harness replaces `main`, leaving the rest unused.
#![cfg_attr(test, allow(dead_code))]

#[path = "../../../fe-gpui/examples/screenshots/compare.rs"]
mod compare;

use std::{
    cell::Cell,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    rc::Rc,
    sync::Arc,
};

use fe_editor::EditorModel;
use fe_gpui::{EditorSettings, EditorTheme};
use fe_workbench::{Launch, Workbench};
use gpui::{
    AppContext as _, Focusable as _, Pixels, Size, VisualTestAppContext, WindowHandle, px, size,
};
use image::RgbaImage;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The window size of the scene, in logical pixels.
const SCENE_SIZE: Size<Pixels> = size(px(1000.), px(600.));

/// How many frames a scene may take to settle.
const MAX_FRAMES: usize = 10;

/// Pinned so goldens don't depend on which fonts are installed (TESTING.md).
const FONT_FAMILY: &str = "Menlo";

const SCENE: &str = "workbench";

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
fn write_project(parent: &Path) -> Result<PathBuf> {
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

/// Opens `root` in a workbench, off screen, with `src/` expanded and
/// `README.md` and `src/main.rs` open, then renders it once the background
/// work its frames start (the listing, loading languages, parsing) has landed.
fn capture(
    cx: &mut VisualTestAppContext,
    root: &Path,
    size: Size<Pixels>,
    open_demo: bool,
) -> Result<RgbaImage> {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.open_offscreen_window(size, |window, cx| {
        let theme = EditorTheme {
            font_family: FONT_FAMILY.into(),
            ..EditorTheme::default_dark(cx)
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
    // Views notify themselves when background work lands. Draw until a frame
    // and everything it started leave the workbench, its panel and its
    // editors quiet: that frame shows the result. Copied from fe-gpui's
    // `capture` (crates/fe-gpui/examples/screenshots/main.rs), which watches
    // one view.
    let notifications = Rc::new(Cell::new(0));
    let mut observers = Vec::new();
    macro_rules! observe {
        ($entity:expr) => {{
            let notifications = notifications.clone();
            observers.push(cx.update(|cx| {
                cx.observe(&$entity, move |_, _| {
                    notifications.set(notifications.get() + 1)
                })
            }));
        }};
    }
    observe!(workbench);
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    observe!(panel);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    observe!(pane);
    let editors: Vec<_> = pane.read_with(cx, |pane, _| {
        pane.tabs().iter().map(|tab| tab.editor.clone()).collect()
    });
    for editor in editors {
        observe!(editor);
    }
    let mut settled = false;
    for _ in 0..MAX_FRAMES {
        let before = notifications.get();
        cx.update_window(window.into(), |_, window, cx| {
            window.draw(cx).clear(cx);
            window.simulate_next_frame(cx);
        })?;
        cx.run_until_parked();
        if notifications.get() == before {
            settled = true;
            break;
        }
    }
    let image = if settled {
        cx.capture_screenshot(window.into()).map_err(Into::into)
    } else {
        Err(format!("still changing after {MAX_FRAMES} frames").into())
    };
    cx.update_window(window.into(), |_, window, _| window.remove_window())?;
    cx.run_until_parked();
    image
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

/// fe-workbench's directory. `cargo run` sets it at run time; prefer that over
/// the compile-time value, which a target directory shared between worktrees
/// can leave pointing at another (possibly deleted) checkout.
fn manifest_dir() -> PathBuf {
    std::env::var_os("CARGO_MANIFEST_DIR")
        .map_or_else(|| env!("CARGO_MANIFEST_DIR").into(), PathBuf::from)
}

fn workspace_dir() -> PathBuf {
    let manifest_dir = manifest_dir();
    manifest_dir
        .ancestors()
        .nth(2)
        .map_or_else(|| manifest_dir.clone(), Path::to_owned)
}

fn golden_dir() -> PathBuf {
    manifest_dir().join("examples/screenshots/golden")
}

/// Renders the scene and compares it with its golden, or rewrites the golden.
/// Returns whether it matched.
fn check_scene(cx: &mut VisualTestAppContext, update: bool) -> Result<bool> {
    let out_dir = workspace_dir().join("target/screenshots");
    let parent = tempfile::tempdir()?;
    let root = write_project(parent.path())?;
    let image = capture(cx, &root, SCENE_SIZE, true)?;
    let golden_path = golden_dir().join(format!("{SCENE}.png"));
    if update {
        image.save(&golden_path)?;
        println!("updated {}", golden_path.display());
        return Ok(true);
    }
    let golden = image::open(&golden_path)
        .map_err(|error| format!("reading {}: {error}", golden_path.display()))?
        .into_rgba8();
    match compare::compare(&golden, &image) {
        Ok(()) => {
            println!("ok      {SCENE}");
            Ok(true)
        }
        Err(mismatch) => {
            // The `workbench` prefix keeps these apart from fe-gpui's
            // scenes in the shared output directory.
            let (actual_path, diff_path) =
                compare::save_failure(&out_dir, SCENE, &image, &mismatch)?;
            println!("FAILED  {SCENE}: {mismatch}");
            println!("        actual: {}", actual_path.display());
            println!("        diff:   {}", diff_path.display());
            Ok(false)
        }
    }
}

/// `render <out.png> <dir> [--size WxH]`: the folder in a workbench, with
/// nothing opened.
fn render(cx: &mut VisualTestAppContext, args: &[String]) -> Result<()> {
    let usage = "usage: screenshots render <out.png> <dir> [--size WxH]";
    let (out, dir, rest) = match args {
        [out, dir, rest @ ..] => (out, dir, rest),
        _ => return Err(usage.into()),
    };
    let size = match rest {
        [] => SCENE_SIZE,
        [flag, value] if flag == "--size" => {
            let (w, h) = value.split_once('x').ok_or(usage)?;
            size(px(w.parse()?), px(h.parse()?))
        }
        _ => return Err(usage.into()),
    };
    let root = std::path::absolute(dir)?;
    capture(cx, &root, size, false)?.save(out)?;
    println!("wrote {out}");
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        Arc::new(fe_ui::Assets),
    );
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
    });
    let result = match args.first().map(String::as_str) {
        Some("render") => render(&mut cx, &args[1..]).map(|()| true),
        None => check_scene(&mut cx, std::env::var_os("UPDATE_SCREENSHOTS").is_some()),
        Some(_) => Err("usage: screenshots [render <out.png> <dir> [--size WxH]]".into()),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
