//! A file link's `#line,col` landing on its column: in a viewer, for
//! overlapping opens in two panes, behind a Save As and when the column is
//! oversized.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, FileViewer, commands::RunCommand};
use fe_lsp::fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result};
use gpui::{
    Bounds, Entity, EntityInputHandler as _, Focusable as _, Modifiers, TestAppContext,
    VisualTestContext,
};
use lsp_types::{DocumentLinkOptions, PositionEncodingKind, ServerCapabilities};
use serde_json::{Value, json};

use crate::{Launch, Workbench, loader::test_support, pane::Pane, tests::lsp::Lsp, tests::open};

/// The lowered large-file threshold.
const THRESHOLD: u64 = 64 * 1024;

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A project of `files`, a workbench on it, and the folder.
fn project<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
) -> (
    tempfile::TempDir,
    PathBuf,
    Entity<Workbench>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    for (name, text) in files {
        std::fs::write(root.join(name), text).unwrap();
    }
    let launch = Launch {
        root: Some(root.clone()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (dir, root, workbench, cx)
}

/// Follows a link to `path` at `fragment` in the active pane.
fn follow(workbench: &Entity<Workbench>, path: &Path, fragment: &str, cx: &mut VisualTestContext) {
    let path = path.to_owned();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.open_link_file(path, Some(fragment), window, cx)
        })
    });
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// Draws until `done`, a viewer's index being polled on the clock.
fn until(what: &str, cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.executor().advance_clock(Duration::from_millis(200));
        draw(cx);
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The editor of `pane`'s tab for `path`, and its caret.
fn caret(pane: &Entity<Pane>, path: &Path, cx: &mut VisualTestContext) -> Option<usize> {
    pane.read_with(cx, |pane, cx| {
        let editor: &Entity<EditorView> = (pane.tabs().iter())
            .filter_map(|tab| tab.editor())
            .find(|editor| editor.read(cx).model().path() == Some(path))?;
        Some(editor.read(cx).model().primary_selection().head())
    })
}

#[gpui::test]
fn link_column_lands_in_viewer(cx: &mut TestAppContext) {
    // `é` is one unit and two bytes, `😀` two units and four: the `y` is
    // the 7th unit and the 9th byte of the last row, far past the first
    // chunk of the index.
    let rows = (2 * fe_fileview::CHUNK_BYTES as usize) / 12 + 100;
    let mut text: String = (0..rows).map(|row| format!("line {row:06}\n")).collect();
    let last_row = rows as u32;
    let start = text.len();
    text.push_str("é x 😀y\n");
    let (_dir, root, workbench, cx) = project(cx, &[("big.rs", &text)]);
    let path = root.join("big.rs");
    test_support::lower_threshold(&root, THRESHOLD, true);

    follow(&workbench, &path, &format!("L{},7", last_row + 1), cx);
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let viewer: Entity<FileViewer> = pane
        .read_with(cx, |pane, _| pane.tabs()[pane.active()?].viewer().cloned())
        .expect("a viewer is shown");
    // Its index holds, the row not found yet; then it finishes.
    drop(test_support::take_steps(&path));
    until("the caret on the `y`", cx, |cx| {
        viewer.read_with(cx, |viewer, _| {
            viewer.model().selection() == (start + 9..start + 9)
        })
    });
}

#[gpui::test]
fn overlapping_opens_keep_their_landings(cx: &mut TestAppContext) {
    let (_dir, root, workbench, cx) = project(
        cx,
        &[("a.rs", "ab\ncd\n"), ("b.rs", "xy\nzw\n"), ("c.rs", "\n")],
    );
    follow(&workbench, &root.join("c.rs"), "L1", cx);
    draw(cx);
    run("pane.split_right", cx);
    let panes = workbench.read_with(cx, |workbench, cx| workbench.panes(cx));
    assert_eq!(panes.len(), 2);
    let (first, second) = (panes[0].clone(), panes[1].clone());

    // Both opens start before either file is read.
    for pane in [&first, &second] {
        cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.activate(pane.clone(), window, cx)
            })
        });
        let name = if *pane == first { "a.rs" } else { "b.rs" };
        follow(&workbench, &root.join(name), "L2,2", cx);
    }
    draw(cx);
    // The `d`, and the `w`.
    assert_eq!(caret(&first, &root.join("a.rs"), cx), Some(4));
    assert_eq!(caret(&second, &root.join("b.rs"), cx), Some(4));
}

