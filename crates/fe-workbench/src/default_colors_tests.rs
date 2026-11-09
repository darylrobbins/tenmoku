//! Built-in colour swatches in a real workbench, on a paused clock: found
//! in CSS and in other languages' strings, handed over to a server's
//! colours in `auto`, and following `editor.default_color_decorators`.

use std::{fs, path::PathBuf, sync::Arc, time::Duration};

use fe_config::{Settings, SettingsEvent, settings::DefaultColorDecorators};
use fe_gpui::{ColorSwatch, EditorView};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use lsp_types::{ColorProviderCapability, PositionEncodingKind, ServerCapabilities};
use serde_json::{Value, json};
use tempfile::TempDir;

use super::super::REST;
use crate::{Launch, Workbench, color_literals::parse_color, tests::lsp::Lsp};

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Long enough for whatever is to be scanned to be, retries included.
fn settle(cx: &mut VisualTestContext) {
    draw(cx);
    for _ in 0..10 {
        cx.executor().advance_clock(REST);
        cx.run_until_parked();
        draw(cx);
    }
}

/// A rootless workbench: no language server.
fn workbench(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    (workbench, cx)
}

fn write(dir: &TempDir, name: &str, text: &str) -> PathBuf {
    let path = dir.path().join(name);
    fs::write(&path, text).unwrap();
    path
}

fn open(
    workbench: &Entity<Workbench>,
    path: PathBuf,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("the file is shown")
}

/// Opens `text` as a file called `name`, and lets it be scanned.
fn shown_file<'a>(
    cx: &'a mut TestAppContext,
    name: &str,
    text: &str,
) -> (
    TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = write(&dir, name, text);
    let (workbench, cx) = workbench(cx);
    let editor = open(&workbench, path, cx);
    settle(cx);
    (dir, workbench, editor, cx)
}

fn shown(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<ColorSwatch> {
    editor.read_with(cx, |view, _| view.color_swatches())
}

/// The swatch of `literal`, the `nth` (from 0) in `text`.
fn swatch(text: &str, literal: &str, nth: usize) -> ColorSwatch {
    let offset = (text.match_indices(literal).nth(nth))
        .unwrap_or_else(|| panic!("{literal} isn't in the text"))
        .0;
    ColorSwatch {
        offset,
        rgba: parse_color(literal).unwrap(),
    }
}

/// The settings with `editor.color_decorators` `on` and `mode`.
fn event(on: bool, mode: DefaultColorDecorators) -> SettingsEvent {
    let mut settings = Settings::default();
    settings.editor.color_decorators = on;
    settings.editor.default_color_decorators = mode;
    SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    }
}

fn reload(
    workbench: &Entity<Workbench>,
    on: bool,
    mode: DefaultColorDecorators,
    cx: &mut VisualTestContext,
) {
    let event = event(on, mode);
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    draw(cx);
}

/// Types `text` at `offset`.
fn type_at(editor: &Entity<EditorView>, offset: usize, text: &str, cx: &mut VisualTestContext) {
    editor.update(cx, |view, cx| {
        view.model_mut().set_cursor(offset);
        view.model_mut().insert(text);
        cx.notify();
    });
    draw(cx);
}

/// The screenshot's declarations, with an id selector, comments and a
/// named colour.
const CSS: &str = "#add {\n  color: #6c6976;\n  background: #ffffff12;\n  /* #fff */\n  \
                   border-color: oklch(0.62 0.08 195);\n  \
                   box-shadow: 0 1px 2px rgba(0, 0, 0, 0.62);\n  \
                   outline: 1px solid /* #000 */ red;\n}\n";

fn css_swatches() -> Vec<ColorSwatch> {
    vec![
        swatch(CSS, "#6c6976", 0),
        swatch(CSS, "#ffffff12", 0),
        swatch(CSS, "oklch(0.62 0.08 195)", 0),
        swatch(CSS, "rgba(0, 0, 0, 0.62)", 0),
        swatch(CSS, "red", 0),
    ]
}

#[gpui::test]
fn a_css_buffer_without_a_server_gets_swatches(cx: &mut TestAppContext) {
    let (_dir, _workbench, editor, cx) = shown_file(cx, "style.css", CSS);
    assert_eq!(shown(&editor, cx), css_swatches());
}

