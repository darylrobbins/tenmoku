//! Maximize and Move to the Other Split (T346): `ctrl-w z` in vim's normal
//! mode and `cmd-k cmd-m` zoom the active pane, drawn alone until a focus
//! key moves on to where it would have gone unzoomed; `ctrl-w x` trades
//! the active pane with its sibling, focus staying where it was on screen;
//! and the pane menu's Maximize row reads Restore while the pane is zoomed.

use std::{fs, path::Path};

use fe_gpui::set_vim_enabled;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use super::open_root;
use crate::{Pane, Workbench};

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.txt", "b.txt"] {
        fs::write(dir.path().join(name), "x\n").unwrap();
    }
    dir
}

/// A workbench rooted at a project of `a.txt` and `b.txt`, focused,
/// showing `a.txt`, with vim on or off.
fn opened<'a>(
    cx: &'a mut TestAppContext,
    dir: &tempfile::TempDir,
    vim: bool,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| {
        set_vim_enabled(cx, vim);
        window.focus(&workbench.focus_handle(cx), cx);
    });
    open_file(&workbench, &dir.path().join("a.txt"), cx);
    (workbench, cx)
}

fn open_file(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = active(workbench, cx);
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx)));
    settle(cx);
}

fn settle(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    settle(cx);
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

fn panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group(cx).panes().into_iter())
            .cloned()
            .collect()
    })
}

fn zoomed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Entity<Pane>> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.pane_group(cx).zoomed().cloned()
    })
}

fn width(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> f32 {
    pane.read_with(cx, |pane, _| f32::from(pane.bounds().size.width))
}

/// The file the active pane's editor shows, and whether that editor has
/// focus.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (String, bool) {
    cx.update(|window, cx| {
        let workbench = workbench.read(cx);
        let editor = workbench.active_editor(cx).expect("a file is shown");
        let path = editor.read(cx).model().path().expect("a path");
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        (name, editor.focus_handle(cx).is_focused(window))
    })
}

/// The pane menu's rows, if it shows: title and command.
fn menu(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Vec<(String, &'static str)>> {
    workbench.read_with(cx, |workbench, cx| {
        let picker = &workbench.toolbar_picker()?.read(cx).delegate;
        Some(picker.labels().into_iter().zip(picker.commands()).collect())
    })
}

/// Opens `pane`'s "…" menu.
fn more_actions(pane: &Entity<Pane>, cx: &mut VisualTestContext) {
    let at = pane.read_with(cx, |pane, _| pane.action_bounds(2).expect("drawn"));
    cx.simulate_click(at.center(), Modifiers::none());
    settle(cx);
}

#[gpui::test]
fn ctrl_w_z_and_cmd_k_cmd_m_toggle_the_zoom(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, &dir, true);

    // A lone pane does nothing.
    press("ctrl-w z", cx);
    assert_eq!(zoomed(&workbench, cx), None);

    press("ctrl-w v", cx);
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).unwrap();
    assert_eq!(active(&workbench, cx), right);
    let half = width(&right, cx);
    press("ctrl-w z", cx);
    assert_eq!(zoomed(&workbench, cx), Some(right.clone()));
    // Drawn alone over both panes' area; the other keeps its tab.
    assert!(
        width(&right, cx) > half * 1.9,
        "{half} {}",
        width(&right, cx)
    );
    assert_eq!(left.read_with(cx, |pane, _| pane.tabs().len()), 1);
    assert_eq!(shown(&workbench, cx), ("a.txt".into(), true));
    press("ctrl-w z", cx);
    assert_eq!(zoomed(&workbench, cx), None);
    assert!((width(&right, cx) - half).abs() <= 1.);
    assert_eq!(panes(&workbench, cx), [left, right.clone()]);

    // VS Code's key, with vim off.
    cx.update(|_, cx| set_vim_enabled(cx, false));
    settle(cx);
    press("cmd-k cmd-m", cx);
    assert_eq!(zoomed(&workbench, cx), Some(right));
    press("cmd-k cmd-m", cx);
    assert_eq!(zoomed(&workbench, cx), None);
}

