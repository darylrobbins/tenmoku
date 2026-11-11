//! A [`FileViewer`] over generated files, in a headless window, with keys
//! through the real keymap.

use std::{cell::RefCell, fs::OpenOptions, rc::Rc, sync::Arc};

use fe_fileview::{FileView, IndexState, LineIndex};
use gpui::{
    AppContext as _, ClipboardItem, Entity, Modifiers, Pixels, Point, ScrollDelta,
    ScrollWheelEvent, TestAppContext, VisualTestContext, point, px,
};
use tempfile::TempDir;

use super::*;
use crate::EditorTheme;

/// A file of exactly `len` bytes, a multiple of 64, and its rows: each
/// row 63 bytes and a `\n`, but the last, which takes the last row's `\n`
/// as one more char.
fn generated(len: usize) -> (TempDir, Arc<FileView>, Vec<String>) {
    assert_eq!(len % 64, 0);
    let rows = len / 64;
    let mut lines: Vec<String> = (0..rows)
        .map(|row| {
            let mut line = format!("row {row:08} {}", "abcdefghijklmnopqrstuvwxyz".repeat(3));
            line.truncate(63);
            line
        })
        .collect();
    lines.last_mut().expect("rows").push('$');
    let text = lines.join("\n");
    assert_eq!(text.len(), len);
    write_file(&text, lines)
}

fn write_file(text: &str, lines: Vec<String>) -> (TempDir, Arc<FileView>, Vec<String>) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("big.log");
    std::fs::write(&path, text).unwrap();
    let view = FileView::open(&path).unwrap();
    assert_eq!(view.len(), text.len() as u64);
    (dir, Arc::new(view), lines)
}

/// A whole index of `view`, built a chunk at a time on this thread.
fn complete_index(view: &Arc<FileView>) -> Arc<LineIndex> {
    let (index, step) = LineIndex::build_stepped(view.clone());
    step.for_each(drop);
    assert_eq!(index.progress().state, IndexState::Complete);
    Arc::new(index)
}

type Events = Rc<RefCell<Vec<FileViewerEvent>>>;

/// The window's root, which makes the viewer and subscribes to it at once,
/// as a workbench tab would.
struct Host {
    viewer: Entity<FileViewer>,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.viewer.clone()
    }
}

/// A focused viewer in a headless window, and the events it sends.
fn viewer(
    cx: &mut TestAppContext,
    view: Arc<FileView>,
    index: Arc<LineIndex>,
) -> (Entity<FileViewer>, &mut VisualTestContext, Events) {
    cx.update(crate::init);
    let events = Events::default();
    let heard = events.clone();
    let (host, cx) = cx.add_window_view(move |_, cx| {
        let theme = EditorTheme::default_dark(cx);
        let banner = "Read only: too large to edit".into();
        let viewer = cx.new(|cx| FileViewer::new(view, index, banner, theme, cx));
        cx.subscribe(&viewer, move |_, _, event: &FileViewerEvent, _| {
            heard.borrow_mut().push(event.clone())
        })
        .detach();
        Host { viewer }
    });
    let viewer = host.read_with(cx, |host, _| host.viewer.clone());
    cx.update(|window, cx| window.focus(&viewer.focus_handle(cx), cx));
    cx.run_until_parked();
    (viewer, cx, events)
}

fn painted(
    viewer: &Entity<FileViewer>,
    cx: &mut VisualTestContext,
) -> Vec<(u32, String, Point<Pixels>)> {
    viewer.read_with(cx, |viewer, _| viewer.painted_rows.clone())
}

/// The top of the text area, below the banner.
fn text_top(viewer: &Entity<FileViewer>, cx: &mut VisualTestContext) -> Pixels {
    viewer.read_with(cx, |viewer, _| {
        viewer.layout.as_ref().expect("drawn").origin.y
    })
}

/// The centre of a caret at `offset`, which must be on screen.
fn position_of(
    viewer: &Entity<FileViewer>,
    cx: &mut VisualTestContext,
    offset: usize,
) -> Point<Pixels> {
    viewer.update(cx, |viewer, _| {
        let layout = viewer.layout.as_mut().expect("drawn");
        layout.caret_bounds(offset).expect("visible").center()
    })
}

