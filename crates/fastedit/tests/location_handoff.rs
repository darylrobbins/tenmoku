//! Remote folders' `ssh://` locations through a launch: parsed by the
//! command line as they are, handed to the running instance like a path,
//! and reopened from the last quit without looking for the folder; with no
//! remote host support yet, each launch says on stderr that it can't open
//! one: the real binary's too, handing over and launched cold from a
//! terminal (docs/decisions/j8wvgszah0-remote-development.md § 6).

#[allow(dead_code)]
#[path = "../src/cli.rs"]
mod cli;
// cli.rs leaves the catalogue to the test build's includer.
#[allow(dead_code)]
#[path = "../src/ipc.rs"]
mod ipc;
#[path = "../src/l10n.rs"]
mod l10n;
#[allow(dead_code)]
#[path = "../src/reopen.rs"]
mod reopen;

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use cli::{Cli, Target};
use fe_config::recent::ProjectLocation;
use ipc::Request;
use kiln::{command::Value, core::layout::LayoutStore};

const NEON_APP: &str = "ssh://neon/home/me/src/app";

/// What a launch says of a remote folder it can't open.
const UNSUPPORTED: &str =
    "tenmoku: ssh://neon/home/me/src/app: remote folders aren't supported yet\n";

fn neon_app() -> ProjectLocation {
    ProjectLocation::parse(NEON_APP).unwrap()
}

fn remote_target(text: &str) -> Target {
    Target {
        path: PathBuf::from(text),
        line: None,
        col: None,
    }
}

fn cli(targets: Vec<Target>) -> Cli {
    Cli {
        perf: false,
        new_window: false,
        wait: false,
        targets,
    }
}

#[test]
fn an_ssh_argument_is_a_remote_target_as_given() {
    let args: Vec<OsString> = vec![NEON_APP.into(), "ssh://me@box:2222/srv/app".into()];
    let parsed = cli::parse(args).unwrap();
    assert_eq!(
        parsed.targets,
        [
            remote_target(NEON_APP),
            remote_target("ssh://me@box:2222/srv/app")
        ]
    );
    assert_eq!(parsed.targets[0].remote(), Some(Ok(neon_app())));
}

#[test]
fn a_launch_plan_leaves_a_remote_folder_out() {
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().to_owned();
    let launch = cli(vec![
        remote_target(NEON_APP),
        Target {
            path: local.clone(),
            line: None,
            col: None,
        },
    ]);
    let plan = launch.plan();
    assert_eq!(plan.root, Some(local), "the local folder is the root");
    assert!(plan.files.is_empty(), "the location is no file");
}

#[test]
fn a_location_that_does_not_parse_is_said_so() {
    let mut stderr = Vec::new();
    cli::report_remote(&[remote_target("ssh://neon")], &mut stderr);
    assert_eq!(
        String::from_utf8(stderr).unwrap(),
        "tenmoku: ssh://neon: no absolute path after the host\n"
    );
}

