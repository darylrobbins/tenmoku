//! Language servers in the workbench: card 1's fake server, installed
//! through `set_server_launcher`, over a temporary root, its manager on a
//! manual clock, so fe-lsp's timers (the 250 ms backstop among them) only
//! fire when a test moves it.

use std::{
    fs,
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
    rc::Rc,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
    thread,
    time::{Duration, Instant},
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::{Settings, SettingsEvent};
use fe_gpui::{EditorView, commands::RunCommand};
use fe_lsp::{
    ExitInfo, LaunchCancel, ManualClock, ServerIo, ServerLauncher, ServerProcess, ServerSpec,
    fake::{FakeLauncher, FakeServer, OnExit, Record, Script},
};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use lsp_types::{
    LSPAny, LSPObject, LogMessageParams, MessageType, NumberOrString, ProgressParams,
    ProgressParamsValue, ShowMessageParams, Url, WorkDoneProgress, WorkDoneProgressBegin,
    notification::{LogMessage, Notification, Progress, ShowMessage},
};

use crate::{
    Launch, StatusSide, Workbench, lsp::TestClock, open::ConfigDirOverride, set_server_launcher,
};
use fe_gpui::{EditorSettings, EditorTheme};

/// Holds launches, as a slow launcher (a login shell, a container) does:
/// while held, each blocks until released.
#[derive(Default)]
struct Hold {
    /// Held, and whether a launch is waiting.
    state: Mutex<(bool, bool)>,
    changed: Condvar,
}

impl Hold {
    fn set(&self, held: bool) {
        self.state.lock().unwrap().0 = held;
        self.changed.notify_all();
    }

    fn pass(&self) {
        let mut state = self.state.lock().unwrap();
        state.1 = state.0;
        self.changed.notify_all();
        while state.0 {
            state = self.changed.wait(state).unwrap();
        }
    }

    /// Blocks until a launch waits.
    fn wait_waiting(&self) {
        let state = self.state.lock().unwrap();
        let (_state, timeout) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(10), |state| !state.1)
            .unwrap();
        assert!(!timeout.timed_out(), "no launch waits");
    }
}

/// The fakes' launcher: pyright's fake for pyright, the other for the
/// rest, or "not found" while `missing` is set; held while `hold` is.
struct Switch {
    fake: Arc<FakeLauncher>,
    python: Arc<FakeLauncher>,
    missing: Arc<AtomicBool>,
    hold: Arc<Hold>,
    /// Set once the actor of the first server `fake` launches, which has
    /// stopped waiting for its process to exit, has let go of that wait.
    gone: Arc<AtomicBool>,
}

/// A fake's process that sets `gone` once its `exited` has resolved and been
/// dropped: for the shutdown actor, once it has seen the server exit and
/// cancelled its 1-s timeout.
struct Watched {
    inner: Box<dyn ServerProcess>,
    gone: Arc<AtomicBool>,
}

struct WatchedExit<'a> {
    inner: Pin<Box<dyn Future<Output = ExitInfo> + Send + 'a>>,
    resolved: bool,
    gone: &'a AtomicBool,
}

impl Future for WatchedExit<'_> {
    type Output = ExitInfo;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<ExitInfo> {
        let poll = self.inner.as_mut().poll(cx);
        self.resolved |= poll.is_ready();
        poll
    }
}

impl Drop for WatchedExit<'_> {
    fn drop(&mut self) {
        if self.resolved {
            self.gone.store(true, Ordering::SeqCst);
        }
    }
}

impl ServerProcess for Watched {
    fn exited(&mut self) -> Pin<Box<dyn Future<Output = ExitInfo> + Send + '_>> {
        Box::pin(WatchedExit {
            inner: self.inner.exited(),
            resolved: false,
            gone: &self.gone,
        })
    }

    fn kill(&mut self) {
        self.inner.kill();
    }
}

impl ServerLauncher for Switch {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        self.hold.pass();
        if self.missing.load(Ordering::SeqCst) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "no such file"));
        }
        match spec.name.as_str() {
            "pyright" => self.python.launch(spec, root, cancel),
            _ => {
                let mut io = self.fake.launch(spec, root, cancel)?;
                io.process = Box::new(Watched {
                    inner: io.process,
                    gone: self.gone.clone(),
                });
                Ok(io)
            }
        }
    }
}

/// rust-analyzer's readiness notification.
enum ServerStatusNotification {}

impl Notification for ServerStatusNotification {
    type Params = LSPAny;
    const METHOD: &'static str = "experimental/serverStatus";
}

