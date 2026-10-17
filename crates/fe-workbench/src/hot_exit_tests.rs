//! Hot exit ([`super`]): quitting asks nothing about a dirty file or an
//! untitled buffer with text, and the next launch restores both, dirty,
//! with their carets, one undo from the file on disk; a buffer is backed up
//! a second after its last edit, and its backup goes once it's clean or
//! discarded; a crash restores as a quit does, and backups no layout names
//! open as tabs; closing a tab or a window still asks; with vim on, a
//! restored editor is in normal mode and `u` undoes the restore.

use std::{
    cell::Cell,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use fe_buffer::{Backup, BackupId, BackupMeta, BackupStore, Buffer};
use fe_command::{ContextFrame, ContextStack, Invocation, Value};
use fe_gpui::{
    EditorSettings, EditorTheme, EditorView,
    commands::RunCommand,
    pane_item::{self, BufferBackup},
};
use fe_text::Text;
use gpui::{AnyWindowHandle, Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::layout::{self, LayoutStore, WindowLayout};

use super::{BACKUP_DELAY, backups_dir, named};
use crate::{Launch, Workbench};

/// A root holding `a.txt`, its layouts and its backups, in a temporary
/// state directory.
struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
    layouts: LayoutStore,
    backups: BackupStore,
}

fn project() -> Project {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("a.txt"), "alpha\n").unwrap();
    let root = std::fs::canonicalize(&root).unwrap();
    in_state_dir(dir, root)
}

/// `root`'s layouts and backups in the state directory `dir`.
fn in_state_dir(dir: tempfile::TempDir, root: PathBuf) -> Project {
    let layouts = LayoutStore::new(dir.path().join("layouts"), &root);
    let backups = BackupStore::new(backups_dir(&layouts).unwrap());
    Project {
        _dir: dir,
        root,
        layouts,
        backups,
    }
}

/// The project's state as a crash leaves it on disk: a copy of its state
/// directory, with no lock in it held, as the OS drops a dead process's.
fn crash_image(project: &Project) -> Project {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let to = to.join(entry.file_name());
            match entry.file_type().unwrap().is_dir() {
                true => copy(&entry.path(), &to),
                false => drop(std::fs::copy(entry.path(), to).unwrap()),
            }
        }
    }
    let image = tempfile::tempdir().unwrap();
    for name in ["layouts", "backups"] {
        copy(&project._dir.path().join(name), &image.path().join(name));
    }
    in_state_dir(image, project.root.clone())
}

fn init(vim: bool, cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        fe_gpui::set_vim_enabled(cx, vim);
    });
}

/// A launch's window on the root, saving in `layout` and restoring
/// `entry`, drawn and focused.
fn open(
    project: &Project,
    layout: WindowLayout,
    entry: Option<Value>,
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, VisualTestContext) {
    let launch = Launch {
        root: Some(project.root.clone()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        let mut workbench = Workbench::new(launch, theme, EditorSettings::default(), window, cx);
        if let Some(entry) = &entry {
            workbench.restore_layout(entry, window, cx);
        }
        workbench.set_layout(layout, window, cx);
        workbench
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.update(|window, _| window.activate_window());
    vcx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(&mut vcx);
    (workbench, vcx)
}

/// A launch on the root: its first window, restoring its entry if it has
/// one.
fn launch(project: &Project, cx: &mut TestAppContext) -> (Entity<Workbench>, VisualTestContext) {
    let mut windows = layout::launch(&project.layouts, true);
    let (layout, entry) = windows.remove(0);
    open(project, layout, entry, cx)
}

/// A launch on the root after a quit: its one saved window.
fn relaunch(project: &Project, cx: &mut TestAppContext) -> (Entity<Workbench>, VisualTestContext) {
    let mut windows = layout::launch(&project.layouts, true);
    assert_eq!(windows.len(), 1);
    let (layout, entry) = windows.remove(0);
    open(project, layout, Some(entry.expect("a saved window")), cx)
}

/// Opens the root's `a.txt` in the active pane, focused.
fn open_file(workbench: &Entity<Workbench>, project: &Project, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let path = project.root.join("a.txt");
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn wait(time: Duration, cx: &mut VisualTestContext) {
    cx.executor().advance_clock(time);
    draw(cx);
}

fn prompt(cx: &mut VisualTestContext) -> Option<String> {
    cx.pending_prompt().map(|(question, _)| question)
}

fn answer(button: &str, cx: &mut VisualTestContext) {
    cx.simulate_prompt_answer(button);
    draw(cx);
}

/// Every pane's editors, in order.
fn editors(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<EditorView>> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group().panes().iter())
            .flat_map(|pane| {
                (pane.read(cx).tabs().iter())
                    .filter_map(|tab| tab.editor().cloned())
                    .collect::<Vec<_>>()
            })
            .collect()
    })
}

