//! Document links through the fake server: asked for when a buffer opens,
//! handed to its editor, and opened by a cmd-click or `editor.open_link`:
//! files at their fragment's line, web addresses through the platform,
//! other schemes refused; a cmd-click off a link still goes to definition.

use std::{sync::Arc, time::Duration};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::{Settings, SettingsEvent};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Bounds, Entity, EntityInputHandler as _, Focusable as _, Modifiers, TestAppContext};
use lsp_types::{DocumentLinkOptions, PositionEncodingKind, ServerCapabilities};
use serde_json::{Value, json};

use crate::tests::lsp::Lsp;

const LINKS: &str = "textDocument/documentLink";
const RESOLVE: &str = "documentLink/resolve";
/// Row 0 has the links; `helper` is defined on row 1 and called on row 3.
const MAIN: &str = "// see https://a.example/x, lib.rs or ftp://h/f\nfn helper() {}\nfn main() {\n    helper();\n}\n";

fn script() -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        document_link_provider: Some(DocumentLinkOptions {
            resolve_provider: Some(true),
            work_done_progress_options: Default::default(),
        }),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

fn bytes_of(needle: &str) -> std::ops::Range<usize> {
    let start = MAIN.find(needle).unwrap();
    start..start + needle.len()
}

/// The LSP range of `needle`, on row 0, in UTF-8.
fn range_of(needle: &str) -> Value {
    let bytes = bytes_of(needle);
    json!({
        "start": { "line": 0, "character": bytes.start },
        "end": { "line": 0, "character": bytes.end },
    })
}

/// Row 0's links: the web address, `lib.rs` (to `src/lib.rs` at line 1,
/// column 5, or, unless `targeted`, to be resolved) and an `ftp:` one.
fn links(s: &Lsp, targeted: bool) -> Value {
    let lib = format!("{}#L1,5", s.uri("src/lib.rs"));
    let lib = if targeted {
        json!({ "range": range_of("lib.rs"), "target": lib })
    } else {
        json!({ "range": range_of("lib.rs"), "data": { "id": 7 } })
    };
    json!([
        { "range": range_of("https://a.example/x"), "target": "https://a.example/x" },
        lib,
        { "range": range_of("ftp://h/f"), "target": "ftp://h/f" },
    ])
}

/// A trusted workbench showing `src/main.rs`, its editor focused, the
/// links answered as `links` gives them, and, if `on`, shown.
fn setup(cx: &mut TestAppContext, targeted: bool, on: bool) -> (Lsp, Entity<EditorView>) {
    setup_with(cx, on, |s| Scripted::now(Reply::Result(links(s, targeted))))
}

/// As [`setup`], the links answered with what `reply` gives, and, if `on`,
/// waited for only if that is an answer of three links now.
fn setup_with(
    cx: &mut TestAppContext,
    on: bool,
    reply: impl FnOnce(&Lsp) -> Scripted,
) -> (Lsp, Entity<EditorView>) {
    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    let reply = reply(&s);
    let waits = reply.delay.is_zero()
        && matches!(&reply.reply, Reply::Result(Value::Array(links)) if links.len() == 3);
    s.fake.set_reply(LINKS, reply);
    if !on {
        set_links_setting(&mut s, false);
    }
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
    if on && waits {
        until_ticking(&mut s, "the links", |s| {
            editor.read_with(&s.cx, |view, _| view.link_ranges().len() == 3)
        });
    }
    (s, editor)
}

fn set_links_setting(s: &mut Lsp, on: bool) {
    let mut settings = Settings::default();
    settings.editor.links = on;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    let workbench = s.workbench.clone();
    workbench.update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
    s.draw();
}

/// `until`, advancing the manager's manual clock 20 ms per poll.
fn until_ticking(s: &mut Lsp, what: &str, done: impl Fn(&mut Lsp) -> bool) {
    s.until(what, |s| {
        s.clock.advance(Duration::from_millis(20));
        done(s)
    });
}