/// A workbench rooted at a temporary project, with the fakes installed.
pub(crate) struct Lsp {
    pub(crate) workbench: Entity<Workbench>,
    pub(crate) cx: VisualTestContext,
    /// rust-analyzer's fake, and any other server's but pyright's.
    pub(crate) fake: FakeServer,
    pub(crate) python: FakeServer,
    pub(crate) clock: Arc<ManualClock>,
    missing: Arc<AtomicBool>,
    hold: Arc<Hold>,
    /// rust-analyzer's actor is done with its process (see [`Watched`]).
    rust_gone: Arc<AtomicBool>,
    dir: Rc<tempfile::TempDir>,
    /// The project's folder, resolved, as the manager has it.
    pub(crate) root: PathBuf,
    /// The user's config folder, so a trusted root's settings never come
    /// from the real one.
    pub(crate) config: Rc<tempfile::TempDir>,
}

/// The project: `src/main.rs`, `src/lib.rs`, `src/other.rs` and
/// `tool.py`.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    for (name, text) in [
        ("src/main.rs", "fn main() {}\n"),
        ("src/lib.rs", "pub fn f() {}\n"),
        ("src/other.rs", "pub fn g() {}\n"),
        ("tool.py", "print(1)\n"),
    ] {
        fs::write(dir.path().join(name), text).unwrap();
    }
    dir
}

/// Registers the app's and the workbench's commands, once.
fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        if !cx.has_global::<crate::Commands>() {
            fe_gpui::init(cx);
            crate::init(cx);
        }
    });
}

/// A workbench for `launch` in a window of its own, focused, nothing in
/// the background landed yet.
pub(crate) fn window(
    cx: &mut TestAppContext,
    launch: Launch,
) -> (Entity<Workbench>, VisualTestContext) {
    init(cx);
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut cx = VisualTestContext::from_window(*window, cx);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    (workbench, cx)
}

fn rooted(root: &Path) -> Launch {
    Launch {
        root: Some(root.to_owned()),
        file: None,
    }
}

impl Lsp {
    /// The workbench, its root not trusted yet, with fakes following
    /// `rust` and `python`, installed as the host's launcher, and the
    /// manual clock.
    pub(crate) fn with_scripts(cx: &mut TestAppContext, rust: Script, python: Script) -> Self {
        Self::with_first_window(cx, rust, python, false)
    }

    /// As [`Self::with_scripts`], the first window rootless if `rootless`.
    fn with_first_window(
        cx: &mut TestAppContext,
        rust: Script,
        python: Script,
        rootless: bool,
    ) -> Self {
        // Server threads wake the workbench's event loop.
        cx.executor().allow_parking();
        let (fake, python) = (FakeServer::new(rust), FakeServer::new(python));
        let clock = Arc::new(ManualClock::new());
        let missing = Arc::new(AtomicBool::new(false));
        let hold = Arc::new(Hold::default());
        let gone = Arc::new(AtomicBool::new(false));
        let launcher = Arc::new(Switch {
            fake: fake.launcher(),
            python: python.launcher(),
            missing: missing.clone(),
            hold: hold.clone(),
            gone: gone.clone(),
        });
        let manager_clock = clock.clone();
        let config = Rc::new(tempfile::tempdir().unwrap());
        let config_dir = config.path().to_owned();
        cx.update(|cx| {
            set_server_launcher(cx, launcher);
            cx.set_global(TestClock(manager_clock));
            cx.set_global(ConfigDirOverride(Some(config_dir)));
        });
        let dir = Rc::new(project());
        let root = fs::canonicalize(dir.path()).unwrap();
        let launch = if rootless {
            Launch {
                root: None,
                file: None,
            }
        } else {
            rooted(&root)
        };
        let (workbench, cx) = window(cx, launch);
        let mut lsp = Self {
            workbench,
            cx,
            fake,
            python,
            clock,
            missing,
            hold,
            rust_gone: gone,
            dir,
            root,
            config,
        };
        lsp.draw();
        lsp
    }

    /// The workbench, its root not trusted yet.
    pub(crate) fn untrusted(cx: &mut TestAppContext) -> Self {
        Self::with_scripts(cx, Script::default(), Script::default())
    }

    /// A rootless workbench, the app's first and only window, with the
    /// project's folder and the fakes ready for `another_rooted`.
    pub(crate) fn rootless(cx: &mut TestAppContext) -> Self {
        Self::with_first_window(cx, Script::default(), Script::default(), true)
    }

    /// The workbench, its root trusted.
    pub(crate) fn new(cx: &mut TestAppContext) -> Self {
        let mut lsp = Self::untrusted(cx);
        lsp.set_trust(true);
        lsp
    }