/// An editor's path, text, whether it's dirty, and its caret.
fn state(
    editor: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (Option<String>, String, bool, usize) {
    editor.read_with(cx, |editor, _| {
        let model = editor.model();
        let name = (model.path()).map(|path| path.file_name().unwrap().to_string_lossy().into());
        let text = model.buffer().text().to_string();
        let caret = model.primary_selection().head();
        (name, text, model.is_dirty(), caret)
    })
}

/// Shows the active pane's tab `ix`, focused.
fn show(workbench: &Entity<Workbench>, ix: usize, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.activate(ix, window, cx)));
    draw(cx);
    let editor = editors(workbench, cx).remove(ix);
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
}

/// The backups' texts, oldest first.
fn backups(project: &Project) -> Vec<String> {
    (newest(project).into_iter())
        .map(|backup| backup.text.to_string())
        .collect()
}

/// Each buffer's backup, oldest first: the newer of its two slots
/// ([`pane_item::read_newest`]).
fn newest(project: &Project) -> Vec<Backup> {
    let mut ids: Vec<BackupId> = Vec::new();
    for id in project.backups.ids().unwrap() {
        let id = pane_item::primary(id);
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    (ids.into_iter())
        .map(|id| pane_item::read_newest(&project.backups, id).unwrap())
        .collect()
}

/// `app.quit`, as the app quits: nothing is asked, the quit goes ahead and
/// saves the layout, and the window goes as the process ends.
fn quit(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let quits = Rc::new(Cell::new(0));
    let (counted, quitting) = (quits.clone(), workbench.clone());
    cx.update(|_, cx| {
        super::set_quit(cx, move |cx| {
            counted.set(counted.get() + 1);
            let saved = quitting.update(cx, |workbench, cx| workbench.save_layout_at_quit(cx));
            cx.foreground_executor().spawn(saved).detach();
        })
    });
    run("app.quit", cx);
    assert_eq!(prompt(cx), None, "quitting asks nothing");
    assert_eq!(quits.get(), 1, "the quit went ahead");
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
}

#[gpui::test]
fn quit_restores(first: &mut TestAppContext, second: &mut TestAppContext) {
    let project = project();
    init(false, first);
    let (workbench, mut cx) = launch(&project, first);
    open_file(&workbench, &project, &mut cx);
    cx.simulate_input("x");
    run("workbench.new_file", &mut cx);
    cx.simulate_input("hello");
    draw(&mut cx);
    quit(&workbench, &mut cx);
    assert_eq!(backups(&project), ["xalpha\n", "hello"]);

    init(false, second);
    let (workbench, mut cx) = relaunch(&project, second);
    let editors = editors(&workbench, &mut cx);
    assert_eq!(editors.len(), 2);
    let file = (Some("a.txt".into()), "xalpha\n".into(), true, 1);
    assert_eq!(state(&editors[0], &mut cx), file);
    assert_eq!(state(&editors[1], &mut cx), (None, "hello".into(), true, 5));

    show(&workbench, 0, &mut cx);
    cx.simulate_keystrokes("cmd-z");
    draw(&mut cx);
    let (_, text, dirty, _) = state(&editors[0], &mut cx);
    assert_eq!(
        (text.as_str(), dirty),
        ("alpha\n", false),
        "back to the disk text"
    );
}

#[gpui::test]
fn debounced(cx: &mut TestAppContext) {
    let project = project();
    init(false, cx);
    let (workbench, mut cx) = launch(&project, cx);
    let cx = &mut cx;
    open_file(&workbench, &project, cx);
    let half = BACKUP_DELAY / 2;
    let shy = BACKUP_DELAY - Duration::from_millis(100);

    cx.simulate_input("x");
    wait(half, cx);
    cx.simulate_input("y");
    wait(shy, cx);
    assert!(backups(&project).is_empty(), "not yet a second since the y");
    wait(Duration::from_millis(200), cx);
    assert_eq!(backups(&project), ["xyalpha\n"]);

    // Saved: clean, so it goes.
    cx.simulate_keystrokes("cmd-s");
    draw(cx);
    assert_eq!(
        std::fs::read_to_string(project.root.join("a.txt")).unwrap(),
        "xyalpha\n"
    );
    assert!(backups(&project).is_empty(), "saving removes it");

    // Undone to clean.
    cx.simulate_input("z");
    wait(BACKUP_DELAY, cx);
    assert_eq!(backups(&project), ["xyzalpha\n"]);
    cx.simulate_keystrokes("cmd-z");
    draw(cx);
    assert!(backups(&project).is_empty(), "undoing to clean removes it");

    // Discarded.
    cx.simulate_input("w");
    wait(BACKUP_DELAY, cx);
    assert_eq!(backups(&project), ["xywalpha\n"]);
    run("workbench.close_tab", cx);
    assert_eq!(prompt(cx), Some("Discard unsaved changes to a.txt?".into()));
    answer("Discard", cx);
    assert!(editors(&workbench, cx).is_empty());
    assert!(backups(&project).is_empty(), "discarding removes it");
}

#[gpui::test]
fn crash(first: &mut TestAppContext, second: &mut TestAppContext) {
    let project = project();
    init(false, first);
    let (workbench, mut cx) = launch(&project, first);
    open_file(&workbench, &project, &mut cx);
    cx.simulate_input("x");
    run("workbench.new_file", &mut cx);
    cx.simulate_input("hello");
    wait(BACKUP_DELAY, &mut cx);
    assert_eq!(backups(&project), ["xalpha\n", "hello"]);
    let entries = project.layouts.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(named(&entries[0]).len(), 2, "the layout names both backups");

    // An older backup no layout names, and half of one.
    let orphan: BackupId = "1000-1-0".parse().unwrap();
    let meta = BackupMeta::for_buffer(&Buffer::new(""), None, "untitled");
    (project.backups.write(orphan, &meta, &Text::new("orphan"))).unwrap();
    let half = project.backups.dir().join("1000-1-1.txt");
    std::fs::write(&half, "half").unwrap();

    // The first app crashes: it neither quits nor closes its window.
    let project = crash_image(&project);
    let half = project.backups.dir().join("1000-1-1.txt");
    init(false, second);
    let (workbench, mut cx) = relaunch(&project, second);
    let editors = editors(&workbench, &mut cx);
    let states: Vec<_> = editors
        .iter()
        .map(|editor| state(editor, &mut cx))
        .collect();
    assert_eq!(
        states,
        [
            (Some("a.txt".into()), "xalpha\n".into(), true, 1),
            (None, "hello".into(), true, 5),
            (None, "orphan".into(), true, 0),
        ]
    );
    assert!(!half.exists(), "pruned");
    assert_eq!(backups(&project), ["orphan", "xalpha\n", "hello"]);
}

#[gpui::test]
fn close_still_asks(cx: &mut TestAppContext) {
    let project = project();
    init(false, cx);
    let (workbench, mut cx) = launch(&project, cx);
    let cx = &mut cx;
    let window: AnyWindowHandle = cx.update(|window, _| window.window_handle());
    open_file(&workbench, &project, cx);
    cx.simulate_input("x");
    run("workbench.new_file", cx);
    cx.simulate_input("hello");
    wait(BACKUP_DELAY, cx);

    run("workbench.close_tab", cx);
    assert_eq!(
        prompt(cx),
        Some("Discard unsaved changes to untitled?".into())
    );
    answer("Cancel", cx);
    run("window.close", cx);
    assert!(prompt(cx).is_some(), "the window asks");
    answer("Cancel", cx);
    assert!(cx.cx.update(|cx| cx.windows().contains(&window)));
    assert_eq!(editors(&workbench, cx).len(), 2);
    assert_eq!(backups(&project), ["xalpha\n", "hello"], "kept");

    run("window.close", cx);
    answer("Discard", cx);
    assert!(prompt(cx).is_some(), "each dirty editor asks");
    cx.simulate_prompt_answer("Discard");
    cx.run_until_parked();
    assert!(!cx.cx.update(|cx| cx.windows().contains(&window)));
    assert!(backups(&project).is_empty(), "discarded with the window");
}

#[gpui::test]
fn vim(first: &mut TestAppContext, second: &mut TestAppContext, third: &mut TestAppContext) {
    let project = project();
    init(true, first);
    let (workbench, mut cx) = launch(&project, first);
    open_file(&workbench, &project, &mut cx);
    cx.simulate_keystrokes("A");
    cx.simulate_input("!");
    cx.simulate_keystrokes("escape");
    draw(&mut cx);
    let editor = editors(&workbench, &mut cx).remove(0);
    let saved = state(&editor, &mut cx);
    assert_eq!((saved.1.as_str(), saved.2), ("alpha!\n", true));
    quit(&workbench, &mut cx);
    let image = crash_image(&project);

    // Restored, then `u`, back to the file on disk: with the backup's text
    // over the file's, and with the file since changed to the backup's
    // text, when the restore changes no text.
    let disk = project.root.join("a.txt");
    for (project, disk_text, cx) in [(&project, "alpha\n", second), (&image, "alpha!\n", third)] {
        std::fs::write(&disk, disk_text).unwrap();
        init(true, cx);
        let (workbench, mut cx) = relaunch(project, cx);
        let editor = editors(&workbench, &mut cx).remove(0);
        assert_eq!(state(&editor, &mut cx), saved, "at its caret, dirty");
        let mode = editor.read_with(&cx, |editor, _| editor.vim_mode());
        assert_eq!(mode, Some("normal"));
        show(&workbench, 0, &mut cx);
        cx.simulate_keystrokes("u");
        draw(&mut cx);
        let (_, text, dirty, _) = state(&editor, &mut cx);
        assert_eq!(
            (text.as_str(), dirty),
            (disk_text, false),
            "one u undoes the restore"
        );
    }
}

/// An orphan of a file that can't be opened stays in the store, for a
/// launch that can read it; a store that can't be listed isn't pruned.
#[gpui::test]
fn backups_that_dont_read_are_kept(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
    third: &mut TestAppContext,
) {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = |path: &Path, mode| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap()
    };
    let project = project();
    let locked = project.root.join("locked.txt");
    std::fs::write(&locked, "locked\n").unwrap();
    let orphan: BackupId = "1000-1-0".parse().unwrap();
    let meta = BackupMeta::for_buffer(&Buffer::new(""), Some(locked.clone()), "locked.txt");
    (project.backups.write(orphan, &meta, &Text::new("edited\n"))).unwrap();
    mode(&locked, 0o000);

    init(false, first);
    let (workbench, mut cx) = launch(&project, first);
    assert!(editors(&workbench, &mut cx).is_empty());
    assert_eq!(backups(&project), ["edited\n"], "kept");

    // Readable again, it opens.
    mode(&locked, 0o644);
    let image = crash_image(&project);
    init(false, second);
    let (workbench, mut cx) = launch(&image, second);
    let editors = editors(&workbench, &mut cx);
    let opened = (Some("locked.txt".into()), "edited\n".into(), true, 0);
    assert_eq!(state(&editors[0], &mut cx), opened);

    // A store that can't be listed keeps even its half-written backups.
    let image = crash_image(&project);
    let half = image.backups.dir().join("1000-1-1.txt");
    std::fs::write(&half, "half").unwrap();
    mode(image.backups.dir(), 0o000);
    init(false, third);
    let (_workbench, mut cx) = launch(&image, third);
    draw(&mut cx);
    mode(image.backups.dir(), 0o700);
    assert!(half.exists(), "not pruned");
}

