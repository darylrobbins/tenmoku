//! Fastedit's pane policy under kiln's `Workspace`: focus follows a pane the
//! workspace activates, to its focus target, and a split doesn't copy a tab
//! a loading file or a read error hides.

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;
use std::{
    fs,
    path::{Path, PathBuf},
};

use gpui::{Entity, Focusable as _, Modifiers, MouseButton, TestAppContext, VisualTestContext};

use super::open_root;
use crate::{Pane, Workbench, pane_group::Direction};

/// A test project: its folder, and on Windows the handle that locks
/// `locked.txt`, closed first.
struct Project {
    #[cfg(windows)]
    _lock: fs::File,
    _dir: tempfile::TempDir,
}

fn project() -> (Project, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    fs::write(root.join("a.txt"), "alpha\n").unwrap();
    fs::write(root.join("b.txt"), "beta\n").unwrap();
    // Unreadable: a read of it fails.
    fs::write(root.join("locked.txt"), "x\n").unwrap();
    #[cfg(unix)]
    fs::set_permissions(root.join("locked.txt"), fs::Permissions::from_mode(0o000)).unwrap();
    // Windows has no mode for that, but a handle that shares nothing makes
    // every other open of the file fail.
    #[cfg(windows)]
    let lock = {
        use std::os::windows::fs::OpenOptionsExt as _;
        fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(root.join("locked.txt"))
            .unwrap()
    };
    let project = Project {
        #[cfg(windows)]
        _lock: lock,
        _dir: dir,
    };
    (project, root)
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn panes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<Pane>> {
    workbench.read_with(cx, |workbench, cx| {
        workbench
            .pane_group(cx)
            .panes()
            .into_iter()
            .cloned()
            .collect()
    })
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

/// Whether focus is on what `pane` would give it back to.
fn on_target(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| pane.read(cx).focus_target(cx).is_focused(window))
}

/// Whether a loading message or a read error covers the pane's a.txt tab.
fn hides_its_tab(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> bool {
    pane.read_with(cx, |pane, _| {
        !pane.tabs().is_empty() && pane.active_item().is_none()
    })
}

fn click_pane(pane: &Entity<Pane>, cx: &mut VisualTestContext) {
    let center = pane.read_with(cx, |pane, _| pane.bounds()).center();
    cx.simulate_mouse_down(center, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(center, MouseButton::Left, Modifiers::none());
}

/// A window showing a.txt, with `rel` then opened in its pane: read and failed
/// if `settled`, else shown loading for good.
fn reading<'a>(
    cx: &'a mut TestAppContext,
    root: &Path,
    rel: &str,
    settled: bool,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let (workbench, cx) = open_root(cx, root);
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.open(root.join("a.txt"), window, cx));
    });
    settle(cx);
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        if settled {
            pane.update(cx, |pane, cx| pane.open(root.join(rel), window, cx));
        } else {
            // Shown as being read by the host, which never hands a result
            // over: it stays loading however long the test parks.
            pane.update(cx, |pane, cx| pane.opening(root.join(rel), cx));
        }
    });
    if settled {
        settle(cx);
    }
    (workbench, cx)
}

/// Splits right as the split command does, without parking, so a read still
/// pending stays so, and returns the (old, new) panes.
fn split(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Entity<Pane>, Entity<Pane>) {
    workbench.update_in(cx, |workbench, window, cx| {
        let new = (workbench.workspace().clone()).update(cx, |workspace, cx| {
            workspace.split(Direction::Right, window, cx)
        });
        workbench.activate(new, window, cx);
    });
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let [old, new] = <[_; 2]>::try_from(panes(workbench, cx)).expect("two panes");
    assert_eq!(active(workbench, cx), new);
    (old, new)
}

#[gpui::test]
fn splitting_over_a_pending_read_gives_an_empty_pane(cx: &mut TestAppContext) {
    let (_dir, root) = project();

    // A file still being read hides a.txt: the split stays empty.
    let (workbench, cx) = reading(cx, &root, "b.txt", false);
    let (_, new) = split(&workbench, cx);
    assert!(new.read_with(cx, |pane, _| pane.tabs().is_empty()));
}

#[gpui::test]
fn splitting_after_a_failed_read_gives_an_empty_pane(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (workbench, cx) = reading(cx, &root, "locked.txt", true);
    let (_, new) = split(&workbench, cx);
    assert!(new.read_with(cx, |pane, _| pane.tabs().is_empty()));
}

#[gpui::test]
fn clicking_an_empty_or_loading_pane_focuses_it(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    let (workbench, cx) = reading(cx, &root, "b.txt", false);
    let (loading, empty) = split(&workbench, cx);

    click_pane(&loading, cx);
    assert!(hides_its_tab(&loading, cx));
    assert_eq!(active(&workbench, cx), loading);
    assert!(on_target(&loading, cx));
    assert!(!on_target(&empty, cx));

    click_pane(&empty, cx);
    assert_eq!(active(&workbench, cx), empty);
    assert!(on_target(&empty, cx));
}

#[gpui::test]
fn closing_onto_a_loading_or_error_pane_focuses_its_target(cx: &mut TestAppContext) {
    let (_dir, root) = project();
    for (rel, settled) in [("b.txt", false), ("locked.txt", true)] {
        let (workbench, cx) = reading(cx, &root, rel, settled);
        let (left, empty) = split(&workbench, cx);
        let workspace = workbench.read_with(cx, |workbench, _| workbench.workspace().clone());
        workspace.update_in(cx, |workspace, window, cx| {
            workspace.close_pane(&empty, window, cx)
        });
        settle(cx);
        assert!(hides_its_tab(&left, cx), "{rel}");
        assert_eq!(active(&workbench, cx), left, "{rel}");
        assert!(on_target(&left, cx), "{rel}");
        let editor = left.read_with(cx, |pane, _| pane.tabs()[0].editor().unwrap().clone());
        assert!(
            !cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)),
            "{rel}"
        );
    }
}