    /// Another window's workbench, over the same project and fakes,
    /// trusted.
    fn another(&mut self) -> Self {
        let launch = rooted(&self.root);
        self.another_at(launch)
    }

    /// Another window's workbench for `launch`, with the same fakes, its
    /// root trusted.
    fn another_at(&mut self, launch: Launch) -> Self {
        let mut lsp = self.another_untrusted_at(launch);
        lsp.set_trust(true);
        lsp
    }

    /// Another window's workbench rooted at `root`, with the same fakes,
    /// its root's trust left as it is.
    pub(crate) fn another_rooted(&mut self, root: &Path) -> Self {
        self.another_untrusted_at(rooted(root))
    }

    /// Another window's workbench for `launch`, with the same fakes, its
    /// root's trust left as it is.
    fn another_untrusted_at(&mut self, launch: Launch) -> Self {
        let (workbench, cx) = window(&mut self.cx, launch);
        let mut lsp = Self {
            workbench,
            cx,
            fake: self.fake.clone(),
            python: self.python.clone(),
            clock: self.clock.clone(),
            missing: self.missing.clone(),
            hold: self.hold.clone(),
            rust_gone: self.rust_gone.clone(),
            dir: self.dir.clone(),
            root: self.root.clone(),
            config: self.config.clone(),
        };
        lsp.draw();
        lsp
    }

    /// Writes the user's `settings.toml`, which a trusted root's settings
    /// are read from at the next reload.
    pub(crate) fn write_user_settings(&self, text: &str) {
        let path = self.config.path().join(fe_config::paths::SETTINGS_FILE);
        fs::write(path, text).unwrap();
    }

    pub(crate) fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    pub(crate) fn uri(&self, name: &str) -> Url {
        Url::from_file_path(self.path(name)).unwrap()
    }

    pub(crate) fn set_trust(&mut self, trusted: bool) {
        let workbench = self.workbench.clone();
        workbench.update(&mut self.cx, |workbench, cx| {
            workbench.set_root_trust(trusted, cx)
        });
        self.draw();
    }

    /// Lets background work land, then draws a frame.
    /// Lets background work land, then draws a frame, twice: a frame may
    /// start work (resolving a file's path) that the next one uses.
    pub(crate) fn draw(&mut self) {
        for _ in 0..2 {
            self.cx.run_until_parked();
            self.cx.update(|window, cx| window.draw(cx).clear(cx));
        }
    }

    /// Opens `path` in the active pane, and draws.
    fn open_path(&mut self, path: PathBuf) {
        let pane = self.workbench.read_with(&self.cx, |w, _| w.pane().clone());
        self.cx
            .update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        self.draw();
    }

    /// Opens `name` in the active pane, and draws once it's shown.
    pub(crate) fn open(&mut self, name: &str) {
        let pane = self.workbench.read_with(&self.cx, |w, _| w.pane().clone());
        let path = self.path(name);
        self.cx
            .update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        self.draw();
        assert_eq!(self.active_path(), Some(self.path(name)));
    }

    fn active_editor(&mut self) -> Entity<EditorView> {
        self.workbench
            .read_with(&self.cx, |w, cx| w.active_editor(cx).cloned())
            .expect("a tab is shown")
    }

    fn active_path(&mut self) -> Option<PathBuf> {
        let editor = self.active_editor();
        editor.read_with(&self.cx, |view, _| view.model().path().map(Path::to_owned))
    }

    /// Runs workbench command `id` as an unbound key or menu item would.
    pub(crate) fn run(&mut self, id: &str) {
        let run = RunCommand(
            Invocation::of(id)
                .unwrap()
                .context(ContextStack::new().with(ContextFrame::new("workbench"))),
        );
        self.cx
            .update(|window, cx| window.dispatch_action(Box::new(run), cx));
        self.draw();
    }

    /// The server the active editor's buffer went to, if it was opened on
    /// the manager.
    pub(crate) fn active_server(&mut self) -> Option<Option<fe_lsp::ServerId>> {
        self.workbench
            .read_with(&self.cx, |w, cx| w.lsp_active_server(cx))
    }

    /// The LSP status item's text, as the last frame drew it.
    pub(crate) fn item(&mut self) -> Option<String> {
        let bar = self
            .workbench
            .read_with(&self.cx, |w, _| w.status_bar().clone());
        let items = bar.read_with(&self.cx, |bar, _| bar.drawn(StatusSide::Right));
        items
            .into_iter()
            .find(|(id, _)| id == crate::lsp::ITEM)
            .map(|(_, text)| text.to_string())
    }

