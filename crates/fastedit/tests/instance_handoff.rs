// The launch handoff runs over Unix sockets; Windows builds stand in for
// them, and every launch there is its own instance.
#![cfg(unix)]

//! A `tenmoku` launch handed to the running instance: the server's end in a
//! live app, with the real binary, or `ipc::start`, as the client.

#[allow(dead_code)]
#[path = "../src/cli.rs"]
mod cli;
// cli.rs leaves the catalogue to the test build's includer.
#[allow(dead_code)]
#[path = "../src/ipc.rs"]
mod ipc;
#[path = "../src/l10n.rs"]
mod l10n;

use std::{
    fs,
    io::Read as _,
    os::unix::net::UnixListener,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use cli::Target;
use fe_gpui::{EditorSettings, EditorTheme};
use fe_workbench::{Launch, Workbench};
use gpui::{App, AppContext as _, TestAppContext, WindowHandle, WindowOptions};
use ipc::{Handoff, How, Request, Start};

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

/// Opens a window on `root`, as the host's opener does.
fn open_window(root: Option<PathBuf>, cx: &mut App) -> Option<WindowHandle<Workbench>> {
    let window = cx.open_window(WindowOptions::default(), |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        let launch = Launch { root, file: None };
        cx.new(|cx| Workbench::new(launch, theme, EditorSettings::default(), window, cx))
    });
    Some(window.unwrap())
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

fn next_handoff(handoffs: &async_channel::Receiver<Handoff>) -> Handoff {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(handoff) = handoffs.try_recv() {
            return handoff;
        }
        assert!(Instant::now() < deadline, "no request came");
        thread::sleep(Duration::from_millis(5));
    }
}

/// Acts on `handoff` as the host does, and waits until its files have landed.
fn act_on(handoff: Handoff, cx: &mut TestAppContext) {
    let request = handoff.request.clone();
    let landed = cx.update(|cx| ipc::route(request, cx, open_window));
    handoff.answer();
    let (done, finished) = async_channel::bounded::<()>(1);
    cx.update(|cx| {
        cx.spawn(async move |_| {
            landed.await;
            done.try_send(()).ok();
        })
        .detach()
    });
    let deadline = Instant::now() + Duration::from_secs(60);
    while finished.try_recv().is_err() {
        cx.run_until_parked();
        assert!(Instant::now() < deadline, "the files never landed");
        thread::sleep(Duration::from_millis(2));
    }
    cx.run_until_parked();
}

fn tabs(window: WindowHandle<Workbench>, cx: &mut TestAppContext) -> Vec<PathBuf> {
    cx.update(|cx| {
        window
            .read_with(cx, |workbench, cx| {
                let pane = workbench.pane().read(cx);
                (pane.tabs().iter())
                    .filter_map(|tab| tab.path.clone())
                    .collect()
            })
            .unwrap()
    })
}

/// The active tab's file, and its caret's 0-based row and byte column.
fn active(window: WindowHandle<Workbench>, cx: &mut TestAppContext) -> (PathBuf, u32, u32) {
    cx.update(|cx| {
        window
            .read_with(cx, |workbench, cx| {
                let pane = workbench.pane().read(cx);
                let path = pane.tabs()[pane.active().unwrap()].path.clone().unwrap();
                let editor = pane.active_editor().unwrap().read(cx);
                let model = editor.model();
                let head = model.primary_selection().head();
                let point = model.snapshot().text().offset_to_point(head);
                (path, point.row, point.column)
            })
            .unwrap()
    })
}

/// A window that's still there, on `root`.
fn window_on(root: &Path, cx: &mut TestAppContext) -> WindowHandle<Workbench> {
    let windows: Vec<_> = cx
        .windows()
        .into_iter()
        .filter_map(|window| window.downcast::<Workbench>())
        .collect();
    let found = windows.into_iter().rev().find(|window| {
        cx.update(|cx| {
            window
                .read_with(cx, |workbench, _| workbench.root() == Some(root))
                .unwrap()
        })
    });
    found.unwrap_or_else(|| panic!("no window on {}", root.display()))
}

/// Kills the client if the test fails first: it'd stand a window up if it
/// found no server.
struct Client(std::process::Child);

