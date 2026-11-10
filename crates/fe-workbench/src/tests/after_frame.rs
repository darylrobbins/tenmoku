//! [`AfterFrame`]: the hook the app installs to run once after each frame
//! the window draws, whoever asked for the frame, and never in an idle one.

use std::{cell::RefCell, fs, path::Path, rc::Rc, time::Duration};

use fe_gpui::{EditorSettings, EditorTheme, EditorView};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext, px, size};

use crate::{AfterFrame, Launch, Workbench};

/// A workbench on `root` in a 1400 × 600 window, active, as the app's is.
fn open<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.simulate_resize(size(px(1400.), px(600.)));
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(cx);
    (workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn open_file(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let opened = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(opened, window, cx)));
    draw(cx);
    workbench.read_with(cx, |workbench, cx| {
        (workbench.open_editors(cx).into_iter())
            .rfind(|editor| editor.read(cx).model().path() == Some(path))
            .expect("open")
    })
}

/// What the editor showed when the hook ran: its viewport and how many
/// frames it had drawn since its timings were cleared.
type Seen = Rc<RefCell<Vec<(Option<(u32, u32)>, usize)>>>;

/// Installs a hook that records what `editor` shows each time it runs.
fn watch(cx: &mut VisualTestContext, editor: &Entity<EditorView>) -> Seen {
    let seen = Seen::default();
    let log = seen.clone();
    let editor = editor.clone();
    cx.update(|_, cx| {
        editor.update(cx, |editor, _| editor.frame_timings_mut().clear());
        cx.set_global(AfterFrame(Rc::new(move |_, cx| {
            let editor = editor.read(cx);
            log.borrow_mut().push((
                editor.viewport_rows(),
                editor.frame_timings().frames().len(),
            ));
        })));
    });
    seen
}

fn frames_drawn(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    editor.read_with(cx, |editor, _| editor.frame_timings().frames().len())
}

#[gpui::test]
fn runs_once_after_each_drawn_frame(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "row\n".repeat(100)).unwrap();
    let (workbench, cx) = open(cx, dir.path());
    let editor = open_file(&workbench, &path, cx);
    let seen = watch(cx, &editor);
    let top = editor
        .read_with(cx, |editor, _| editor.viewport_rows())
        .unwrap()
        .0;
    assert_eq!(top, 0, "the file opens at its top");

    // (a) A child-only notify: the edit is the editor's, and the Workbench
    // isn't told. Ten rows go in at the end, and the caret with them; the
    // frame scrolls to it. A viewport that only reached the old last row
    // would top out at 76: more means the frame showed the inserted rows.
    editor.update(cx, |editor, cx| {
        let end = editor.model().text().len();
        editor.select_and_reveal(end..end, cx);
        editor.model_mut().insert(&"more\n".repeat(10));
        cx.notify();
    });
    draw(cx);
    let drawn = frames_drawn(&editor, cx);
    let calls = seen.borrow().clone();
    assert_eq!(calls.len(), drawn, "one call per frame drawn: {calls:?}");
    let (rows, frames_then) = *calls.last().unwrap();
    assert_eq!(
        frames_then, drawn,
        "the last call came after its frame was drawn"
    );
    assert!(
        rows.expect("laid out").0 > 76,
        "ran after the frame: the editor showed the inserted rows, {rows:?}"
    );

    // (b) A refresh the platform forces, nobody notified: one more frame,
    // one more call.
    cx.update(|window, _| window.refresh());
    draw(cx);
    let drawn_after = frames_drawn(&editor, cx);
    assert!(drawn_after > drawn, "the refresh drew a frame");
    assert_eq!(seen.borrow().len(), drawn_after, "one call per frame drawn");
}

#[gpui::test]
fn an_idle_window_runs_none(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\n").unwrap();
    let (workbench, cx) = open(cx, dir.path());
    let editor = open_file(&workbench, &path, cx);
    let seen = watch(cx, &editor);
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    let drawn = frames_drawn(&editor, cx);
    assert_eq!(seen.borrow().len(), drawn, "a call only for a frame drawn");
    let settled = seen.borrow().len();
    cx.executor().advance_clock(Duration::from_secs(5));
    cx.run_until_parked();
    assert_eq!(seen.borrow().len(), settled, "idle: no further calls");
}