#[gpui::test]
fn paints_the_first_rows_and_the_end(cx: &mut TestAppContext) {
    let (_dir, view, lines) = generated(8 * 1024 * 1024);
    let index = complete_index(&view);
    let (viewer, cx, events) = viewer(cx, view, index);

    let top = text_top(&viewer, cx);
    let line_height = viewer.read_with(cx, |viewer, _| {
        px(viewer.layout.as_ref().unwrap().frame.line_height)
    });
    assert!(top >= line_height, "the banner is above the text: {top:?}");
    let rows = painted(&viewer, cx);
    assert!(rows.len() > 10, "{} rows", rows.len());
    for (ix, (row, text, origin)) in rows.iter().enumerate() {
        assert_eq!(*row as usize, ix);
        assert_eq!(text, &lines[ix]);
        assert!(origin.y >= top, "row {row} at {origin:?}, under {top:?}");
    }

    let over_text = point(px(200.), top + px(100.));
    cx.simulate_event(ScrollWheelEvent {
        position: over_text,
        delta: ScrollDelta::Lines(point(0., -(lines.len() as f32 * 2.))),
        ..Default::default()
    });
    cx.run_until_parked();
    let rows = painted(&viewer, cx);
    let (last, ..) = rows.last().expect("rows");
    assert_eq!(*last as usize, lines.len() - 1);
    for (row, text, origin) in &rows {
        assert_eq!(text, &lines[*row as usize]);
        assert!(origin.y >= top - line_height, "row {row} at {origin:?}");
    }
    assert_eq!(*events.borrow(), []);
}

#[gpui::test]
fn selects_and_copies(cx: &mut TestAppContext) {
    let text = "zero one two\nfirst row\nsecond row here\nthird";
    let lines = text.lines().map(str::to_owned).collect();
    let (_dir, view, _) = write_file(text, lines);
    let index = complete_index(&view);
    let (viewer, cx, events) = viewer(cx, view, index);
    let selection = |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.model().selection());

    let second = text.find("second").unwrap();
    let from = position_of(&viewer, cx, 5);
    cx.simulate_click(from, Modifiers::none());
    assert_eq!(selection(cx), 5..5);
    let to = position_of(&viewer, cx, second + 3);
    cx.simulate_click(to, Modifiers::shift());
    assert_eq!(selection(cx), 5..second + 3);

    cx.simulate_keystrokes("cmd-c");
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some(&text[5..second + 3]));
    assert_eq!(*events.borrow(), []);

    let unchanged = |cx: &mut VisualTestContext| {
        viewer.read_with(cx, |viewer, _| {
            let model = viewer.model();
            (
                model.text().slice_to_string(0..model.text().len()),
                model.selection(),
            )
        })
    };
    cx.simulate_keystrokes("x");
    assert_eq!(*events.borrow(), [FileViewerEvent::ReadOnly]);
    assert_eq!(unchanged(cx), (text.to_owned(), 5..second + 3));

    cx.write_to_clipboard(ClipboardItem::new_string("pasted".into()));
    cx.simulate_keystrokes("cmd-v");
    assert_eq!(
        *events.borrow(),
        [FileViewerEvent::ReadOnly, FileViewerEvent::ReadOnly]
    );
    assert_eq!(unchanged(cx), (text.to_owned(), 5..second + 3));
}

#[gpui::test]
fn rows_grow_while_indexing(cx: &mut TestAppContext) {
    // About 4.4 chunks.
    let (_dir, view, lines) = generated(18_000 * 64);
    let (index, step) = LineIndex::build_stepped(view.clone());
    let (viewer, cx, events) = viewer(cx, view, Arc::new(index));
    let rows =
        |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.model().display().row_count());
    let polling = |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.is_polling());

    let mut before = rows(cx);
    assert_eq!(before, 1);
    assert!(polling(cx));
    let mut steps = 0;
    for progress in step {
        steps += 1;
        cx.executor().advance_clock(POLL_INTERVAL);
        cx.run_until_parked();
        let now = rows(cx);
        assert!(now > before, "step {steps}: {now} rows after {before}");
        before = now;
        assert_eq!(polling(cx), progress.state == IndexState::Building);
    }
    assert!(steps >= 4, "{steps} steps");
    assert_eq!(before as usize, lines.len());
    assert!(!polling(cx));
    assert_eq!(*events.borrow(), []);
}

