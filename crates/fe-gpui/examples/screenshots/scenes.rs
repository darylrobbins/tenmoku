//! The scenes and their capture, with the Metal renderer: macOS only.

use std::{ops::Range, process::ExitCode, rc::Rc};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_editor::EditorModel;
use fe_gpui::{EditorTheme, EditorView, GutterAnnotation, GutterAnnotations, MinimapOptions};
use gpui::{AppContext as _, Focusable as _, Pixels, Size, VisualTestAppContext, px, size};
use kiln_platform::screenshots::{Goldens, Result, RgbaImage, Settle, exit_code, render_args};

/// The window size of every scene, in logical pixels.
const SCENE_SIZE: Size<Pixels> = size(px(800.), px(480.));

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
    /// The gutter annotation column's entries: buffer rows, text and heat.
    annotations: Annotations,
    /// The minimap beside the text, if it shows.
    minimap: Option<MinimapOptions>,
    /// The screen row scrolled to the top, if not where the caret puts it.
    top: Option<f32>,
    /// Whether sticky scroll's band shows: only in its own scene, so the
    /// scenes from before it keep their goldens.
    sticky_scroll: bool,
}

type Annotations = &'static [(Range<u32>, &'static str, Option<f32>)];

fn scenes() -> Vec<Scene> {
    let scene = |name, path, text: &str| Scene {
        name,
        path,
        text: text.to_owned(),
        cold: &[],
        annotations: &[],
        minimap: None,
        top: None,
        sticky_scroll: false,
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
            annotations: &[],
            minimap: None,
            top: None,
            sticky_scroll: false,
        },
        Scene {
            name: "markdown_fence",
            path: "scene.md",
            text: MARKDOWN.to_owned(),
            // markdown_inline is warm by now: highlighted_rust's doc comments inject it (T1015).
            cold: &["markdown", "python"],
            annotations: &[],
            minimap: None,
            top: None,
            sticky_scroll: false,
        },
        Scene {
            name: "large_c",
            path: "scene.c",
            text: large_c(),
            cold: &["c"],
            annotations: &[],
            minimap: None,
            top: None,
            sticky_scroll: false,
        },
        Scene {
            name: "annotated_gutter",
            path: "scene.rs",
            text: RUST.to_owned(),
            cold: &[],
            annotations: &[
                (0..3, "2026-10-11 Ann Example", Some(1.0)),
                (3..5, "2025-03-02 Bob Example", Some(0.0)),
                (6..9, "2026-10-11 Ann Example", Some(1.0)),
            ],
            minimap: None,
            top: None,
            sticky_scroll: false,
        },
        Scene {
            name: "minimap",
            path: "scene.rs",
            text: long_rust(),
            cold: &[],
            annotations: &[],
            minimap: Some(MinimapOptions {
                show_slider: fe_display::minimap::MinimapShowSlider::Always,
                ..MinimapOptions::default()
            }),
            top: Some(140.),
            sticky_scroll: false,
        },
        Scene {
            name: "sticky_scroll",
            path: "scene.rs",
            text: STICKY.to_owned(),
            cold: &[],
            annotations: &[],
            minimap: None,
            // Four rows into `count`: its impl's header and its own pinned.
            top: Some(15.),
            sticky_scroll: true,
        },
    ]
}