/// A dirty file renamed is backed up under its new path, without another
/// edit, and restores there.
#[gpui::test]
fn renamed(first: &mut TestAppContext, second: &mut TestAppContext) {
    let project = project();
    init(false, first);
    let (workbench, mut cx) = launch(&project, first);
    open_file(&workbench, &project, &mut cx);
    cx.simulate_input("x");
    wait(BACKUP_DELAY, &mut cx);
    let (a, b) = (project.root.join("a.txt"), project.root.join("b.txt"));
    std::fs::rename(&a, &b).unwrap();
    workbench.update(&mut cx, |workbench, cx| workbench.follow_rename(&a, &b, cx));
    wait(BACKUP_DELAY, &mut cx);
    assert_eq!(newest(&project)[0].meta.path, Some(b.clone()));
    quit(&workbench, &mut cx);

    init(false, second);
    let (workbench, mut cx) = relaunch(&project, second);
    let editor = editors(&workbench, &mut cx).remove(0);
    let restored = (Some("b.txt".into()), "xalpha\n".into(), true, 1);
    assert_eq!(state(&editor, &mut cx), restored);
    assert!(!a.exists());
}

/// A launch leaves alone the backups another live app owns, though no
/// layout names them yet: it neither opens nor prunes them, and their owner
/// goes on writing them.
#[gpui::test]
fn live_backups_are_left_alone(first: &mut TestAppContext, second: &mut TestAppContext) {
    let project = project();
    init(false, first);
    let (owner, mut owner_cx) = launch(&project, first);
    open_file(&owner, &project, &mut owner_cx);
    owner_cx.simulate_input("x");
    wait(BACKUP_DELAY, &mut owner_cx);
    std::fs::remove_file(project.layouts.file()).unwrap();

    init(false, second);
    let (workbench, mut cx) = launch(&project, second);
    assert!(editors(&workbench, &mut cx).is_empty(), "not opened");
    assert_eq!(backups(&project), ["xalpha\n"], "not pruned");

    owner_cx.simulate_input("y");
    wait(BACKUP_DELAY, &mut owner_cx);
    assert_eq!(backups(&project), ["xyalpha\n"]);
}

