//! A buffer deletion waiting for the file that replaces its buffer, while a
//! Save As holds that file: the Save As being cancelled ends the wait at
//! once, whatever the pane was shown meanwhile, so opening the file again
//! never runs the deletion.

use gpui::{Entity, TestAppContext, VisualTestContext};

use super::super::super::vim_host_tests::{active, draw, window, write};
use crate::{Pane, Workbench};

fn tab_names(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Vec<String> {
    pane.read_with(cx, |pane, _| {
        (pane.tabs().iter())
            .map(|tab| fe_gpui::pane_item::file_name(tab.path.as_deref()))
            .collect()
    })
}

fn open(pane: &Entity<Pane>, path: &std::path::Path, cx: &mut VisualTestContext) {
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx)));
}

/// Buffer 2 is to go for `c.txt`, which a Save As holds; `meanwhile` runs,
/// then the Save As is cancelled and `c.txt` opened again. `deleted` is
/// whether `c.txt` replacing the buffer is what the pane then opens.
fn cancelled_save_as(
    cx: &mut TestAppContext,
    deleted: bool,
    meanwhile: impl FnOnce(&Entity<Pane>, &std::path::Path, &mut VisualTestContext),
) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &["a.txt", "b.txt", "c.txt", "d.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path(), &["a.txt", "b.txt"]);
    let cx = &mut vcx;
    let pane: Entity<Pane> =
        workbench.read_with(cx, |workbench: &Workbench, _| workbench.pane().clone());
    let editor = active(&workbench, cx);
    let path = dir.path().join("c.txt");
    let identity = crate::open::file_identity(&path);
    let saving = crate::open::reserve(fe_buffer::Buffer::new("").id(), path.clone(), identity);

    open(&pane, &path, cx);
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.delete_when_opened(&editor, &pane, path.clone(), (vec![2], false), window, cx)
        })
    });
    meanwhile(&pane, dir.path(), cx);
    draw(cx);
    assert_eq!(tab_names(&pane, cx)[..2], ["a.txt", "b.txt"]);

    // The Save As is cancelled: the wait is over, and nothing is deleted.
    drop(saving);
    cx.run_until_parked();
    draw(cx);
    let waiting = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            cx.global::<super::VimBuffers>()
                .windows
                .values()
                .map(|window| window.waiting.len())
                .sum::<usize>()
        })
    };
    assert_eq!(waiting(cx), 0, "waiter cancelled at once");
    let kept = |cx: &mut VisualTestContext| tab_names(&pane, cx).contains(&"b.txt".to_owned());
    assert_eq!(kept(cx), !deleted);

    // Opening the file again doesn't run a cancelled deletion.
    open(&pane, &path, cx);
    cx.run_until_parked();
    draw(cx);
    assert_eq!(kept(cx), !deleted, "{:?}", tab_names(&pane, cx));
}

#[gpui::test]
fn deletion_waiter_cancelled_by_save_as_cancel(cx: &mut TestAppContext) {
    // Nothing took the pane's place: the file opens, the replacement
    // lands, and the deletion that waited for it runs, once.
    cancelled_save_as(cx, true, |_, _, _| ());
}

#[gpui::test]
fn deletion_waiter_cancelled_by_other_open(cx: &mut TestAppContext) {
    cancelled_save_as(cx, false, |pane, dir, cx| {
        open(pane, &dir.join("d.txt"), cx)
    });
}

#[gpui::test]
fn deletion_waiter_cancelled_by_activation(cx: &mut TestAppContext) {
    cancelled_save_as(cx, false, |pane, _, cx| {
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(0, window, cx)));
    });
}

#[gpui::test]
fn symbolic_ranges_use_lines_for_counts_and_buffers_for_buffers(cx: &mut TestAppContext) {
    use super::super::super::vim_host_tests::{ex, message};
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &["a.txt", "b.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path(), &["a.txt", "b.txt"]);
    let cx = &mut vcx;
    let line = |text: &str, cx: &mut VisualTestContext| {
        ex(text, cx);
        draw(cx);
        message(&workbench, cx).map(|message| message.to_string())
    };
    assert_eq!(line("b 1", cx), None);
    // Three lines, the caret on the third, in buffer 1.
    let editor = active(&workbench, cx);
    editor.update(cx, |editor, cx| {
        editor.model_mut().insert("1\n2\n3");
        cx.notify()
    });
    let backwards = Some("E493: Backwards range given".to_owned());
    // `.` is line 3, not buffer 1; `$` is line 3, not buffer 2.
    assert_eq!(line(".,1bn", cx), backwards);
    assert_eq!(line("$,1bf", cx), backwards);
    assert_eq!(line("$,1bl", cx), backwards);
    // Not backwards as lines, though `.` is the first buffer.
    assert_eq!(line(".,$bf", cx), None);
    // Backwards comes before an end past the last buffer.
    assert_eq!(line("9,$b", cx), backwards);
    assert_eq!(line("9,$bd", cx), backwards);
    assert_eq!(line("9,$bd 1", cx), backwards);
}

#[gpui::test]
fn last_line_excludes_a_terminating_newline(cx: &mut TestAppContext) {
    use super::super::super::vim_host_tests::{ex, message};
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &["a.txt", "b.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path(), &["a.txt", "b.txt"]);
    let cx = &mut vcx;
    let line = |text: &str, cx: &mut VisualTestContext| {
        ex(text, cx);
        draw(cx);
        message(&workbench, cx).map(|message| message.to_string())
    };
    assert_eq!(line("b 1", cx), None);
    let editor = active(&workbench, cx);
    editor.update(cx, |editor, cx| {
        editor.model_mut().select_all();
        editor.model_mut().insert("one\n");
        cx.notify()
    });
    // `one\n` is one line: `$` is 1.
    assert_eq!(line("$,1bf", cx), None);
    assert_eq!(
        line("2,$bf", cx),
        Some("E493: Backwards range given".to_owned())
    );
}
