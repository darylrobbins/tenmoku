// The launch handoff runs over Unix sockets; Windows builds stand in for
// them, and every launch there is its own instance.
#![cfg(unix)]

//! `tenmoku --wait` end to end: the server's end in a live app, with the real
//! binary, or `ipc::start`, as the client waiting on it.

#[allow(dead_code)]
#[path = "../src/cli.rs"]
mod cli;
#[allow(dead_code)]
#[path = "../src/ipc.rs"]
mod ipc;
#[allow(dead_code)]
#[path = "../src/wait.rs"]
mod wait;

use std::{
    fs,
    os::unix::{net::UnixStream, process::ExitStatusExt as _},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use cli::Target;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use fe_workbench::{Launch, Workbench};
use gpui::{App, AppContext as _, TestAppContext, WindowHandle, WindowOptions};
use ipc::{Handoff, How, Request, Start};

const LONG: Duration = Duration::from_secs(60);

fn how(temp_dir: &Path) -> How {
    How {
        no_ipc: false,
        foreground: true,
        parent_carries: false,
        terminal: false,
        launch_services: false,
        temp_dir: temp_dir.to_owned(),
    }
}

fn no_detach() -> std::io::Result<()> {
    panic!("this launch doesn't detach")
}

fn open_window(root: Option<PathBuf>, cx: &mut App) -> Option<WindowHandle<Workbench>> {
    let window = cx.open_window(WindowOptions::default(), |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        let launch = Launch { root, file: None };
        cx.new(|cx| Workbench::new(launch, theme, EditorSettings::default(), window, cx))
    });
    Some(window.unwrap())
}

/// A folder holding `a.txt`, and where a server's socket goes.
fn project() -> (tempfile::TempDir, tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    fs::write(root.join("a.txt"), "a\n").unwrap();
    (dir, base, root)
}

/// A live app with a window on `root`, serving under `temp_dir`.
fn serving(
    cx: &mut TestAppContext,
    root: &Path,
    temp_dir: &Path,
) -> (
    WindowHandle<Workbench>,
    async_channel::Receiver<ipc::Handoff>,
) {
    cx.executor().allow_parking();
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
        wait::init(cx);
    });
    let window = cx
        .update(|cx| open_window(Some(root.to_owned()), cx))
        .unwrap();
    cx.run_until_parked();
    let empty = Request {
        targets: vec![],
        new_window: false,
        wait: false,
        cwd: root.to_owned(),
    };
    let Start::Serve(server) = ipc::start(&empty, &how(temp_dir), no_detach) else {
        panic!("this launch is the server");
    };
    (window, server.listen())
}

fn wait_request(path: &Path) -> Request {
    Request {
        targets: vec![Target {
            path: path.to_owned(),
            line: None,
            col: None,
        }],
        new_window: false,
        wait: true,
        cwd: path.parent().unwrap().to_owned(),
    }
}

/// A client on a thread, which is over when `ipc::start` returns.
fn client(request: Request, temp_dir: &Path) -> thread::JoinHandle<bool> {
    let how = how(temp_dir);
    thread::spawn(move || matches!(ipc::start(&request, &how, no_detach), Start::Exit))
}

