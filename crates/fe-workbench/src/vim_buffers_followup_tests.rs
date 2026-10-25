//! The buffer commands' follow-ups: `$` addressing the last known buffer,
//! a range the host checks, a deletion waiting for the file that replaces
//! its buffer, a switch that opens a file at once being noted, and a closed
//! window's unread transitions going.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{AppContext as _, Entity, TestAppContext, VisualTestContext};

use super::super::super::loader::Opened;
use super::super::super::vim_host_tests::{active, draw, ex, message, window, write};
use super::VimBuffers;
use crate::{Pane, Workbench};

fn line(line: &str, workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    ex(line, cx);
    draw(cx);
    message(workbench, cx).map(|message| message.to_string())
}

fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    let editor = active(workbench, cx);
    editor.read_with(cx, |editor, _| {
        fe_gpui::pane_item::file_name(editor.model().path())
    })
}

fn pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

fn tab_names(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> Vec<String> {
    pane.read_with(cx, |pane, _| {
        (pane.tabs().iter())
            .map(|tab| fe_gpui::pane_item::file_name(tab.path.as_deref()))
            .collect()
    })
}

#[gpui::test]
fn dollar_addresses_last_known_buffer(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &["a.txt", "b.txt", "c.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path(), &["a.txt", "b.txt", "c.txt"]);
    let cx = &mut vcx;

    // The highest buffer deleted is still the last one, as in nvim: `:$b`
    // lists it again, rather than showing the highest one listed.
    assert_eq!(line("b 1", &workbench, cx), None);
    assert_eq!(line("bd 3", &workbench, cx), None);
    assert_eq!(line("$b", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "c.txt");
    // And past it is E16, as is a range ending there that runs backwards.
    assert_eq!(
        line("4b", &workbench, cx).as_deref(),
        Some("E16: Invalid range")
    );
    assert_eq!(line("b 1", &workbench, cx), None);
    assert_eq!(line("bd 3", &workbench, cx), None);
    assert_eq!(
        line("$,.b", &workbench, cx).as_deref(),
        Some("E493: Backwards range given")
    );
}

#[gpui::test]
fn ranges_are_checked_against_the_buffers_there_are(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &["a.txt", "b.txt", "c.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path(), &["a.txt", "b.txt", "c.txt"]);
    let cx = &mut vcx;
    assert_eq!(line("b 1", &workbench, cx), None);
    for (command, error) in [
        ("9b 2", "E16: Invalid range"),
        ("9bd 2", "E16: Invalid range"),
        ("1,9bd", "E16: Invalid range"),
        ("2,1b", "E493: Backwards range given"),
        ("3,2b 1", "E493: Backwards range given"),
        ("2,1bd", "E493: Backwards range given"),
    ] {
        assert_eq!(line(command, &workbench, cx).as_deref(), Some(error));
        assert_eq!(shown(&workbench, cx), "a.txt", ":{command}");
        assert_eq!(tab_names(&pane(&workbench, cx), cx).len(), 3, ":{command}");
    }
    // In range, the argument still wins.
    assert_eq!(line("2b 3", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "c.txt");
}

#[gpui::test]
fn delete_waits_for_async_replacement(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &["a.txt", "b.txt", "c.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path(), &["a.txt", "b.txt"]);
    let cx = &mut vcx;
    let pane = pane(&workbench, cx);
    let editor = active(&workbench, cx);

    // Buffer 2 is to be deleted, and the file that replaces it is still
    // being read: its tab stays until the file lands.
    let path = dir.path().join("c.txt");
    pane.update(cx, |pane, cx| pane.opening(path.clone(), cx));
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.delete_when_opened(&editor, &pane, path.clone(), (vec![2], false), window, cx)
        })
    });
    draw(cx);
    assert_eq!(tab_names(&pane, cx), ["a.txt", "b.txt"]);
    // Tabs shown meanwhile don't end the wait.
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(0, window, cx)));
    draw(cx);
    assert_eq!(tab_names(&pane, cx), ["a.txt", "b.txt"]);

    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            let opened = Ok(Opened::Text(Some(Buffer::new("c"))));
            pane.file_read(path.clone(), opened, window, cx)
        })
    });
    draw(cx);
    assert_eq!(tab_names(&pane, cx), ["a.txt", "c.txt"]);
}

