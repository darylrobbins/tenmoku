//! Follow-ups from the review of T119: sole-tab keys, divider presses over
//! topology changes, sole-tab drops and the insertion marker's bounds.

use std::{fs, io, path::PathBuf};

use fe_gpui::{EditorSettings, EditorTheme};
use gpui::{
    Bounds, Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext,
    point, px, size,
};

use crate::{Launch, Member, Pane, Workbench, pane_drag};

/// A workbench over a temporary folder holding a.txt and b.rs, with a.txt
/// open in its one pane.
fn harness(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "alpha\n").unwrap();
    fs::write(dir.path().join("b.rs"), "fn b() {}\n").unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(|window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let pane = active(&workbench, cx);
    let path = dir.path().join("a.txt");
    pane.update_in(cx, |pane, window, cx| pane.open(path, window, cx));
    draw(cx);
    (workbench, cx, dir)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

fn panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    workbench.read_with(cx, |workbench, _| {
        workbench
            .pane_group()
            .panes()
            .into_iter()
            .cloned()
            .collect()
    })
}

fn shape(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    fn member(of: &Member) -> String {
        match of {
            Member::Pane(_) => "P".into(),
            Member::Axis { axis, members } => {
                let inner: Vec<_> = members.iter().map(member).collect();
                let axis = if *axis == crate::Axis::Horizontal {
                    "H"
                } else {
                    "V"
                };
                format!("{axis}[{}]", inner.join(", "))
            }
        }
    }
    workbench.read_with(cx, |workbench, _| member(workbench.pane_group().root()))
}

fn tab(pane: &Entity<Pane>, ix: usize, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    pane.read_with(cx, |pane, _| pane.tab_bounds(ix).expect("drawn"))
}

fn drag(from: Point<Pixels>, to: Point<Pixels>, cx: &mut VisualTestContext) {
    let none = Modifiers::none();
    cx.simulate_mouse_down(from, MouseButton::Left, none);
    cx.simulate_mouse_move(
        point(from.x + px(3.), from.y + px(3.)),
        MouseButton::Left,
        none,
    );
    cx.simulate_mouse_move(to, MouseButton::Left, none);
    draw(cx);
    cx.simulate_mouse_up(to, MouseButton::Left, none);
    draw(cx);
}

/// F1: a sole tab moved by key with no neighbour splits off, as before.
#[gpui::test]
fn sole_tab_key_move_splits(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = harness(cx);
    let before = active(&workbench, cx);
    cx.simulate_keystrokes("ctrl-cmd-right");
    draw(cx);
    let after = panes(&workbench, cx);
    assert_eq!(after.len(), 1);
    assert_ne!(after[0], before, "the tab went to a new pane");
    let names = after[0].read_with(cx, |pane, _| pane.tabs().len());
    assert_eq!(names, 1);
}

/// F2: closing a pane mid-drag, collapsing the root axis, leaves the
/// surviving axis's ratios alone.
#[gpui::test]
fn collapsed_axis_ignores_divider_drag(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = harness(cx);
    cx.simulate_keystrokes("cmd-\\ cmd-k cmd-\\");
    draw(cx);
    assert_eq!(shape(&workbench, cx), "H[P, V[P, P]]");
    cx.simulate_keystrokes("cmd-k cmd-left");
    draw(cx);
    let left = active(&workbench, cx);
    assert_eq!(panes(&workbench, cx)[0], left);
    let b = left.read_with(cx, |pane, _| pane.bounds());
    let from = point(b.right() + px(0.5) - px(2.), b.center().y);
    let none = Modifiers::none();
    cx.simulate_mouse_down(from, MouseButton::Left, none);
    cx.simulate_mouse_move(point(from.x + px(3.), from.y), MouseButton::Left, none);
    draw(cx);
    cx.simulate_keystrokes("cmd-k cmd-w");
    draw(cx);
    assert_eq!(shape(&workbench, cx), "V[P, P]");
    let ratios = |cx: &mut VisualTestContext| {
        workbench.update(cx, |workbench, _| {
            workbench.pane_group_mut().ratios(&[]).unwrap()
        })
    };
    let before = ratios(cx);
    for dy in [40., 80.] {
        cx.simulate_mouse_move(
            point(from.x + px(dy), from.y + px(dy)),
            MouseButton::Left,
            none,
        );
        draw(cx);
    }
    cx.simulate_mouse_up(from, MouseButton::Left, none);
    assert_eq!(ratios(cx), before);
}

