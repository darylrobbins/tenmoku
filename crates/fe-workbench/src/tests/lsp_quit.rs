//! Quitting force-stops language servers that outlast the graceful wait
//! (T210): a launch in progress is cancelled, a server that never answers
//! is killed, and a launcher that ignores its cancel is given up on after
//! the force wait. The manager's clock is manual and never moves, so only
//! `shut_down_all_within`'s real-time grace and force ever run out.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use fe_gpui::{EditorSettings, EditorTheme};
use fe_lsp::{
    LaunchCancel, ManualClock, ServerIo, ServerLauncher, ServerSpec,
    fake::{FakeLauncher, FakeServer, InitializeReply, Script},
};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    Launch, Workbench,
    lsp::{Quit, TestClock, shut_down_all_within},
    set_server_launcher,
};

const GRACE: Duration = Duration::from_millis(400);
const FORCE: Duration = Duration::from_millis(400);

/// Blocks each launch until its cancel fires, then gives up with
/// `Interrupted`; remembers it saw the cancel.
struct Cooperating {
    entered: Mutex<mpsc::Sender<()>>,
    cancelled: Arc<AtomicBool>,
}

impl ServerLauncher for Cooperating {
    fn launch(&self, _: &ServerSpec, _: &Path, cancel: &LaunchCancel) -> io::Result<ServerIo> {
        self.entered.lock().unwrap().send(()).ok();
        let started = Instant::now();
        while !cancel.is_cancelled() {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "never cancelled"
            );
            thread::sleep(Duration::from_millis(2));
        }
        self.cancelled.store(true, Ordering::SeqCst);
        Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
    }
}

/// Launches the fake with a stderr that never ends, so the server thread
/// reads on after the server's graceful exit, its process group possibly
/// still alive.
struct EndlessStderr(Arc<fe_lsp::fake::FakeLauncher>);

impl ServerLauncher for EndlessStderr {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        let mut io = self.0.launch(spec, root, cancel)?;
        io.stderr = Some(Box::pin(futures::io::repeat(b'!')));
        Ok(io)
    }
}

/// Blocks each launch until released, ignoring the cancel, then launches
/// the fake.
struct Ignoring {
    fake: Arc<FakeLauncher>,
    entered: Mutex<mpsc::Sender<()>>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl ServerLauncher for Ignoring {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        self.entered.lock().unwrap().send(()).ok();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(10))
            .ok();
        self.fake.launch(spec, root, cancel)
    }
}

/// A trusted workbench over a project with `main.rs`, started with
/// `launcher` as the host's.
struct Quitting {
    workbench: Entity<Workbench>,
    cx: VisualTestContext,
    root: PathBuf,
    _dir: tempfile::TempDir,
}

impl Quitting {
    fn new(cx: &mut TestAppContext, launcher: Arc<dyn ServerLauncher>) -> Self {
        // Server threads wake the workbench's event loop.
        cx.executor().allow_parking();
        let clock = Arc::new(ManualClock::new());
        cx.update(|cx| {
            if !cx.has_global::<crate::Commands>() {
                fe_gpui::init(cx);
                crate::init(cx);
            }
            set_server_launcher(cx, launcher);
            cx.set_global(TestClock(clock));
        });
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let launch = Launch {
            root: Some(root.clone()),
            file: None,
        };
        let window = cx.add_window(move |window, cx| {
            let theme = EditorTheme::default_dark(cx);
            Workbench::new(launch, theme, EditorSettings::default(), window, cx)
        });
        let workbench = window.root(cx).unwrap();
        let mut cx = VisualTestContext::from_window(*window, cx);
        cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
        let mut quitting = Self {
            workbench,
            cx,
            root,
            _dir: dir,
        };
        quitting.draw();
        let workbench = quitting.workbench.clone();
        workbench.update(&mut quitting.cx, |workbench, cx| {
            workbench.set_root_trust(true, cx)
        });
        quitting.draw();
        quitting
    }

