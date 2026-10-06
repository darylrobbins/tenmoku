use std::fs;

use fe_project::{FileTree, FsChange, TreeOptions, fixture};
use gpui::{Modifiers, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase, point, px};

use super::{click_row, open, open_root, panel, rows};
use crate::Launch;

#[gpui::test]
fn the_root_lists_off_the_main_thread(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fixture::generate(dir.path(), 3, 2).unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx, loaded) = open(cx, launch);
    assert_eq!(loaded, 0, "nothing listed by Workbench::new");

    // `open` ran the background executor.
    let panel = panel(&workbench, cx);
    let expected = FileTree::open(dir.path(), TreeOptions::default()).unwrap();
    let expected: Vec<_> = expected
        .rows(0..expected.row_count())
        .map(|row| (0, row.name.to_string_lossy().into_owned()))
        .collect();
    assert_eq!(expected.len(), 4, "3 directories and .gitignore");
    assert_eq!(rows(&panel, cx), expected);
    let built = panel.read_with(cx, |panel, _| panel.built());
    assert_eq!(built.last(), Some(&(0..4)), "the frame built every row");
}

#[gpui::test]
fn clicking_a_directory_toggles_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fixture::generate(dir.path(), 2, 3).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = panel(&workbench, cx);
    let collapsed = rows(&panel, cx);
    let listings = |cx: &mut gpui::VisualTestContext| {
        panel.read_with(cx, |panel, _| panel.tree().unwrap().stats().listings)
    };

    let name = fixture::dir_name(1);
    click_row(&panel, &name, cx);
    let at = collapsed.iter().position(|(_, row)| *row == name).unwrap();
    let children: Vec<_> = (0..3).map(|ix| (1, fixture::file_name(ix))).collect();
    let mut expanded = collapsed.clone();
    expanded.splice(at + 1..at + 1, children);
    assert_eq!(rows(&panel, cx), expanded);

    let before = listings(cx);
    click_row(&panel, &name, cx);
    assert_eq!(rows(&panel, cx), collapsed);
    assert_eq!(listings(cx), before, "collapsing lists nothing");
}

#[gpui::test]
fn only_visible_rows_are_built(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fixture::generate(dir.path(), 1, 100_000).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    let panel = panel(&workbench, cx);
    click_row(&panel, &fixture::dir_name(0), cx);
    let count = panel.read_with(cx, |panel, _| panel.tree().unwrap().row_count());
    assert_eq!(count, 100_002, "the directory, its files and .gitignore");

    // The rows that fit, one more for a partly scrolled one, and the first
    // row measured twice (the list's request_layout and prepaint).
    let (fit, list) = panel.read_with(cx, |panel, _| {
        let row = panel.row_bounds(0).unwrap();
        let list = panel.list_bounds();
        ((list.size.height / row.size.height).ceil() as usize, list)
    });
    assert!(fit > 10 && fit < 1000, "{fit} rows fit");
    let built = |cx: &mut gpui::VisualTestContext| panel.read_with(cx, |panel, _| panel.built());
    let total =
        |built: &[std::ops::Range<usize>]| built.iter().map(|range| range.len()).sum::<usize>();
    let first = built(cx);
    assert!(total(&first) <= fit + 1 + 2, "{first:?} for {fit} rows");

    // Scrolling far past the end stops at the last row.
    cx.simulate_event(ScrollWheelEvent {
        position: list.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-1e9))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    let last = built(cx);
    assert!(total(&last) <= fit + 1 + 2, "{last:?} for {fit} rows");
    let visible = last.last().unwrap();
    assert_eq!(visible.end, count, "{last:?}");
    let names: Vec<_> = panel.read_with(cx, |panel, _| {
        let tree = panel.tree().unwrap();
        tree.rows(count - 2..count)
            .map(|row| row.name.to_string_lossy().into_owned())
            .collect()
    });
    assert_eq!(names, [fixture::file_name(99_999), ".gitignore".into()]);
}