/// F3: a pane's only tab dropped on its own centre, bar or edge changes
/// nothing: a pending read or read error stays, and the read completes as
/// before.
#[gpui::test]
fn sole_tab_self_drops_keep_pending_state(cx: &mut TestAppContext) {
    for error in [false, true] {
        let (workbench, cx, dir) = harness(cx);
        let pane = active(&workbench, cx);
        let b: PathBuf = dir.path().join("b.rs");
        pane.update(cx, |pane, cx| pane.opening(b.clone(), cx));
        if error {
            let b = b.clone();
            pane.update_in(cx, |pane, window, cx| {
                pane.file_read(b, Err(io::Error::other("boom")), window, cx)
            });
        }
        draw(cx);
        let shows = |cx: &mut VisualTestContext| {
            pane.read_with(cx, |pane, _| {
                (pane.is_opening(&b), pane.active_editor().is_none())
            })
        };
        let expected = (!error, true);
        assert_eq!(shows(cx), expected);

        let bar = tab(&pane, 0, cx);
        let area = pane.read_with(cx, |pane, _| pane.content_bounds());
        let targets = [
            area.center(),
            point(area.right() - px(5.), area.center().y),
            bar.center(),
            point(bar.right() + px(20.), bar.center().y),
        ];
        for to in targets {
            drag(bar.center(), to, cx);
            assert_eq!(panes(&workbench, cx), std::slice::from_ref(&pane), "{to:?}");
            assert_eq!(shows(cx), expected, "{to:?}");
        }
        if !error {
            pane.update_in(cx, |pane, window, cx| {
                pane.file_read(b.clone(), Ok(None), window, cx)
            });
            draw(cx);
            let (tabs, editor) = pane.read_with(cx, |pane, _| {
                (pane.tabs().len(), pane.active_editor().is_some())
            });
            assert_eq!((tabs, editor), (2, true));
        }
    }
}

/// F4: the marker stays inside the bar, for a tab reaching past either end.
#[test]
fn marker_stays_in_bar() {
    let bar = Bounds::new(point(px(100.), px(0.)), size(px(400.), px(30.)));
    // Scrolled: a tab from x=-180 to 420 relative to the bar.
    let long = Bounds::new(point(px(-80.), px(0.)), size(px(600.), px(30.)));
    let inside = |marker: Bounds<Pixels>| {
        assert!(
            marker.left() >= bar.left() && marker.right() <= bar.right(),
            "{marker:?}"
        );
    };
    for index in [0, 1] {
        inside(pane_drag::marker(&[long], index, bar));
    }
    let far = Bounds::new(point(px(700.), px(0.)), size(px(100.), px(30.)));
    inside(pane_drag::marker(&[long, far], 1, bar));
    inside(pane_drag::marker(&[long, far], 2, bar));
}

/// F4: a scrolled bar paints its marker within the bar's bounds.
#[gpui::test]
fn painted_marker_is_inside_the_bar(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = harness(cx);
    let pane = active(&workbench, cx);
    let long = dir
        .path()
        .join("a-very-long-file-name-that-overflows-any-bar.rs");
    fs::write(&long, "x\n").unwrap();
    pane.update_in(cx, |pane, window, cx| pane.open(long, window, cx));
    draw(cx);
    cx.simulate_resize(size(px(700.), px(400.)));
    cx.simulate_keystrokes("cmd-\\");
    draw(cx);
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    // Scrolled so the long tab's middle is just inside the bar's end, its
    // right edge far past it.
    let bar = left.read_with(cx, |pane, _| pane.bounds());
    let last = tab(&left, 1, cx);
    let by = last.center().x - bar.right() + px(50.);
    left.read_with(cx, |pane, _| pane.scroll_tabs(by));
    draw(cx);
    let last = tab(&left, 1, cx);
    assert!(last.right() > bar.right() + px(100.), "{last:?} {bar:?}");
    let to = point(bar.right() - px(5.), last.center().y);
    drag_hover(tab(&right, 0, cx).center(), to, cx);
    let marker = left.read_with(cx, |pane, _| pane.drop_hint_bounds());
    {
        let marker = marker.expect("a marker shows");
        assert!(
            marker.left() >= bar.left() && marker.right() <= bar.right(),
            "{marker:?} {bar:?}"
        );
    }
}

fn drag_hover(from: Point<Pixels>, to: Point<Pixels>, cx: &mut VisualTestContext) {
    let none = Modifiers::none();
    cx.simulate_mouse_down(from, MouseButton::Left, none);
    cx.simulate_mouse_move(
        point(from.x + px(3.), from.y + px(3.)),
        MouseButton::Left,
        none,
    );
    cx.simulate_mouse_move(to, MouseButton::Left, none);
    draw(cx);
}
