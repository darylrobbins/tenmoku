//! Linked editing through the fake server: the pair linked once the caret
//! has rested in a tag's name, typing renaming it, no request with the
//! setting off, a caret outside the ranges, an answer for an old caret
//! dropped, and moving away or the setting off clearing the link.

use std::{fs, ops::Range, time::Duration};

use fe_gpui::EditorView;
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{Entity, Focusable as _, TestAppContext};
use lsp_types::{LinkedEditingRangeServerCapabilities, PositionEncodingKind, ServerCapabilities};
use serde_json::json;

use crate::tests::lsp::Lsp;

const METHOD: &str = "textDocument/linkedEditingRange";
const REST: Duration = Duration::from_millis(250);

/// The tag's name is at 1..4 and 7..10.
const TEXT: &str = "<div></div>\n";

/// A server that links the open and close tag names.
fn script(delay: Duration) -> (Script, Scripted) {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        linked_editing_range_provider: Some(LinkedEditingRangeServerCapabilities::Simple(true)),
        ..init.capabilities
    };
    let range = |from: u32, to: u32| {
        json!({
            "start": { "line": 0, "character": from },
            "end": { "line": 0, "character": to },
        })
    };
    let reply = json!({ "ranges": [range(1, 4), range(7, 10)] });
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
/// editor focused, the caret at the start, `editor.linked_editing` as
/// `on`.
fn setup(cx: &mut TestAppContext, on: bool, delay: Duration) -> (Lsp, Entity<EditorView>) {
    let (rust, reply) = script(delay);
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
    set_setting(&mut s, on);
    (s, editor)
}

fn set_setting(s: &mut Lsp, on: bool) {
    s.workbench
        .read_with(&s.cx, |w, _| w.lsp.linked_editing_on.set(on));
    s.draw();
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

fn linked(s: &Lsp, editor: &Entity<EditorView>) -> Option<Vec<Range<usize>>> {
    editor.read_with(&s.cx, |view, _| {
        view.model().linked_ranges().map(<[_]>::to_vec)
    })
}

fn text(s: &Lsp, editor: &Entity<EditorView>) -> String {
    editor.read_with(&s.cx, |view, _| view.model().text().to_string())
}

fn is_request(record: &Record) -> bool {
    matches!(record, Record::Request { method, .. } if method == METHOD)
}

fn requests(s: &Lsp) -> usize {
    (s.fake.records().iter())
        .filter(|record| is_request(record))
        .count()
}

fn rest(s: &mut Lsp) {
    s.cx.executor()
        .advance_clock(REST + Duration::from_millis(10));
    s.draw();
}

#[gpui::test]
fn typing_in_the_open_tag_renames_the_close_tag(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true, Duration::ZERO);

    // Nothing is linked before the caret has rested.
    move_caret(&mut s, &editor, 4);
    assert_eq!(requests(&s), 0);
    assert_eq!(linked(&s, &editor), None);
    rest(&mut s);
    s.until("the link", |s| linked(s, &editor).is_some());
    assert_eq!(linked(&s, &editor), Some(vec![1..4, 7..10]));
    assert_eq!(requests(&s), 1);

    editor.update(&mut s.cx, |view, _| view.model_mut().insert("x"));
    assert_eq!(text(&s, &editor), "<divx></divx>\n");
    // Linked ranges in the linked range aren't asked for again.
    rest(&mut s);
    assert_eq!(requests(&s), 1);
    assert_eq!(linked(&s, &editor), Some(vec![1..5, 8..12]));
}

#[gpui::test]
fn setting_off_sends_no_request(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, false, Duration::ZERO);
    move_caret(&mut s, &editor, 2);
    rest(&mut s);
    rest(&mut s);
    assert_eq!(requests(&s), 0);
    assert_eq!(linked(&s, &editor), None);
}

#[gpui::test]
fn caret_outside_the_ranges_links_nothing(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true, Duration::ZERO);
    // Before the `<`: the server answers, but no range holds the caret.
    move_caret(&mut s, &editor, 5);
    move_caret(&mut s, &editor, 0);
    rest(&mut s);
    s.fake
        .wait_for("the request", |records| records.iter().any(is_request));
    s.draw();
    assert_eq!(linked(&s, &editor), None);
    assert_eq!(requests(&s), 1);
}

#[gpui::test]
fn old_caret_answer_is_dropped(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true, Duration::from_secs(1));
    move_caret(&mut s, &editor, 2);
    rest(&mut s);
    s.fake
        .wait_for("the request", |records| records.iter().any(is_request));

    // The caret moves before the answer lands.
    move_caret(&mut s, &editor, 3);
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_secs(1));
    s.draw();
    assert_eq!(linked(&s, &editor), None);
}

#[gpui::test]
fn moving_away_clears_the_link(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, true, Duration::ZERO);
    move_caret(&mut s, &editor, 2);
    rest(&mut s);
    s.until("the link", |s| linked(s, &editor).is_some());

    // To the close tag's name: still linked.
    move_caret(&mut s, &editor, 8);
    assert!(linked(&s, &editor).is_some());

    // Out of the names: cleared.
    move_caret(&mut s, &editor, 0);
    assert_eq!(linked(&s, &editor), None);

    // Linked again, then the setting off clears it.
    move_caret(&mut s, &editor, 2);
    rest(&mut s);
    s.until("the link again", |s| linked(s, &editor).is_some());
    set_setting(&mut s, false);
    assert_eq!(linked(&s, &editor), None);
}
