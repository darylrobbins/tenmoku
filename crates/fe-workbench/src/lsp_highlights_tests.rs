//! Caret highlights through the fake server: its document highlights once
//! the caret has rested, dropped for an old caret, the word's occurrences
//! without a provider, and the active editor's alone.

use std::{fs, time::Duration};

use fe_editor::decorations::{DecorationKind, Tint};
use fe_gpui::EditorView;
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::{OneOf, PositionEncodingKind, ServerCapabilities};
use serde_json::json;

use crate::tests::lsp::Lsp;

const METHOD: &str = "textDocument/documentHighlight";
const REST: Duration = Duration::from_millis(250);

/// `foo` at 0, 8 and 20; `foobar` isn't one of them.
const TEXT: &str = "foo bar foo; foobar foo\n";

/// A rust server that provides document highlights if `provider`, and
/// answers a read of the second `foo` and a write of the third.
fn script(provider: bool, delay: Duration) -> (Script, Scripted) {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        document_highlight_provider: provider.then_some(OneOf::Left(true)),
        ..init.capabilities
    };
    let range = |from: u32, to: u32| {
        json!({
            "start": { "line": 0, "character": from },
            "end": { "line": 0, "character": to },
        })
    };
    let reply = json!([
        { "range": range(8, 11), "kind": 2 },
        { "range": range(20, 23), "kind": 3 },
    ]);
    let script = Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    };
    (
        script,
        Scripted {
            reply: Reply::Result(reply),
            delay,
        },
    )
}

/// The workbench with `src/main.rs` (as TEXT) open on its server and its
/// editor focused, the caret at the start.
fn setup(cx: &mut TestAppContext, provider: bool, delay: Duration) -> (Lsp, Entity<EditorView>) {
    let (rust, reply) = script(provider, delay);
    let mut s = Lsp::with_fake_clock(cx, rust, Script::default());
    s.fake.set_reply(METHOD, reply);
    s.set_trust(true);
    fs::write(s.path("src/main.rs"), TEXT).unwrap();
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = s
        .workbench
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    (s, editor)
}

fn move_caret(s: &mut Lsp, editor: &Entity<EditorView>, offset: usize) {
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(offset);
            cx.notify();
        })
    });
    s.draw();
}

/// The highlights `editor` shows: range and tint, by start.
fn shown(s: &mut Lsp, editor: &Entity<EditorView>) -> Vec<(std::ops::Range<usize>, Tint)> {
    editor.read_with(&s.cx, |view, _| {
        let model = view.model();
        let all = model.decorations(0..model.text().len());
        (all.into_iter())
            .filter(|item| item.kind == DecorationKind::Highlight)
            .map(|item| (item.range, item.tint))
            .collect()
    })
}

fn is_highlight(record: &Record) -> bool {
    matches!(record, Record::Request { method, .. } if method == METHOD)
}

fn requests(s: &Lsp) -> usize {
    (s.fake.records().iter())
        .filter(|record| is_highlight(record))
        .count()
}

fn rest(s: &mut Lsp) {
    s.cx.executor()
        .advance_clock(REST + Duration::from_millis(10));
    s.draw();
}

#[gpui::test]
fn server_highlights_show_where_the_caret_rests(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true, Duration::ZERO);

    // Two moves inside the wait, to other words: one request, after it.
    move_caret(&mut s, &editor, 5);
    move_caret(&mut s, &editor, 1);
    assert_eq!(requests(&s), 0);
    assert_eq!(shown(&mut s, &editor), vec![]);
    rest(&mut s);
    s.until("the highlights", |s| !shown(s, &editor).is_empty());
    // A read and a write alike: the identifier highlight (T1550).
    assert_eq!(
        shown(&mut s, &editor),
        vec![(8..11, Tint::Muted), (20..23, Tint::Muted)]
    );
    assert_eq!(requests(&s), 1);
    s.draw();
    let (drawn, fill) = editor.read_with(&s.cx, |editor, _| {
        let drawn = editor
            .drawn_annotations()
            .expect("drawn")
            .highlights
            .clone();
        (drawn, editor.theme().identifier_highlight)
    });
    let colors: Vec<_> = drawn.iter().map(|&(_, color)| color).collect();
    assert_eq!(colors, [fill, fill]);

    // Off the symbol, they're gone before the next answer.
    move_caret(&mut s, &editor, 5);
    assert_eq!(shown(&mut s, &editor), vec![]);
}