#[test]
fn a_remote_target_crosses_the_wire_as_its_uri() {
    let request = Request {
        targets: vec![remote_target(NEON_APP)],
        new_window: false,
        wait: false,
        cwd: PathBuf::from("/home/me"),
    };
    let line = request.to_line();
    assert!(
        line.contains(r#""path":"ssh://neon/home/me/src/app""#),
        "{line}"
    );
    assert_eq!(Request::from_line(&line).unwrap(), request);
}

/// A saved entry, as a quit leaves it, showing `file` in its one pane.
fn json_entry(id: u64, saved: u64, session: u64, file: &Path) -> Value {
    serde_json::json!({
        "id": id,
        "saved": saved,
        "session": session,
        "panes": {"tree": {"pane": {"items": [
            {"kind": "editor", "document": 0, "state": {"path": file}}
        ], "active": 0}}, "active": []},
    })
}

/// Puts `entry` in `store`'s file, its root recorded, as a write does.
fn write(store: &LayoutStore, entry: Value) {
    fs::create_dir_all(store.file().parent().unwrap()).unwrap();
    let text = serde_json::json!({
        "version": 1,
        "root": store.root(),
        "windows": [entry],
    });
    fs::write(store.file(), serde_json::to_vec(&text).unwrap()).unwrap();
}

/// The layouts of a last quit with a window on `NEON_APP` and, if given, one
/// on the local folder `local`.
fn last_quit(layouts: &Path, local: Option<&Path>) {
    let remote = LayoutStore::new(layouts, Path::new(NEON_APP));
    write(
        &remote,
        json_entry(1, 100, 7, Path::new("/home/me/src/app/main.rs")),
    );
    if let Some(local) = local {
        let store = LayoutStore::new(layouts, local);
        write(&store, json_entry(2, 200, 7, &local.join("a.txt")));
    }
}

#[test]
fn a_remote_window_reopens_without_looking_for_its_folder() {
    let dir = tempfile::tempdir().unwrap();
    let layouts = dir.path().join("layouts");
    last_quit(&layouts, None);
    let windows = reopen::launch_locations_in(Some(layouts), None, true, &|_| true);
    let roots: Vec<_> = windows.iter().map(|(root, ..)| root.clone()).collect();
    assert_eq!(roots, [Some(neon_app())]);
    assert_eq!(
        windows[0].2.as_ref().and_then(|entry| entry["id"].as_u64()),
        Some(1),
        "with its saved window"
    );
}

#[test]
fn the_app_says_on_stderr_it_cant_reopen_a_remote_window() {
    let dir = tempfile::tempdir().unwrap();
    let layouts = dir.path().join("layouts");
    let local = dunce::canonicalize(dir.path()).unwrap().join("a");
    fs::create_dir(&local).unwrap();
    last_quit(&layouts, Some(&local));
    let mut stderr = Vec::new();
    let windows = reopen::launch_windows_reporting(Some(layouts.clone()), None, true, &mut stderr);
    assert_eq!(String::from_utf8(stderr).unwrap(), UNSUPPORTED);
    let roots: Vec<_> = windows.iter().map(|(root, ..)| root.clone()).collect();
    assert_eq!(roots, [Some(local)], "the local window reopens");
    drop(windows);

    // The remote window's entry is kept, as a gone folder's is.
    let remote = LayoutStore::new(&layouts, Path::new(NEON_APP));
    let kept: Value = serde_json::from_slice(&fs::read(remote.file()).unwrap()).unwrap();
    assert_eq!(kept["windows"][0]["id"].as_u64(), Some(1), "kept");
}

#[test]
fn a_launch_with_only_a_remote_window_to_reopen_opens_one_with_no_folder() {
    let dir = tempfile::tempdir().unwrap();
    let layouts = dir.path().join("layouts");
    last_quit(&layouts, None);
    let mut stderr = Vec::new();
    let windows = reopen::launch_windows_reporting(Some(layouts), None, true, &mut stderr);
    assert_eq!(String::from_utf8(stderr).unwrap(), UNSUPPORTED);
    assert!(matches!(&windows[..], [(None, Some(_), None)]));
}

#[cfg(unix)]
mod handoff {
    //! The launching process's end, as `ipc::start` or the real binary, and
    //! the running instance's, in a live app.

    use std::{
        cell::Cell,
        process::{Command, Stdio},
        rc::Rc,
    };

    use fe_gpui::{EditorSettings, EditorTheme};
    use fe_workbench::{Launch, Workbench};
    use gpui::{App, AppContext as _, TestAppContext, WindowHandle, WindowOptions};

    use super::*;
    use crate::ipc::{Handoff, How, Start};

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

    fn tabs(window: WindowHandle<Workbench>, cx: &mut TestAppContext) -> usize {
        cx.update(|cx| {
            window
                .read_with(cx, |workbench, cx| workbench.pane().read(cx).tabs().len())
                .unwrap()
        })
    }

    /// A live app with a window on `root`.
    fn app(cx: &mut TestAppContext, root: &Path) -> WindowHandle<Workbench> {
        cx.executor().allow_parking();
        cx.update(|cx| {
            fe_gpui::init(cx);
            fe_workbench::init(cx);
        });
        let window = cx
            .update(|cx| open_window(Some(root.to_owned()), cx))
            .unwrap();
        cx.run_until_parked();
        window
    }

    /// Routes `request` as the host does, counting the windows it opens.
    fn route(request: Request, cx: &mut TestAppContext) -> usize {
        let opened = Rc::new(Cell::new(0));
        let counted = opened.clone();
        let landed = cx.update(|cx| {
            ipc::route(request, cx, move |root, cx| {
                counted.set(counted.get() + 1);
                open_window(root, cx)
            })
        });
        landed.detach();
        cx.run_until_parked();
        opened.get()
    }

    #[test]
    fn a_launch_says_on_stderr_it_cant_open_a_remote_folder() {
        // `tenmoku ssh://…` from a terminal, with no instance running: it
        // detaches, and says so before it does.
        let base = tempfile::tempdir().unwrap();
        let parsed = cli::parse(vec![NEON_APP.into()]).unwrap();
        let request = Request::of(&parsed, PathBuf::from("/"));
        let how = How {
            foreground: false,
            terminal: true,
            ..how(base.path())
        };
        let detached = Cell::new(false);
        let mut stderr = Vec::new();
        let detach = || {
            detached.set(true);
            Ok(())
        };
        let start = ipc::start_telling(&request, &how, detach, &mut stderr);
        assert!(matches!(start, Start::Exit), "it detached");
        assert!(detached.get());
        assert_eq!(String::from_utf8(stderr).unwrap(), UNSUPPORTED);
    }

    #[gpui::test]
    fn a_handed_off_remote_folder_opens_nothing(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let base = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let window = app(cx, &root);
        let empty = Request {
            targets: vec![],
            new_window: false,
            wait: false,
            cwd: root.clone(),
        };
        let Start::Serve(server) = ipc::start(&empty, &how(base.path()), no_detach) else {
            panic!("this launch is the server");
        };
        let handoffs = server.listen();
        let tabs_before = tabs(window, cx);

        // The real `tenmoku ssh://neon/home/me/src/app`, from the folder.
        let client = Command::new(env!("CARGO_BIN_EXE_tenmoku"))
            .arg(NEON_APP)
            .current_dir(&root)
            .env("TMPDIR", base.path())
            .env_remove("TENMOKU_NO_IPC")
            .env_remove("TENMOKU_FOREGROUND")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let handoff: Handoff = handoffs.recv_blocking().expect("the request comes");
        assert_eq!(handoff.request.targets, [remote_target(NEON_APP)]);
        let request = handoff.request.clone();
        let opened = route(request, cx);
        handoff.answer();
        let output = client.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(UNSUPPORTED), "the launch says so: {stderr}");
        assert_eq!(opened, 0, "no window opens");
        assert_eq!(tabs(window, cx), tabs_before, "no tab opens");
    }

    #[gpui::test]
    fn a_remote_only_new_window_handoff_opens_no_window(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let window = app(cx, &root);
        let tabs_before = tabs(window, cx);
        let request = Request {
            targets: vec![remote_target(NEON_APP)],
            new_window: true,
            wait: false,
            cwd: root.clone(),
        };
        assert_eq!(route(request, cx), 0, "no window opens");
        assert_eq!(tabs(window, cx), tabs_before, "no tab opens");
    }
}

