//! The scenes and their capture, with the Metal renderer: macOS only.

use std::{
    cell::Cell,
    path::{Path, PathBuf},
    process::ExitCode,
    rc::Rc,
};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_editor::EditorModel;
use fe_gpui::{EditorTheme, EditorView};
use gpui::{AppContext as _, Focusable as _, Pixels, Size, VisualTestAppContext, px, size};
use image::RgbaImage;

use crate::compare;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The window size of every scene, in logical pixels.
const SCENE_SIZE: Size<Pixels> = size(px(800.), px(480.));

/// How many frames a scene may take to settle. Each takes two today.
const MAX_FRAMES: usize = 10;

/// Pinned so goldens don't depend on which fonts are installed (TESTING.md).
const FONT_FAMILY: &str = "Menlo";

/// A file to show, with `‸` marking carets and `«…»` selections (head at
/// the end).
struct Scene {
    name: &'static str,
    path: &'static str,
    text: String,
    /// Languages the scene must be the first to load. Its highlights then
    /// land from at least two rounds of background work: the language, then
    /// a parse too long for a frame or an injected language.
    cold: &'static [&'static str],
}

fn scenes() -> Vec<Scene> {
    let scene = |name, path, text: &str| Scene {
        name,
        path,
        text: text.to_owned(),
        cold: &[],
    };
    let numbered: String = (1..=140)
        .map(|n| {
            let mark = if n == 120 { "‸" } else { "" };
            format!("let value_{n} = {mark}{n} * 2;\n")
        })
        .collect();
    vec![
        scene("highlighted_rust", "scene.rs", RUST),
        scene(
            "multi_line_selection",
            "scene.rs",
            &RUST
                .replacen("‸", "", 1)
                .replacen("pub fn insert", "pub fn «insert", 1)
                .replacen("self.len += 1;", "self.len += 1;»", 1),
        ),
        scene(
            "multi_cursor",
            "scene.rs",
            &RUST
                .replacen("‸", "", 1)
                .replacen("pub fn new", "‸pub fn new", 1)
                .replacen("pub fn insert", "‸pub fn insert", 1)
                .replacen("pub fn get", "‸pub fn get", 1),
        ),
        scene("tabs", "scene.go", GO),
        scene(
            "long_line",
            "scene.rs",
            "‸// A long line is cut off at the window's right edge rather than wrapped.\n\
             const GREETING: &str = \"The quick brown fox jumps over the lazy dog, then keeps running past the right edge of the window and well beyond it\";\n\
             \n\
             fn main() {\n    println!(\"{GREETING}\");\n}\n",
        ),
        Scene {
            name: "three_digit_gutter",
            path: "scene.rs",
            text: numbered,
            cold: &[],
        },
        Scene {
            name: "markdown_fence",
            path: "scene.md",
            text: MARKDOWN.to_owned(),
            cold: &["markdown", "markdown_inline", "python"],
        },
        Scene {
            name: "large_c",
            path: "scene.c",
            text: large_c(),
            cold: &["c"],
        },
    ]
}

const MARKDOWN: &str = "‸# Notes\n\n\
Plain text with *emphasis*, **strong** words and `inline code`.\n\n\
```python\n\
def greet(name: str) -> str:\n    # Python, injected into the fence\n    return f\"hello, {name}\"\n\
```\n\n\
- a list item\n\
- [a link](https://example.com)\n";

/// About 400 KB of C, too much to parse within a frame's budget.
fn large_c() -> String {
    let mut text = String::from("‸#include <stdio.h>\n\n");
    for n in 0..4_000 {
        text.push_str(&format!(
            "/* Doubles its argument. */\nstatic int double_{n}(int x) {{\n    return x * 2; // {n}\n}}\n\n"
        ));
    }
    text
}

const RUST: &str = r#"‸use std::collections::HashMap;

/// A cache that remembers the most recent values by key.
#[derive(Debug, Default)]
pub struct Cache<V> {
    entries: HashMap<String, V>,
    order: Vec<String>,
    capacity: usize,
    len: usize,
}

impl<V: Clone> Cache<V> {
    pub fn new(capacity: usize) -> Self {
        Self { entries: HashMap::new(), order: Vec::new(), capacity, len: 0 }
    }

    pub fn insert(&mut self, key: &str, value: V) {
        if self.len == self.capacity {
            let oldest = self.order.remove(0);
            self.entries.remove(&oldest);
            self.len -= 1;
        }
        self.order.push(key.to_string());
        self.entries.insert(key.to_string(), value);
        self.len += 1;
    }

    pub fn get(&self, key: &str) -> Option<V> {
        self.entries.get(key).cloned() // 'static lifetimes aren't needed
    }
}
"#;

const GO: &str = "package main\n\nimport \"fmt\"\n\nfunc main() {\n\tfor i := 0; i < 3; i++ {\n\t\tif i%2 == 0 {\n\t\t\t‸fmt.Println(\"even\", i)\n\t\t}\n\t}\n}\n";

