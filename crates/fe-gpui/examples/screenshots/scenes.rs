//! The scenes and their capture, with the Metal renderer: macOS only.

use std::{ops::Range, process::ExitCode, rc::Rc};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_editor::{
    EditorModel,
    diagnostics::{Diagnostic, Severity},
};
use fe_gpui::{EditorTheme, EditorView, GutterAnnotation, GutterAnnotations, MinimapOptions};
use fe_vim::{
    Remap, RemapMode,
    search::{FindQuery, Pattern},
};
use gpui::{
    AnyWindowHandle, AppContext as _, Focusable as _, Pixels, Size, VisualTestAppContext, px, size,
};
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
    /// Whether the minimap's marks have sources: a find pattern, an error and
    /// a warning, and a diff base ([`add_marks`]). Only in their own scene.
    marks: bool,
    /// Run once the editor is open and focused, before the capture: keys
    /// typed, settings set. Whatever it turns on, [`reset`] turns off after.
    setup: Option<Setup>,
}

type Setup = fn(&mut VisualTestAppContext, AnyWindowHandle);

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
        marks: false,
        setup: None,
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
            marks: false,
            setup: None,
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
            marks: false,
            setup: None,
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
            marks: false,
            setup: None,
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
            marks: false,
            setup: None,
        },
        Scene {
            name: "minimap",
            path: "scene.rs",
            text: long_rust(),
            cold: &[],
            annotations: &[],
            // Blocks: `minimap_characters` is the default look.
            minimap: Some(MinimapOptions {
                show_slider: fe_display::minimap::MinimapShowSlider::Always,
                render_characters: false,
                ..MinimapOptions::default()
            }),
            top: Some(140.),
            sticky_scroll: false,
            marks: false,
            setup: None,
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
            marks: false,
            setup: None,
        },
        Scene {
            name: "minimap_marks",
            path: "scene.rs",
            text: marked_rust(),
            cold: &[],
            annotations: &[],
            minimap: Some(MinimapOptions {
                show_slider: fe_display::minimap::MinimapShowSlider::Always,
                ..MinimapOptions::default()
            }),
            top: Some(140.),
            sticky_scroll: false,
            marks: true,
            setup: None,
        },
        Scene {
            name: "minimap_characters",
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
            marks: false,
            setup: None,
        },
        Scene {
            name: "minimap_left",
            path: "scene.rs",
            text: marked_rust(),
            cold: &[],
            annotations: &[],
            minimap: Some(MinimapOptions {
                side: fe_display::minimap::MinimapSide::Left,
                show_slider: fe_display::minimap::MinimapShowSlider::Always,
                ..MinimapOptions::default()
            }),
            top: Some(140.),
            sticky_scroll: false,
            marks: true,
            setup: None,
        },
        Scene {
            name: "which_key_popup",
            path: "scene.rs",
            text: RUST.to_owned(),
            cold: &[],
            annotations: &[],
            minimap: None,
            top: None,
            sticky_scroll: false,
            marks: false,
            setup: Some(leader_popup),
        },
        Scene {
            name: "which_key_icons",
            path: "scene.rs",
            text: RUST.to_owned(),
            cold: &[],
            annotations: &[],
            minimap: None,
            top: None,
            sticky_scroll: false,
            marks: false,
            setup: Some(lazyvim_popup),
        },
    ]
}

/// The bundled `lazyvim` preset, whose `<leader>` popup the
/// which_key_icons scene shows: LazyVim's groups, each with its icon, and
/// its commands, each with its icon if it has one.
const LAZYVIM: &str = include_str!("../../../fastedit/presets/lazyvim.vimrc");

/// Vim on with the [`LAZYVIM`] preset's maps and group names, `<Space>` the
/// leader, the host's commands known by title, and `<Space>` held: the
/// popup shows its keys, with their icons.
fn lazyvim_popup(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    let space = [fe_vim::VimKey::Char(' ')];
    let preset = fe_vim::parse_vimrc_with_leader(LAZYVIM, &space);
    assert!(preset.issues.is_empty(), "{:?}", preset.issues);
    fe_gpui::set_host_commands(fe_workbench::command_list());
    cx.update(|cx| {
        fe_gpui::set_vim_enabled(cx, true);
        let issues = fe_gpui::set_vim_remaps(cx, &preset.remaps, "<Space>");
        assert!(issues.is_empty(), "{issues:?}");
        fe_gpui::set_vim_which_key_groups(cx, preset.groups);
    });
    cx.run_until_parked();
    cx.update_window(window, |root, window, cx| {
        let view = root.downcast::<EditorView>().expect("an editor");
        view.update(cx, |view, cx| view.show_which_key_for(&space, window, cx));
    })
    .expect("the scene's window");
}