#[gpui::test]
fn sync_path_switch_records_history(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &["a.txt", "b.txt", "c.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path(), &["a.txt", "b.txt", "c.txt"]);
    let cx = &mut vcx;
    // A split pane showing b, and c open in the first: `:b 3` opens c in
    // the split at once, over the first pane's text.
    let run = fe_gpui::commands::RunCommand(
        fe_command::Invocation::of("pane.split_right")
            .unwrap()
            .context(
                fe_command::ContextStack::new().with(fe_command::ContextFrame::new("workbench")),
            ),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    assert_eq!(line("b 2", &workbench, cx), None);
    assert_eq!(line("b 3", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "c.txt");
    // The switch is what the pane showed last: `#` goes back.
    assert_eq!(line("b#", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "b.txt");
    assert_eq!(line("b#", &workbench, cx), None);
    assert_eq!(shown(&workbench, cx), "c.txt");
}

#[gpui::test]
fn closed_window_drops_unread_transitions(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &["a.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path(), &["a.txt"]);
    let pending =
        |cx: &mut TestAppContext| cx.update(|cx| cx.default_global::<VimBuffers>().pending.len());
    let pane = pane(&workbench, &mut vcx);
    draw(&mut vcx);

    // A tab added after the last read, and the window closed before the
    // next: nothing will read it.
    // (In a pane of the window's that isn't in its workspace, so no
    // notification brings a read.)
    let stray = pane.update(&mut vcx, |pane, cx| cx.new(|cx| pane.new_peer(cx)));
    stray.update(&mut vcx, |pane, cx| {
        pane.add(EditorModel::new(Buffer::new("one"), None), cx);
    });
    assert!(pending(cx) > 0);
    vcx.update(|window, _| window.remove_window());
    drop((pane, stray, workbench, vcx));
    cx.run_until_parked();
    // The release is seen as the next update starts.
    cx.update(|_| ());
    assert_eq!(pending(cx), 0);
}

#[gpui::test]
fn deferred_delete_rechecks_modified_buffers(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &["a.txt", "b.txt", "c.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path(), &["a.txt", "b.txt"]);
    let cx = &mut vcx;
    let pane = pane(&workbench, cx);
    let editor = active(&workbench, cx);
    let path = dir.path().join("c.txt");
    pane.update(cx, |pane, cx| pane.opening(path.clone(), cx));
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.delete_when_opened(&editor, &pane, path.clone(), (vec![2], false), window, cx)
        })
    });
    // Buffer 2 is changed while the file is read: `:bd` without `!` refuses.
    editor.update(cx, |editor, cx| {
        editor.model_mut().insert("x");
        cx.notify()
    });
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            let opened = Ok(Opened::Text(Some(Buffer::new("c"))));
            pane.file_read(path.clone(), opened, window, cx)
        })
    });
    draw(cx);
    assert_eq!(tab_names(&pane, cx), ["a.txt", "b.txt", "c.txt"]);
}

#[gpui::test]
fn deferred_delete_is_cancelled_when_the_replacement_does_not_open(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), &["a.txt", "b.txt", "c.txt"]);
    let (workbench, mut vcx) = window(cx, dir.path(), &["a.txt", "b.txt"]);
    let cx = &mut vcx;
    let pane = pane(&workbench, cx);
    let editor = active(&workbench, cx);
    let path = dir.path().join("c.txt");
    pane.update(cx, |pane, cx| pane.opening(path.clone(), cx));
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.delete_when_opened(&editor, &pane, path.clone(), (vec![2], false), window, cx)
        })
    });
    // The read fails: nothing replaces buffer 2, so it stays.
    cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            let failed = Err(std::io::Error::other("gone"));
            pane.file_read(path.clone(), failed, window, cx)
        })
    });
    draw(cx);
    assert_eq!(tab_names(&pane, cx), ["a.txt", "b.txt"]);
}