#[gpui::test]
fn a_late_answer_for_an_old_caret_is_dropped(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true, Duration::from_secs(1));
    move_caret(&mut s, &editor, 1);
    rest(&mut s);
    s.fake
        .wait_for("the request", |records| records.iter().any(is_highlight));

    // The caret moves to another word before the answer lands.
    move_caret(&mut s, &editor, 5);
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_secs(1));
    s.draw();
    assert_eq!(shown(&mut s, &editor), vec![]);
}

#[gpui::test]
fn words_highlight_when_the_server_has_no_provider(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, false, Duration::ZERO);
    move_caret(&mut s, &editor, 1);
    rest(&mut s);
    s.until("the words", |s| !shown(s, &editor).is_empty());
    assert_eq!(
        shown(&mut s, &editor),
        vec![
            (0..3, Tint::Muted),
            (8..11, Tint::Muted),
            (20..23, Tint::Muted)
        ]
    );
    assert_eq!(requests(&s), 0);
}

#[gpui::test]
fn only_the_active_editor_highlights(cx: &mut TestAppContext) {
    let (mut s, first) = setup(cx, false, Duration::ZERO);
    move_caret(&mut s, &first, 1);
    rest(&mut s);
    s.until("the words", |s| !shown(s, &first).is_empty());

    // A split of the file becomes active: the first's are cleared, and
    // only the split's show once its caret has rested.
    s.run("pane.split_right");
    let second = s
        .workbench
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    assert_ne!(second, first);
    assert_eq!(shown(&mut s, &first), vec![]);
    move_caret(&mut s, &second, 1);
    rest(&mut s);
    s.until("the split's words", |s| !shown(s, &second).is_empty());
    assert_eq!(shown(&mut s, &second).len(), 3);
    assert_eq!(shown(&mut s, &first), vec![]);

    // Another file in the active pane: the split's are cleared too.
    s.open("src/lib.rs");
    assert_eq!(shown(&mut s, &second), vec![]);
    assert_eq!(shown(&mut s, &first), vec![]);
}

#[gpui::test]
fn a_move_within_the_word_waits_again(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true, Duration::ZERO);
    move_caret(&mut s, &editor, 1);
    s.cx.executor().advance_clock(Duration::from_millis(200));
    s.draw();
    move_caret(&mut s, &editor, 2);
    s.cx.executor().advance_clock(Duration::from_millis(100));
    s.draw();
    assert_eq!(requests(&s), 0);
    rest(&mut s);
    s.until("the request", |s| requests(s) == 1);
}

#[gpui::test]
fn an_edit_clears_the_words_until_they_are_found_again(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, false, Duration::ZERO);
    move_caret(&mut s, &editor, 1);
    rest(&mut s);
    s.until("the words", |s| !shown(s, &editor).is_empty());
    s.cx.update(|_, cx| {
        editor.update(cx, |view, cx| {
            view.model_mut().set_cursor(24);
            view.model_mut().insert("z");
            view.model_mut().set_cursor(1);
            cx.notify();
        })
    });
    s.draw();
    assert_eq!(shown(&mut s, &editor), vec![]);
    rest(&mut s);
    s.until("the words again", |s| shown(s, &editor).len() == 3);
}

#[gpui::test]
fn words_highlight_a_buffer_no_server_has(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx, true, Duration::ZERO);
    fs::write(s.path("notes.txt"), TEXT).unwrap();
    s.open("notes.txt");
    let editor = s
        .workbench
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    move_caret(&mut s, &editor, 1);
    rest(&mut s);
    s.until("the words", |s| shown(s, &editor).len() == 3);
    assert_eq!(requests(&s), 0);
}

#[gpui::test]
fn a_move_within_a_highlighted_word_keeps_the_words(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, false, Duration::ZERO);
    move_caret(&mut s, &editor, 1);
    rest(&mut s);
    s.until("the words", |s| shown(s, &editor).len() == 3);
    move_caret(&mut s, &editor, 2);
    assert_eq!(shown(&mut s, &editor).len(), 3);
    move_caret(&mut s, &editor, 9);
    assert_eq!(shown(&mut s, &editor).len(), 3);
}