/// A quit waiting behind a window's close leaves that close as it was:
/// cancelled, the quit then asks nothing; accepted, the window still asks
/// about each editor, and their backups go with it.
#[gpui::test]
fn quit_behind_a_close(cx: &mut TestAppContext) {
    let project = project();
    init(false, cx);
    let (workbench, mut cx) = launch(&project, cx);
    let cx = &mut cx;
    open_file(&workbench, &project, cx);
    cx.simulate_input("x");
    run("workbench.new_file", cx);
    cx.simulate_input("hello");
    wait(BACKUP_DELAY, cx);
    let quits = Rc::new(Cell::new(0));
    let counted = quits.clone();
    cx.update(|_, cx| super::set_quit(cx, move |_| counted.set(counted.get() + 1)));
    let a = Some("Discard unsaved changes to a.txt?".to_owned());

    run("window.close", cx);
    assert_eq!(prompt(cx), a);
    run("app.quit", cx);
    answer("Cancel", cx);
    assert_eq!(prompt(cx), None, "the quit asks nothing");
    assert_eq!(quits.get(), 1);
    assert_eq!(backups(&project), ["xalpha\n", "hello"]);

    run("window.close", cx);
    run("app.quit", cx);
    assert_eq!(prompt(cx), a);
    answer("Discard", cx);
    let untitled = Some("Discard unsaved changes to untitled?".to_owned());
    assert_eq!(prompt(cx), untitled, "the window still asks");
    cx.simulate_prompt_answer("Discard");
    cx.run_until_parked();
    assert_eq!(quits.get(), 2);
    assert!(backups(&project).is_empty(), "discarded with the window");
}