#[gpui::test]
fn link_survives_save_as_reservation(cx: &mut TestAppContext) {
    let (_dir, root, workbench, cx) = project(cx, &[("a.rs", "ab\ncd\n")]);
    let path = root.join("a.rs");
    let saving = crate::open::reserve(
        fe_buffer::Buffer::new("").id(),
        path.clone(),
        crate::open::file_identity(&path),
    );
    follow(&workbench, &path, "L2,2", cx);
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_eq!(caret(&pane, &path, cx), None, "waiting behind the Save As");
    drop(saving);
    draw(cx);
    assert_eq!(caret(&pane, &path, cx), Some(4));
}

#[gpui::test]
fn oversized_fragment_column_is_clamped(cx: &mut TestAppContext) {
    let (_dir, root, workbench, cx) = project(cx, &[("a.rs", "abc\ndef\n")]);
    let path = root.join("a.rs");
    // 2^32 + 2: truncated to 32 bits it would be the column of the `b`.
    follow(&workbench, &path, "L1,4294967298", cx);
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_eq!(caret(&pane, &path, cx), Some(3), "the end of the row");
    // Past a `u64`'s digits as well.
    follow(&workbench, &path, "L2,99999999999999999999999", cx);
    draw(cx);
    assert_eq!(caret(&pane, &path, cx), Some(7));
}

/// A large project file of `rows` rows `line 000000`, then `tail`.
fn large_text(tail: &str) -> (String, usize, u32) {
    let rows = (2 * fe_fileview::CHUNK_BYTES as usize) / 12 + 100;
    let mut text: String = (0..rows).map(|row| format!("line {row:06}\n")).collect();
    let start = text.len();
    text.push_str(tail);
    (text, start, rows as u32)
}

#[gpui::test]
fn viewer_waits_out_the_placeholder_row(cx: &mut TestAppContext) {
    let (text, _, _) = large_text("");
    let (_dir, root, workbench, cx) = project(cx, &[("big.rs", &text)]);
    let path = root.join("big.rs");
    test_support::lower_threshold(&root, THRESHOLD, true);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.clone(), window, cx)));
    draw(cx);
    let viewer: Entity<FileViewer> = pane
        .read_with(cx, |pane, _| pane.tabs()[pane.active()?].viewer().cloned())
        .expect("a viewer is shown");
    // One chunk indexed: the last row it has is a placeholder.
    let mut steps = test_support::take_steps(&path);
    let mut step = steps.pop().expect("a held index");
    step.next().expect("the index is building");
    cx.executor().advance_clock(Duration::from_millis(200));
    draw(cx);
    let rows = viewer.read_with(cx, |viewer, _| viewer.model().text().row_count());
    assert!(viewer.read_with(cx, |viewer, _| viewer.is_polling()));

    // Its column is 6: five units along `line 0000nn`.
    follow(&workbench, &path, &format!("L{rows},6"), cx);
    draw(cx);
    drop(step);
    until("the caret five units along", cx, |cx| {
        viewer.read_with(cx, |viewer, _| {
            let text = viewer.model().text();
            let at = viewer.model().selection();
            !viewer.is_polling()
                && text.row_count() > rows
                && at == (text.row_start(rows - 1) + 5..text.row_start(rows - 1) + 5)
        })
    });
}

#[gpui::test]
fn a_later_open_of_the_same_file_cancels_the_landing(cx: &mut TestAppContext) {
    let (_dir, root, workbench, cx) = project(cx, &[("a.rs", "ab\ncd\n")]);
    let path = root.join("a.rs");
    // The second open supersedes the first while the file is read.
    follow(&workbench, &path, "L2,2", cx);
    follow(&workbench, &path, "L1", cx);
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert_eq!(caret(&pane, &path, cx), Some(0));
}

#[gpui::test]
fn a_queued_viewer_landing_checks_the_open_again(cx: &mut TestAppContext) {
    let (text, _, _) = large_text("");
    let (_dir, root, workbench, cx) = project(cx, &[("big.rs", &text), ("b.rs", "b\n")]);
    let path = root.join("big.rs");
    test_support::lower_threshold(&root, THRESHOLD, false);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.clone(), window, cx)));
    let viewer: Entity<FileViewer> = {
        draw(cx);
        pane.read_with(cx, |pane, _| pane.tabs()[pane.active()?].viewer().cloned())
            .expect("a viewer is shown")
    };
    until("the index", cx, |cx| {
        viewer.read_with(cx, |viewer, _| !viewer.is_polling())
    });

    // The landing queues its reopen; another open comes before it runs.
    let other = root.join("b.rs");
    cx.update(|window, cx| {
        let handle = window.window_handle();
        workbench.update(cx, |workbench, cx| {
            workbench.land_link_column(&pane, handle, path.clone(), (0, 5), cx)
        });
        pane.update(cx, |pane, cx| pane.open(other.clone(), window, cx));
    });
    draw(cx);
    let active = pane.read_with(cx, |pane, _| {
        pane.tabs()[pane.active().unwrap()].path.clone()
    });
    assert_eq!(
        active,
        Some(other),
        "the stale landing doesn't take the tab back"
    );
}