#[gpui::test]
fn hex_in_strings_only_outside_css(cx: &mut TestAppContext) {
    let text = "accent = \"#d4856a\"\n# see #123 and #abcdef\nn = \"#abc\"\n";
    let (_dir, _workbench, editor, cx) = shown_file(cx, "theme.toml", text);
    assert_eq!(shown(&editor, cx), [swatch(text, "#d4856a", 0)]);
}

#[gpui::test]
fn style_in_html_gets_swatches(cx: &mut TestAppContext) {
    let text = "<p>#00ff00</p>\n<style>a { color: #ff0000 }</style>\n";
    let (_dir, _workbench, editor, cx) = shown_file(cx, "page.html", text);
    assert_eq!(shown(&editor, cx), [swatch(text, "#ff0000", 0)]);
}

#[gpui::test]
fn never_shows_none_without_a_server(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = shown_file(cx, "style.css", CSS);
    assert_eq!(shown(&editor, cx), css_swatches());
    reload(&workbench, true, DefaultColorDecorators::Never, cx);
    assert_eq!(shown(&editor, cx), []);
    settle(cx);
    assert_eq!(shown(&editor, cx), []);
    // Back to `auto`, they're found again.
    reload(&workbench, true, DefaultColorDecorators::Auto, cx);
    settle(cx);
    assert_eq!(shown(&editor, cx), css_swatches());
}

#[gpui::test]
fn color_decorators_off_shows_none(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = shown_file(cx, "style.css", CSS);
    assert_eq!(shown(&editor, cx), css_swatches());
    for mode in [DefaultColorDecorators::Always, DefaultColorDecorators::Auto] {
        reload(&workbench, false, mode, cx);
        settle(cx);
        assert_eq!(shown(&editor, cx), [], "{mode:?}");
    }
    reload(&workbench, true, DefaultColorDecorators::Auto, cx);
    settle(cx);
    assert_eq!(shown(&editor, cx), css_swatches());
}

#[gpui::test]
fn an_edit_rescans_after_the_rest(cx: &mut TestAppContext) {
    let text = "a {\n  color: #000000;\n}\n";
    let (_dir, _workbench, editor, cx) = shown_file(cx, "style.css", text);
    let first = swatch(text, "#000000", 0);
    assert_eq!(shown(&editor, cx), [first]);

    // A new declaration before the end of the block, typed in two goes.
    let at = text.find('}').unwrap();
    type_at(&editor, at, "  background: ", cx);
    let edited = "a {\n  color: #000000;\n  background: #00ff00;\n}\n";
    cx.executor().advance_clock(Duration::from_millis(300));
    draw(cx);
    type_at(&editor, at + "  background: ".len(), "#00ff00;\n", cx);
    assert_eq!(
        editor.read_with(cx, |view, _| view.model().text().to_string()),
        edited
    );

    // Half a second after the first edit, but not the last: nothing yet.
    cx.executor().advance_clock(Duration::from_millis(300));
    draw(cx);
    assert_eq!(shown(&editor, cx), [first]);
    cx.executor().advance_clock(Duration::from_millis(150));
    draw(cx);
    assert_eq!(shown(&editor, cx), [first]);

    // Half a second after the last.
    cx.executor().advance_clock(Duration::from_millis(60));
    draw(cx);
    assert_eq!(shown(&editor, cx), [first, swatch(edited, "#00ff00", 0)]);
}

#[gpui::test]
fn only_rows_near_the_view_are_scanned(cx: &mut TestAppContext) {
    const ROWS: usize = 2000;
    let mut text = String::new();
    for row in 0..ROWS {
        match row {
            0 => text.push_str(".top { color: #010203; }\n"),
            1900 => text.push_str(".far { color: #123456; }\n"),
            _ => text.push_str(&format!(".r{row} {{ margin: 0; }}\n")),
        }
    }
    let (_dir, _workbench, editor, cx) = shown_file(cx, "long.css", &text);
    let top = swatch(&text, "#010203", 0);
    assert_eq!(shown(&editor, cx), [top]);

    editor.update(cx, |view, cx| view.scroll_to_buffer_line(1890., cx));
    draw(cx);
    assert_eq!(shown(&editor, cx), [top], "not before the rest");
    cx.executor().advance_clock(REST);
    draw(cx);
    assert_eq!(shown(&editor, cx), [top, swatch(&text, "#123456", 0)]);
}