#[gpui::test]
fn completing_before_the_first_poll_shows_every_row(cx: &mut TestAppContext) {
    let (_dir, view, lines) = generated(18_000 * 64);
    let (index, step) = LineIndex::build_stepped(view.clone());
    let (viewer, cx, events) = viewer(cx, view, Arc::new(index));
    assert!(viewer.read_with(cx, |v, _| v.is_polling()));
    step.for_each(drop);

    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    viewer.read_with(cx, |viewer, _| {
        assert_eq!(viewer.model().display().row_count() as usize, lines.len());
        assert!(!viewer.is_polling());
    });
    assert_eq!(*events.borrow(), []);
}

#[gpui::test]
fn an_index_that_failed_before_the_view_is_reported(cx: &mut TestAppContext) {
    let (dir, view, _) = generated(18_000 * 64);
    let (index, step) = LineIndex::build_stepped(view.clone());
    // Shorter than recorded: the scan reads short and stops.
    let file = OpenOptions::new()
        .write(true)
        .open(dir.path().join("big.log"));
    file.unwrap().set_len(1000).unwrap();
    step.for_each(drop);
    assert_eq!(index.progress().state, IndexState::Changed);

    let (viewer, cx, events) = viewer(cx, view, Arc::new(index));
    assert_eq!(*events.borrow(), [FileViewerEvent::Changed]);
    assert!(!viewer.read_with(cx, |v, _| v.is_polling()));
}

#[gpui::test]
fn a_copy_that_reads_a_truncated_file_reports_it_once(cx: &mut TestAppContext) {
    // Bigger than the copy's chunk reads around the view's two ends.
    let (dir, view, _) = generated(18_000 * 64);
    let index = complete_index(&view);
    let (_viewer, cx, events) = viewer(cx, view, index);
    cx.simulate_keystrokes("cmd-a");
    assert_eq!(*events.borrow(), []);

    let file = OpenOptions::new()
        .write(true)
        .open(dir.path().join("big.log"));
    file.unwrap().set_len(1000).unwrap();
    cx.simulate_keystrokes("cmd-c");
    assert_eq!(*events.borrow(), [FileViewerEvent::Changed]);
    cx.simulate_keystrokes("cmd-c");
    assert_eq!(*events.borrow(), [FileViewerEvent::Changed]);
}

#[gpui::test]
fn a_refresh_that_finds_fewer_rows_keeps_the_last_snapshot(cx: &mut TestAppContext) {
    let (_dir, view, _) = generated(18_000 * 64);
    let (index, mut step) = LineIndex::build_stepped(view.clone());
    let (viewer, cx, events) = viewer(cx, view.clone(), Arc::new(index));
    step.next();
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    let rows =
        |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.model().display().row_count());
    let shown = rows(cx);
    assert!(shown > 1);

    // The snapshot a racing read would make: one row.
    let (fresh, _unscanned) = LineIndex::build_stepped(view);
    viewer.update(cx, |v, cx| {
        v.index = Arc::new(fresh);
        v.refresh(cx);
    });
    assert_eq!(rows(cx), shown);
    assert!(!viewer.read_with(cx, |v, _| v.is_polling()));
    assert_eq!(*events.borrow(), [FileViewerEvent::Changed]);
}

#[gpui::test]
fn an_index_that_changes_while_polling_keeps_its_rows(cx: &mut TestAppContext) {
    let (dir, view, _) = generated(18_000 * 64);
    let (index, mut step) = LineIndex::build_stepped(view.clone());
    let (viewer, cx, events) = viewer(cx, view, Arc::new(index));
    step.next();
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    let rows =
        |cx: &mut VisualTestContext| viewer.read_with(cx, |v, _| v.model().display().row_count());
    let shown = rows(cx);
    assert!(shown > 1);
    let generation = viewer.read_with(cx, |v, _| v.generation);

    let file = OpenOptions::new()
        .write(true)
        .open(dir.path().join("big.log"));
    file.unwrap().set_len(1000).unwrap();
    step.for_each(drop);
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    assert_eq!(rows(cx), shown);
    // The poll saw the failure and installed no snapshot.
    assert_eq!(viewer.read_with(cx, |v, _| v.generation), generation);
    assert!(!viewer.read_with(cx, |v, _| v.is_polling()));
    assert_eq!(*events.borrow(), [FileViewerEvent::Changed]);
}