/// The which_key_popup scene's leader maps: items (one each on `<Space>`,
/// `<Bar>`, `<Tab>` and `<C-a>`) and groups, one also a map of its own.
const LEADER_MAPS: &[(&str, &str)] = &[
    ("<leader><Space>", "<Cmd>fe:editor.select_all<CR>"),
    ("<leader><Bar>", ":vsplit<CR>"),
    ("<leader><Tab>", "gt"),
    ("<leader><C-a>", "ggVG"),
    ("<leader>u", "<Cmd>fe:editor.undo<CR>"),
    ("<leader>w", ":w<CR>"),
    ("<leader>ff", ":find<CR>"),
    ("<leader>fr", ":browse oldfiles<CR>"),
    ("<leader>bd", ":bdelete<CR>"),
    ("<leader>bb", ":buffer #<CR>"),
    ("<leader>q", ":q<CR>"),
    ("<leader>qa", ":qa<CR>"),
];

/// Vim on with [`LEADER_MAPS`], `<Space>` the leader, and `<Space>` held:
/// the popup shows its keys. An off-screen window is never the active one,
/// where alone typed keys wait for a map, so the editor holds it directly.
fn leader_popup(cx: &mut VisualTestAppContext, window: AnyWindowHandle) {
    cx.update(|cx| {
        fe_gpui::set_vim_enabled(cx, true);
        let remaps: Vec<Remap> = (LEADER_MAPS.iter())
            .map(|&(from, to)| Remap {
                modes: vec![RemapMode::Normal],
                from: from.into(),
                to: to.into(),
                recursive: false,
            })
            .collect();
        let issues = fe_gpui::set_vim_remaps(cx, &remaps, "<Space>");
        assert!(issues.is_empty(), "{issues:?}");
    });
    cx.run_until_parked();
    cx.update_window(window, |root, window, cx| {
        let view = root.downcast::<EditorView>().expect("an editor");
        let space = [fe_vim::VimKey::Char(' ')];
        view.update(cx, |view, cx| view.show_which_key_for(&space, window, cx));
    })
    .expect("the scene's window");
}