/// A crash part-way through rewriting a backup leaves the slot written
/// before it: the uncommitted slot as each step of a store's write leaves
/// it, its meta gone, its new text in a temporary file, or its new text in
/// place without its meta, restores the newest write before.
#[gpui::test]
fn interrupted_rewrite(
    first: &mut TestAppContext,
    meta_gone: &mut TestAppContext,
    temp_text: &mut TestAppContext,
    text_in_place: &mut TestAppContext,
) {
    let project = project();
    init(false, first);
    let (workbench, mut cx) = launch(&project, first);
    open_file(&workbench, &project, &mut cx);
    for typed in ["x", "y", "z"] {
        cx.simulate_input(typed);
        wait(BACKUP_DELAY, &mut cx);
    }
    let backup = newest(&project).remove(0);
    assert_eq!(backup.text.to_string(), "xyzalpha\n");
    let id = pane_item::primary(backup.id);
    let older = if backup.id == id {
        pane_item::twin(id)
    } else {
        id
    };
    let older_file =
        |project: &Project, ext: &str| project.backups.dir().join(format!("{older}.{ext}"));
    assert!(older_file(&project, "toml").exists(), "both slots written");

    // Each step of the older slot's rewrite: its meta removed, then its
    // new text in a temporary file, then that text renamed into place.
    for (step, cx) in [meta_gone, temp_text, text_in_place]
        .into_iter()
        .enumerate()
    {
        let image = crash_image(&project);
        std::fs::remove_file(older_file(&image, "toml")).unwrap();
        match step {
            0 => {}
            1 => {
                let temp = format!(".{older}.txt.1-0.tmp");
                std::fs::write(image.backups.dir().join(temp), "wxyzalpha\n").unwrap();
            }
            _ => std::fs::write(older_file(&image, "txt"), "wxyzalpha\n").unwrap(),
        }
        init(false, cx);
        let (workbench, mut cx) = relaunch(&image, cx);
        let editor = editors(&workbench, &mut cx).remove(0);
        let (_, text, dirty, _) = state(&editor, &mut cx);
        assert_eq!((text.as_str(), dirty), ("xyzalpha\n", true));
    }
}

