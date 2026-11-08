// The launch handoff runs over Unix sockets; Windows builds stand in for
// them, and every launch there is its own instance.
#![cfg(unix)]

//! What ends a wait, in a live workbench: which tabs count, wherever they are.

use std::{
    cell::Cell,
    fs,
    io::Read as _,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use fe_workbench::Launch;
use gpui::{Focusable as _, TestAppContext};

use super::*;
use crate::ipc;

/// A folder holding `a.txt` and `b.txt`.
fn project() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let (a, b) = (root.join("a.txt"), root.join("b.txt"));
    fs::write(&a, "a\n").unwrap();
    fs::write(&b, "b\n").unwrap();
    (dir, a, b)
}

fn init_app(cx: &mut TestAppContext) {
    // A client's watch is a real thread, which wakes the app when it ends.
    cx.executor().allow_parking();
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
        init(cx);
    });
}

fn open_window(root: &Path, cx: &mut TestAppContext) -> WindowHandle<Workbench> {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    window
        .update(cx, |workbench, window, cx| {
            window.focus(&workbench.focus_handle(cx), cx)
        })
        .unwrap();
    cx.run_until_parked();
    window
}

fn open(window: WindowHandle<Workbench>, path: &Path, cx: &mut TestAppContext) {
    window
        .update(cx, |workbench, window, cx| {
            workbench.open_document(path.to_owned(), window, cx)
        })
        .unwrap();
    cx.run_until_parked();
}

/// A client waiting on `files`, whose tabs are open by now.
fn waiting(files: &[&Path], cx: &mut TestAppContext) -> UnixStream {
    let (waiter, client) = Waiter::pair();
    client.set_nonblocking(true).unwrap();
    let files = files.iter().map(|file| file.to_path_buf()).collect();
    cx.update(|cx| add(waiter, files, Task::ready(()), cx));
    cx.run_until_parked();
    client
}

/// What the client has been told so far.
fn told(client: &mut UnixStream) -> String {
    let mut said = Vec::new();
    match client.read_to_end(&mut said) {
        Ok(_) => {}
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock),
    }
    String::from_utf8(said).unwrap()
}

const CLOSED: &str = "{\"closed\":true}\n";

fn run(id: &str, window: WindowHandle<Workbench>, cx: &mut TestAppContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    window
        .update(cx, |_, window, cx| {
            window.dispatch_action(Box::new(run), cx)
        })
        .unwrap();
    cx.run_until_parked();
}

#[gpui::test]
fn reply_comes_only_after_the_last_tab_of_the_request_closes(cx: &mut TestAppContext) {
    let (_dir, a, b) = project();
    init_app(cx);
    let window = open_window(a.parent().unwrap(), cx);
    open(window, &a, cx);
    open(window, &b, cx);
    let mut client = waiting(&[&a, &b], cx);
    assert_eq!(told(&mut client), "", "both are open");

    run("workbench.close_tab", window, cx);
    assert_eq!(told(&mut client), "", "a is still open");
    cx.update(|cx| assert_eq!(pending(cx), 1));

    run("workbench.close_tab", window, cx);
    assert_eq!(told(&mut client), CLOSED);
    cx.update(|cx| assert_eq!(pending(cx), 0));
}

/// Not until the files have landed: a file being read has no tab yet.
#[gpui::test]
fn no_reply_comes_until_the_files_have_landed(cx: &mut TestAppContext) {
    let (_dir, a, _) = project();
    init_app(cx);
    let window = open_window(a.parent().unwrap(), cx);
    let (waiter, mut client) = Waiter::pair();
    client.set_nonblocking(true).unwrap();
    let (land, landing) = async_channel::bounded::<()>(1);
    let landed = cx.update(|cx| {
        cx.spawn(async move |_| {
            landing.recv().await.ok();
        })
    });
    cx.update(|cx| add(waiter, vec![a.clone()], landed, cx));
    // Any change to a pane looks at the tabs.
    run("pane.split_right", window, cx);
    assert_eq!(told(&mut client), "", "it hasn't landed");

    open(window, &a, cx);
    land.try_send(()).unwrap();
    cx.run_until_parked();
    assert_eq!(told(&mut client), "", "it landed in a tab");
    run("workbench.close_tab", window, cx);
    assert_eq!(told(&mut client), CLOSED);
}

#[gpui::test]
fn a_file_already_open_counts_and_a_tab_moved_to_another_pane_still_holds_the_wait(
    cx: &mut TestAppContext,
) {
    let (_dir, a, b) = project();
    init_app(cx);
    let window = open_window(a.parent().unwrap(), cx);
    open(window, &b, cx);
    open(window, &a, cx);
    // Open before the request, which asks for it by another path to it.
    let link = a.with_file_name("link.txt");
    std::os::unix::fs::symlink(&a, &link).unwrap();
    let mut client = waiting(&[&link], cx);
    assert_eq!(told(&mut client), "", "its tab was open already");

    run("pane.move_tab_right", window, cx);
    let panes = |cx: &mut TestAppContext| {
        window
            .read_with(cx, |workbench, cx| workbench.pane_group(cx).panes().len())
            .unwrap()
    };
    assert_eq!(panes(cx), 2, "the tab moved to a pane of its own");
    assert_eq!(told(&mut client), "", "it's moved, not closed");

    // A second window's tab of it holds the wait too.
    let other = open_window(a.parent().unwrap(), cx);
    open(other, &a, cx);
    run("workbench.close_tab", window, cx);
    assert_eq!(told(&mut client), "", "the other window has it");

    // The window goes, with its tab.
    window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    other
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(told(&mut client), CLOSED);
}