impl Drop for Client {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

#[gpui::test]
fn second_launch_routes_files_to_the_live_window_and_exits_0(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let base = tempfile::tempdir().unwrap();
    let project = dunce::canonicalize(dir.path()).unwrap();
    let (a, b) = (project.join("a.txt"), project.join("b.txt"));
    fs::write(&a, "one\ntwo words\nthree\n").unwrap();
    fs::write(&b, "bee\n").unwrap();
    let (window, handoffs) = serving(cx, &project, base.path());

    // The real binary, by a relative path and with a position.
    let child = Command::new(env!("CARGO_BIN_EXE_tenmoku"))
        .args(["a.txt:2:5", "b.txt"])
        .current_dir(&project)
        .env("TMPDIR", base.path())
        .env_remove("TENMOKU_NO_IPC")
        .env_remove("TENMOKU_FOREGROUND")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut client = Client(child);
    let handoff = next_handoff(&handoffs);
    assert_eq!(handoff.request.cwd, project);
    assert_eq!(handoff.request.targets.len(), 2);
    act_on(handoff, cx);

    // It's over, having said nothing.
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = client.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "the client never exited");
        thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(status.code(), Some(0));
    let mut said = String::new();
    let mut stdout = client.0.stdout.take().unwrap();
    stdout.read_to_string(&mut said).unwrap();
    assert_eq!(said, "");

    // In the live window, in order, the last active, the first at its place.
    assert_eq!(cx.windows().len(), 1);
    assert_eq!(tabs(window, cx), [a.clone(), b.clone()]);
    assert_eq!(active(window, cx), (b, 0, 0));
    cx.update(|cx| {
        let task = window
            .update(cx, |workbench, window, cx| {
                workbench.open_document_at(a.clone(), None, None, window, cx)
            })
            .unwrap();
        task.detach();
    });
    cx.run_until_parked();
    assert_eq!(active(window, cx), (a, 1, 4));
}

#[gpui::test]
fn new_window_opens_a_second_window_on_the_target_folder(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let base = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let project = root.join("project");
    let other = root.join("other");
    fs::create_dir_all(&project).unwrap();
    fs::create_dir_all(&other).unwrap();
    let inside = project.join("a.txt");
    fs::write(&inside, "a\nb\n").unwrap();
    let elsewhere = other.join("c.txt");
    fs::write(&elsewhere, "c\n").unwrap();
    let (first, handoffs) = serving(cx, &project, base.path());

    let send = |targets: Vec<Target>| {
        let sent = Request {
            targets,
            new_window: true,
            wait: false,
            cwd: root.clone(),
        };
        let how = how(base.path());
        thread::spawn(move || {
            assert!(matches!(ipc::start(&sent, &how, no_detach), Start::Exit));
        })
    };
    let at = |path: &Path, line| Target {
        path: path.to_owned(),
        line,
        col: None,
    };

    // A folder no window is on.
    let client = send(vec![at(&elsewhere, Some(1))]);
    act_on(next_handoff(&handoffs), cx);
    client.join().unwrap();
    assert_eq!(cx.windows().len(), 2);
    let second = window_on(&other, cx);
    assert_ne!(second.window_id(), first.window_id());
    assert_eq!(tabs(second, cx), std::slice::from_ref(&elsewhere));
    assert_eq!(tabs(first, cx), Vec::<PathBuf>::new());

    // Always: the folder of a file the first window would otherwise take.
    let client = send(vec![at(&inside, Some(2))]);
    act_on(next_handoff(&handoffs), cx);
    client.join().unwrap();
    assert_eq!(cx.windows().len(), 3);
    let third = window_on(&project, cx);
    assert_ne!(third.window_id(), first.window_id());
    assert_eq!(tabs(third, cx), std::slice::from_ref(&inside));
    assert_eq!(active(third, cx), (inside, 1, 0));
    assert_eq!(tabs(first, cx), Vec::<PathBuf>::new());

    // A folder given outright, and no target at all.
    let client = send(vec![at(&other, None)]);
    act_on(next_handoff(&handoffs), cx);
    client.join().unwrap();
    assert_eq!(cx.windows().len(), 4);
    let client = send(vec![]);
    act_on(next_handoff(&handoffs), cx);
    client.join().unwrap();
    assert_eq!(cx.windows().len(), 5);
}

/// A live server that a launch would hand over to, were it not told otherwise.
#[test]
fn tenmoku_no_ipc_env_skips_both_sides() {
    let dir = tempfile::tempdir().unwrap();
    let socket = ipc::socket_path(dir.path());
    fs::create_dir(socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&socket).unwrap();
    listener.set_nonblocking(true).unwrap();

    let mut how = How::from_env(|name| (name == "TENMOKU_NO_IPC").then(|| "1".into()));
    assert!(how.no_ipc, "the variable sets it");
    how.temp_dir = dir.path().to_owned();
    let request = Request {
        targets: vec![],
        new_window: false,
        wait: true,
        cwd: dir.path().to_owned(),
    };
    let detach = || panic!("no detaching");
    assert!(matches!(ipc::start(&request, &how, detach), Start::Alone));
    assert!(listener.accept().is_err(), "the client side is skipped");

    // Nor does it make a server's folder, socket or lock where there's none.
    let empty = tempfile::tempdir().unwrap();
    how.temp_dir = empty.path().to_owned();
    assert!(matches!(ipc::start(&request, &how, detach), Start::Alone));
    assert_eq!(fs::read_dir(empty.path()).unwrap().count(), 0);
}