fn parse(marked: &str) -> (String, Vec<ResolvedSelection>) {
    let mut text = String::new();
    let mut selections = Vec::new();
    let mut open = None;
    for ch in marked.chars() {
        match ch {
            '‸' => selections.push(ResolvedSelection {
                range: text.len()..text.len(),
                reversed: false,
            }),
            '«' => open = Some(text.len()),
            '»' => selections.push(ResolvedSelection {
                range: open.take().expect("balanced")..text.len(),
                reversed: false,
            }),
            _ => text.push(ch),
        }
    }
    (text, selections)
}

/// Renders `model` in a focused editor, off screen, once the background work
/// its frames start (loading languages, parsing) has landed. Also returns
/// how many times the view asked to be drawn again meanwhile.
fn capture(
    cx: &mut VisualTestAppContext,
    model: EditorModel,
    size: Size<Pixels>,
) -> Result<(RgbaImage, usize)> {
    let window = cx.open_offscreen_window(size, |_, cx| {
        let theme = EditorTheme {
            font_family: FONT_FAMILY.into(),
            ..EditorTheme::default_dark(cx)
        };
        cx.new(|cx| EditorView::new(model, theme, cx))
    })?;
    let view = cx.update_window(window.into(), |view, window, cx| {
        let view = view.downcast::<EditorView>().expect("an editor");
        window.focus(&view.focus_handle(cx), cx);
        view
    })?;
    // The view notifies itself when background work lands or a frame asks
    // for another (as while soft wrap catches up). Draw until a frame and
    // everything it started leave it quiet: that frame shows the result.
    // (A test-mode app also redraws a window whenever it's notified, so the
    // work can land over fewer frames than it has rounds.)
    let notifications = Rc::new(Cell::new(0));
    let _observer = cx.update(|cx| {
        let notifications = notifications.clone();
        cx.observe(&view, move |_, _| {
            notifications.set(notifications.get() + 1)
        })
    });
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
        cx.capture_screenshot(window.into())
            .map(|image| (image, notifications.get()))
            .map_err(Into::into)
    } else {
        Err(format!("still changing after {MAX_FRAMES} frames").into())
    };
    cx.update_window(window.into(), |_, window, _| window.remove_window())?;
    cx.run_until_parked();
    image
}

/// fe-gpui's directory. `cargo run` sets it at run time; prefer that over the
/// compile-time value, which a target directory shared between worktrees can
/// leave pointing at another (possibly deleted) checkout.
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

/// Renders every scene and compares it with its golden, or rewrites the
/// goldens. Returns whether all scenes matched.
fn check_scenes(cx: &mut VisualTestAppContext, update: bool) -> Result<bool> {
    let out_dir = workspace_dir().join("target/screenshots");
    let mut passed = true;
    for scene in scenes() {
        if let Some(loaded) = scene
            .cold
            .iter()
            .find(|name| fe_syntax::loaded_language(name).is_some())
        {
            return Err(format!("{}: {loaded} was loaded by an earlier scene", scene.name).into());
        }
        let (text, selections) = parse(&scene.text);
        let mut model = EditorModel::new(Buffer::new(&text), Some(scene.path.into()));
        model.set_selections(selections);
        let (image, notifications) = capture(cx, model, SCENE_SIZE)?;
        if !scene.cold.is_empty() && notifications < 2 {
            return Err(format!(
                "{}: no background work landed after the language loaded",
                scene.name
            )
            .into());
        }
        let golden_path = golden_dir().join(format!("{}.png", scene.name));
        if update {
            image.save(&golden_path)?;
            println!("updated {}", golden_path.display());
            continue;
        }
        let golden = image::open(&golden_path)
            .map_err(|error| format!("reading {}: {error}", golden_path.display()))?
            .into_rgba8();
        if compare::skip_other_scale(&golden, &image) {
            println!(
                "skipped {}: {:?} is the golden's {:?} at another scale ({})",
                scene.name,
                image.dimensions(),
                golden.dimensions(),
                compare::ALLOW_OTHER_SCALE
            );
            continue;
        }
        match compare::compare(&golden, &image) {
            Ok(()) => println!("ok      {}", scene.name),
            Err(mismatch) => {
                passed = false;
                let (actual_path, diff_path) =
                    compare::save_failure(&out_dir, scene.name, &image, &mismatch)?;
                println!("FAILED  {}: {mismatch}", scene.name);
                println!("        actual: {}", actual_path.display());
                println!("        diff:   {}", diff_path.display());
            }
        }
    }
    Ok(passed)
}

/// `render <out.png> <file> [--size WxH]`
fn render(cx: &mut VisualTestAppContext, args: &[String]) -> Result<()> {
    let usage = "usage: screenshots render <out.png> <file> [--size WxH]";
    let (out, file, rest) = match args {
        [out, file, rest @ ..] => (out, file, rest),
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
    let model = EditorModel::open(file).map_err(|error| format!("opening {file}: {error}"))?;
    capture(cx, model, size)?.0.save(out)?;
    println!("wrote {out}");
    Ok(())
}

pub(crate) fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cx = VisualTestAppContext::new(gpui_platform::current_platform(false));
    cx.update(fe_gpui::init);
    let result = match args.first().map(String::as_str) {
        Some("render") => render(&mut cx, &args[1..]).map(|()| true),
        None => check_scenes(&mut cx, std::env::var_os("UPDATE_SCREENSHOTS").is_some()),
        Some(_) => Err("usage: screenshots [render <out.png> <file> [--size WxH]]".into()),
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