/// Close Pane closes a tab with no word from the pane but its release.
#[gpui::test]
fn closing_the_pane_ends_the_wait(cx: &mut TestAppContext) {
    let (_dir, a, b) = project();
    init_app(cx);
    let window = open_window(a.parent().unwrap(), cx);
    open(window, &b, cx);
    run("pane.split_right", window, cx);
    open(window, &a, cx);
    let mut client = waiting(&[&a], cx);

    run("pane.close", window, cx);
    assert_eq!(told(&mut client), CLOSED);
}

#[gpui::test]
fn save_as_to_a_new_path_ends_the_wait_for_the_old_path(cx: &mut TestAppContext) {
    let (_dir, a, _) = project();
    init_app(cx);
    let window = open_window(a.parent().unwrap(), cx);
    open(window, &a, cx);
    let mut client = waiting(&[&a], cx);

    let moved = a.with_file_name("moved.txt");
    run("workbench.save_as", window, cx);
    assert!(cx.did_prompt_for_new_path(), "the save panel shows");
    assert_eq!(told(&mut client), "", "nothing's saved yet");
    cx.simulate_new_path_selection(|_| Some(moved.clone()));
    cx.run_until_parked();

    let tabs: Vec<_> = cx.update(|cx| open_tabs(cx));
    assert_eq!(tabs, [moved], "the tab goes on, as another file");
    assert_eq!(told(&mut client), CLOSED);
}

#[gpui::test]
fn folders_and_no_targets_reply_at_once(cx: &mut TestAppContext) {
    let (_dir, a, _) = project();
    let root = a.parent().unwrap().to_owned();
    init_app(cx);
    let window = open_window(&root, cx);
    open(window, &a, cx);

    let request = |targets: Vec<crate::cli::Target>| Request {
        targets,
        new_window: false,
        wait: true,
        cwd: root.clone(),
    };
    let folder = crate::cli::Target {
        path: root.clone(),
        line: None,
        col: None,
    };
    assert_eq!(files_of(&request(vec![])), Vec::<PathBuf>::new());
    assert_eq!(
        files_of(&request(vec![folder.clone()])),
        Vec::<PathBuf>::new()
    );
    // A file among them is waited on, by way of the working directory.
    let relative = crate::cli::Target {
        path: "a.txt".into(),
        ..folder.clone()
    };
    assert_eq!(files_of(&request(vec![folder, relative])), [a]);

    let (waiter, mut client) = Waiter::pair();
    client.set_nonblocking(true).unwrap();
    cx.update(|cx| add(waiter, vec![], Task::ready(()), cx));
    assert_eq!(told(&mut client), CLOSED);
    cx.update(|cx| assert_eq!(pending(cx), 0));
}

/// This process's own wait, the launch being the editor: it quits once the
/// files have closed, and not before.
#[gpui::test]
fn a_launch_with_no_server_quits_when_its_own_files_close(cx: &mut TestAppContext) {
    let (_dir, a, _) = project();
    init_app(cx);
    let window = open_window(a.parent().unwrap(), cx);
    open(window, &a, cx);
    let quits = Rc::new(Cell::new(0));
    let quit = {
        let quits = quits.clone();
        move |_: &mut App| quits.set(quits.get() + 1)
    };
    cx.update(|cx| add_own(quit, vec![a.clone()], Task::ready(()), cx));
    cx.run_until_parked();
    assert_eq!(quits.get(), 0, "the file is open");

    run("workbench.close_tab", window, cx);
    assert_eq!(quits.get(), 1);
    cx.update(|cx| assert_eq!(pending(cx), 0));
}

/// A folder, or no target at all: nothing to wait for, so at once.
#[gpui::test]
fn a_launch_with_no_server_and_no_files_quits_at_once(cx: &mut TestAppContext) {
    let (_dir, a, _) = project();
    init_app(cx);
    let _window = open_window(a.parent().unwrap(), cx);
    let quits = Rc::new(Cell::new(0));
    let quit = {
        let quits = quits.clone();
        move |_: &mut App| quits.set(quits.get() + 1)
    };
    cx.update(|cx| add_own(quit, vec![], Task::ready(()), cx));
    assert_eq!(quits.get(), 1);
}

/// A quit that's already happening has nothing to do for its own launch.
#[gpui::test]
fn a_launch_of_its_own_is_not_quit_again_by_the_quit(cx: &mut TestAppContext) {
    let (_dir, a, _) = project();
    init_app(cx);
    let window = open_window(a.parent().unwrap(), cx);
    open(window, &a, cx);
    let quits = Rc::new(Cell::new(0));
    let quit = {
        let quits = quits.clone();
        move |_: &mut App| quits.set(quits.get() + 1)
    };
    cx.update(|cx| add_own(quit, vec![a.clone()], Task::ready(()), cx));
    cx.run_until_parked();
    cx.quit();
    assert_eq!(quits.get(), 0);
}

/// A landing that would never finish is over once its window has closed.
#[gpui::test]
fn a_landing_is_given_up_when_its_window_closes(cx: &mut TestAppContext) {
    let (_dir, a, _) = project();
    init_app(cx);
    let window = open_window(a.parent().unwrap(), cx);
    let _other = open_window(a.parent().unwrap(), cx);
    let (_held, holding) = async_channel::bounded::<()>(1);
    let (done, finished) = async_channel::bounded::<()>(1);
    cx.update(|cx| {
        let never = cx.spawn(async move |_| {
            holding.recv().await.ok();
        });
        let landing = ipc::while_open(window, never, cx);
        cx.spawn(async move |_| {
            landing.await;
            done.try_send(()).ok();
        })
        .detach();
    });
    cx.run_until_parked();
    assert!(finished.try_recv().is_err(), "the window is open");

    window
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert!(finished.try_recv().is_ok());
}
