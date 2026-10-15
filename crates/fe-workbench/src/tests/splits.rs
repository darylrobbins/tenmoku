use std::{fs, path::PathBuf};

use fe_project::fixture;
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, MouseButton, Pixels, TestAppContext,
    VisualTestContext, px,
};

use super::{click_row, open_root, panel};
use crate::{Axis, Member, Pane, Workbench};

/// Files of a project, each with its contents; `fold.txt` folds by indent.
const FILES: [(&str, &str); 4] = [
    ("a.txt", "alpha\n"),
    ("b.rs", "fn b() {}\n"),
    ("c.md", "# c\n"),
    ("fold.txt", "a:\n  b\nc:\n  d\n"),
];

fn project() -> (tempfile::TempDir, Vec<PathBuf>) {
    let dir = tempfile::tempdir().unwrap();
    fixture::generate(dir.path(), 1, 1).unwrap();
    let paths = FILES
        .iter()
        .map(|(name, text)| {
            let path = dir.path().join(name);
            fs::write(&path, text).unwrap();
            path
        })
        .collect();
    (dir, paths)
}

fn panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    workbench.read_with(cx, |workbench, _| {
        let group = workbench.pane_group();
        group.panes().into_iter().cloned().collect()
    })
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

/// The pane's tabs as file names, and its active tab's index.
fn tabs(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> (Vec<String>, Option<usize>) {
    pane.read_with(cx, |pane, _| {
        let names = pane
            .tabs()
            .iter()
            .map(|tab| crate::pane::file_name(tab.path.as_deref()))
            .collect();
        (names, pane.active())
    })
}

/// Each pane's tabs, in layout order.
fn layout(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Vec<String>> {
    panes(workbench, cx)
        .iter()
        .map(|pane| tabs(pane, cx).0)
        .collect()
}

fn bounds(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    pane.read_with(cx, |pane, _| pane.bounds())
}

/// The tree's shape: `P` for a pane, `H[…]` and `V[…]` for axes.
fn shape(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    fn member(of: &Member) -> String {
        match of {
            Member::Pane(_) => "P".into(),
            Member::Axis { axis, members } => {
                let inner: Vec<_> = members.iter().map(member).collect();
                let axis = if *axis == Axis::Horizontal { "H" } else { "V" };
                format!("{axis}[{}]", inner.join(", "))
            }
        }
    }
    workbench.read_with(cx, |workbench, _| member(workbench.pane_group().root()))
}

fn editor_focused(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> bool {
    let editor = pane.read_with(cx, |pane, _| pane.active_editor().cloned());
    let editor = editor.expect("an editor shows");
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

/// Types `text` into the focused editor, then draws a frame.
fn type_text(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(text);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn text(pane: &Entity<Pane>, ix: usize, cx: &mut VisualTestContext) -> String {
    pane.read_with(cx, |pane, cx| {
        pane.tabs()[ix]
            .editor()
            .unwrap()
            .read(cx)
            .model()
            .text()
            .to_string()
    })
}

fn dirty(pane: &Entity<Pane>, ix: usize, cx: &mut VisualTestContext) -> bool {
    pane.read_with(cx, |pane, cx| {
        pane.tabs()[ix]
            .editor()
            .unwrap()
            .read(cx)
            .model()
            .is_dirty()
    })
}

/// A mouse down then up at the middle of `pane`'s editor.
fn click_pane(pane: &Entity<Pane>, cx: &mut VisualTestContext) {
    let center = bounds(pane, cx).center();
    cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::none());
}

#[gpui::test]
fn split_right_shares_the_buffer(cx: &mut TestAppContext) {
    let (dir, _) = project();
    let (workbench, cx) = open_root(cx, dir.path());
    click_row(&panel(&workbench, cx), "a.txt", cx);

    cx.simulate_keystrokes("cmd-\\");
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    assert_eq!(shape(&workbench, cx), "H[P, P]");
    let (l, r) = (bounds(&left, cx), bounds(&right, cx));
    assert!(l.right() <= r.left(), "{l:?} {r:?}");
    assert_eq!(l.top(), r.top());
    assert!((l.size.width - r.size.width).abs() <= px(1.), "{l:?} {r:?}");
    assert!(l.size.width > px(100.), "{l:?}");
    assert_eq!(active(&workbench, cx), right);
    assert!(editor_focused(&right, cx));
    assert_eq!(tabs(&right, cx), (vec!["a.txt".into()], Some(0)));
    let shared = workbench.read_with(cx, |_, cx| {
        let model =
            |pane: &Entity<Pane>| pane.read(cx).tabs()[0].editor().unwrap().read(cx).model();
        model(&left).shares_buffer_with(model(&right))
    });
    assert!(shared);

    // The left editor is told of the right one's edit, and shows it after a
    // frame; both tabs are dirty.
    let left_editor = left.read_with(cx, |pane, _| pane.tabs()[0].editor().unwrap().clone());
    let notified = std::rc::Rc::new(std::cell::Cell::new(0));
    let _observe = cx.update(|_, cx| {
        let notified = notified.clone();
        cx.observe(&left_editor, move |_, _| notified.set(notified.get() + 1))
    });
    type_text("x", cx);
    assert!(notified.get() > 0);
    assert_eq!(text(&right, 0, cx), "xalpha\n");
    assert_eq!(text(&left, 0, cx), "xalpha\n");
    assert!(dirty(&left, 0, cx) && dirty(&right, 0, cx));
    assert_eq!(cx.window_title().as_deref(), Some("a.txt ● — fastedit"));

    // And the other way, each view keeping its own caret.
    click_pane(&left, cx);
    cx.simulate_keystrokes("cmd-down");
    type_text("y", cx);
    assert_eq!(text(&right, 0, cx), "xalpha\ny");
    let carets = workbench.read_with(cx, |_, cx| {
        [&left, &right].map(|pane| {
            let model = pane.read(cx).tabs()[0].editor().unwrap().read(cx).model();
            model.selections()[0].range.end
        })
    });
    assert_eq!(carets, [8, 1]);
}

#[gpui::test]
fn split_down_nests(cx: &mut TestAppContext) {
    let (dir, _) = project();
    let (workbench, cx) = open_root(cx, dir.path());
    click_row(&panel(&workbench, cx), "a.txt", cx);

    cx.simulate_keystrokes("cmd-k cmd-\\");
    assert_eq!(shape(&workbench, cx), "V[P, P]");
    let [top, bottom] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    let (t, b) = (bounds(&top, cx), bounds(&bottom, cx));
    assert!(t.bottom() <= b.top(), "{t:?} {b:?}");
    assert_eq!(t.left(), b.left());
    assert!(
        (t.size.height - b.size.height).abs() <= px(1.),
        "{t:?} {b:?}"
    );
    assert_eq!(active(&workbench, cx), bottom);
    assert_eq!(tabs(&bottom, cx).0, ["a.txt"]);

    // Splitting the lower pane right nests an axis there.
    cx.simulate_keystrokes("cmd-\\");
    assert_eq!(shape(&workbench, cx), "V[P, H[P, P]]");
    let lower_right = active(&workbench, cx);
    assert_eq!(panes(&workbench, cx)[2], lower_right);
    let (b, r) = (bounds(&bottom, cx), bounds(&lower_right, cx));
    assert!(b.right() <= r.left() && b.top() == r.top(), "{b:?} {r:?}");

    // A further split right in that row adds a sibling, not an axis.
    cx.simulate_keystrokes("cmd-\\");
    assert_eq!(shape(&workbench, cx), "V[P, H[P, P, P]]");

    // Closing collapses an axis left with one member.
    cx.simulate_keystrokes("cmd-k cmd-w");
    assert_eq!(shape(&workbench, cx), "V[P, H[P, P]]");
    cx.simulate_keystrokes("cmd-k cmd-w");
    assert_eq!(shape(&workbench, cx), "V[P, P]");
    assert_eq!(panes(&workbench, cx), [top.clone(), bottom.clone()]);
    cx.simulate_keystrokes("cmd-k cmd-w");
    assert_eq!(shape(&workbench, cx), "P");
}

#[gpui::test]
fn focus_follows_clicks_and_keys(cx: &mut TestAppContext) {
    let (dir, _) = project();
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = panel(&workbench, cx);
    click_row(&panel, "a.txt", cx);
    cx.simulate_keystrokes("cmd-\\");
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    assert_eq!(active(&workbench, cx), right);

    // A mouse down in the left pane activates it, and a tree click then
    // opens there.
    let center = bounds(&left, cx).center();
    cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::none());
    assert_eq!(active(&workbench, cx), left);
    cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::none());
    click_row(&panel, "b.rs", cx);
    assert_eq!(
        tabs(&left, cx),
        (vec!["a.txt".into(), "b.rs".into()], Some(1))
    );
    assert_eq!(tabs(&right, cx).0, ["a.txt"]);

    // So does one in the right pane's tab bar.
    let tab = right.read_with(cx, |pane, _| pane.tab_bounds(0).expect("drawn"));
    cx.simulate_click(tab.center(), Modifiers::none());
    assert_eq!(active(&workbench, cx), right);
    assert!(editor_focused(&right, cx));

    // Keys move to the neighbour in each direction, and nowhere without one.
    cx.simulate_keystrokes("cmd-k cmd-left");
    assert_eq!(active(&workbench, cx), left);
    assert!(editor_focused(&left, cx));
    for keys in ["cmd-k cmd-left", "cmd-k cmd-up", "cmd-k cmd-down"] {
        cx.simulate_keystrokes(keys);
        assert_eq!(active(&workbench, cx), left, "{keys}");
    }
    cx.simulate_keystrokes("cmd-k cmd-right");
    assert_eq!(active(&workbench, cx), right);
    cx.simulate_keystrokes("cmd-k cmd-right");
    assert_eq!(active(&workbench, cx), right);
    let caret = |cx: &mut VisualTestContext| {
        right.read_with(cx, |pane, cx| {
            let model = pane.tabs()[0].editor().unwrap().read(cx).model();
            model.selections()[0].range.end
        })
    };
    assert_eq!(caret(cx), 0, "cmd-left after cmd-k isn't the editor's");

    // Up and down, in a pane split below the left one.
    cx.simulate_keystrokes("cmd-k cmd-left cmd-k cmd-\\");
    let lower_left = active(&workbench, cx);
    assert_eq!(shape(&workbench, cx), "H[V[P, P], P]");
    cx.simulate_keystrokes("cmd-k cmd-up");
    assert_eq!(active(&workbench, cx), left);
    cx.simulate_keystrokes("cmd-k cmd-up");
    assert_eq!(active(&workbench, cx), left);
    cx.simulate_keystrokes("cmd-k cmd-down");
    assert_eq!(active(&workbench, cx), lower_left);
    cx.simulate_keystrokes("cmd-k cmd-right");
    assert_eq!(active(&workbench, cx), right);

    // The editor's own cmd-k chord still runs.
    click_row(&panel, "fold.txt", cx);
    cx.simulate_keystrokes("cmd-k cmd-0");
    let folds = workbench.read_with(cx, |workbench, cx| {
        let editor = workbench.active_editor(cx).expect("fold.txt shows");
        let folds = editor.read(cx).model().display().folds();
        folds
            .iter()
            .map(|fold| (fold.start, fold.end))
            .collect::<Vec<_>>()
    });
    assert_eq!(folds, [(0, 1), (2, 3)]);
}

#[gpui::test]
fn move_tab_between_panes(cx: &mut TestAppContext) {
    let (dir, _) = project();
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = panel(&workbench, cx);
    click_row(&panel, "a.txt", cx);
    click_row(&panel, "b.rs", cx);
    let first = active(&workbench, cx);

    // With no pane to the right, the move splits first.
    cx.simulate_keystrokes("ctrl-cmd-right");
    assert_eq!(shape(&workbench, cx), "H[P, P]");
    assert_eq!(layout(&workbench, cx), [vec!["a.txt"], vec!["b.rs"]]);
    let right = active(&workbench, cx);
    assert_ne!(right, first);
    assert!(editor_focused(&right, cx));

    // Moving the left pane's only tab empties it, and it closes.
    cx.simulate_keystrokes("cmd-k cmd-left ctrl-cmd-right");
    assert_eq!(shape(&workbench, cx), "P");
    assert_eq!(panes(&workbench, cx), std::slice::from_ref(&right));
    assert_eq!(
        tabs(&right, cx),
        (vec!["b.rs".into(), "a.txt".into()], Some(1))
    );
    assert!(editor_focused(&right, cx));

    // Moving a file the target shows activates its tab there, and the
    // moved tab closes.
    cx.simulate_keystrokes("cmd-\\");
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    assert_eq!(
        layout(&workbench, cx),
        [vec!["b.rs", "a.txt"], vec!["a.txt"]]
    );
    cx.simulate_keystrokes("cmd-k cmd-left ctrl-cmd-right");
    assert_eq!(layout(&workbench, cx), [vec!["b.rs"], vec!["a.txt"]]);
    assert_eq!(active(&workbench, cx), right);
    assert_eq!(tabs(&right, cx).1, Some(0));
    assert_eq!(tabs(&left, cx).1, Some(0));

    // Moves down and up go by layout too.
    click_row(&panel, "b.rs", cx);
    cx.simulate_keystrokes("ctrl-cmd-down");
    assert_eq!(shape(&workbench, cx), "H[P, V[P, P]]");
    assert_eq!(
        layout(&workbench, cx),
        [vec!["b.rs"], vec!["a.txt"], vec!["b.rs"]]
    );
    cx.simulate_keystrokes("ctrl-cmd-up");
    assert_eq!(shape(&workbench, cx), "H[P, P]");
    assert_eq!(
        layout(&workbench, cx),
        [vec!["b.rs"], vec!["a.txt", "b.rs"]]
    );
    assert_eq!(tabs(&right, cx).1, Some(1));
}

#[gpui::test]
fn closing_panes_and_tabs(cx: &mut TestAppContext) {
    let (dir, _) = project();
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = panel(&workbench, cx);
    click_row(&panel, "a.txt", cx);
    let first = active(&workbench, cx);

    // Closing a pane focuses its neighbour; the last pane stays.
    cx.simulate_keystrokes("cmd-\\");
    cx.simulate_keystrokes("cmd-k cmd-w");
    assert_eq!(panes(&workbench, cx), std::slice::from_ref(&first));
    assert_eq!(active(&workbench, cx), first);
    assert!(editor_focused(&first, cx));
    cx.simulate_keystrokes("cmd-k cmd-w");
    assert_eq!(panes(&workbench, cx), std::slice::from_ref(&first));

    // Closing a pane's last tab closes the pane.
    cx.simulate_keystrokes("cmd-\\ cmd-w");
    assert_eq!(panes(&workbench, cx), std::slice::from_ref(&first));
    assert!(editor_focused(&first, cx));

    // A dirty tab closes when another pane shows its buffer.
    cx.simulate_keystrokes("cmd-\\");
    type_text("x", cx);
    cx.simulate_keystrokes("cmd-w");
    assert_eq!(panes(&workbench, cx), std::slice::from_ref(&first));
    assert!(dirty(&first, 0, cx));
    assert_eq!(text(&first, 0, cx), "xalpha\n");

    // A pane holding a buffer's only view, dirty, stays open; so does the
    // tab.
    cx.simulate_keystrokes("cmd-\\");
    click_row(&panel, "c.md", cx);
    let right = active(&workbench, cx);
    type_text("y", cx);
    cx.simulate_keystrokes("cmd-k cmd-w");
    assert_eq!(panes(&workbench, cx), [first.clone(), right.clone()]);
    cx.simulate_keystrokes("cmd-w");
    assert_eq!(
        tabs(&right, cx),
        (vec!["a.txt".into(), "c.md".into()], Some(1))
    );

    // The left pane's a.txt is shown on the right too, so it closes, and the
    // emptied pane with it.
    cx.simulate_keystrokes("cmd-k cmd-left cmd-k cmd-w");
    assert_eq!(panes(&workbench, cx), std::slice::from_ref(&right));
    assert_eq!(active(&workbench, cx), right);
    assert!(editor_focused(&right, cx));
    assert!(dirty(&right, 0, cx));
}

#[gpui::test]
fn the_window_title_follows_the_active_pane(cx: &mut TestAppContext) {
    let (dir, _) = project();
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = panel(&workbench, cx);
    click_row(&panel, "a.txt", cx);
    cx.simulate_keystrokes("cmd-\\");
    click_row(&panel, "b.rs", cx);
    assert_eq!(cx.window_title().as_deref(), Some("b.rs — fastedit"));
    let [left, _] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");

    click_pane(&left, cx);
    assert_eq!(active(&workbench, cx), left);
    assert_eq!(cx.window_title().as_deref(), Some("a.txt — fastedit"));
}

#[gpui::test]
fn reads_land_in_the_pane_that_asked(cx: &mut TestAppContext) {
    let (dir, paths) = project();
    let launch = crate::Launch {
        root: Some(dir.path().to_owned()),
        file: Some((paths[0].clone(), None)),
    };
    let (workbench, cx, _) = super::open(cx, launch);
    let first = active(&workbench, cx);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));

    // The launch file, still being read, isn't split; its read lands where
    // it shows as loading, not in the new active pane.
    cx.simulate_keystrokes("cmd-\\");
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    assert_eq!((left.clone(), tabs(&right, cx).0), (first, vec![]));
    let read = Ok(fe_buffer::Buffer::new("alpha\n").into());
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.file_read(paths[0].clone(), read, window, cx)
        })
    });
    assert_eq!(layout(&workbench, cx), [vec!["a.txt"], vec![]]);

    // A read landing in a pane that's no longer active activates it, so
    // focus and the active pane agree.
    assert_eq!(active(&workbench, cx), left);
    // As `Pane::open` does, with no read started.
    cx.update(|window, cx| {
        left.update(cx, |pane, cx| {
            pane.opening(paths[2].clone(), cx);
            window.focus(&pane.focus_handle(cx), cx);
        })
    });
    cx.simulate_keystrokes("cmd-k cmd-right");
    assert_eq!(active(&workbench, cx), right);
    let read = Ok(fe_buffer::Buffer::new("# c\n").into());
    cx.update(|window, cx| {
        left.update(cx, |pane, cx| {
            pane.file_read(paths[2].clone(), read, window, cx)
        })
    });
    assert_eq!(active(&workbench, cx), left);
    assert!(editor_focused(&left, cx));
    assert_eq!(cx.window_title().as_deref(), Some("c.md — fastedit"));
    cx.simulate_keystrokes("cmd-w");
    assert_eq!(tabs(&left, cx).0, ["a.txt"]);
}