#[gpui::test]
fn independent_open_after_reservation_release_cancels_landing(cx: &mut TestAppContext) {
    let (_dir, root, workbench, cx) = project(cx, &[("a.rs", "ab\ncd\n")]);
    let path = root.join("a.rs");
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    // The target is open already.
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.clone(), window, cx)));
    draw(cx);
    let saving = crate::open::reserve(
        fe_buffer::Buffer::new("").id(),
        path.clone(),
        crate::open::file_identity(&path),
    );
    follow(&workbench, &path, "L2,2", cx);
    draw(cx);
    // The Save As is over; before the link's open continues, another open
    // of the file comes, at the first row.
    drop(saving);
    cx.update(|window, cx| {
        let reveal = crate::pane::Reveal {
            row: 0,
            columns: 0..0,
            within_row: true,
        };
        pane.update(cx, |pane, cx| {
            pane.open_at(path.clone(), Some(reveal), window, cx)
        });
    });
    draw(cx);
    assert_ne!(caret(&pane, &path, cx), Some(4), "the old column landed");
}

#[gpui::test]
fn delayed_landing_keeps_focus(cx: &mut TestAppContext) {
    let (text, start, last) = large_text("é x 😀y\n");
    let (_dir, root, workbench, cx) = project(cx, &[("big.rs", &text)]);
    let path = root.join("big.rs");
    test_support::lower_threshold(&root, THRESHOLD, true);
    follow(&workbench, &path, &format!("L{},7", last + 1), cx);
    draw(cx);
    let first = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let viewer: Entity<FileViewer> = first
        .read_with(cx, |pane, _| pane.tabs()[pane.active()?].viewer().cloned())
        .expect("a viewer is shown");
    // The index holds while another pane takes the focus.
    let steps = test_support::take_steps(&path);
    run("pane.split_right", cx);
    let panes = workbench.read_with(cx, |workbench, cx| workbench.panes(cx));
    let second = panes.iter().find(|pane| **pane != first).unwrap().clone();
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.activate(second.clone(), window, cx)
        })
    });
    draw(cx);
    let focused = |pane: &Entity<Pane>, cx: &mut VisualTestContext| {
        cx.update(|window, cx| pane.focus_handle(cx).contains_focused(window, cx))
    };
    assert!(focused(&second, cx) && !focused(&first, cx));
    drop(steps);
    until("the caret on the `y`", cx, |cx| {
        viewer.read_with(cx, |viewer, _| {
            viewer.model().selection() == (start + 9..start + 9)
        })
    });
    assert!(focused(&second, cx), "the landing took the focus");
    assert!(!focused(&first, cx));
}

#[gpui::test]
fn oversized_column_on_long_row_is_bounded(cx: &mut TestAppContext) {
    // One row of 1 MiB, past several chunks of the index.
    let tail = "x".repeat(1 << 20);
    let (text, start, last) = large_text(&format!("{tail}\n"));
    let (_dir, root, workbench, cx) = project(cx, &[("big.rs", &text)]);
    let path = root.join("big.rs");
    test_support::lower_threshold(&root, THRESHOLD, true);
    follow(&workbench, &path, &format!("L{},4294967295", last + 1), cx);
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let viewer: Entity<FileViewer> = pane
        .read_with(cx, |pane, _| pane.tabs()[pane.active()?].viewer().cloned())
        .expect("a viewer is shown");
    drop(test_support::take_steps(&path));
    // The end of the row, found chunk by chunk.
    until("the caret at the end of the row", cx, |cx| {
        viewer.read_with(cx, |viewer, _| {
            viewer.model().selection() == (start + tail.len()..start + tail.len())
        })
    });
}