#[gpui::test]
fn fs_changes_update_rows(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fixture::generate(root, 2, 3).unwrap();
    let (workbench, cx) = open_root(cx, root);
    let panel = panel(&workbench, cx);
    click_row(&panel, &fixture::dir_name(0), cx);

    let inner = root.join(fixture::dir_name(0));
    fs::write(inner.join("new.txt"), "").unwrap();
    fs::remove_file(inner.join(fixture::file_name(1))).unwrap();
    fs::write(root.join("top.txt"), "").unwrap();
    // Unexpanded: nothing to show.
    fs::write(root.join(fixture::dir_name(1)).join("hidden.txt"), "").unwrap();
    let changes = [
        inner.join("new.txt"),
        inner.join(fixture::file_name(1)),
        root.join("top.txt"),
        root.join(fixture::dir_name(1)).join("hidden.txt"),
    ]
    .map(FsChange::Path);
    panel.update(cx, |panel, cx| panel.apply_fs_changes(&changes, cx));
    cx.run_until_parked();

    let rows = rows(&panel, cx);
    let row = |depth: usize, name: &str| (depth, name.to_owned());
    assert!(rows.contains(&row(1, "new.txt")), "{rows:?}");
    assert!(!rows.contains(&row(1, &fixture::file_name(1))), "{rows:?}");
    assert!(rows.contains(&row(0, "top.txt")), "{rows:?}");
    assert!(
        !rows.iter().any(|(_, name)| name == "hidden.txt"),
        "{rows:?}"
    );
    // dir_0000 and its 3 files, dir_0001, .gitignore and top.txt.
    assert_eq!(rows.len(), 7, "{rows:?}");
}

#[gpui::test]
fn listings_recover_after_the_root_was_unreachable(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let outer = dir.path().join("outer");
    let root = outer.join("project");
    fs::create_dir_all(&root).unwrap();
    fixture::generate(&root, 2, 2).unwrap();
    let (workbench, cx) = open_root(cx, &root);
    let panel = panel(&workbench, cx);
    click_row(&panel, &fixture::dir_name(0), cx);

    // dir_0001 is listed while a directory above the root is away, for which
    // no watcher events come, and dir_0000 changes meanwhile.
    let child = root.join(fixture::dir_name(1));
    let listing = panel.read_with(cx, |panel, _| {
        let tree = panel.tree().unwrap();
        let id = tree.find(&child).unwrap();
        (id, tree.listing(id).unwrap())
    });
    let away = dir.path().join("away");
    fs::rename(&outer, &away).unwrap();
    let listed = listing.1.run();
    fs::rename(&away, &outer).unwrap();
    assert!(listed.result().is_err());
    fs::write(root.join(fixture::dir_name(0)).join("added.txt"), "").unwrap();

    // The child's result makes the root owe a listing; the root's, once back,
    // makes the expanded directory owe one. No watcher batch is applied.
    panel.update(cx, |panel, cx| panel.listed(listing.0, listed, cx));
    cx.run_until_parked();
    assert!(
        rows(&panel, cx).contains(&(1, "added.txt".to_owned())),
        "{:?}",
        rows(&panel, cx)
    );
}

#[gpui::test]
fn failed_listings_retry_once_anything_changes(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = tempfile::tempdir().unwrap();
    let outer = dir.path().join("outer");
    let root = outer.join("project");
    fs::create_dir_all(&root).unwrap();
    fixture::generate(&root, 2, 2).unwrap();
    let locked = root.join("locked");
    fs::create_dir(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    let (workbench, cx) = open_root(cx, &root);
    let panel = panel(&workbench, cx);
    click_row(&panel, &fixture::dir_name(0), cx);
    let expanded = root.join(fixture::dir_name(0));

    // With a directory above the root away, a rescan's listings of the root
    // and the expanded directory fail, and wait for a change. Back, with a
    // change no watcher batch reports.
    let fail_and_return = |added: &str, cx: &mut gpui::VisualTestContext| {
        let away = dir.path().join("away");
        fs::rename(&outer, &away).unwrap();
        panel.update(cx, |panel, cx| {
            panel.apply_fs_changes(&[FsChange::Rescan], cx)
        });
        cx.run_until_parked();
        let waiting = panel.read_with(cx, |panel, _| {
            let tree = panel.tree().unwrap();
            let expanded = tree.find(&expanded).unwrap();
            [tree.root(), expanded].map(|id| panel.waits_to_retry(id))
        });
        assert_eq!(waiting, [true, true]);
        fs::rename(&away, &outer).unwrap();
        fs::write(expanded.join(added), "").unwrap();
    };
    let shown = |name: &str, cx: &mut gpui::VisualTestContext| {
        rows(&panel, cx).contains(&(1, name.to_owned()))
    };

    // Another directory's listing succeeding retries them.
    fail_and_return("one.txt", cx);
    let other = root.join(fixture::dir_name(1));
    let (id, listing) = panel.read_with(cx, |panel, _| {
        let tree = panel.tree().unwrap();
        let id = tree.find(&other).unwrap();
        (id, tree.listing(id).unwrap())
    });
    let listed = listing.run();
    assert!(listed.result().is_ok());
    panel.update(cx, |panel, cx| panel.listed(id, listed, cx));
    cx.run_until_parked();
    assert!(shown("one.txt", cx), "{:?}", rows(&panel, cx));

    // So does a click, here on a directory that fails to list itself.
    fail_and_return("two.txt", cx);
    click_row(&panel, "locked", cx);
    assert!(shown("two.txt", cx), "{:?}", rows(&panel, cx));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
}