    /// Draws until the status item reads `text`. Panics after 10 s.
    pub(crate) fn wait_item(&mut self, text: Option<&str>) {
        self.until(&format!("the item to read {text:?}"), |lsp| {
            lsp.item().as_deref() == text
        });
    }

    /// Draws until `done` holds. Panics after 10 s, naming `what`.
    pub(crate) fn until(&mut self, what: &str, done: impl Fn(&mut Self) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.draw();
            if done(self) {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// The notifications named `method` that launch `launch` got for
    /// `uri`.
    pub(crate) fn sent(&self, launch: usize, method: &str, uri: &Url) -> usize {
        count(&self.fake.records_of(launch), method, uri)
    }
}

/// How many of `records` are notification `method` for `uri`.
fn count(records: &[Record], method: &str, uri: &Url) -> usize {
    records
        .iter()
        .filter(|record| match record {
            Record::Notification { method: m, params } => {
                m == method && params["textDocument"]["uri"] == uri.as_str()
            }
            _ => false,
        })
        .count()
}

fn begin_progress(fake: &FakeServer, title: &str) {
    fake.notify::<Progress>(ProgressParams {
        token: NumberOrString::String("load".into()),
        value: ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(WorkDoneProgressBegin {
            title: title.into(),
            ..Default::default()
        })),
    });
}

fn quiescent(fake: &FakeServer) {
    let mut status = LSPObject::new();
    status.insert("health".into(), "ok".into());
    status.insert("quiescent".into(), true.into());
    fake.notify::<ServerStatusNotification>(LSPAny::Object(status));
}

/// Whether `records` has `shutdown`, then `exit`.
fn shut_down(records: &[Record]) -> bool {
    let shutdown = records.iter().position(|r| r.method() == "shutdown");
    let exit = records.iter().position(|r| r.is_notification("exit"));
    matches!((shutdown, exit), (Some(shutdown), Some(exit)) if shutdown < exit)
}

#[gpui::test]
fn opening_a_file_starts_its_server(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    assert_eq!(
        s.workbench.read_with(&s.cx, |w, _| w.lsp_manager()),
        Some(false)
    );
    assert_eq!(s.fake.launches(), 0);

    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    let replica = s
        .fake
        .wait_for_document(&main, "didOpen", |r| r.is_some())
        .unwrap();
    assert_eq!(replica.text, "fn main() {}\n");
    assert_eq!(replica.language_id, "rust");
    let launched = s.fake.launched();
    assert_eq!(launched.len(), 1);
    assert_eq!(launched[0].0.name, "rust-analyzer");
    assert_eq!(launched[0].1, s.root);

    // A second file, and the first again in a split.
    s.open("src/lib.rs");
    s.fake
        .wait_for_document(&s.uri("src/lib.rs"), "didOpen", |r| r.is_some());
    s.open("src/main.rs");
    s.cx.simulate_keystrokes("cmd-\\");
    s.draw();
    let panes = s
        .workbench
        .read_with(&s.cx, |w, _| w.pane_group().panes().len());
    assert_eq!(panes, 2);
    assert_eq!(s.active_path(), Some(s.path("src/main.rs")));
    // Anything sent for the split goes before this.
    s.open("src/other.rs");
    s.fake
        .wait_for_document(&s.uri("src/other.rs"), "didOpen", |r| r.is_some());
    assert_eq!(s.fake.launches(), 1);
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 1);
    assert_eq!(s.sent(1, "textDocument/didClose", &main), 0);