/// Undoes what a scene's setup turned on, for the scenes after it.
fn reset(cx: &mut VisualTestAppContext) {
    fe_gpui::set_host_commands(Vec::new());
    cx.update(|cx| {
        fe_gpui::set_vim_remaps(cx, &[], "\\");
        fe_gpui::set_vim_which_key_groups(cx, Vec::new());
        fe_gpui::set_vim_enabled(cx, false);
    });
    cx.run_until_parked();
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

/// A row [`marked_rust`] adds to the fifth module, which its diff base lacks.
const ADDED_ROW: &str = "        /// The value at `key`, if any.\n";

/// [`long_rust`] with [`ADDED_ROW`] above the fifth module's `get`, and its
/// `insert` selected, from its name to its last statement.
fn marked_rust() -> String {
    let mut text = long_rust().replacen("‸", "", 1);
    text.insert_str(nth(&text, "        pub fn get", 4), ADDED_ROW);
    let end = nth(&text, "self.len += 1;", 4) + "self.len += 1;".len();
    text.insert(end, '»');
    text.insert(nth(&text, "insert(&mut", 4), '«');
    text
}

/// Where `needle`'s `n`th occurrence in `text`, from 0, starts.
fn nth(text: &str, needle: &str, n: usize) -> usize {
    text.match_indices(needle).nth(n).expect("in the text").0
}

/// The minimap_marks scene's sources besides its selection and find
/// pattern, all in the fifth module: an error and a warning, and a diff base
/// with a row modified, [`ADDED_ROW`] added and one deleted.
fn add_marks(model: &mut EditorModel) {
    let text = model.text().to_string();
    let at = |needle: &str| {
        let start = nth(&text, needle, 4);
        start..start + needle.len()
    };
    let items = vec![
        (at("&oldest"), Diagnostic::new(Severity::Error, "moved")),
        (
            at("capacity, len: 0"),
            Diagnostic::new(Severity::Warning, "unused"),
        ),
    ];
    let version = model.snapshot().version();
    assert!(model.apply_diagnostics(1u32, (1u64, 1u64), version, items, Some((1, 1))));
    let mut base = text.clone();
    // Modified since: `to_string` was `to_owned`.
    let modified = nth(&base, "key.to_string());", 4);
    base.replace_range(
        modified..modified + "key.to_string()".len(),
        "key.to_owned()",
    );
    // Deleted since: a row after `self.len -= 1;`.
    let deleted = nth(&base, "self.len -= 1;\n", 4) + "self.len -= 1;\n".len();
    base.insert_str(deleted, "                // Full: the oldest went.\n");
    let added = nth(&base, ADDED_ROW, 0);
    base.replace_range(added..added + ADDED_ROW.len(), "");
    model.set_diff_base(Some(base.into()));
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
#[allow(clippy::too_many_arguments)]
fn capture(
    cx: &mut VisualTestAppContext,
    model: EditorModel,
    annotations: Annotations,
    minimap: Option<MinimapOptions>,
    top: Option<f32>,
    sticky_scroll: bool,
    find: Option<Pattern>,
    size: Size<Pixels>,
    font_family: &'static str,
    gutter: bool,
    setup: Option<Setup>,
) -> Result<(RgbaImage, usize)> {
    let window = cx.open_offscreen_window(size, |_, cx| {
        let theme = EditorTheme {
            font_family: font_family.into(),
            sticky_scroll: fe_config::StickyScrollSettings {
                enabled: sticky_scroll,
                ..Default::default()
            },
            ..EditorTheme::default_dark(cx)
        };
        let shown = (!annotations.is_empty()).then(|| gutter_annotations(&model, annotations));
        cx.new(|cx| {
            let mut view = EditorView::new(model, theme, cx);
            view.set_gutter_annotations(shown, cx);
            view.set_minimap(minimap, cx);
            view.set_find(find, cx);
            view.set_gutter(gutter, cx);
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
    if let Some(setup) = setup {
        setup(cx, window.into());
    }
    // The view notifies itself when background work lands or a frame asks
    // for another (as while soft wrap catches up): draw until it's quiet.
    let mut settle = Settle::new();
    settle.observe(&view, cx);
    settle.capture(cx, window.into())
}

/// A font whose `f` has ink past its advance, which a message's last glyph
/// must keep, whole or cut.
const OVERHANG_FONT: &str = "Times New Roman";

/// Barely more than the messages, with no gutter, so the ink past an advance
/// is more of the scene than a comparison's tolerance: a mask at the advance
/// fails it, where it would pass in a scene of the usual size.
const OVERHANG_SIZE: Size<Pixels> = size(px(50.), px(84.));

/// Messages after their rows, all at an `f`: two whole and two cut.
fn overhang_model() -> EditorModel {
    let text = "a\nb\nc\nd\n";
    let mut model = EditorModel::new(Buffer::new(text), Some("scene.txt".into()));
    let cut = "f".repeat(100);
    let items = (["of", "elf", &cut, &cut].iter().enumerate())
        .map(|(row, message)| {
            let start = row * 2;
            (
                start..start + 1,
                Diagnostic::new(Severity::Error, message.to_string()),
            )
        })
        .collect();
    let version = model.snapshot().version();
    assert!(model.apply_diagnostics(1u32, (1u64, 1u64), version, items, Some((1, 1))));
    model
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
        if scene.marks {
            add_marks(&mut model);
        }
        let find = scene.marks.then(|| {
            Pattern::from_query(&FindQuery {
                text: "entries".into(),
                match_case: true,
                ..FindQuery::default()
            })
            .expect("a pattern")
        });
        let (image, notifications) = capture(
            cx,
            model,
            scene.annotations,
            scene.minimap,
            scene.top,
            scene.sticky_scroll,
            find,
            SCENE_SIZE,
            FONT_FAMILY,
            true,
            scene.setup,
        )?;
        if scene.setup.is_some() {
            reset(cx);
        }
        if !scene.cold.is_empty() && notifications < 2 {
            return Err(format!(
                "{}: no background work landed after the language loaded",
                scene.name
            )
            .into());
        }
        passed &= goldens.check(scene.name, &image)?;
    }
    let (image, _) = capture(
        cx,
        overhang_model(),
        &[],
        None,
        None,
        false,
        None,
        OVERHANG_SIZE,
        OVERHANG_FONT,
        false,
        None,
    )?;
    passed &= goldens.check("inline_diagnostics_overhang", &image)?;
    Ok(passed)
}

/// `render <out.png> <file> [--size WxH]`
fn render(cx: &mut VisualTestAppContext, args: &[String]) -> Result<()> {
    let usage = "usage: screenshots render <out.png> <file> [--size WxH]";
    let (out, file, size) = render_args(args, SCENE_SIZE, usage)?;
    let model = EditorModel::open(file).map_err(|error| format!("opening {file}: {error}"))?;
    capture(
        cx,
        model,
        &[],
        None,
        None,
        true,
        None,
        size,
        FONT_FAMILY,
        true,
        None,
    )?
    .0
    .save(out)?;
    println!("wrote {out}");
    Ok(())
}

pub(crate) fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // kiln-ui's icons, which the which-key popup draws.
    let mut cx = VisualTestAppContext::with_asset_source(
        gpui_platform::current_platform(false),
        std::sync::Arc::new(fe_ui::Assets),
    );
    cx.update(fe_gpui::init);
    exit_code(match args.first().map(String::as_str) {
        Some("render") => render(&mut cx, &args[1..]).map(|()| true),
        None => check_scenes(&mut cx),
        Some(_) => Err("usage: screenshots [render <out.png> <file> [--size WxH]]".into()),
    })
}