fn requests(s: &Lsp, method: &str) -> Vec<Value> {
    (s.fake.records().iter())
        .filter_map(|record| match record {
            Record::Request {
                method: m, params, ..
            } if m == method => Some(params.clone()),
            _ => None,
        })
        .collect()
}

fn toasts(s: &Lsp) -> Vec<String> {
    let layer = s.workbench.read_with(&s.cx, |w, _| w.toasts().clone());
    layer.read_with(&s.cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

/// Cmd-clicks just right of the caret position at `offset` in `editor`,
/// the pointer moved there with cmd held first, as a hand does.
fn cmd_click(s: &mut Lsp, editor: &Entity<EditorView>, offset: usize) {
    let position = editor.update_in(&mut s.cx, |editor, window, cx| {
        let caret = editor
            .bounds_for_range(offset..offset, Bounds::default(), window, cx)
            .expect("on screen");
        gpui::point(caret.left() + gpui::px(1.), caret.center().y)
    });
    s.cx.simulate_mouse_move(position, None, Modifiers::command());
    s.draw();
    s.cx.simulate_click(position, Modifiers::command());
    s.draw();
}

/// Cmd-clicks the right half of the glyph before byte `offset` in
/// `editor`, which puts the caret at `offset`.
fn cmd_click_before(s: &mut Lsp, editor: &Entity<EditorView>, offset: usize) {
    let position = editor.update_in(&mut s.cx, |editor, window, cx| {
        let caret = editor
            .bounds_for_range(offset..offset, Bounds::default(), window, cx)
            .expect("on screen");
        gpui::point(caret.left() - gpui::px(1.), caret.center().y)
    });
    s.cx.simulate_mouse_move(position, None, Modifiers::command());
    s.draw();
    s.cx.simulate_click(position, Modifiers::command());
    s.draw();
}

/// The active editor's file and its primary selection.
fn at(s: &Lsp) -> (std::path::PathBuf, std::ops::Range<usize>) {
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown");
    editor.read_with(&s.cx, |view, _| {
        let model = view.model();
        (
            model.path().unwrap().to_owned(),
            model.primary_selection().range.clone(),
        )
    })
}

#[gpui::test]
fn cmd_click_on_a_file_link_opens_it_at_the_fragment(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true, true);
    assert_eq!(
        editor.read_with(&s.cx, |view, _| view.link_ranges()),
        [
            bytes_of("https://a.example/x"),
            bytes_of("lib.rs"),
            bytes_of("ftp://h/f"),
        ]
    );
    let lib = s.path("src/lib.rs");
    cmd_click(&mut s, &editor, bytes_of("lib.rs").start + 2);
    // Line 1, column 5: `fn` in `pub fn f() {}`.
    s.until("lib.rs at its line", |s| at(s) == (lib.clone(), 4..4));
    assert_eq!(s.cx.opened_url(), None);
    assert!(toasts(&s).is_empty(), "{:?}", toasts(&s));
    assert!(requests(&s, RESOLVE).is_empty());
}

#[gpui::test]
fn a_missing_file_link_toasts(cx: &mut TestAppContext) {
    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    let target = s.uri("src/gone.rs").to_string();
    let answer = json!([{ "range": range_of("lib.rs"), "target": target }]);
    s.fake
        .set_reply(LINKS, Scripted::now(Reply::Result(answer)));
    s.set_trust(true);
    std::fs::write(s.path("src/main.rs"), MAIN).unwrap();
    s.open("src/main.rs");
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    until_ticking(&mut s, "the link", |s| {
        editor.read_with(&s.cx, |view, _| !view.link_ranges().is_empty())
    });
    cmd_click(&mut s, &editor, bytes_of("lib.rs").start + 2);
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(&s), ["Can't open gone.rs"]);
}