// With a server.

const COLORS: &str = "textDocument/documentColor";
/// A colour in a comment on row 0, which only the server finds, and one
/// in a string, which only Tenmoku does.
const MAIN: &str = "// #ff0000\nfn main() { let c = \"#00ff00\"; }\n";

fn script() -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        color_provider: Some(ColorProviderCapability::Simple(true)),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

const SERVER_RED: [f32; 4] = [1., 0., 0., 1.];

/// The server's answer: a colour over row 0's `#ff0000`.
fn server_colors() -> Value {
    let start = MAIN.find("#ff0000").unwrap();
    json!([{
        "range": {
            "start": { "line": 0, "character": start },
            "end": { "line": 0, "character": start + 7 },
        },
        "color": { "red": 1., "green": 0., "blue": 0., "alpha": 1. },
    }])
}

/// A trusted workbench with `mode`, showing `src/main.rs`, the server
/// answering `reply`.
fn setup(
    cx: &mut TestAppContext,
    mode: DefaultColorDecorators,
    reply: Value,
) -> (Lsp, Entity<EditorView>) {
    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    s.fake
        .set_reply(COLORS, Scripted::now(Reply::Result(reply)));
    let workbench = s.workbench.clone();
    let event = event(true, mode);
    workbench.update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
    s.set_trust(true);
    std::fs::write(s.path("src/main.rs"), MAIN).unwrap();
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    (s, editor)
}

fn shown_in(s: &Lsp, editor: &Entity<EditorView>) -> Vec<ColorSwatch> {
    editor.read_with(&s.cx, |view, _| view.color_swatches())
}

/// `until`, advancing both clocks on each poll.
fn until_ticking(s: &mut Lsp, what: &str, done: impl Fn(&mut Lsp) -> bool) {
    s.until(what, |s| {
        s.clock.advance(Duration::from_millis(20));
        s.cx.executor().advance_clock(Duration::from_millis(100));
        done(s)
    });
}

fn asked(s: &Lsp) -> usize {
    (s.fake.records().iter())
        .filter(|record| matches!(record, Record::Request { method, .. } if method == COLORS))
        .count()
}

/// Types a new row at the end of `main.rs`.
fn type_at_end(s: &mut Lsp, editor: &Entity<EditorView>) {
    editor.update(&mut s.cx, |view, cx| {
        let end = view.model().text().len();
        view.model_mut().set_cursor(end);
        view.model_mut().insert("\n");
        cx.notify();
    });
    s.draw();
}

#[gpui::test]
fn a_server_answer_replaces_them_in_auto(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, DefaultColorDecorators::Auto, json!([]));
    let built_in = vec![swatch(MAIN, "#00ff00", 0)];
    until_ticking(&mut s, "the built-in swatch", |s| {
        shown_in(s, &editor) == built_in
    });
    assert!(asked(&s) >= 1);

    // The server finds a colour: only its swatch shows.
    let server = vec![ColorSwatch {
        offset: MAIN.find("#ff0000").unwrap(),
        rgba: SERVER_RED,
    }];
    s.fake
        .set_reply(COLORS, Scripted::now(Reply::Result(server_colors())));
    type_at_end(&mut s, &editor);
    until_ticking(&mut s, "the server's swatch", |s| {
        shown_in(s, &editor) == server
    });

    // Then none: the built-in one is back.
    s.fake
        .set_reply(COLORS, Scripted::now(Reply::Result(json!([]))));
    type_at_end(&mut s, &editor);
    until_ticking(&mut s, "the built-in swatch again", |s| {
        shown_in(s, &editor) == built_in
    });
}

#[gpui::test]
fn always_ignores_the_server(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, DefaultColorDecorators::Always, server_colors());
    let built_in = vec![swatch(MAIN, "#00ff00", 0)];
    until_ticking(&mut s, "the built-in swatch", |s| {
        shown_in(s, &editor) == built_in
    });
    type_at_end(&mut s, &editor);
    for _ in 0..10 {
        s.clock.advance(Duration::from_millis(100));
        s.cx.executor().advance_clock(REST);
        s.draw();
    }
    assert_eq!(asked(&s), 0);
    assert_eq!(shown_in(&s, &editor), built_in);
}
