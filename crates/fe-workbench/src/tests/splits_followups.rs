use std::{fs, path::PathBuf};

use fe_buffer::Buffer;
use fe_gpui::EditorView;
use fe_project::fixture;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::{click_row, open, open_root, panel};
use crate::{Launch, Pane, Workbench};

/// A project with `a.txt` and `d.json`.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fixture::generate(dir.path(), 1, 1).unwrap();
    fs::write(dir.path().join("a.txt"), "alpha\n").unwrap();
    fs::write(dir.path().join("d.json"), "{\"a\": [1, 2]}\n").unwrap();
    dir
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

/// The pane's tabs as file names.
fn tabs(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Vec<String> {
    pane.read_with(cx, |pane, _| {
        let tabs = pane.tabs().iter();
        tabs.map(|tab| crate::pane::file_name(tab.path.as_deref()))
            .collect()
    })
}

fn editor(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    pane.read_with(cx, |pane, _| pane.tabs()[0].editor.clone())
}

/// Whether the editor's syntax has caught up with its model, which its
/// frame needs to be highlighted.
fn highlighted(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |view, _| {
        view.syntax()
            .is_some_and(|syntax| syntax.version() == view.model().snapshot().version())
    })
}

/// A workbench launched on `l.txt` in `dir`, focused as the host does,
/// which the test reads in the host's place.
fn launch<'a>(
    cx: &'a mut TestAppContext,
    dir: &tempfile::TempDir,
) -> (Entity<Workbench>, &'a mut VisualTestContext, PathBuf) {
    let path = dir.path().join("l.txt");
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path.clone(), None)),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    (workbench, cx, path)
}

fn read_launch(workbench: &Entity<Workbench>, path: PathBuf, cx: &mut VisualTestContext) {
    let buffer = Buffer::new("launched\n");
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.file_read(path, Ok(Some(buffer)), window, cx)
    });
    cx.run_until_parked();
}

#[gpui::test]
fn shared_highlighted_view_is_highlighted_on_first_redraw(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx) = open_root(cx, dir.path());
    click_row(&panel(&workbench, cx), "d.json", cx);
    cx.simulate_keystrokes("cmd-\\");
    cx.run_until_parked();
    let [left, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    let (left, right) = (editor(&left, cx), editor(&right, cx));
    assert!(highlighted(&left, cx) && highlighted(&right, cx));

    // The edit's frame draws both, without asking for another.
    cx.simulate_input(" ");
    assert_eq!(
        left.read_with(cx, |view, _| view.model().text().to_string()),
        " {\"a\": [1, 2]}\n"
    );
    assert!(highlighted(&right, cx));
    assert!(highlighted(&left, cx), "the other pane is highlighted");
}

#[gpui::test]
fn launch_read_goes_to_its_pane_after_its_tab_changes(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx, path) = launch(cx, &dir);
    let [launched] = <[_; 1]>::try_from(panes(&workbench, cx)).expect("one pane");

    // The launch pane opens another file, its tab, before the launch read
    // lands; another pane is active by then.
    click_row(&panel(&workbench, cx), "a.txt", cx);
    cx.run_until_parked();
    cx.simulate_keystrokes("cmd-\\");
    let [_, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    assert_eq!(
        workbench.read_with(cx, |workbench, _| workbench.pane().clone()),
        right
    );

    read_launch(&workbench, path, cx);
    assert_eq!(tabs(&launched, cx), ["a.txt", "l.txt"]);
    assert_eq!(tabs(&right, cx), ["a.txt"]);
}

#[gpui::test]
fn launch_read_is_dropped_when_its_pane_closed(cx: &mut TestAppContext) {
    let dir = project();
    let (workbench, cx, path) = launch(cx, &dir);

    // Another pane opens a file, and the launch pane closes while reading.
    cx.simulate_keystrokes("cmd-\\");
    click_row(&panel(&workbench, cx), "a.txt", cx);
    cx.run_until_parked();
    let [_, right] = <[_; 2]>::try_from(panes(&workbench, cx)).expect("two panes");
    cx.simulate_keystrokes("cmd-k cmd-left cmd-k cmd-w");
    assert_eq!(panes(&workbench, cx), std::slice::from_ref(&right));

    read_launch(&workbench, path, cx);
    assert_eq!(panes(&workbench, cx), std::slice::from_ref(&right));
    assert_eq!(tabs(&right, cx), ["a.txt"]);
}