mod a11y {
    //! The viewer's text area as a screen reader reads it: the editor's
    //! builder over the viewer's frame, loaded in `accesskit_consumer`
    //! (design section 9, shape 1). GPUI's test platform can't turn a11y
    //! on, so the builder is called on the drawn frame, as prepaint does.

    use accesskit_consumer::Tree;
    use gpui::accesskit::{self, Node, NodeId, Role, TreeId, TreeUpdate};

    use super::*;
    use crate::{
        element::a11y_text::{A11yText, RunKey},
        file_viewer::element::build_a11y_text,
    };

    const ROOT: NodeId = NodeId(0);
    const AREA: NodeId = NodeId(1);

    fn run_id(key: RunKey) -> NodeId {
        NodeId(((u64::from(key.buffer_row) + 1) << 20) | (u64::from(key.segment) + 1000))
    }

    /// The viewer's text for its last frame.
    pub(super) fn text_of(viewer: &Entity<FileViewer>, cx: &mut VisualTestContext) -> A11yText {
        viewer.update(cx, |viewer, _| {
            let mut layout = viewer.layout.take().expect("drawn");
            let text = build_a11y_text(&mut layout, viewer, 1.);
            viewer.layout = Some(layout);
            text
        })
    }

    pub(super) struct Read {
        pub(super) document: String,
        pub(super) lines: Vec<String>,
        pub(super) selected: String,
        pub(super) focus_line: usize,
        pub(super) read_only: bool,
        pub(super) area: accesskit::Rect,
    }

    pub(super) fn reads(text: &A11yText) -> Read {
        let mut area = Node::new(Role::MultilineTextInput);
        let runs = text.write(&mut area, run_id);
        area.set_children(runs.iter().map(|(id, _)| *id).collect::<Vec<_>>());
        let mut root = Node::new(Role::Window);
        root.set_children(vec![AREA]);
        let mut nodes = runs;
        nodes.push((AREA, area));
        nodes.push((ROOT, root));
        let tree = Tree::new(
            TreeUpdate {
                nodes,
                tree: Some(accesskit::Tree::new(ROOT)),
                tree_id: TreeId::ROOT,
                focus: AREA,
            },
            true,
        );
        let state = tree.state();
        let area = state.focus().expect("focused");
        assert!(area.supports_text_ranges());
        let selection = area.text_selection().expect("a selection");
        let focus = area.text_selection_focus().unwrap();
        // A caret has a box wherever it is.
        assert!(!focus.to_degenerate_range().bounding_boxes().is_empty());
        let first = area.document_range().bounding_boxes();
        Read {
            document: area.document_range().text(),
            lines: (0..text.runs.len())
                .map(|ix| area.line_range_from_index(ix).expect("a line").text())
                .collect(),
            selected: selection.text(),
            focus_line: focus.to_line_index(),
            read_only: area.is_read_only(),
            area: first[0],
        }
    }

    fn scrolled_to(viewer: &Entity<FileViewer>, cx: &mut VisualTestContext, rows: u32) {
        viewer.update(cx, |viewer, cx| {
            let line = viewer.layout.as_ref().unwrap().frame.line_height;
            viewer.scroll = point(px(0.), px(line * rows as f32));
            cx.notify();
        });
        cx.run_until_parked();
    }

    fn file_slice(lines: &[String], rows: std::ops::Range<usize>) -> String {
        let mut out = lines[rows.clone()].join("\n");
        if rows.end < lines.len() {
            out.push('\n');
        }
        out
    }