/// About 300 rows of Rust: [`RUST`] in nine modules, the caret on row 150.
fn long_rust() -> String {
    let mut text = String::new();
    for n in 0..9 {
        text.push_str(&format!("mod part_{n} {{\n"));
        for line in RUST.replacen("‸", "", 1).lines() {
            match line.is_empty() {
                true => text.push('\n'),
                false => text.push_str(&format!("    {line}\n")),
            }
        }
        text.push_str("}\n");
    }
    let row_150 = text.match_indices('\n').nth(149).expect("300 rows").0 + 1;
    text.insert(row_150, '‸');
    text
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

/// An impl with a function longer than the window, for sticky scroll.
const STICKY: &str = r#"use std::collections::HashMap;

/// Word counts over a stream of lines.
#[derive(Debug, Default)]
pub struct Counter {
    counts: HashMap<String, usize>,
    lines: usize,
    longest: Option<String>,
}

impl Counter {
    /// Counts each word of `text`, lowercased, without its punctuation, and
    /// returns how many it counted.
    pub fn count(&mut self, text: &str) -> usize {
        let mut added = 0;
        for line in text.lines() {
            self.lines += 1;
            for word in line.split_whitespace() {
                let word: String = word
                    .chars()
                    .filter(|ch| ch.is_alphanumeric())
                    .flat_map(char::to_lowercase)
                    .collect();
                if word.is_empty() {
                    continue;
                }
                let longer = match &self.longest {
                    Some(longest) => word.len() > longest.len(),
                    None => true,
                };
                if longer {
                    self.longest = Some(word.clone());
                }
                *self.counts.entry(word).or_insert(0) += 1;
                ‸added += 1;
            }
        }
        added
    }

    /// The `n` most counted words, most first.
    pub fn top(&self, n: usize) -> Vec<(&str, usize)> {
        let mut words: Vec<_> = self.counts.iter().map(|(w, c)| (w.as_str(), *c)).collect();
        words.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        words.truncate(n);
        words
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

/// `entries` anchored in `model`'s buffer, with no popovers or clicks.
fn gutter_annotations(model: &EditorModel, entries: Annotations) -> GutterAnnotations {
    let snapshot = model.snapshot();
    let text = snapshot.text();
    GutterAnnotations {
        version: snapshot.version(),
        entries: (entries.iter())
            .map(|(rows, label, heat)| GutterAnnotation {
                rows: snapshot.anchor_before(text.row_start(rows.start))
                    ..snapshot.anchor_before(text.row_start(rows.end)),
                text: (*label).into(),
                heat: *heat,
            })
            .collect(),
        hover: Rc::new(|_, _| None),
        on_click: Rc::new(|_, _, _, _| {}),
    }
}

/// Renders `model` in a focused editor, off screen, with `annotations` in
/// its gutter if there are any, once the background work
/// its frames start (loading languages, parsing) has landed. Also returns
/// how many times the view asked to be drawn again meanwhile.
fn capture(
    cx: &mut VisualTestAppContext,
    model: EditorModel,
    annotations: Annotations,
    minimap: Option<MinimapOptions>,
    top: Option<f32>,
    sticky_scroll: bool,
    size: Size<Pixels>,
) -> Result<(RgbaImage, usize)> {
    let window = cx.open_offscreen_window(size, |_, cx| {
        let theme = EditorTheme {
            font_family: FONT_FAMILY.into(),
            sticky_scroll,
            ..EditorTheme::default_dark(cx)
        };
        let shown = (!annotations.is_empty()).then(|| gutter_annotations(&model, annotations));
        cx.new(|cx| {
            let mut view = EditorView::new(model, theme, cx);
            view.set_gutter_annotations(shown, cx);
            view.set_minimap(minimap, cx);
            if let Some(top) = top {
                view.set_scroll_top(top, cx);
            }
            view
        })
    })?;
    let view = cx.update_window(window.into(), |view, window, cx| {
        let view = view.downcast::<EditorView>().expect("an editor");
        window.focus(&view.focus_handle(cx), cx);
        view
    })?;
    // The view notifies itself when background work lands or a frame asks
    // for another (as while soft wrap catches up): draw until it's quiet.
    let mut settle = Settle::new();
    settle.observe(&view, cx);
    settle.capture(cx, window.into())
}

/// Renders every scene and compares it with its golden, or rewrites the
/// goldens. Returns whether all scenes matched.
fn check_scenes(cx: &mut VisualTestAppContext) -> Result<bool> {
    let goldens = Goldens::new(env!("CARGO_MANIFEST_DIR"), "examples/screenshots/golden");
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
        let (image, notifications) = capture(
            cx,
            model,
            scene.annotations,
            scene.minimap,
            scene.top,
            scene.sticky_scroll,
            SCENE_SIZE,
        )?;
        if !scene.cold.is_empty() && notifications < 2 {
            return Err(format!(
                "{}: no background work landed after the language loaded",
                scene.name
            )
            .into());
        }
        passed &= goldens.check(scene.name, &image)?;
    }
    Ok(passed)
}

/// `render <out.png> <file> [--size WxH]`
fn render(cx: &mut VisualTestAppContext, args: &[String]) -> Result<()> {
    let usage = "usage: screenshots render <out.png> <file> [--size WxH]";
    let (out, file, size) = render_args(args, SCENE_SIZE, usage)?;
    let model = EditorModel::open(file).map_err(|error| format!("opening {file}: {error}"))?;
    capture(cx, model, &[], None, None, true, size)?
        .0
        .save(out)?;
    println!("wrote {out}");
    Ok(())
}

pub(crate) fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cx = VisualTestAppContext::new(gpui_platform::current_platform(false));
    cx.update(fe_gpui::init);
    exit_code(match args.first().map(String::as_str) {
        Some("render") => render(&mut cx, &args[1..]).map(|()| true),
        None => check_scenes(&mut cx),
        Some(_) => Err("usage: screenshots [render <out.png> <file> [--size WxH]]".into()),
    })
}