#[gpui::test]
fn cmd_click_on_a_url_opens_it_through_the_platform(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true, true);
    let main = s.path("src/main.rs");
    let url = bytes_of("https://a.example/x");
    cmd_click(&mut s, &editor, url.start + "https://a.ex".len());
    assert_eq!(s.cx.opened_url().as_deref(), Some("https://a.example/x"));
    // Nothing else opened.
    assert_eq!(at(&s).0, main);
    assert!(toasts(&s).is_empty(), "{:?}", toasts(&s));
}

#[gpui::test]
fn unsupported_scheme_toasts(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true, true);
    let main = s.path("src/main.rs");
    let ftp = bytes_of("ftp://h/f");
    cmd_click(&mut s, &editor, ftp.start + 1);
    s.until("the toast", |s| !toasts(s).is_empty());
    assert_eq!(toasts(&s), ["Can't open link: unsupported scheme"]);
    assert_eq!(s.cx.opened_url(), None);
    // The click put the caret there; no definition was gone to.
    assert_eq!(at(&s), (main, ftp.start + 1..ftp.start + 1));
}

#[gpui::test]
fn a_target_less_link_resolves_first(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, false, true);
    s.fake.set_reply(
        RESOLVE,
        Scripted::now(Reply::Result(json!({
            "range": range_of("lib.rs"),
            "target": format!("{}#L1,5", s.uri("src/lib.rs")),
            "data": { "id": 7 },
        }))),
    );
    let lib = s.path("src/lib.rs");
    cmd_click(&mut s, &editor, bytes_of("lib.rs").start + 2);
    until_ticking(&mut s, "lib.rs at its line", |s| {
        at(s) == (lib.clone(), 4..4)
    });
    let sent = requests(&s, RESOLVE);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["data"], json!({ "id": 7 }));
    assert_eq!(sent[0]["range"], range_of("lib.rs"));
    // The fragment's other spelling.
    assert_eq!(super::line_column("12,5"), Some((11, 4)));
    assert_eq!(super::line_column("L12"), Some((11, 0)));
    assert_eq!(super::line_column("L0"), None);
    assert_eq!(super::line_column("top"), None);
}

#[gpui::test]
fn cmd_click_off_a_link_still_goes_to_definition(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true, true);
    let main = s.path("src/main.rs");
    // `helper` in `helper();`: search finds its definition on row 1.
    let call = MAIN.rfind("helper").unwrap();
    cmd_click(&mut s, &editor, call + 2);
    let def = MAIN.find("helper").unwrap();
    s.until("the definition", |s| {
        at(s) == (main.clone(), def..def + "helper".len())
    });
    assert_eq!(s.cx.opened_url(), None);
    assert!(toasts(&s).is_empty(), "{:?}", toasts(&s));
}

#[gpui::test]
fn editor_links_off_asks_nothing(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true, false);
    for _ in 0..10 {
        s.clock.advance(Duration::from_millis(100));
        s.draw();
    }
    assert!(requests(&s, LINKS).is_empty());
    assert!(editor.read_with(&s.cx, |view, _| view.link_ranges().is_empty()));
    // A cmd-click on the address is go to definition's.
    cmd_click(&mut s, &editor, bytes_of("https://a.example/x").start + 2);
    assert_eq!(s.cx.opened_url(), None);

    // On, they're asked for; off again, they go.
    set_links_setting(&mut s, true);
    until_ticking(&mut s, "the links", |s| {
        editor.read_with(&s.cx, |view, _| view.link_ranges().len() == 3)
    });
    assert_eq!(requests(&s, LINKS).len(), 1);
    set_links_setting(&mut s, false);
    assert!(editor.read_with(&s.cx, |view, _| view.link_ranges().is_empty()));
}