#[test]
fn utf16_columns_are_read_over_chunks() {
    let chunks = || {
        ["a😀", "é", "b"]
            .into_iter()
            .map(std::borrow::Cow::Borrowed)
    };
    assert_eq!(super::utf16_to_byte_chunks(chunks(), 0), 0);
    assert_eq!(super::utf16_to_byte_chunks(chunks(), 1), 1);
    // Inside a surrogate pair: before it.
    assert_eq!(super::utf16_to_byte_chunks(chunks(), 2), 1);
    assert_eq!(super::utf16_to_byte_chunks(chunks(), 3), 5);
    assert_eq!(super::utf16_to_byte_chunks(chunks(), 4), 7);
    assert_eq!(super::utf16_to_byte_chunks(chunks(), u32::MAX), 8);
}

const LINKS: &str = "textDocument/documentLink";
const RESOLVE: &str = "documentLink/resolve";
const MAIN: &str = "// see lib.rs or ftp://h/f\nfn main() {}\n";

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

fn range_of(needle: &str) -> Value {
    let bytes = bytes_of(needle);
    json!({
        "start": { "line": 0, "character": bytes.start },
        "end": { "line": 0, "character": bytes.end },
    })
}

/// A workbench on `src/main.rs`, whose `lib.rs` link has no target (the
/// server resolves it to `src/lib.rs`, after `resolve_delay`) and whose
/// `ftp:` one has; the links shown.
fn setup(cx: &mut TestAppContext, resolve_delay: Duration) -> (Lsp, Entity<EditorView>) {
    let mut s = Lsp::with_fake_clock(cx, script(), Script::default());
    let answer = json!([
        { "range": range_of("lib.rs"), "data": { "id": 7 } },
        { "range": range_of("ftp://h/f"), "target": "ftp://h/f" },
        { "range": range_of("see"), "target": s.uri("src/lib.rs").to_string() },
    ]);
    s.fake
        .set_reply(LINKS, Scripted::now(Reply::Result(answer)));
    let resolved = json!({
        "range": range_of("lib.rs"),
        "target": s.uri("src/lib.rs").to_string(),
        "data": { "id": 7 },
    });
    s.fake.set_reply(
        RESOLVE,
        Scripted {
            reply: Reply::Result(resolved),
            delay: resolve_delay,
        },
    );
    s.set_trust(true);
    std::fs::write(s.path("src/main.rs"), MAIN).unwrap();
    std::fs::write(s.path("src/other.rs"), "other\n").unwrap();
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didOpen", |r| r.is_some_and(|r| r.open));
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .expect("main.rs is shown");
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    until_ticking(&mut s, "the links", |s| {
        editor.read_with(&s.cx, |view, _| view.link_ranges().len() == 3)
    });
    (s, editor)
}

fn until_ticking(s: &mut Lsp, what: &str, done: impl Fn(&mut Lsp) -> bool) {
    s.until(what, |s| {
        s.clock.advance(Duration::from_millis(20));
        done(s)
    });
}

fn requests(s: &Lsp, method: &str) -> usize {
    (s.fake.records().iter())
        .filter(|record| matches!(record, Record::Request { method: m, .. } if m == method))
        .count()
}