#[gpui::test]
fn a_focus_key_while_zoomed_ends_it_where_it_would_have_unzoomed(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, &dir, true);
    press("ctrl-w v", cx);
    press("ctrl-w s", cx);
    let [left, upper, lower] = <[_; 3]>::try_from(panes(&workbench, cx)).unwrap();
    assert_eq!(active(&workbench, cx), lower);

    // The zoomed pane's frame covers the others: up is where it was.
    press("ctrl-w z", cx);
    assert_eq!(zoomed(&workbench, cx), Some(lower.clone()));
    press("ctrl-w k", cx);
    assert_eq!(active(&workbench, cx), upper);
    assert_eq!(zoomed(&workbench, cx), None);
    assert_eq!(shown(&workbench, cx), ("a.txt".into(), true));

    // Resize keys do nothing while zoomed, and evening keeps it.
    press("ctrl-w z", cx);
    let ratios = workbench.read_with(cx, |workbench, cx| workbench.pane_group(cx).ratios(&[]));
    press("ctrl-w >", cx);
    press("ctrl-w =", cx);
    assert_eq!(zoomed(&workbench, cx), Some(upper.clone()));
    let after = workbench.read_with(cx, |workbench, cx| workbench.pane_group(cx).ratios(&[]));
    assert_eq!(after, ratios);

    press("ctrl-w h", cx);
    assert_eq!(active(&workbench, cx), left);
    assert_eq!(zoomed(&workbench, cx), None);
    assert_eq!(shown(&workbench, cx), ("a.txt".into(), true));
}

#[gpui::test]
fn ctrl_w_x_exchanges_with_focus_kept_in_place(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, &dir, true);
    press("ctrl-w v", cx);
    open_file(&workbench, &dir.path().join("b.txt"), cx);
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).unwrap();
    assert_eq!(active(&workbench, cx), right);
    assert_eq!(shown(&workbench, cx), ("b.txt".into(), true));

    // The right pane is last: it trades with the one before it, and the
    // one now on the right, showing a.txt, has focus.
    press("ctrl-w x", cx);
    assert_eq!(panes(&workbench, cx), [right.clone(), left.clone()]);
    assert_eq!(active(&workbench, cx), left);
    assert_eq!(shown(&workbench, cx), ("a.txt".into(), true));
    let (on_left, on_right) = (
        right.read_with(cx, |pane, _| pane.bounds()),
        left.read_with(cx, |pane, _| pane.bounds()),
    );
    assert!(on_right.left() > on_left.right());

    press("ctrl-w ctrl-x", cx);
    assert_eq!(panes(&workbench, cx), [left, right.clone()]);
    assert_eq!(active(&workbench, cx), right);
    assert_eq!(shown(&workbench, cx), ("b.txt".into(), true));
}

#[gpui::test]
fn the_menu_row_reads_restore_while_zoomed(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = opened(cx, &dir, false);
    press("cmd-\\", cx);
    let pane = active(&workbench, cx);

    more_actions(&pane, cx);
    let rows = menu(&workbench, cx).expect("shown");
    assert_eq!(
        (rows[2].0.as_str(), rows[2].1),
        ("Maximize", "pane.toggle_maximize")
    );
    assert!(!rows.iter().any(|row| row.0 == "Restore"));
    // Picked, it zooms the pane.
    cx.simulate_input("maximize");
    settle(cx);
    press("enter", cx);
    assert_eq!(zoomed(&workbench, cx), Some(pane.clone()));

    more_actions(&pane, cx);
    let rows = menu(&workbench, cx).expect("shown");
    assert_eq!(
        (rows[2].0.as_str(), rows[2].1),
        ("Restore", "pane.toggle_maximize")
    );
    assert!(!rows.iter().any(|row| row.0 == "Maximize"));
    assert_eq!(
        (rows[3].0.as_str(), rows[3].1),
        ("Move to the Other Split", "pane.exchange")
    );
    cx.simulate_input("restore");
    settle(cx);
    press("enter", cx);
    assert_eq!(zoomed(&workbench, cx), None);
}