    fn draw(&mut self) {
        for _ in 0..2 {
            self.cx.run_until_parked();
            self.cx.update(|window, cx| window.draw(cx).clear(cx));
        }
    }

    fn open_main(&mut self) {
        let pane = self.workbench.read_with(&self.cx, |w, _| w.pane().clone());
        let path = self.root.join("main.rs");
        self.cx
            .update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        self.draw();
    }

    fn quit(&mut self, grace: Duration, force: Duration) -> Quit {
        self.cx
            .update(|_, cx| shut_down_all_within(cx, grace, force))
    }

    /// Closes the window, as the last one's closing does.
    fn close_window(&mut self) {
        self.cx.update(|window, _| window.remove_window());
        self.cx.run_until_parked();
    }
}

fn wait_until(what: &str, done: impl Fn() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(10), "{what}");
        thread::sleep(Duration::from_millis(2));
    }
}

#[gpui::test]
fn quit_cancels_a_blocked_launch(cx: &mut TestAppContext) {
    let fake = FakeServer::new(Script::default());
    let (entered, entering) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let launcher = Arc::new(Cooperating {
        entered: Mutex::new(entered),
        cancelled: cancelled.clone(),
    });
    let mut q = Quitting::new(cx, launcher);
    q.open_main();
    entering.recv_timeout(Duration::from_secs(10)).unwrap();

    let quit = q.quit(GRACE, FORCE);
    assert_eq!(quit, Quit { forced: 0, left: 0 });
    wait_until("the launcher sees the cancel", || {
        cancelled.load(Ordering::SeqCst)
    });
    assert_eq!(fake.launches(), 0);
    assert!(fake.records().is_empty());
}

#[gpui::test]
fn quit_kills_what_outlasts_the_grace(cx: &mut TestAppContext) {
    let fake = FakeServer::new(Script {
        answer_shutdown: false,
        ..Script::default()
    });
    let mut q = Quitting::new(cx, fake.launcher());
    q.open_main();
    fake.wait_initialized();

    let started = Instant::now();
    let quit = q.quit(GRACE, FORCE);
    assert_eq!(quit, Quit { forced: 1, left: 0 });
    assert!(started.elapsed() >= GRACE, "{:?}", started.elapsed());
    assert_eq!(fake.kills(), 1);
}

#[gpui::test]
fn a_launcher_ignoring_cancel_is_left(cx: &mut TestAppContext) {
    let fake = FakeServer::new(Script::default());
    let (entered, entering) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let launcher = Arc::new(Ignoring {
        fake: fake.launcher(),
        entered: Mutex::new(entered),
        release: Mutex::new(released),
    });
    let mut q = Quitting::new(cx, launcher);
    q.open_main();
    entering.recv_timeout(Duration::from_secs(10)).unwrap();

    let started = Instant::now();
    let quit = q.quit(GRACE, FORCE);
    let took = started.elapsed();
    assert_eq!(quit, Quit { forced: 1, left: 1 });
    assert!(took >= GRACE + FORCE, "{took:?}");
    assert!(took < GRACE + FORCE + Duration::from_secs(2), "{took:?}");

    // Once it returns, what it launched is killed, never initialized.
    release.send(()).unwrap();
    wait_until("the late server is killed", || fake.kills() == 1);
    assert_eq!(fake.launches(), 1);
    assert!(
        !fake.records().iter().any(|r| r.method() == "initialize"),
        "{:#?}",
        fake.records()
    );
}

#[gpui::test]
fn quit_kills_a_server_awaiting_initialize(cx: &mut TestAppContext) {
    let slow = Script {
        initialize_delay: Duration::from_secs(5),
        ..Script::default()
    };
    let never = Script {
        initialize: InitializeReply::Never,
        ..Script::default()
    };
    for script in [never, slow] {
        for window_open in [true, false] {
            let fake = FakeServer::new(script.clone());
            let mut q = Quitting::new(cx, fake.launcher());
            q.open_main();
            fake.wait_for("initialize", |records| {
                records.iter().any(|r| r.method() == "initialize")
            });
            if !window_open {
                q.close_window();
            }
            let quit = if window_open {
                q.quit(GRACE, FORCE)
            } else {
                cx.update(|cx| shut_down_all_within(cx, GRACE, FORCE))
            };
            assert_eq!(quit.left, 0, "{script:?} {window_open}");
            assert_eq!(fake.kills(), 1, "{script:?} {window_open}");
        }
    }
}