#[gpui::test]
fn open_link_command_opens_the_link_at_the_caret(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true, true);
    let run = |s: &mut Lsp| {
        let context = ContextStack::new()
            .with(ContextFrame::new(crate::KEY_CONTEXT))
            .with(ContextFrame::new(fe_editor::commands::CONTEXT));
        let run = RunCommand(Invocation::of("editor.open_link").unwrap().context(context));
        s.cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
        s.draw();
    };
    // Just past its end is at it, as a caret left there by typing is.
    let url = bytes_of("https://a.example/x");
    editor.update(&mut s.cx, |view, _| view.model_mut().set_cursor(url.end));
    run(&mut s);
    assert_eq!(s.cx.opened_url().as_deref(), Some("https://a.example/x"));

    // Off a link, nothing.
    let main = s.path("src/main.rs");
    let call = MAIN.rfind("helper").unwrap();
    editor.update(&mut s.cx, |view, _| view.model_mut().set_cursor(call));
    run(&mut s);
    assert_eq!(at(&s), (main, call..call));
    assert!(toasts(&s).is_empty(), "{:?}", toasts(&s));
}

#[gpui::test]
fn a_click_counts_the_glyph_under_the_pointer_not_the_caret(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true, true);
    let main = s.path("src/main.rs");
    let url = bytes_of("https://a.example/x");
    // The right half of the space before it: the caret lands on the link's
    // start, but the click wasn't on it.
    cmd_click_before(&mut s, &editor, url.start);
    assert_eq!(s.cx.opened_url(), None);
    assert_eq!(at(&s).0, main);
    // The right half of its last glyph: the caret lands just past it.
    cmd_click_before(&mut s, &editor, url.end);
    assert_eq!(s.cx.opened_url().as_deref(), Some("https://a.example/x"));
}

#[gpui::test]
fn links_landing_under_a_still_pointer_underline_at_once(cx: &mut TestAppContext) {
    // Answered a second after it's asked, on the manager's clock.
    let (mut s, editor) = setup_with(cx, true, |s| Scripted {
        reply: Reply::Result(links(s, true)),
        delay: Duration::from_secs(1),
    });
    // The hover follows the modifier only in the active window.
    s.cx.update(|window, cx| {
        window.activate_window();
        window.focus(&editor.focus_handle(cx), cx);
    });
    s.draw();
    let url = bytes_of("https://a.example/x");
    let over = url.start + "https://a.ex".len();
    let position = editor.update_in(&mut s.cx, |editor, window, cx| {
        let caret = editor
            .bounds_for_range(over..over, Bounds::default(), window, cx)
            .expect("on screen");
        gpui::point(caret.left() + gpui::px(1.), caret.center().y)
    });
    s.cx.simulate_mouse_move(position, None, Modifiers::command());
    s.draw();
    let hover = |s: &Lsp| editor.read_with(&s.cx, |view, _| view.hover_range());
    let example = bytes_of("example");
    assert_eq!(
        hover(&s),
        Some(example.clone()),
        "the word, until they land"
    );
    s.clock.advance(Duration::from_secs(1));
    until_ticking(&mut s, "the whole link", |s| hover(s) == Some(url.clone()));
    // Taken away, the word again, with nothing moved.
    set_links_setting(&mut s, false);
    s.until("the word again", |s| hover(s) == Some(example.clone()));
}

#[gpui::test]
fn a_retry_asks_again(cx: &mut TestAppContext) {
    let (mut s, editor) = setup_with(cx, true, |_| {
        Scripted::now(Reply::Error {
            code: -32801,
            message: "content modified".into(),
        })
    });
    until_ticking(&mut s, "the first ask", |s| requests(s, LINKS).len() == 1);
    let answer = links(&s, true);
    s.fake
        .set_reply(LINKS, Scripted::now(Reply::Result(answer)));
    // Asked again after a rest, with no edit. The rest's timer is armed
    // only once the error reply has been handled, which on a loaded machine
    // can be after the first request is recorded, so the executor's clock
    // is advanced on every poll rather than once, up front.
    until_ticking(&mut s, "the links", |s| {
        s.cx.executor().advance_clock(Duration::from_millis(510));
        editor.read_with(&s.cx, |view, _| view.link_ranges().len() == 3)
    });
    assert_eq!(requests(&s, LINKS).len(), 2);
}