    #[gpui::test]
    fn runs_are_the_files_rows_in_order_with_margins_both_sides(cx: &mut TestAppContext) {
        let (_dir, view, lines) = generated(8 * 1024 * 1024);
        let index = complete_index(&view);
        let (viewer, cx, _) = viewer(cx, view, index);
        scrolled_to(&viewer, cx, 5000);

        let text = text_of(&viewer, cx);
        let first = text.runs.first().unwrap().key.buffer_row as usize;
        let last = text.runs.last().unwrap().key.buffer_row as usize;
        let visible = viewer.read_with(cx, |v, _| {
            let rows = &v.layout.as_ref().unwrap().frame.rows;
            (
                rows[0].buffer_row as usize,
                rows.last().unwrap().buffer_row as usize,
            )
        });
        assert!(
            first < visible.0 && last > visible.1,
            "{first} {visible:?} {last}"
        );
        assert!(text.runs.len() <= 300);
        let keys: Vec<_> = text.runs.iter().map(|run| run.key).collect();
        assert!(keys.windows(2).all(|pair| pair[0] < pair[1]), "in order");
        assert!(keys.iter().all(|key| key.segment == 0), "no soft wrap");

        let read = reads(&text);
        assert_eq!(read.document, file_slice(&lines, first..last + 1));
        assert_eq!(read.lines.len(), text.runs.len());
        assert_eq!(read.lines[0], format!("{}\n", lines[first]));
        assert!(read.read_only, "the viewer's text node is read-only");
        assert!(read.area.width() > 0. && read.area.height() > 0.);
    }

    #[gpui::test]
    fn a_row_wider_than_the_view_is_one_run_not_wrapped(cx: &mut TestAppContext) {
        // The viewer has no soft wrap: a long row is one run, whole.
        let long = "word ".repeat(400);
        let text = format!("short\n{long}\nend");
        let lines = text.split('\n').map(str::to_owned).collect();
        let (_dir, view, lines) = write_file(&text, lines);
        let index = complete_index(&view);
        let (viewer, cx, _) = viewer(cx, view, index);

        let text_now = text_of(&viewer, cx);
        let keys: Vec<(u32, u32)> = text_now
            .runs
            .iter()
            .map(|run| (run.key.buffer_row, run.key.segment))
            .collect();
        assert_eq!(keys, [(0, 0), (1, 0), (2, 0)]);
        let read = reads(&text_now);
        assert_eq!(
            read.lines,
            [
                "short\n".to_owned(),
                format!("{}\n", lines[1]),
                "end".to_owned(),
            ]
        );
        assert_eq!(read.document, text);
    }

    #[gpui::test]
    fn the_selection_reads_as_the_files_bytes(cx: &mut TestAppContext) {
        let text = "zero one two\nfirst row\r\nsecond row here\n\nthird";
        let lines = text.split('\n').map(str::to_owned).collect();
        let (_dir, view, _) = write_file(text, lines);
        let index = complete_index(&view);
        let (viewer, cx, _) = viewer(cx, view, index);

        let second = text.find("second").unwrap();
        viewer.update(cx, |viewer, cx| viewer.select_from_a11y(5, second + 3, cx));
        cx.run_until_parked();
        let text_now = text_of(&viewer, cx);
        let read = reads(&text_now);
        assert_eq!(read.selected, &text[5..second + 3]);
        assert_eq!(read.focus_line, 2);

        // Backwards.
        viewer.update(cx, |viewer, cx| viewer.select_from_a11y(second + 3, 5, cx));
        cx.run_until_parked();
        let read = reads(&text_of(&viewer, cx));
        assert_eq!(read.selected, &text[5..second + 3]);
        assert_eq!(read.focus_line, 0);

        // A caret on the blank line, and on the CRLF's break.
        let blank = text.find("\n\n").unwrap() + 1;
        for caret in [blank, text.find('\r').unwrap()] {
            viewer.update(cx, |viewer, cx| viewer.select_from_a11y(caret, caret, cx));
            cx.run_until_parked();
            assert_eq!(reads(&text_of(&viewer, cx)).selected, "");
        }

        // Select All over the whole file is the whole document.
        viewer.update(cx, |viewer, cx| viewer.select_from_a11y(0, text.len(), cx));
        cx.run_until_parked();
        let read = reads(&text_of(&viewer, cx));
        assert_eq!(read.selected, read.document);
        assert_eq!(read.document, text);
    }