/// A save that finishes while newer edits stay unsaved, after they were
/// backed up, has the backup written again with the file as saved: so the
/// restored buffer saves without asking about a change on disk.
#[gpui::test]
fn saved_with_newer_edits(first: &mut TestAppContext, second: &mut TestAppContext) {
    let project = project();
    init(false, first);
    let (workbench, mut cx) = launch(&project, first);
    open_file(&workbench, &project, &mut cx);
    let editor = editors(&workbench, &mut cx).remove(0);
    cx.simulate_input("x");
    let job = editor.update(&mut cx, |editor, _| {
        editor.model().prepare_save(false).unwrap()
    });
    let saved = job.run().unwrap();
    cx.simulate_input("y");
    wait(BACKUP_DELAY, &mut cx);
    assert_eq!(
        backups(&project),
        ["xyalpha\n"],
        "backed up before the save finishes"
    );
    editor.update(&mut cx, |editor, cx| {
        assert!(editor.model_mut().finish_save(saved));
        cx.notify();
    });
    wait(BACKUP_DELAY, &mut cx);
    let stamp = editor.read_with(&cx, |editor, _| editor.model().buffer().disk_stamp());
    assert_eq!(newest(&project)[0].meta.disk_stamp, stamp, "written again");
    quit(&workbench, &mut cx);

    init(false, second);
    let (workbench, mut cx) = relaunch(&project, second);
    show(&workbench, 0, &mut cx);
    cx.simulate_keystrokes("cmd-s");
    draw(&mut cx);
    assert_eq!(prompt(&mut cx), None, "no change on disk to ask about");
    let disk = std::fs::read_to_string(project.root.join("a.txt")).unwrap();
    assert_eq!(disk, "xyalpha\n");
}