/// The real binary, launched from a terminal with no instance running.
/// Linux only: a detached server there that can't reach a display ends at
/// once, where macOS would open a real window.
#[cfg(target_os = "linux")]
mod cold_launch {
    use std::{
        ffi::CStr,
        fs::File,
        os::fd::{AsRawFd as _, FromRawFd as _, OwnedFd},
        process::{Command, Stdio},
    };

    use super::*;

    /// A new pseudo-terminal: its controller, and its terminal end.
    fn pty() -> (File, File) {
        // SAFETY: plain libc calls on a descriptor this function opened, with
        // a buffer as long as it says.
        unsafe {
            let controller = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            assert!(controller >= 0, "posix_openpt");
            let controller = OwnedFd::from_raw_fd(controller);
            assert_eq!(libc::grantpt(controller.as_raw_fd()), 0, "grantpt");
            assert_eq!(libc::unlockpt(controller.as_raw_fd()), 0, "unlockpt");
            let mut name = [0 as libc::c_char; 128];
            let named = libc::ptsname_r(controller.as_raw_fd(), name.as_mut_ptr(), name.len());
            assert_eq!(named, 0, "ptsname_r");
            let name = CStr::from_ptr(name.as_ptr()).to_str().unwrap().to_owned();
            let terminal = File::options().read(true).write(true).open(name).unwrap();
            (File::from(controller), terminal)
        }
    }

    #[test]
    fn a_terminal_launch_says_on_its_stderr_it_cant_open_a_remote_folder() {
        let dir = tempfile::tempdir().unwrap();
        let sub = |name: &str| {
            let path = dir.path().join(name);
            fs::create_dir(&path).unwrap();
            path
        };
        let (_controller, terminal) = pty();
        // Its stdin is the terminal: the launch detaches, as one from a shell
        // does, and its detached server, given a display it can't reach,
        // ends at once.
        let output = Command::new(env!("CARGO_BIN_EXE_tenmoku"))
            .arg(NEON_APP)
            .current_dir(dir.path())
            .env("TMPDIR", sub("tmp"))
            .env("HOME", sub("home"))
            .env("TENMOKU_CONFIG_DIR", sub("config"))
            .env("TENMOKU_STATE_DIR", sub("state"))
            .env("WAYLAND_DISPLAY", dir.path().join("no-compositor"))
            .env_remove("DISPLAY")
            .env_remove("TENMOKU_NO_IPC")
            .env_remove("TENMOKU_FOREGROUND")
            .stdin(Stdio::from(terminal))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(String::from_utf8_lossy(&output.stderr), UNSUPPORTED);
    }
}
