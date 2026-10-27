//! Compare Active File with Saved: `diff.compare_saved` opens the file on
//! disk, read-only, against its open buffer.

use std::fs;

use fe_buffer::Origin;
use fe_gpui::{DiffView, EditorView, diff_view::NO_CHANGES};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    Workbench,
    tests::find_bar::{draw, editor, opened, pane, run},
};

fn tab_titles(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    workbench.read_with(cx, |workbench, cx| {
        let tabs = workbench.pane().read(cx).tabs();
        (tabs.iter())
            .map(|tab| tab.item().tab_title(cx).to_string())
            .collect()
    })
}

fn active_diff(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<DiffView> {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.active_item()
            .and_then(|item| item.downcast_ref::<DiffView>().cloned())
            .expect("a diff view is shown")
    })
}

fn text(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.model().buffer().text().to_string())
}

fn read_only(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |editor, _| editor.is_read_only())
}

fn hunks(diff: &Entity<DiffView>, cx: &mut VisualTestContext) -> usize {
    diff.read_with(cx, |diff, _| diff.diff().hunks().len())
}

/// Replaces the first line of `editor`'s buffer with `with`.
fn edit_first_line(editor: &Entity<EditorView>, with: &str, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        let end = editor
            .model()
            .buffer()
            .text()
            .to_string()
            .find('\n')
            .unwrap();
        (editor.model_mut().buffer_mut()).edit([(0..end, with)], Origin::User);
        cx.notify();
    });
    draw(cx);
}

fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

#[gpui::test]
fn opens_saved_against_buffer(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "alpha\nbeta\n")]);
    let tab = editor(&pane(&workbench, cx), cx);
    edit_first_line(&tab, "ALPHA", cx);

    run("diff.compare_saved", cx);
    assert_eq!(
        tab_titles(&workbench, cx),
        ["a.txt", "a.txt (Saved ↔ Working Tree)"]
    );
    let diff = active_diff(&workbench, cx);
    let [left, right] = diff.read_with(cx, |diff, _| [diff.left().clone(), diff.right().clone()]);
    assert_eq!(text(&left, cx), "alpha\nbeta\n");
    assert!(read_only(&left, cx));
    assert_eq!(text(&right, cx), "ALPHA\nbeta\n");
    assert!(!read_only(&right, cx));
    let shared = right.read_with(cx, |right, cx| {
        right.model().shares_buffer_with(tab.read(cx).model())
    });
    assert!(shared);
    assert_eq!(hunks(&diff, cx), 1);
}

#[gpui::test]
fn no_changes_and_no_file(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "alpha\n")]);

    run("diff.compare_saved", cx);
    let diff = active_diff(&workbench, cx);
    assert_eq!(hunks(&diff, cx), 0);
    run("git.next_change", cx);
    let message = diff.read_with(cx, |diff, _| diff.message().map(ToString::to_string));
    assert_eq!(message.as_deref(), Some(NO_CHANGES));

    // Asking again shows the tab, not a second one.
    let tab = pane(&workbench, cx).read_with(cx, |pane, _| pane.tabs().len());
    assert_eq!(tab, 2);
    workbench.update_in(cx, |workbench, window, cx| {
        let pane = workbench.pane().clone();
        pane.update(cx, |pane, cx| pane.activate(0, window, cx));
    });
    draw(cx);
    run("diff.compare_saved", cx);
    assert_eq!(tab_titles(&workbench, cx).len(), 2);
    assert_eq!(active_diff(&workbench, cx), diff);

    // An untitled buffer opens no tab.
    run("workbench.close_tab", cx);
    run("workbench.close_tab", cx);
    run("workbench.new_file", cx);
    let before = tab_titles(&workbench, cx);
    run("diff.compare_saved", cx);
    assert_eq!(tab_titles(&workbench, cx), before);
    assert_eq!(toasts(&workbench, cx), ["E32: No file name"]);
}

#[gpui::test]
fn a_deleted_file_answers_missing(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(cx, &[("a.txt", "alpha\n")]);
    let path = dir.path().join("a.txt");
    fs::remove_file(&path).unwrap();
    run("diff.compare_saved", cx);
    assert_eq!(tab_titles(&workbench, cx), ["a.txt"]);
    let answer = format!("File missing: {}", path.display());
    assert_eq!(toasts(&workbench, cx), [answer]);
}

#[gpui::test]
fn follows_edits_and_save(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(cx, &[("a.txt", "alpha\nbeta\n")]);
    let tab = editor(&pane(&workbench, cx), cx);
    run("diff.compare_saved", cx);
    let diff = active_diff(&workbench, cx);
    let [left, right] = diff.read_with(cx, |diff, _| [diff.left().clone(), diff.right().clone()]);
    assert_eq!(hunks(&diff, cx), 0);

    edit_first_line(&right, "ALPHA", cx);
    assert_eq!(hunks(&diff, cx), 1);
    assert_eq!(text(&tab, cx), "ALPHA\nbeta\n");

    // Saving doesn't touch the snapshot.
    run("editor.save", cx);
    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "ALPHA\nbeta\n"
    );
    assert_eq!(text(&left, cx), "alpha\nbeta\n");
    assert_eq!(hunks(&diff, cx), 1);
    // An edit leaves the left as it was, and the disk is not read again.
    edit_first_line(&right, "again", cx);
    fs::write(dir.path().join("a.txt"), "other\n").unwrap();
    draw(cx);
    assert_eq!(text(&left, cx), "alpha\nbeta\n");
    assert_eq!(hunks(&diff, cx), 1);
}

#[gpui::test]
fn asking_again_needs_no_disk_and_works_from_the_saved_side(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(cx, &[("a.txt", "alpha\n")]);
    run("diff.compare_saved", cx);
    let diff = active_diff(&workbench, cx);
    let left = diff.read_with(cx, |diff, _| diff.left().clone());
    fs::remove_file(dir.path().join("a.txt")).unwrap();

    // From the focused read-only Saved side, with the file gone.
    cx.update(|window, cx| {
        let focus = left.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    });
    draw(cx);
    run("diff.compare_saved", cx);
    assert_eq!(tab_titles(&workbench, cx).len(), 2);
    assert_eq!(active_diff(&workbench, cx), diff);
    assert!(toasts(&workbench, cx).is_empty());
}