/// A write or removal that fails is tried again, with no further edit, once
/// the store can be written: a dirty buffer's write, a saved buffer's
/// removal, and a discarded buffer's removal.
#[gpui::test]
fn failures_are_retried(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;
    let project = project();
    init(false, cx);
    let (workbench, mut cx) = launch(&project, cx);
    let cx = &mut cx;
    let writable = |writable: bool| {
        let mode = if writable { 0o700 } else { 0o500 };
        let permissions = std::fs::Permissions::from_mode(mode);
        std::fs::set_permissions(project.backups.dir(), permissions).unwrap();
    };
    open_file(&workbench, &project, cx);
    cx.simulate_input("x");
    wait(BACKUP_DELAY, cx);

    // A directory where the next write's text goes (a store's write makes
    // its directory writable again).
    let next = pane_item::twin(newest(&project)[0].id);
    let blocked = project.backups.dir().join(format!("{next}.txt"));
    std::fs::create_dir_all(blocked.join("in-the-way")).unwrap();
    cx.simulate_input("y");
    wait(BACKUP_DELAY, cx);
    assert_eq!(backups(&project), ["xalpha\n"], "the write failed");
    std::fs::remove_dir_all(&blocked).unwrap();
    wait(BACKUP_DELAY * 2, cx);
    assert_eq!(backups(&project), ["xyalpha\n"], "and was tried again");

    writable(false);
    cx.simulate_keystrokes("cmd-s");
    draw(cx);
    assert_eq!(backups(&project), ["xyalpha\n"], "the removal failed");
    writable(true);
    wait(BACKUP_DELAY, cx);
    assert!(backups(&project).is_empty(), "and was tried again");

    cx.simulate_input("z");
    wait(BACKUP_DELAY, cx);
    writable(false);
    run("workbench.close_tab", cx);
    answer("Discard", cx);
    assert_eq!(backups(&project), ["xyzalpha\n"], "the removal failed");
    writable(true);
    wait(BACKUP_DELAY, cx);
    assert!(backups(&project).is_empty(), "and was tried again");
}

/// Owner files are made, locked and removed under the store's
/// `owners.lock`: pruning waits for an owner caught between opening its
/// file and locking it, then leaves the file it locked; a removal takes its
/// owner file away before letting it go, so the backup can be claimed anew.
#[test]
fn owner_files_stay_put_while_owned() {
    let dir = tempfile::tempdir().unwrap();
    let store = BackupStore::new(dir.path().join("backups"));
    std::fs::create_dir_all(store.dir()).unwrap();
    let id = BackupId::new();
    let owner = store.dir().join(format!("{id}.owner"));
    let owners = std::fs::File::create(store.dir().join("owners.lock")).unwrap();

    // An owner between opening its file and locking it.
    owners.lock().unwrap();
    let opened = std::fs::File::create(&owner).unwrap();
    let (done, finished) = std::sync::mpsc::channel();
    let pruning = std::thread::spawn({
        let store = store.clone();
        move || {
            pane_item::prune_owners(&store, &[]).unwrap();
            done.send(()).unwrap();
        }
    });
    let waited = finished.recv_timeout(Duration::from_millis(100));
    assert!(waited.is_err(), "pruning waits for owners.lock");
    opened.lock().unwrap();
    owners.unlock().unwrap();
    pruning.join().unwrap();
    assert!(owner.exists(), "the owned file stays");
    let claimed = BufferBackup::claim(store.clone(), id).unwrap();
    assert!(claimed.is_none(), "and is still the owner's");
    drop(opened);

    // Owned, written and removed: the owner file goes, and the backup can
    // be claimed again.
    let backup = BufferBackup::claim(store.clone(), id).unwrap().unwrap();
    let meta = BackupMeta::for_buffer(&Buffer::new(""), None, "untitled");
    store.write(id, &meta, &Text::new("draft")).unwrap();
    backup.remove_job().run().unwrap();
    assert!(!owner.exists());
    assert!(store.ids().unwrap().is_empty());
    assert!(BufferBackup::claim(store, id).unwrap().is_some());
}