/// The real binary, waiting on `a.txt`.
fn client_process(root: &Path, temp_dir: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_tenmoku"))
        .args(["--wait", "a.txt"])
        .current_dir(root)
        .env("TMPDIR", temp_dir)
        .env_remove("TENMOKU_NO_IPC")
        .env_remove("TENMOKU_FOREGROUND")
        .env_remove("TENMOKU_PARENT_CARRIES")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn next_handoff(handoffs: &async_channel::Receiver<Handoff>) -> Handoff {
    let deadline = Instant::now() + LONG;
    loop {
        if let Ok(handoff) = handoffs.try_recv() {
            return handoff;
        }
        assert!(Instant::now() < deadline, "no request came");
        thread::sleep(Duration::from_millis(5));
    }
}

/// Serves the next request, as the host does, until its file has its tab.
fn serve_next(
    handoffs: &async_channel::Receiver<Handoff>,
    window: WindowHandle<Workbench>,
    cx: &mut TestAppContext,
) {
    let handoff = next_handoff(handoffs);
    let path = handoff.request.targets[0].path.clone();
    cx.update(|cx| wait::serve(handoff, cx, open_window));
    settle_until(cx, |cx| {
        tabs(window, cx).contains(&path) || cx.update(|cx| wait::pending(cx)) == 0
    });
}

fn tabs(window: WindowHandle<Workbench>, cx: &mut TestAppContext) -> Vec<PathBuf> {
    window
        .read_with(cx, |workbench, cx| {
            (workbench.pane_group(cx).panes().into_iter())
                .flat_map(|pane| pane.read(cx).tabs())
                .filter_map(|tab| tab.path.clone())
                .collect()
        })
        .unwrap()
}

/// Runs the app until `done`, which real threads and files are on their way to.
fn settle_until(cx: &mut TestAppContext, mut done: impl FnMut(&mut TestAppContext) -> bool) {
    let deadline = Instant::now() + LONG;
    loop {
        cx.run_until_parked();
        if done(cx) {
            return;
        }
        assert!(Instant::now() < deadline, "it never came about");
        thread::sleep(Duration::from_millis(2));
    }
}

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

/// How `child` ends, the app running meanwhile.
fn status_of(child: &mut Child, cx: &mut TestAppContext) -> ExitStatus {
    let mut status = None;
    settle_until(cx, |_| {
        status = child.try_wait().unwrap();
        status.is_some()
    });
    status.unwrap()
}

/// A little while for an answer that shouldn't come, to come.
fn still_waiting(thread: &thread::JoinHandle<bool>, cx: &mut TestAppContext) {
    for _ in 0..20 {
        cx.run_until_parked();
        thread::sleep(Duration::from_millis(5));
    }
    assert!(!thread.is_finished(), "the client is still waiting");
}

/// Cmd-Q, or the last window's closing: `cx.quit()` runs what they both do.
#[gpui::test]
fn quitting_the_app_answers_every_pending_wait(cx: &mut TestAppContext) {
    let (_dir, base, root) = project();
    let b = root.join("b.txt");
    fs::write(&b, "b\n").unwrap();
    let (window, handoffs) = serving(cx, &root, base.path());

    let first = client(wait_request(&root.join("a.txt")), base.path());
    serve_next(&handoffs, window, cx);
    let second = client(wait_request(&b), base.path());
    serve_next(&handoffs, window, cx);
    assert_eq!(cx.update(|cx| wait::pending(cx)), 2);
    still_waiting(&first, cx);
    still_waiting(&second, cx);

    cx.quit();
    assert!(first.join().unwrap(), "told the files have closed");
    assert!(second.join().unwrap(), "told the files have closed");
}

#[gpui::test]
fn a_dirty_tab_waits_until_its_save_or_discard_prompt_resolves(cx: &mut TestAppContext) {
    let (_dir, base, root) = project();
    let (window, handoffs) = serving(cx, &root, base.path());
    let waiting = client(wait_request(&root.join("a.txt")), base.path());
    serve_next(&handoffs, window, cx);

    window
        .update(cx, |workbench, _, cx| {
            let editor = workbench.pane().read(cx).active_editor().unwrap().clone();
            editor.update(cx, |editor, cx| {
                editor.model_mut().insert("edited ");
                cx.notify();
            });
        })
        .unwrap();
    cx.run_until_parked();

    run("workbench.close_tab", window, cx);
    let (question, _) = cx.pending_prompt().expect("it asks");
    assert!(question.contains("a.txt"), "{question}");
    still_waiting(&waiting, cx);

    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(tabs(window, cx), [root.join("a.txt")]);
    still_waiting(&waiting, cx);

    run("workbench.close_tab", window, cx);
    assert!(cx.pending_prompt().is_some(), "it asks again");
    still_waiting(&waiting, cx);
    cx.simulate_prompt_answer("Discard");
    settle_until(cx, |_| waiting.is_finished());
    assert!(waiting.join().unwrap(), "told the files have closed");
    assert_eq!(tabs(window, cx), Vec::<PathBuf>::new());
}

/// A client that goes, whichever way, is forgotten: its tab stays open, and
/// closing that tells no one.
#[gpui::test]
fn client_disconnect_forgets_the_wait(cx: &mut TestAppContext) {
    let (_dir, base, root) = project();
    let a = root.join("a.txt");
    let (window, handoffs) = serving(cx, &root, base.path());
    let forgotten = |cx: &mut TestAppContext| cx.update(|cx| wait::pending(cx)) == 0;

    for (signal, name) in [(libc::SIGINT, "SIGINT"), (libc::SIGTERM, "SIGTERM")] {
        let mut waiting = client_process(&root, base.path());
        serve_next(&handoffs, window, cx);
        assert_eq!(cx.update(|cx| wait::pending(cx)), 1, "{name}");

        // SAFETY: the child is ours, and hasn't been waited for.
        assert_eq!(unsafe { libc::kill(waiting.id() as i32, signal) }, 0);
        let status = status_of(&mut waiting, cx);
        assert_eq!(status.signal(), Some(signal), "{name}");
        settle_until(cx, forgotten);
        assert_eq!(tabs(window, cx), std::slice::from_ref(&a), "{name}");
    }

    // A connection closed with no signal at all.
    let socket = ipc::socket_path(base.path());
    let mut stream = UnixStream::connect(socket).unwrap();
    std::io::Write::write_all(
        &mut stream,
        format!("{}\n", wait_request(&a).to_line()).as_bytes(),
    )
    .unwrap();
    serve_next(&handoffs, window, cx);
    assert_eq!(cx.update(|cx| wait::pending(cx)), 1);
    drop(stream);
    settle_until(cx, forgotten);

    run("workbench.close_tab", window, cx);
    assert_eq!(tabs(window, cx), Vec::<PathBuf>::new());
}

/// The real client's exit codes: 0 once told, 1 if the server goes first.
#[gpui::test]
fn the_client_exits_0_when_told_and_1_when_the_server_crashes(cx: &mut TestAppContext) {
    let (_dir, base, root) = project();
    let (window, handoffs) = serving(cx, &root, base.path());

    let mut told = client_process(&root, base.path());
    serve_next(&handoffs, window, cx);
    run("workbench.close_tab", window, cx);
    assert_eq!(status_of(&mut told, cx).code(), Some(0));

    // A server that takes the request and is gone, as a crash leaves it.
    let mut stranded = client_process(&root, base.path());
    drop(next_handoff(&handoffs).hold());
    assert_eq!(status_of(&mut stranded, cx).code(), Some(1));
}

/// A request's window closes with the file still being read, another window
/// left open: nothing will land, and the client is told. The app is stepped
/// only as far as asking for the file, so its read hasn't run as the window goes.
#[gpui::test]
fn a_window_closing_during_the_read_ends_the_wait(cx: &mut TestAppContext) {
    let (_dir, base, root) = project();
    let other = root.join("other");
    fs::create_dir(&other).unwrap();
    let held = other.join("held.txt");
    fs::write(&held, "held\n").unwrap();
    let (_window, handoffs) = serving(cx, &root, base.path());
    let closing = cx
        .update(|cx| open_window(Some(other.clone()), cx))
        .unwrap();
    cx.run_until_parked();

    let waiting = client(wait_request(&held), base.path());
    let handoff = next_handoff(&handoffs);
    cx.update(|cx| wait::serve(handoff, cx, open_window));
    assert_eq!(cx.update(|cx| wait::pending(cx)), 1);
    let deadline = Instant::now() + LONG;
    while !closing
        .update(cx, |_, window, _| window.is_window_active())
        .unwrap()
    {
        assert!(cx.executor().tick(), "the request ran out of things to do");
        assert!(Instant::now() < deadline, "the file was never asked for");
    }
    assert_eq!(tabs(closing, cx), Vec::<PathBuf>::new(), "it's being read");
    closing
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    settle_until(cx, |_| waiting.is_finished());
    assert!(waiting.join().unwrap(), "told the files have closed");
}

/// `TENMOKU_FOREGROUND=1 tenmoku --wait a.txt` with no server: it serves, with
/// no client to tell, and the end of the wait is its own.
#[gpui::test]
fn a_foreground_wait_with_no_server_ends_with_its_tabs(cx: &mut TestAppContext) {
    let (_dir, base, root) = project();
    let a = root.join("a.txt");
    cx.executor().allow_parking();
    cx.update(|cx| {
        fe_gpui::init(cx);
        fe_workbench::init(cx);
        wait::init(cx);
    });
    let request = wait_request(&a);
    let how = how(base.path());
    let Start::Serve(_server) = ipc::start(&request, &how, no_detach) else {
        panic!("with no server, it serves");
    };
    assert!(how.waits_by_itself(request.wait));

    let window = cx.update(|cx| open_window(Some(root.clone()), cx)).unwrap();
    let opened = window
        .update(cx, |workbench, window, cx| {
            workbench.open_document(a.clone(), window, cx);
        })
        .is_ok();
    assert!(opened);
    cx.run_until_parked();
    let (done, quit) = async_channel::bounded::<()>(1);
    cx.update(|cx| {
        wait::add_own(
            move |_| _ = done.try_send(()),
            vec![a.clone()],
            gpui::Task::ready(()),
            cx,
        );
    });
    cx.run_until_parked();
    assert!(quit.try_recv().is_err(), "its file is open");
    run("workbench.close_tab", window, cx);
    assert!(quit.try_recv().is_ok());
}