/// Has the server answer one link, over `needle` on row 0, to `target`,
/// and waits for it: an edit at the end of `main.rs` asks again.
fn retarget(s: &mut Lsp, editor: &Entity<EditorView>, needle: &str, target: String) {
    s.open("src/main.rs");
    let answer = json!([{ "range": range_of(needle), "target": target }]);
    s.fake
        .set_reply(LINKS, Scripted::now(Reply::Result(answer)));
    editor.update(&mut s.cx, |view, cx| {
        let end = view.model().text().len();
        view.model_mut().set_cursor(end);
        view.model_mut().insert("\n");
        cx.notify();
    });
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    s.cx.executor().advance_clock(Duration::from_millis(510));
    s.draw();
    let range = bytes_of(needle);
    until_ticking(s, "the new link", |s| {
        editor.read_with(&s.cx, |view, _| view.link_ranges() == vec![range.clone()])
    });
}

/// The active editor's file, and the char at its caret.
fn caret_char(s: &Lsp) -> (std::path::PathBuf, Option<char>) {
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("a tab is shown");
    editor.read_with(&s.cx, |view, _| {
        let model = view.model();
        let head = model.primary_selection().head();
        (model.path().unwrap().to_owned(), model.text().char_at(head))
    })
}

#[gpui::test]
fn fragment_columns_count_utf16_units_of_the_loaded_text(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true, true);
    // `é` is one unit and two bytes, `😀` two units and four.
    let lib = s.path("src/lib.rs");
    std::fs::write(&lib, "é x 😀y\n").unwrap();
    let target = format!("{}#L1,7", s.uri("src/lib.rs"));
    retarget(&mut s, &editor, "lib.rs", target);
    cmd_click(&mut s, &editor, bytes_of("lib.rs").start + 2);
    // Units é, space, x, space, 😀 (two), then `y`.
    s.until("lib.rs at the `y`", |s| {
        caret_char(s) == (lib.clone(), Some('y'))
    });

    // Unsaved edits count, not the file on disk.
    let lib_editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    lib_editor.update(&mut s.cx, |view, cx| {
        view.model_mut().set_cursor(0);
        view.model_mut().insert("😀😀");
        cx.notify();
    });
    s.draw();
    let target = format!("{}#L1,5", s.uri("src/lib.rs"));
    retarget(&mut s, &editor, "ftp://h/f", target);
    cmd_click(&mut s, &editor, bytes_of("ftp://h/f").start + 2);
    s.until("the unsaved `é`", |s| {
        caret_char(s) == (lib.clone(), Some('é'))
    });

    // A UTF-8 BOM the loader drops is no column.
    let bom = s.path("src/bom.rs");
    std::fs::write(&bom, "\u{feff}hello\n").unwrap();
    let target = format!("{}#L1,2", s.uri("src/bom.rs"));
    retarget(&mut s, &editor, "https://a.example/x", target);
    cmd_click(&mut s, &editor, bytes_of("https://a.example/x").start + 2);
    s.until("bom.rs at the `e`", |s| {
        caret_char(s) == (bom.clone(), Some('e'))
    });

    // CRLF rows, as the editor has them.
    let crlf = s.path("src/crlf.rs");
    std::fs::write(&crlf, "ab\r\ncd\r\n").unwrap();
    let target = format!("{}#L2,2", s.uri("src/crlf.rs"));
    retarget(&mut s, &editor, "see", target);
    cmd_click(&mut s, &editor, bytes_of("see").start + 1);
    s.until("crlf.rs at the `d`", |s| {
        caret_char(s) == (crlf.clone(), Some('d'))
    });
}