#[gpui::test]
fn quit_kills_a_server_stopped_but_not_reaped(cx: &mut TestAppContext) {
    for window_open in [true, false] {
        let fake = FakeServer::new(Script::default());
        let mut q = Quitting::new(cx, Arc::new(EndlessStderr(fake.launcher())));
        q.open_main();
        fake.wait_initialized();
        if !window_open {
            q.close_window();
        }
        // It shuts down and exits, reporting `Stopped`, but its thread
        // reads stderr on until the switch is thrown.
        let quit = if window_open {
            q.quit(GRACE, FORCE)
        } else {
            cx.update(|cx| shut_down_all_within(cx, GRACE, FORCE))
        };
        assert_eq!(quit, Quit { forced: 1, left: 0 }, "{window_open}");
        assert_eq!(fake.kills(), 1, "{window_open}");
    }
}

#[gpui::test]
fn quit_counts_every_server_still_cleaning_up(cx: &mut TestAppContext) {
    // Two windows' servers stop and exit, their threads reading endless
    // stderr on: both are force-stopped, not one for the two.
    let (first, second) = (
        FakeServer::new(Script::default()),
        FakeServer::new(Script::default()),
    );
    let mut q = Quitting::new(cx, Arc::new(EndlessStderr(first.launcher())));
    q.open_main();
    first.wait_initialized();
    let mut other = Quitting::new(cx, Arc::new(EndlessStderr(second.launcher())));
    other.open_main();
    second.wait_initialized();

    let quit = q.quit(GRACE, FORCE);
    assert_eq!(quit, Quit { forced: 2, left: 0 });
    assert_eq!((first.kills(), second.kills()), (1, 1));
}

#[gpui::test]
fn quit_counts_running_and_cleaning_up_servers(cx: &mut TestAppContext) {
    // One server stops at once but its thread cleans up on; the other
    // never answers `shutdown`.
    let (cleaning, running) = (
        FakeServer::new(Script::default()),
        FakeServer::new(Script {
            answer_shutdown: false,
            ..Script::default()
        }),
    );
    let mut q = Quitting::new(cx, Arc::new(EndlessStderr(cleaning.launcher())));
    q.open_main();
    cleaning.wait_initialized();
    let mut other = Quitting::new(cx, running.launcher());
    other.open_main();
    running.wait_initialized();

    let quit = q.quit(GRACE, FORCE);
    assert_eq!(quit, Quit { forced: 2, left: 0 });
    assert_eq!((cleaning.kills(), running.kills()), (1, 1));
}

#[gpui::test]
fn quit_leaves_every_launcher_ignoring_cancel(cx: &mut TestAppContext) {
    let mut held = Vec::new();
    let mut quits = Vec::new();
    for _ in 0..2 {
        let fake = FakeServer::new(Script::default());
        let (entered, entering) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let launcher = Arc::new(Ignoring {
            fake: fake.launcher(),
            entered: Mutex::new(entered),
            release: Mutex::new(released),
        });
        let mut q = Quitting::new(cx, launcher);
        q.open_main();
        entering.recv_timeout(Duration::from_secs(10)).unwrap();
        held.push((fake, release));
        quits.push(q);
    }

    let quit = quits[0].quit(GRACE, FORCE);
    assert_eq!(quit, Quit { forced: 2, left: 2 });
    for (fake, release) in held {
        release.send(()).unwrap();
        wait_until("the late server is killed", || fake.kills() == 1);
    }
}