    // A workbench with no root has no manager.
    let launch = Launch {
        root: None,
        file: Some((
            s.path("src/lib.rs"),
            Some(fe_buffer::Buffer::new("pub fn f() {}\n")),
        )),
    };
    let (workbench, mut cx) = window(&mut s.cx, launch);
    workbench.update(&mut cx, |w, cx| w.set_root_trust(true, cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert!(workbench.read_with(&cx, |w, cx| w.active_editor(cx).is_some()));
    assert_eq!(workbench.read_with(&cx, |w, _| w.lsp_manager()), None);
    assert_eq!(s.fake.launches(), 1);
}

#[gpui::test]
fn paths_through_parent_folders_start_servers(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let root = s.root.clone();

    // A folder launch through `..`.
    let mut folder = s.another_at(rooted(&root.join("src/..")));
    folder.open_path(root.join("src/../src/main.rs"));
    s.fake
        .wait_for_document(&s.uri("src/main.rs"), "main.rs", |r| r.is_some());
    assert_eq!(s.fake.launched()[0].1, root);

    // A file launch through `..`: its folder is the root.
    let file = root.join("src/../src/lib.rs");
    let buffer = fe_buffer::Buffer::new("pub fn f() {}\n");
    let _file = s.another_at(Launch {
        root: file.parent().map(Path::to_owned),
        file: Some((file, Some(buffer))),
    });
    s.fake
        .wait_for_document(&s.uri("src/lib.rs"), "lib.rs", |r| r.is_some());
    assert_eq!(s.fake.launched()[1].1, root.join("src"));

    // A new root through `..`, and a file through it. macOS resolves `..`
    // after a file (`tool.py/..`); Linux refuses it (ENOTDIR), as a shell
    // would, so it goes through a directory there.
    let new_root = if cfg!(target_os = "macos") {
        root.join("tool.py/../src")
    } else {
        root.join("src/../src")
    };
    let workbench = s.workbench.clone();
    s.cx.update(|window, cx| workbench.update(cx, |w, cx| w.set_root(new_root, window, cx)));
    s.draw();
    s.set_trust(true);
    s.open_path(root.join("src/../src/other.rs"));
    s.fake
        .wait_for_document(&s.uri("src/other.rs"), "other.rs", |r| r.is_some());
    assert_eq!(s.fake.launched()[2].1, root.join("src"));
    assert_eq!(s.fake.launches(), 3);
}

#[gpui::test]
fn edits_and_saves_reach_the_server(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_for_document(&main, "didOpen", |r| r.is_some());

    // The manual clock never moves, so fe-lsp's backstop never drains: the
    // frame's nudge does.
    s.cx.simulate_input("x");
    s.draw();
    let replica = s.fake.wait_for_document(&main, "didChange", |replica| {
        replica.is_some_and(|replica| replica.text == "xfn main() {}\n")
    });
    let version = replica.unwrap().version;
    assert!(s.clock.sleeping().iter().all(|left| !left.is_zero()));

    s.cx.simulate_keystrokes("cmd-s");
    s.draw();
    let replica = s.fake.wait_for_document(&main, "didSave", |replica| {
        replica.is_some_and(|replica| !replica.saves.is_empty())
    });
    assert_eq!(replica.unwrap().saves[0].version, version);

    // A split copy closes; the original still shows the buffer.
    s.cx.simulate_keystrokes("cmd-\\");
    s.draw();
    s.cx.simulate_keystrokes("cmd-w");
    s.draw();
    let panes = s
        .workbench
        .read_with(&s.cx, |w, _| w.pane_group().panes().len());
    assert_eq!(panes, 1);
    // Anything sent for that close goes before this.
    s.open("src/lib.rs");
    s.fake
        .wait_for_document(&s.uri("src/lib.rs"), "didOpen", |r| r.is_some());
    assert_eq!(s.sent(1, "textDocument/didClose", &main), 0);

    // The last view of it closes.
    s.open("src/main.rs");
    s.cx.simulate_keystrokes("cmd-w");
    s.draw();
    s.fake.wait_for_document(&main, "didClose", |replica| {
        replica.is_some_and(|r| !r.open)
    });
    assert_eq!(s.sent(1, "textDocument/didClose", &main), 1);
    assert_eq!(s.sent(1, "textDocument/didOpen", &main), 1);
}

#[gpui::test]
fn status_item_follows_the_server(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.open("src/main.rs");
    s.wait_item(Some("rust-analyzer: starting…"));
    s.fake.wait_initialized();
    begin_progress(&s.fake, "Roots Scanned");
    s.wait_item(Some("rust-analyzer: indexing Roots Scanned"));
    quiescent(&s.fake);
    s.wait_item(None);

    // Two crashes.
    s.fake.crash_at(1);
    s.wait_item(Some("rust-analyzer: restarting (1/5)"));
    s.clock.wait_for_sleep(Duration::from_secs(1));
    s.clock.advance(Duration::from_secs(1));
    s.fake.wait_initialized_at(2);
    s.fake.crash_at(2);
    s.wait_item(Some("rust-analyzer: restarting (2/5)"));

    // Started again where there's nothing to launch.
    s.missing.store(true, Ordering::SeqCst);
    s.run("lsp.restart");
    s.wait_item(Some("rust-analyzer: stopped: rust-analyzer not found"));

    // A Python editor shows pyright's state.
    s.missing.store(false, Ordering::SeqCst);
    s.open("tool.py");
    s.wait_item(Some("pyright: starting…"));
    assert_eq!(s.python.launched()[0].0.name, "pyright");
    s.open("src/main.rs");
    s.wait_item(Some("rust-analyzer: stopped: rust-analyzer not found"));

    // A click shows the log.
    let tabs = |s: &mut Lsp| {
        s.workbench
            .read_with(&s.cx, |w, cx| w.pane().read(cx).tabs().len())
    };
    let before = tabs(&mut s);
    let bounds =
        s.cx.debug_bounds(format!("status-item:{}", crate::lsp::ITEM).leak())
            .expect("drawn");
    s.cx.simulate_click(bounds.center(), Modifiers::none());
    s.draw();
    let commands = s.workbench.read_with(&s.cx, |w, _| w.commands_run.clone());
    assert_eq!(commands.last().unwrap().command.as_str(), "lsp.show_log");
    assert_eq!(tabs(&mut s), before + 1);
    assert_eq!(s.active_path(), None);
}

#[gpui::test]
fn restart_and_show_log(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.open("src/main.rs");
    let main = s.uri("src/main.rs");
    s.fake.wait_initialized();
    // The restart needs the workbench to have applied the server's Starting status.
    s.wait_item(Some("rust-analyzer: starting…"));

    s.run("lsp.restart");
    s.fake.wait_initialized_at(2);
    assert!(shut_down(&s.fake.records_of(1)));
    let initialize = |launch| {
        s.fake
            .timeline()
            .iter()
            .position(|(at, record)| *at == launch && record.method() == "initialize")
    };
    let exit = s
        .fake
        .timeline()
        .iter()
        .position(|(at, record)| *at == 1 && record.is_notification("exit"));
    assert!(exit < initialize(2), "{:#?}", s.fake.timeline());
    s.fake
        .wait_for_document(&main, "didOpen again", |r| r.is_some_and(|r| r.launch == 2));

    // From Stopped.
    s.missing.store(true, Ordering::SeqCst);
    s.run("lsp.restart");
    s.wait_item(Some("rust-analyzer: stopped: rust-analyzer not found"));
    s.missing.store(false, Ordering::SeqCst);
    s.run("lsp.restart");
    s.fake.wait_initialized_at(3);
    s.wait_item(Some("rust-analyzer: starting…"));
    s.fake
        .wait_for_document(&main, "didOpen at 3", |r| r.is_some_and(|r| r.launch == 3));

    // The log: stderr and window/logMessage.
    s.fake.stderr(b"from stderr\n");
    s.fake.notify::<LogMessage>(LogMessageParams {
        typ: MessageType::INFO,
        message: "a logged line".into(),
    });
    s.until("both in the log", |s| {
        let log = s
            .workbench
            .read_with(&s.cx, |w, _| w.lsp_log("rust-analyzer"));
        log.contains("from stderr") && log.contains("a logged line")
    });
    s.run("lsp.show_log");
    let editor = s.active_editor();
    let text = editor.read_with(&s.cx, |view, _| view.model().text().to_string());
    assert!(text.contains("from stderr\n"), "{text}");
    assert!(text.contains("a logged line"), "{text}");
    assert_eq!(s.active_path(), None);

    // window/showMessage shows until the next status.
    s.open("src/main.rs");
    s.fake.notify::<ShowMessage>(ShowMessageParams {
        typ: MessageType::WARNING,
        message: "careful".into(),
    });
    s.wait_item(Some("careful"));
    begin_progress(&s.fake, "Loading");
    s.wait_item(Some("rust-analyzer: indexing Loading"));
}

/// Reloads `s`'s settings with `lsp.enabled` set to `enabled`, in the
/// user's file too, which a trusted root's are read from.
fn reload(s: &mut Lsp, enabled: bool) {
    s.write_user_settings(&format!("lsp.enabled = {enabled}\n"));
    let mut settings = Settings::default();
    settings.lsp.enabled = enabled;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    let workbench = s.workbench.clone();
    workbench.update(&mut s.cx, |w, cx| w.reload_settings(&event, cx));
    s.draw();
}

#[gpui::test]
fn settings_and_shutdown(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    // Two servers.
    s.open("src/main.rs");
    s.fake.wait_initialized();
    s.open("tool.py");
    s.python.wait_initialized();
    s.open("src/main.rs");

    reload(&mut s, false);
    s.fake.wait_for_at(1, "shutdown, exit", shut_down);
    s.python.wait_for_at(1, "shutdown, exit", shut_down);
    s.wait_item(Some("rust-analyzer: stopped"));

    // Back on: the open files' servers start again.
    reload(&mut s, true);
    s.fake.wait_initialized_at(2);
    s.python.wait_initialized_at(2);

    s.cx.update(|window, _| window.remove_window());
    s.cx.run_until_parked();
    s.fake.wait_for_at(2, "shutdown, exit", shut_down);
    s.python.wait_for_at(2, "shutdown, exit", shut_down);
    assert_eq!(s.workbench.read_with(&s.cx, |w, _| w.lsp_manager()), None);
}

/// Drives the manual clock through quitting with `s.fake` (rust-analyzer) and
/// `s.python` (pyright), launch 1, so that each timeout runs out only once
/// the fakes have seen what precedes it, as in production, where a server has
/// the timeout's time to read `exit`:
///
/// 1. both have `shutdown` and two 2-s sleeps are armed: advance 2 s;
/// 2. both have `shutdown`, then `exit`, rust-analyzer's actor has seen it exit
///    (so its 1-s sleep is cancelled), and exactly one 1-s sleep is armed,
///    pyright's: advance 1 s;
/// 3. until dropped, advance to each sleep as it's armed.
///
/// A step that isn't reached in 10 s of real time is recorded, the driver
/// falls through to 3 so that quitting can't hang, and `Drop` panics.
struct QuitDriver(
    Arc<AtomicBool>,
    Option<thread::JoinHandle<Vec<&'static str>>>,
);

impl QuitDriver {
    const PATIENCE: Duration = Duration::from_secs(10);

    fn new(
        clock: Arc<ManualClock>,
        rust: FakeServer,
        python: FakeServer,
        rust_gone: Arc<AtomicBool>,
    ) -> Self {
        let done = Arc::new(AtomicBool::new(false));
        let thread = thread::spawn({
            let done = done.clone();
            move || {
                let mut missed = Vec::new();
                let have = |f: &dyn Fn(&[Record]) -> bool| {
                    f(&rust.records_of(1)) && f(&python.records_of(1))
                };
                let sleeps = |left: Duration, n: usize| {
                    // Other timers may be armed too.
                    clock.sleeping().iter().filter(|s| **s == left).count() == n
                };
                let wait = |ready: &dyn Fn() -> bool| {
                    let give_up = Instant::now() + Self::PATIENCE;
                    while !ready() {
                        if done.load(Ordering::SeqCst) || Instant::now() >= give_up {
                            return false;
                        }
                        thread::sleep(Duration::from_millis(1));
                    }
                    true
                };
                let has_shutdown =
                    |records: &[Record]| records.iter().any(|r| r.method() == "shutdown");
                let steps: [(&'static str, Duration, &dyn Fn() -> bool); 2] = [
                    (
                        "2 s: both shutdown, two 2-s sleeps",
                        Duration::from_secs(2),
                        &|| have(&has_shutdown) && sleeps(Duration::from_secs(2), 2),
                    ),
                    (
                        "1 s: both shut_down, one 1-s sleep",
                        Duration::from_secs(1),
                        // Once rust-analyzer's actor has seen it exit and
                        // dropped its 1-s sleep, the one left is pyright's.
                        &|| {
                            have(&shut_down)
                                && rust_gone.load(Ordering::SeqCst)
                                && sleeps(Duration::from_secs(1), 1)
                        },
                    ),
                ];
                for (name, by, ready) in steps {
                    if !wait(ready) {
                        missed.push(name);
                        break;
                    }
                    clock.advance(by);
                }
                while !done.load(Ordering::SeqCst) {
                    if let Some(next) = clock.sleeping().into_iter().min() {
                        clock.advance(next.max(Duration::from_millis(1)));
                    }
                    thread::sleep(Duration::from_millis(1));
                }
                missed
            }
        });
        Self(done, Some(thread))
    }
}

impl Drop for QuitDriver {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
        let missed = self.1.take().unwrap().join().unwrap();
        if !thread::panicking() {
            assert!(missed.is_empty(), "QuitDriver missed step: {missed:?}");
        }
    }
}

#[gpui::test]
fn quitting_waits_for_slow_servers(cx: &mut TestAppContext) {
    // rust-analyzer never answers shutdown, but exits on exit; pyright
    // answers neither, so it's killed.
    let rust = Script {
        answer_shutdown: false,
        ..Script::default()
    };
    let python = Script {
        answer_shutdown: false,
        on_exit: OnExit::Ignore,
        ..Script::default()
    };
    let mut s = Lsp::with_scripts(cx, rust, python);
    s.set_trust(true);
    s.open("src/main.rs");
    s.fake.wait_initialized();
    // Another window, left open, with pyright.
    let mut t = s.another();
    t.open("tool.py");
    t.python.wait_initialized();

    // The last of s's windows closes: its server shuts down on its thread.
    s.cx.update(|window, _| window.remove_window());
    s.cx.run_until_parked();
    assert_eq!(s.fake.kills() + s.python.kills(), 0);

    // Quitting shuts t's down too, and waits for both through their
    // timeouts.
    let started = Instant::now();
    {
        let _driver = QuitDriver::new(
            s.clock.clone(),
            s.fake.clone(),
            s.python.clone(),
            s.rust_gone.clone(),
        );
        t.cx.update(|_, cx| crate::lsp::shut_down_all(cx));
    }
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
    let shutdown = |records: &[Record]| records.iter().any(|r| r.method() == "shutdown");
    let rust = s.fake.records_of(1);
    assert!(shutdown(&rust) && shut_down(&rust), "{rust:#?}");
    assert_eq!(s.fake.kills(), 0);
    let python = s.python.records_of(1);
    assert!(shut_down(&python), "{python:#?}");
    assert_eq!(s.python.kills(), 1);
    assert_eq!(t.workbench.read_with(&t.cx, |w, _| w.lsp_manager()), None);
}

#[gpui::test]
fn quitting_waits_for_a_held_launch(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.hold.set(true);
    s.open("src/main.rs");
    s.hold.wait_waiting();

    // The last window closes, then the app quits, with the launch still
    // held, as a login shell holds a local one for up to 2 s: once it
    // returns, the server is killed rather than initialized, and quitting
    // waits for that.
    s.cx.update(|window, _| window.remove_window());
    s.cx.run_until_parked();
    let hold = s.hold.clone();
    // Timed before the release thread starts its sleep, so the lower bound
    // below cannot undercount.
    let started = Instant::now();
    let release = thread::spawn(move || {
        thread::sleep(Duration::from_millis(200));
        hold.set(false);
    });
    // No window left: through the app.
    s.cx.cx.update(crate::lsp::shut_down_all);
    let took = started.elapsed();
    release.join().unwrap();
    assert!(took >= Duration::from_millis(200), "{took:?}");
    assert!(took < Duration::from_secs(6), "{took:?}");
    assert_eq!(s.fake.launches(), 1);
    assert_eq!(s.fake.kills(), 1);
    let records = s.fake.records();
    assert!(
        !records.iter().any(|r| r.method() == "initialize"),
        "{records:#?}"
    );
}

#[gpui::test]
fn settings_are_per_window(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    let mut t = s.another();
    reload(&mut s, false);
    // A trusted root's settings are read from the files, which every
    // window shares: put the user's back, so only s's in-memory ones differ.
    s.write_user_settings("");

    // t's new root's manager starts with t's settings, not s's.
    let src = s.path("src");
    let workbench = t.workbench.clone();
    t.cx.update(|window, cx| workbench.update(cx, |w, cx| w.set_root(src.clone(), window, cx)));
    t.draw();
    t.set_trust(true);
    t.open("src/main.rs");
    t.fake.wait_initialized();
    assert_eq!(t.fake.launched()[0].1, src);

    // s's stays off.
    s.open("src/lib.rs");
    assert_eq!(s.active_server(), Some(None));
    assert_eq!(s.fake.launches(), 1);
}

#[gpui::test]
fn messages_are_per_server(cx: &mut TestAppContext) {
    let mut s = Lsp::new(cx);
    s.open("tool.py");
    s.python.wait_initialized();
    s.open("src/main.rs");
    s.fake.wait_initialized();
    let show = |fake: &FakeServer, message: &str| {
        fake.notify::<ShowMessage>(ShowMessageParams {
            typ: MessageType::INFO,
            message: message.into(),
        })
    };
    show(&s.fake, "from rust-analyzer");
    s.wait_item(Some("from rust-analyzer"));
    show(&s.python, "from pyright");
    s.open("tool.py");
    s.wait_item(Some("from pyright"));
    s.open("src/main.rs");
    s.wait_item(Some("from rust-analyzer"));

    // rust-analyzer's status clears its message only.
    begin_progress(&s.fake, "Loading");
    s.wait_item(Some("rust-analyzer: indexing Loading"));
    s.open("tool.py");
    s.wait_item(Some("from pyright"));
}

#[gpui::test]
fn host_launcher_replaces_local(cx: &mut TestAppContext) {
    // Without a launcher set, fe-lsp's own. The root isn't trusted, so it
    // launches nothing.
    let dir = project();
    let (workbench, cx_a) = window(cx, rooted(dir.path()));
    cx_a.run_until_parked();
    assert_eq!(
        workbench.read_with(&cx_a, |w, _| w.lsp_manager()),
        Some(true)
    );

    let mut s = Lsp::new(cx);
    assert_eq!(
        s.workbench.read_with(&s.cx, |w, _| w.lsp_manager()),
        Some(false)
    );
    s.open("src/main.rs");
    s.fake.wait_initialized();
    assert_eq!(s.fake.launched().len(), 1);
    assert_eq!(s.fake.launched()[0].1, s.root);
}