    #[gpui::test]
    fn a_selection_beyond_the_window_is_projected_onto_it(cx: &mut TestAppContext) {
        let (_dir, view, _) = generated(8 * 1024 * 1024);
        let index = complete_index(&view);
        let len = view.len() as usize;
        let (viewer, cx, _) = viewer(cx, view, index);
        scrolled_to(&viewer, cx, 5000);

        viewer.update(cx, |viewer, cx| viewer.select_from_a11y(0, len, cx));
        cx.run_until_parked();
        let text_now = text_of(&viewer, cx);
        let read = reads(&text_now);
        assert_eq!(read.selected, read.document);
        assert!(read.document.len() <= 64 * 1024);
    }
}

mod a11y_edges {
    //! The shared builder's edge cases, through the viewer's own frame and
    //! shaper (design sections 4 and 9).

    use gpui::accesskit::{Node, NodeId, Role};

    use super::{
        a11y::{reads, text_of},
        *,
    };
    use crate::element::a11y_text::RunKey;

    #[gpui::test]
    fn invalid_bytes_are_question_marks_and_selection_maps_to_file_bytes(cx: &mut TestAppContext) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("bin.log");
        let bytes = b"ab\xffcd\nef\xc3\x28gh\nend";
        std::fs::write(&path, bytes).unwrap();
        let view = Arc::new(FileView::open(&path).unwrap());
        let index = complete_index(&view);
        let (viewer, cx, _) = viewer(cx, view, index);
        viewer.update(cx, |v, cx| v.select_from_a11y(1, 10, cx));
        cx.run_until_parked();
        let text = text_of(&viewer, cx);
        let read = reads(&text);
        assert_eq!(
            read.document.chars().count(),
            bytes.len(),
            "{:?}",
            read.document
        );
        assert!(read.document.contains("ab?cd"));
        // The run offsets are file bytes.
        let run = &text.runs[1];
        assert_eq!(run.start, 6);
        assert_eq!(text.offset(run.key, 2), Some(8));
        assert_eq!(read.selected, "b?cd\nef?(");
        assert_eq!(read.focus_line, 1);
    }

    #[gpui::test]
    fn the_empty_file_has_one_empty_run_with_a_box(cx: &mut TestAppContext) {
        let (_dir, view, lines) = write_file("", vec![String::new()]);
        let _ = lines;
        let index = complete_index(&view);
        let (viewer, cx, _) = viewer(cx, view, index);
        let text = text_of(&viewer, cx);
        assert_eq!(text.runs.len(), 1);
        assert!(text.runs[0].character_lengths.is_empty());
        let read = reads(&text);
        assert_eq!((read.document.as_str(), read.selected.as_str()), ("", ""));
    }

    #[gpui::test]
    fn an_empty_last_line_and_crlf_blank_lines_have_boxes(cx: &mut TestAppContext) {
        let text = "a\r\n\r\nb\n";
        let (_dir, view, _) = write_file(text, vec![]);
        let index = complete_index(&view);
        let (viewer, cx, _) = viewer(cx, view, index);
        let built = text_of(&viewer, cx);
        assert_eq!(reads(&built).document, text);
        // Carets on each break and on the empty last line: `reads` asks
        // for the focus's box each time.
        for caret in 0..=text.len() {
            if !text.is_char_boundary(caret) || text[..caret].ends_with('\r') {
                continue;
            }
            viewer.update(cx, |v, cx| v.select_from_a11y(caret, caret, cx));
            cx.run_until_parked();
            reads(&text_of(&viewer, cx));
        }
    }

    #[gpui::test]
    fn a_caret_scrolled_off_either_edge_is_at_the_window_edge(cx: &mut TestAppContext) {
        let (_dir, view, _) = generated(8 * 1024 * 1024);
        let index = complete_index(&view);
        let len = view.len() as usize;
        let (viewer, cx, _) = viewer(cx, view, index);
        viewer.update(cx, |v, cx| {
            let line = v.layout.as_ref().unwrap().frame.line_height;
            v.scroll = point(px(0.), px(line * 5000.));
            v.select_from_a11y(0, 0, cx);
            // select_from_a11y wants it in view; scroll back off.
            v.autoscroll = false;
            v.scroll = point(px(0.), px(line * 5000.));
            cx.notify();
        });
        cx.run_until_parked();
        let text = text_of(&viewer, cx);
        assert_eq!(text.focus, (text.runs[0].key, 0));
        let read = reads(&text);
        assert_eq!(read.focus_line, 0);
        viewer.update(cx, |v, cx| {
            let line = v.layout.as_ref().unwrap().frame.line_height;
            v.model.set_selection(len, len);
            v.autoscroll = false;
            v.scroll = point(px(0.), px(line * 5000.));
            cx.notify();
        });
        cx.run_until_parked();
        let text = text_of(&viewer, cx);
        let last = text.runs.last().unwrap();
        assert_eq!(text.focus.0, last.key);
        reads(&text);
    }

    #[gpui::test]
    fn a_selection_past_a_truncated_row_ends_at_the_cut(cx: &mut TestAppContext) {
        let long = "x".repeat(200 * 1024);
        let text = format!("short\n{long}\nend");
        let (_dir, view, _) = write_file(&text, vec![]);
        let index = complete_index(&view);
        let (viewer, cx, _) = viewer(cx, view, index);
        viewer.update(cx, |v, cx| v.select_from_a11y(0, text.len() - 1, cx));
        cx.run_until_parked();
        let built = text_of(&viewer, cx);
        let read = reads(&built);
        assert!(read.document.len() <= 64 * 1024);
        assert_eq!(read.selected, read.document);
    }

    #[gpui::test]
    fn boxes_for_visible_margin_and_spanning_ranges_and_hit_testing(cx: &mut TestAppContext) {
        let (_dir, view, _) = generated(8 * 1024 * 1024);
        let index = complete_index(&view);
        let (viewer, cx, _) = viewer(cx, view, index);
        let built = text_of(&viewer, cx);
        let (first_visible, last_visible) = viewer.read_with(cx, |v, _| {
            let rows = &v.layout.as_ref().unwrap().frame.rows;
            (rows[0].buffer_row, rows.last().unwrap().buffer_row)
        });
        let mut area = Node::new(Role::MultilineTextInput);
        let run_id = |key: RunKey| NodeId(((u64::from(key.buffer_row) + 1) << 20) | 7);
        let runs = built.write(&mut area, run_id);
        area.set_children(runs.iter().map(|(id, _)| *id).collect::<Vec<_>>());
        let mut root = Node::new(Role::Window);
        root.set_children(vec![NodeId(1)]);
        let mut nodes = runs;
        nodes.push((NodeId(1), area));
        nodes.push((NodeId(0), root));
        let tree = accesskit_consumer::Tree::new(
            gpui::accesskit::TreeUpdate {
                nodes,
                tree: Some(gpui::accesskit::Tree::new(NodeId(0))),
                tree_id: gpui::accesskit::TreeId::ROOT,
                focus: NodeId(1),
            },
            true,
        );
        let state = tree.state();
        let area = state.focus().unwrap();
        let line = |row: u32| {
            let index = built.runs.iter().position(|r| r.key.buffer_row == row);
            area.line_range_from_index(index.expect("a run")).unwrap()
        };
        let visible = line(first_visible).bounding_boxes();
        assert!(!visible.is_empty());
        let margin_row = built.runs.last().unwrap().key.buffer_row;
        assert!(margin_row > last_visible, "a margin row below");
        assert!(!line(margin_row).bounding_boxes().is_empty());
        let spanning = line(first_visible)
            .start()
            .to_degenerate_range()
            .bounding_boxes();
        assert!(!spanning.is_empty());
        let span = area.document_range().bounding_boxes();
        assert!(!span.is_empty());
        // A visible character's centre hits that character: the fourth of
        // the first visible row.
        let ix = built
            .runs
            .iter()
            .position(|r| r.key.buffer_row == first_visible)
            .unwrap();
        let run = &built.runs[ix];
        let at = |c: usize| gpui::accesskit::Point {
            x: run.bounds.x0 + f64::from(run.positions[c]) + f64::from(run.widths[c]) / 2.,
            y: (run.bounds.y0 + run.bounds.y1) / 2.,
        };
        let before: usize = built.runs[..ix]
            .iter()
            .map(|r| r.text.chars().count())
            .sum();
        for c in [0, 3, 10] {
            let hit = area.text_position_at_point(at(c));
            assert_eq!(hit.to_line_index(), ix);
            assert_eq!(hit.to_global_utf16_index(), before + c, "char {c}");
        }
    }
}