/// A discarded buffer's removal that failed is finished by quitting, before
/// its retry is due: from a closed tab, then from a closed window.
#[gpui::test]
fn failed_cleanup_finishes_at_quit(first: &mut TestAppContext, second: &mut TestAppContext) {
    let project = project();
    let writable =
        |writable: bool| set_mode(project.backups.dir(), if writable { 0o700 } else { 0o500 });

    init(false, first);
    let (workbench, mut cx) = launch(&project, first);
    open_file(&workbench, &project, &mut cx);
    cx.simulate_input("x");
    run("workbench.new_file", &mut cx);
    cx.simulate_input("hello");
    wait(BACKUP_DELAY, &mut cx);
    writable(false);
    run("workbench.close_tab", &mut cx);
    answer("Discard", &mut cx);
    assert_eq!(
        backups(&project),
        ["xalpha\n", "hello"],
        "the removal failed"
    );
    writable(true);
    quit(&workbench, &mut cx);
    assert_eq!(backups(&project), ["xalpha\n"], "quitting removed it");

    let project = crash_image(&project);
    let writable =
        |writable: bool| set_mode(project.backups.dir(), if writable { 0o700 } else { 0o500 });
    init(false, second);
    let (_workbench, mut cx) = relaunch(&project, second);
    assert_eq!(backups(&project), ["xalpha\n"]);
    writable(false);
    run("window.close", &mut cx);
    cx.simulate_prompt_answer("Discard");
    cx.run_until_parked();
    assert_eq!(backups(&project), ["xalpha\n"], "the removal failed");
    writable(true);
    second.quit();
    assert!(backups(&project).is_empty(), "quitting removed it");
}

/// A write after the clock went back still restores over the slot written
/// before it: each write's `written` is kept past the newest slot's.
#[gpui::test]
fn clock_gone_back(first: &mut TestAppContext, second: &mut TestAppContext) {
    let project = project();
    let orphan: BackupId = "1000-1-0".parse().unwrap();
    let mut meta = BackupMeta::for_buffer(&Buffer::new(""), None, "untitled");
    meta.written += Duration::from_secs(24 * 60 * 60);
    (project.backups.write(orphan, &meta, &Text::new("old"))).unwrap();

    init(false, first);
    let (workbench, mut cx) = launch(&project, first);
    show(&workbench, 0, &mut cx);
    cx.simulate_input("new ");
    wait(BACKUP_DELAY, &mut cx);
    let twin = project.backups.read(pane_item::twin(orphan)).unwrap();
    assert_eq!(twin.text.to_string(), "new old");
    assert!(twin.meta.written > meta.written, "past the slot before");

    let image = crash_image(&project);
    init(false, second);
    let (workbench, mut cx) = relaunch(&image, second);
    let editor = editors(&workbench, &mut cx).remove(0);
    assert_eq!(state(&editor, &mut cx).1, "new old");
}

/// A backup with one slot corrupt and the other unreadable for now isn't
/// given up as corrupt: nothing is pruned, and it opens once readable.
#[gpui::test]
fn corrupt_slot_beside_an_unreadable_one(first: &mut TestAppContext, second: &mut TestAppContext) {
    let project = project();
    let orphan: BackupId = "1000-1-0".parse().unwrap();
    let meta = BackupMeta::for_buffer(&Buffer::new(""), None, "untitled");
    (project.backups.write(orphan, &meta, &Text::new("good"))).unwrap();
    let twin = pane_item::twin(orphan);
    (project.backups.write(twin, &meta, &Text::new("newer"))).unwrap();
    let file = |id: BackupId, ext: &str| project.backups.dir().join(format!("{id}.{ext}"));
    std::fs::write(file(twin, "txt"), "tampered").unwrap();
    set_mode(&file(orphan, "txt"), 0o000);

    init(false, first);
    let (workbench, mut cx) = launch(&project, first);
    assert!(editors(&workbench, &mut cx).is_empty());
    set_mode(&file(orphan, "txt"), 0o600);
    assert!(
        file(orphan, "txt").exists() && file(orphan, "toml").exists(),
        "not pruned"
    );

    let image = crash_image(&project);
    init(false, second);
    let (workbench, mut cx) = launch(&image, second);
    let editor = editors(&workbench, &mut cx).remove(0);
    assert_eq!(state(&editor, &mut cx).1, "good");
}

fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}