fn toasts(s: &Lsp) -> Vec<String> {
    let layer = s.workbench.read_with(&s.cx, |w, _| w.toasts().clone());
    layer.read_with(&s.cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

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

/// Types at the end of `editor`.
fn type_at_end(s: &mut Lsp, editor: &Entity<EditorView>) {
    editor.update(&mut s.cx, |view, cx| {
        let end = view.model().text().len();
        view.model_mut().set_cursor(end);
        view.model_mut().insert("// x\n");
        cx.notify();
    });
    s.draw();
    // The server has the text now; the links' rest isn't over.
    let main = s.uri("src/main.rs");
    s.fake
        .wait_for_document(&main, "didChange", |r| r.is_some());
    s.clock.advance(Duration::from_millis(100));
    s.draw();
}

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

fn open_link_command(s: &mut Lsp) {
    let context = ContextStack::new()
        .with(ContextFrame::new(crate::KEY_CONTEXT))
        .with(ContextFrame::new(fe_editor::commands::CONTEXT));
    let run = RunCommand(Invocation::of("editor.open_link").unwrap().context(context));
    s.cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    s.draw();
}

#[gpui::test]
fn a_link_clicked_right_after_an_edit_opens_its_target(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, Duration::ZERO);
    let lib = s.path("src/lib.rs");
    let main = s.path("src/main.rs");
    let asked = requests(&s, LINKS);

    // Before the rest after the edit is over.
    type_at_end(&mut s, &editor);
    cmd_click(&mut s, &editor, bytes_of("lib.rs").start + 2);
    until_ticking(&mut s, "lib.rs", |s| at(s).0 == lib);
    assert!(requests(&s, LINKS) > asked, "asked at the text now");
    assert!(toasts(&s).is_empty(), "{:?}", toasts(&s));

    // And Open Link, from the caret.
    s.open("src/main.rs");
    assert_eq!(at(&s).0, main);
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    type_at_end(&mut s, &editor);
    let caret = bytes_of("lib.rs").start + 2;
    editor.update(&mut s.cx, |view, _| view.model_mut().set_cursor(caret));
    open_link_command(&mut s);
    until_ticking(&mut s, "lib.rs again", |s| at(s).0 == lib);
    assert!(toasts(&s).is_empty(), "{:?}", toasts(&s));
    assert_eq!(requests(&s, RESOLVE), 2);
}

#[gpui::test]
fn a_superseded_link_open_is_dropped(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, Duration::from_secs(1));
    let other = s.path("src/other.rs");

    // Resolving when another file takes the editor's place.
    cmd_click(&mut s, &editor, bytes_of("lib.rs").start + 2);
    s.open("src/other.rs");
    assert_eq!(at(&s).0, other);
    s.clock.advance(Duration::from_secs(2));
    for _ in 0..20 {
        s.clock.advance(Duration::from_millis(100));
        s.draw();
    }
    assert_eq!(requests(&s, RESOLVE), 1);
    assert_eq!(at(&s).0, other, "the link opened nothing");

    // A file link whose existence check an open that came after overtakes:
    // both start before either is looked at.
    s.open("src/main.rs");
    let editor = (s.workbench)
        .read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())
        .unwrap();
    s.cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    s.draw();
    let main = s.path("src/main.rs");
    let see = bytes_of("see").start + 1;
    let ftp = bytes_of("ftp://h/f").start + 1;
    s.cx.update(|window, cx| {
        s.workbench.update(cx, |workbench, cx| {
            workbench.open_link_at(&editor, see, false, window, cx);
            // A newer open starts, before the file is looked for.
            workbench.open_link_at(&editor, ftp, false, window, cx);
        })
    });
    s.draw();
    for _ in 0..20 {
        s.clock.advance(Duration::from_millis(100));
        s.draw();
    }
    assert_eq!(at(&s).0, main, "the older check opened nothing");
}

#[gpui::test]
fn a_link_open_overtaken_by_navigation_in_its_editor_is_dropped(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, Duration::from_secs(1));
    let main = s.path("src/main.rs");
    let wait = |s: &mut Lsp| {
        s.clock.advance(Duration::from_secs(2));
        for _ in 0..20 {
            s.clock.advance(Duration::from_millis(100));
            s.draw();
        }
    };

    // The caret goes elsewhere in the file while the link resolves.
    cmd_click(&mut s, &editor, bytes_of("lib.rs").start + 2);
    editor.update(&mut s.cx, |view, _| view.model_mut().set_cursor(0));
    s.draw();
    wait(&mut s);
    assert_eq!(requests(&s, RESOLVE), 1);
    assert_eq!(at(&s), (main.clone(), 0..0), "the link opened nothing");

    // Another file is shown, then this one again.
    cmd_click(&mut s, &editor, bytes_of("lib.rs").start + 2);
    let caret = at(&s);
    s.open("src/other.rs");
    s.draw();
    s.open("src/main.rs");
    s.draw();
    assert_eq!(at(&s).0, main);
    wait(&mut s);
    assert_eq!(requests(&s, RESOLVE), 2);
    assert_eq!(at(&s), caret, "the link opened nothing");
}

#[gpui::test]
fn a_round_trip_between_tabs_with_no_draw_drops_the_link_open(cx: &mut TestAppContext) {
    let (mut s, editor) = setup(cx, Duration::from_secs(1));
    let main = s.path("src/main.rs");
    s.open("src/other.rs");
    s.open("src/main.rs");
    s.draw();
    cmd_click(&mut s, &editor, bytes_of("lib.rs").start + 2);
    let caret = at(&s);
    assert_eq!(caret.0, main);

    // To the other tab and back, within one update.
    let pane = s.workbench.read_with(&s.cx, |w, _| w.pane().clone());
    s.cx.update(|window, cx| {
        let home = pane.read(cx).active().unwrap();
        pane.update(cx, |pane, cx| {
            pane.activate(1 - home, window, cx);
            pane.activate(home, window, cx);
        });
    });
    s.clock.advance(Duration::from_secs(2));
    for _ in 0..20 {
        s.clock.advance(Duration::from_millis(100));
        s.draw();
    }
    assert_eq!(requests(&s, RESOLVE), 1);
    assert_eq!(at(&s), caret, "the link opened nothing");
}
