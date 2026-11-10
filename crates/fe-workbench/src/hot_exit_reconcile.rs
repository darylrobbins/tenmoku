//! Hot exit's comparison of a buffer restored from its backup with its
//! file ([`super::reconcile_later`]): a backup of just what the file
//! holds restores clean, and a comparison landing after the file, the
//! editor's path or the buffer changed counts for nothing, the buffer
//! compared again once its change pauses. Declared from the module itself,
//! not from [`super::tests`], with that module's helpers.

use std::{cell::Cell, rc::Rc};

use fe_gpui::EditorView;
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::BACKUP_DELAY;
use super::tests::{
    Project, backups, crash_image, draw, editors, init, launch, open_file, project, quit, relaunch,
    state, wait,
};
use crate::Workbench;

/// A session that quit with `a.txt` typed into, `xalpha\n`, backed up,
/// whose file has since come to hold just that: what the next launch
/// restores, and compares with the file ([`super::reconcile_later`]).
fn backed_up_over_its_text(cx: &mut TestAppContext) -> Project {
    let project = project();
    init(false, cx);
    let (workbench, mut cx) = launch(&project, cx);
    open_file(&workbench, &project, &mut cx);
    cx.simulate_input("x");
    draw(&mut cx);
    quit(&workbench, &mut cx);
    std::fs::write(project.root.join("a.txt"), "xalpha\n").unwrap();
    project
}

/// Relaunches `project` with `landing` run as the restored buffer's
/// comparison with its file lands, before it's checked; asserts it ran.
fn relaunch_landing(
    project: &Project,
    landing: impl FnOnce(&mut EditorView, &mut gpui::Context<EditorView>) + 'static,
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, VisualTestContext, Entity<EditorView>) {
    let ran = Rc::new(Cell::new(false));
    let noted = ran.clone();
    super::LANDING.set(Some(Box::new(move |editor, cx| {
        noted.set(true);
        landing(editor, cx);
    })));
    init(false, cx);
    let (workbench, mut cx) = relaunch(project, cx);
    assert!(ran.get(), "the comparison landed");
    let editor = editors(&workbench, &mut cx).remove(0);
    (workbench, cx, editor)
}

/// Restored from a backup holding just what its file holds, a buffer is
/// clean, and its backup goes; one whose backup differs stays dirty
/// ([`quit_restores`]).
#[gpui::test]
fn a_backup_of_the_files_text_restores_clean(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
) {
    let project = backed_up_over_its_text(first);
    init(false, second);
    let (workbench, mut cx) = relaunch(&project, second);
    let editor = editors(&workbench, &mut cx).remove(0);
    let (_, text, dirty, _) = state(&editor, &mut cx);
    assert_eq!((text.as_str(), dirty), ("xalpha\n", false));
    assert!(backups(&project).is_empty());
}

/// A file replaced or removed between the comparison's read and its
/// landing leaves the buffer dirty, with its backup.
#[gpui::test]
fn a_file_changed_as_the_comparison_lands_keeps_the_backup(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
    third: &mut TestAppContext,
) {
    let project = backed_up_over_its_text(first);
    let image = crash_image(&project);
    let disk = project.root.join("a.txt");
    let replaced = disk.clone();
    let (_workbench, mut cx, editor) = relaunch_landing(
        &project,
        move |_, _| std::fs::write(&replaced, "theirs!\n").unwrap(),
        second,
    );
    let (_, text, dirty, _) = state(&editor, &mut cx);
    assert_eq!((text.as_str(), dirty), ("xalpha\n", true), "replaced");
    assert_eq!(backups(&project), ["xalpha\n"]);

    std::fs::write(&disk, "xalpha\n").unwrap();
    let removed = disk.clone();
    let (_workbench, mut cx, editor) = relaunch_landing(
        &image,
        move |_, _| std::fs::remove_file(&removed).unwrap(),
        third,
    );
    let (_, text, dirty, _) = state(&editor, &mut cx);
    assert_eq!((text.as_str(), dirty), ("xalpha\n", true), "removed");
    assert_eq!(backups(&image), ["xalpha\n"]);
}

/// An editor moved to another path as the comparison lands stays dirty;
/// it's compared again once its change pauses, with the file at its new
/// path, which holds its text too.
#[gpui::test]
fn a_path_changed_as_the_comparison_lands_compares_again(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
) {
    let project = backed_up_over_its_text(first);
    let moved = project.root.join("b.txt");
    std::fs::write(&moved, "xalpha\n").unwrap();
    let (_workbench, mut cx, editor) = relaunch_landing(
        &project,
        move |editor, cx| {
            editor.model_mut().move_path(moved);
            cx.notify();
        },
        second,
    );
    let (name, _, dirty, _) = state(&editor, &mut cx);
    assert_eq!((name.as_deref(), dirty), (Some("b.txt"), true));
    wait(BACKUP_DELAY, &mut cx);
    assert!(!state(&editor, &mut cx).2, "clean once compared again");
    assert!(backups(&project).is_empty());
}

/// An edit undone as the comparison lands leaves the buffer dirty, as its
/// version moved; it's compared again once the edits pause, and is clean.
#[gpui::test]
fn an_edit_undone_as_the_comparison_lands_compares_again(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
) {
    let project = backed_up_over_its_text(first);
    let (_workbench, mut cx, editor) = relaunch_landing(
        &project,
        |editor, cx| {
            let mut buffer = editor.model_mut().buffer_mut();
            buffer.edit([(0..0, "y")], fe_buffer::Origin::User);
            let user = |origin: &fe_buffer::Origin| *origin == fe_buffer::Origin::User;
            buffer.undo(user).unwrap().unwrap();
            drop(buffer);
            cx.notify();
        },
        second,
    );
    let (_, text, dirty, _) = state(&editor, &mut cx);
    assert_eq!((text.as_str(), dirty), ("xalpha\n", true));
    wait(BACKUP_DELAY, &mut cx);
    assert!(!state(&editor, &mut cx).2, "clean once compared again");
    assert!(backups(&project).is_empty());
}
