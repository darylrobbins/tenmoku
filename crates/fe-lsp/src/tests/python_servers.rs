//! Python's servers (T684): `lsp.python_server` picks pyrefly (the
//! default), ty or pyright, and only that one runs; ruff runs beside it
//! when it's installed and says nothing when it isn't; each main server
//! gets the environment's interpreter at its own key; one not found says
//! how to install it; and the root markers they share.

mod one_launch;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fe_buffer::Buffer;
use fe_config::settings::{LspServerSettings, LspSettings, PythonServer};
use lsp_types::request::WorkspaceConfiguration;
use lsp_types::{
    CompletionOptions, ConfigurationItem, ConfigurationParams, PositionEncodingKind, Url,
};
use serde_json::{Value, json};

use super::{Events, WAIT, answer, manager as managed, wait_until};
use crate::fake::{FakeLaunchers, FakeServer, InitializeReply, Reply, Script, Scripted};
use crate::registry::{Registry, builtin_specs, server_for, server_root, servers_for};
use crate::{
    CompletionTrigger, LaunchCancel, LocalLauncher, LspEvent, LspManager, ManualClock, ServerId,
    ServerIo, ServerLauncher, ServerSpec, ServerStatus,
};

/// Python's built-in main servers, in the order `lsp.python_server`
/// lists them, with the command each runs.
const MAINS: [(PythonServer, &str, &str, &[&str]); 3] = [
    (PythonServer::Pyrefly, "pyrefly", "pyrefly", &["lsp"]),
    (PythonServer::Ty, "ty", "ty", &["server"]),
    (
        PythonServer::Pyright,
        "pyright",
        "pyright-langserver",
        &["--stdio"],
    ),
];

/// Every built-in server of Python's, with ruff.
const ALL: [&str; 4] = ["pyrefly", "ty", "pyright", "ruff"];

#[cfg(not(windows))]
const PYTHON: &str = "bin/python";
#[cfg(windows)]
const PYTHON: &str = r"Scripts\python.exe";
#[cfg(not(windows))]
const RUFF: &str = "bin/ruff";
#[cfg(windows)]
const RUFF: &str = r"Scripts\ruff.exe";

const COMPLETION: &str = "textDocument/completion";

/// A file at `file`, and its folders.
fn touch(file: &Path) {
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, "").unwrap();
}

/// An executable file at `file`, as pip installs a program into an
/// environment's bin.
fn program(file: &Path) {
    touch(file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// A project folder, resolved, with an environment at `.venv` if `venv`.
struct Project {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl Project {
    fn new(venv: bool) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        if venv {
            touch(&root.join(".venv").join(PYTHON));
        }
        Self { _dir: dir, root }
    }

    fn venv(&self) -> PathBuf {
        self.root.join(".venv")
    }

    /// The environment's interpreter, as the servers are told it.
    fn python(&self) -> String {
        self.venv().join(PYTHON).display().to_string()
    }

    /// Puts ruff into the environment.
    fn install_ruff(&self) {
        program(&self.venv().join(RUFF));
    }
}

/// Settings choosing `choice` (unset if `None`), with a table for each of
/// `tables`, and ruff's, each looking nowhere on the `PATH` and with
/// Tenmoku's own `$VIRTUAL_ENV` unset, so only the project's environment
/// counts.
fn settings(choice: Option<PythonServer>, tables: &[&str]) -> LspSettings {
    let mut settings = LspSettings {
        python_server: choice,
        ..LspSettings::default()
    };
    for name in tables.iter().copied().chain(["ruff"]) {
        settings.servers.insert(name.into(), hermetic());
    }
    settings
}

/// A server's table that looks nowhere on the `PATH`, with `$VIRTUAL_ENV`
/// unset.
fn hermetic() -> LspServerSettings {
    serde_json::from_value(json!({
        "env": { "PATH": "/nonexistent", "VIRTUAL_ENV": "" },
    }))
    .unwrap()
}

/// The registry's specs under `settings`.
fn specs_of(settings: LspSettings) -> Vec<ServerSpec> {
    let mut registry = Registry::default();
    registry.set_settings(settings);
    registry.specs()
}

/// The names of `specs` that serve Python.
fn python_names(specs: &[ServerSpec]) -> Vec<&str> {
    (specs.iter())
        .filter(|spec| spec.languages.iter().any(|l| l == "python"))
        .map(|spec| spec.name.as_str())
        .collect()
}

/// A completion provider that doesn't resolve.
fn completing() -> Script {
    let mut init = crate::fake::initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities.completion_provider = Some(CompletionOptions {
        trigger_characters: Some(vec![".".into()]),
        resolve_provider: Some(false),
        ..CompletionOptions::default()
    });
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        ..Script::default()
    }
}

/// A manager over a fake per built-in server of Python's, each offering
/// completions, on a clock that moves only when a test moves it.
struct Fixture {
    fakes: Vec<(&'static str, FakeServer)>,
    manager: LspManager,
    events: Events,
}

impl Fixture {
    fn new(settings: &LspSettings) -> Self {
        let fakes: Vec<(&'static str, FakeServer)> = (ALL.iter())
            .map(|&name| (name, FakeServer::new(completing())))
            .collect();
        let launchers =
            FakeLaunchers::new(fakes.iter().map(|(name, fake)| (*name, fake.launcher())));
        let (mut manager, events) =
            managed(Arc::new(launchers), Some(Arc::new(ManualClock::new())));
        manager.update_settings(settings);
        Self {
            fakes,
            manager,
            events,
        }
    }

    fn fake(&self, name: &str) -> &FakeServer {
        &(self.fakes.iter())
            .find(|(named, _)| *named == name)
            .unwrap()
            .1
    }

    /// Opens `main.py` at `root`, the window root.
    fn open(&mut self, root: &Path, buffer: &mut Buffer) -> Url {
        let path = root.join("main.py");
        self.manager
            .open(
                Some(root),
                buffer.id(),
                buffer.text().clone(),
                buffer.version(),
                buffer.subscribe(),
                &path,
                "python",
            )
            .unwrap();
        Url::from_file_path(&path).unwrap()
    }

    /// The fakes launched, by name.
    fn launched(&self) -> Vec<&'static str> {
        (self.fakes.iter())
            .filter(|(_, fake)| fake.launches() > 0)
            .map(|(name, _)| *name)
            .collect()
    }
}

/// The configuration a fake at launch 1 gets for `section`.
fn configuration(fake: &FakeServer, section: &str) -> Value {
    let items = vec![ConfigurationItem {
        scope_uri: None,
        section: Some(section.into()),
    }];
    let answers = fake
        .request_at::<WorkspaceConfiguration>(1, ConfigurationParams { items })
        .unwrap();
    answers[0].clone()
}

#[test]
fn python_files_get_pyrefly_by_default() {
    let specs = specs_of(LspSettings::default());
    let lead = server_for(&specs, "python").unwrap();
    assert_eq!(lead.name, "pyrefly");
    assert_eq!(lead.command, Path::new("pyrefly"));
    assert_eq!(lead.args, ["lsp"]);
    assert!(!lead.add_on);
    // Neither of the others is there to run; ruff is, beside it.
    assert!(
        specs
            .iter()
            .all(|spec| spec.name != "pyright" && spec.name != "ty"),
        "{:?}",
        python_names(&specs)
    );
    assert_eq!(python_names(&specs), ["pyrefly", "ruff"]);
    let servers = servers_for(&specs, "python");
    let add_ons: Vec<&str> = (servers.add_ons.iter()).map(|s| s.name.as_str()).collect();
    assert_eq!(add_ons, ["ruff"]);

    // A file opens on it.
    let project = Project::new(false);
    let mut f = Fixture::new(&LspSettings::default());
    let mut buffer = Buffer::new("x = 1\n");
    f.open(&project.root, &mut buffer);
    f.fake("pyrefly").wait_initialized();
    let launched = f.fake("pyrefly").launched();
    assert_eq!(launched[0].0.command, Path::new("pyrefly"));
    assert_eq!(launched[0].0.args, ["lsp"]);
    assert_eq!(f.launched(), ["pyrefly"]);
}

#[test]
fn each_choice_launches_its_command() {
    for (choice, name, command, args) in MAINS {
        let specs = specs_of(settings(Some(choice), &[]));
        let lead = server_for(&specs, "python").unwrap();
        assert_eq!(
            (
                lead.name.as_str(),
                lead.command.as_path(),
                lead.args.clone()
            ),
            (
                name,
                Path::new(command),
                args.iter().map(|&a| a.to_owned()).collect()
            ),
            "{choice:?}"
        );
        // Only the chosen one, and ruff beside it.
        assert_eq!(python_names(&specs), [name, "ruff"], "{choice:?}");

        // The chosen one launches: none of the others, ruff missing.
        let project = Project::new(false);
        let mut f = Fixture::new(&settings(Some(choice), &[]));
        let mut buffer = Buffer::new("x = 1\n");
        f.open(&project.root, &mut buffer);
        f.fake(name).wait_initialized();
        let launched = f.fake(name).launched();
        assert_eq!(launched[0].0.command, Path::new(command), "{choice:?}");
        assert_eq!(launched[0].0.args, args, "{choice:?}");
        assert_eq!(f.launched(), [name], "{choice:?}");
    }

    // Unset, today's [lsp.servers.pyright] keeps pyright as the lead, and
    // pyrefly doesn't run beside it.
    let pyright = settings(None, &["pyright"]);
    let specs = specs_of(pyright.clone());
    assert_eq!(server_for(&specs, "python").unwrap().name, "pyright");
    assert_eq!(python_names(&specs), ["pyright", "ruff"]);
    // The setting beats the table: pyright doesn't run.
    let chosen = settings(Some(PythonServer::Pyrefly), &["pyright"]);
    assert_eq!(python_names(&specs_of(chosen)), ["pyrefly", "ruff"]);

    // No fallback: the chosen one missing, none of the others starts.
    let project = Project::new(false);
    let fake = FakeServer::new(Script::default());
    let launcher = Arc::new(Refusing {
        missing: "ty",
        fake: fake.launcher(),
    });
    let (mut manager, events) = managed(launcher, Some(Arc::new(ManualClock::new())));
    manager.update_settings(&settings(Some(PythonServer::Ty), &[]));
    let mut buffer = Buffer::new("x = 1\n");
    let path = project.root.join("main.py");
    let ty = manager
        .open(
            Some(&project.root),
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            buffer.subscribe(),
            &path,
            "python",
        )
        .unwrap()
        .unwrap();
    let why = stopped_with(&events, ty);
    assert!(why.is_some_and(|why| why.starts_with("ty not found")));
    wait_until("the document to wait", || {
        manager.server_of(buffer.id()).is_none()
    });
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(fake.launches(), 0, "{:?}", fake.launched());
}

/// Refuses `missing`'s launch as not found, as a host's launcher finding
/// nothing to run does, and hands the others to the fake.
struct Refusing {
    missing: &'static str,
    fake: Arc<crate::fake::FakeLauncher>,
}

impl ServerLauncher for Refusing {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        if spec.name == self.missing {
            return Err(io::ErrorKind::NotFound.into());
        }
        self.fake.launch(spec, root, cancel)
    }
}

/// The text `server` stops with, waiting for its next `Stopped`.
fn stopped_with(events: &Events, server: ServerId) -> Option<String> {
    events.until("a stop", |event| match event {
        LspEvent::Status {
            server: from,
            status: ServerStatus::Stopped(why),
            ..
        } if from == server => Some(why),
        _ => None,
    })
}

#[test]
fn each_server_gets_its_interpreter_section() {
    let project = Project::new(true);
    let python = project.python();
    let cases = [
        (
            PythonServer::Pyrefly,
            "pyrefly",
            "python",
            json!({ "pythonPath": python }),
        ),
        (
            PythonServer::Ty,
            "ty",
            "ty",
            json!({ "configuration": { "environment": { "python": python } } }),
        ),
        (
            PythonServer::Pyright,
            "pyright",
            "python",
            json!({ "pythonPath": python }),
        ),
    ];
    for (choice, name, section, want) in cases {
        let mut f = Fixture::new(&settings(Some(choice), &[name]));
        let mut buffer = Buffer::new("import missing\n");
        f.open(&project.root, &mut buffer);
        let fake = f.fake(name);
        fake.wait_initialized();
        assert_eq!(configuration(fake, section), want, "{name}");
        let launched = fake.launched();
        assert!(launched[0].0.interpreter_setting.is_some(), "{name}");
        // The user's own value stays.
        let mut own = settings(Some(choice), &[name]);
        let table = own.servers.get_mut(name).unwrap();
        let mine = "/usr/bin/python3";
        let mut value = want.clone();
        match name {
            "ty" => value["configuration"]["environment"]["python"] = json!(mine),
            _ => value["pythonPath"] = json!(mine),
        }
        let mut sections = serde_json::Map::new();
        sections.insert(section.into(), value.clone());
        table.settings = Some(sections);
        let mut f = Fixture::new(&own);
        let mut buffer = Buffer::new("import missing\n");
        f.open(&project.root, &mut buffer);
        let fake = f.fake(name);
        fake.wait_initialized();
        assert_eq!(configuration(fake, section), value, "{name}");
    }
    // ruff is told of none.
    let ruff = builtin_specs()
        .into_iter()
        .find(|s| s.name == "ruff")
        .unwrap();
    assert_eq!(ruff.interpreter_setting, None);
}

#[test]
fn ruff_runs_alongside_only_when_found() {
    let ruff = builtin_specs()
        .into_iter()
        .find(|s| s.name == "ruff")
        .unwrap();
    assert!(ruff.add_on && ruff.only_if_found && !ruff.completion);
    assert_eq!(
        (ruff.command.as_path(), ruff.args.as_slice()),
        (Path::new("ruff"), &["server".to_owned()][..])
    );

    // In the environment, beside each choice: placed beside the lead, never
    // asked for completions, though it offers them.
    for (choice, name, _, _) in MAINS {
        let project = Project::new(true);
        project.install_ruff();
        let mut f = Fixture::new(&settings(Some(choice), &[]));
        let mut buffer = Buffer::new("im\n");
        f.open(&project.root, &mut buffer);
        f.fake(name).wait_initialized();
        f.fake("ruff").wait_initialized();
        let launched = f.fake("ruff").launched();
        let (spec, root) = &launched[0];
        assert_eq!(spec.command, Path::new("ruff"), "{name}");
        assert_eq!(spec.args, ["server"], "{name}");
        assert!(spec.add_on, "{name}");
        assert_eq!(root, &project.root, "{name}");
        let deadline = Instant::now() + WAIT;
        while f.manager.servers_of(buffer.id()).len() < 2
            || f.manager
                .capabilities(f.manager.servers_of(buffer.id())[1])
                .is_none()
        {
            assert!(Instant::now() < deadline, "{name}: ruff never placed");
            std::thread::sleep(Duration::from_millis(5));
        }
        let servers = f.manager.servers_of(buffer.id());
        assert_eq!(f.manager.lead_of(buffer.id()), Some(servers[0]), "{name}");
        let routed: Vec<String> = (f.manager.route_servers(buffer.id()).into_iter())
            .map(|server| server.name)
            .collect();
        assert_eq!(routed, [name, "ruff"], "{name}");
        // Its completions are taken out of what it can do.
        let ruff_id = servers[1];
        let capabilities = f.manager.capabilities(ruff_id).unwrap();
        assert!(capabilities.completion_provider.is_none(), "{name}");
        let reply = |label: &str| {
            Scripted::now(Reply::Result(json!({
                "isIncomplete": false,
                "items": [{ "label": label }],
            })))
        };
        f.fake(name).set_reply(COMPLETION, reply("import"));
        f.fake("ruff").set_reply(COMPLETION, reply("ruff-item"));
        let list = answer(f.manager.completion(
            buffer.id(),
            buffer.version(),
            2,
            CompletionTrigger::Invoked,
        ))
        .unwrap()
        .expect("a list");
        let labels: Vec<&str> = (list.items.iter())
            .map(|item| item.label.as_str())
            .collect();
        assert_eq!(labels, ["import"], "{name}");
        let asked = (f.fake("ruff").records().iter())
            .filter(|r| r.method() == COMPLETION)
            .count();
        assert_eq!(asked, 0, "{name}");
        assert_eq!(f.manager.trigger_characters(buffer.id()), ["."], "{name}");
    }

    // Nowhere: never launched, and silent, the lead's status as it'd be
    // alone.
    let project = Project::new(true);
    let statuses: Arc<Mutex<Vec<(String, ServerStatus)>>> = Arc::default();
    let mut f = Fixture::new(&settings(None, &[]));
    let seen = statuses.clone();
    f.events.probe(move |event| {
        if let LspEvent::Status { name, status, .. } = event {
            seen.lock().unwrap().push((name.clone(), status.clone()));
        }
    });
    let mut buffer = Buffer::new("x = 1\n");
    f.open(&project.root, &mut buffer);
    f.fake("pyrefly").wait_initialized();
    let deadline = Instant::now() + WAIT;
    while !statuses
        .lock()
        .unwrap()
        .iter()
        .any(|(name, _)| name == "ruff")
    {
        assert!(Instant::now() < deadline, "ruff's thread never ended");
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(f.fake("ruff").launches(), 0);
    let ruff: Vec<ServerStatus> = (statuses.lock().unwrap().iter())
        .filter(|(name, _)| name == "ruff")
        .map(|(_, status)| status.clone())
        .collect();
    // No "starting…", no error: a stop with nothing to say.
    assert_eq!(ruff, [ServerStatus::Stopped(None)]);
    let lead = f.manager.lead_of(buffer.id()).unwrap();
    assert_eq!(f.manager.servers_of(buffer.id()), [lead]);
    assert_eq!(f.launched(), ["pyrefly"]);

    // With no main server for Python, ruff isn't there to lead in its
    // place, so a missing one never shows as a file's status.
    let mut off = settings(None, &["pyrefly"]);
    off.servers.get_mut("pyrefly").unwrap().enabled = false;
    let specs = specs_of(off);
    assert_eq!(python_names(&specs), Vec::<&str>::new());
    assert!(server_for(&specs, "python").is_none());
}

#[test]
fn missing_lead_names_binary_and_install() {
    let project = Project::new(false);
    let cases = [
        (
            PythonServer::Pyrefly,
            "pyrefly",
            "pyrefly",
            "pip install pyrefly",
        ),
        (PythonServer::Ty, "ty", "ty", "uv tool install ty"),
        (
            PythonServer::Pyright,
            "pyright",
            "pyright-langserver",
            "npm install -g pyright",
        ),
    ];
    for (choice, name, command, install) in cases {
        // LocalLauncher, on no PATH: where it looked, and how to install it.
        let (mut manager, events) =
            managed(Arc::new(LocalLauncher), Some(Arc::new(ManualClock::new())));
        manager.update_settings(&settings(Some(choice), &[name]));
        let mut buffer = Buffer::new("x = 1\n");
        let path = project.root.join("main.py");
        let server = manager
            .open(
                Some(&project.root),
                buffer.id(),
                buffer.text().clone(),
                buffer.version(),
                buffer.subscribe(),
                &path,
                "python",
            )
            .unwrap()
            .unwrap();
        let why = stopped_with(&events, server).expect("why it stopped");
        let want = format!(
            "{command} not found on PATH, and {} has no Python environment (.venv or venv); \
             install it with {install}",
            project.root.display()
        );
        assert_eq!(why, want, "{name}");
        assert!(why.contains(command) && why.contains(install), "{why}");
        // What the status item shows: the manager's latest.
        assert_eq!(
            manager.status_at(name, &project.root),
            Some(ServerStatus::Stopped(Some(want))),
            "{name}"
        );

        // A host's launcher, whose search fe-lsp doesn't know: the name.
        let fake = FakeServer::new(Script::default());
        let launcher = Arc::new(Refusing {
            missing: name,
            fake: fake.launcher(),
        });
        let (mut manager, events) = managed(launcher, Some(Arc::new(ManualClock::new())));
        manager.update_settings(&settings(Some(choice), &[name]));
        let mut buffer = Buffer::new("x = 1\n");
        let server = manager
            .open(
                Some(&project.root),
                buffer.id(),
                buffer.text().clone(),
                buffer.version(),
                buffer.subscribe(),
                &path,
                "python",
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            stopped_with(&events, server),
            Some(format!("{name} not found; install it with {install}")),
            "{name}"
        );
    }
}

#[test]
fn python_root_markers_in_order() {
    let want = [
        "pyproject.toml",
        "pyrefly.toml",
        "ty.toml",
        "setup.py",
        "requirements.txt",
        "pyrightconfig.json",
    ];
    let builtins = builtin_specs();
    for name in ALL {
        let spec = builtins.iter().find(|spec| spec.name == name).unwrap();
        assert_eq!(spec.root_markers, want, "{name}");
    }

    // A file outside the window's folder roots at the nearest folder
    // holding the first marker found anywhere up its tree: each marker
    // beats every one after it, however near that one is.
    let dir = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(dir.path()).unwrap();
    let markers: Vec<String> = want.iter().map(|&m| m.into()).collect();
    // Folder i holds marker i, folder i+1 nested in it.
    let mut folders = Vec::new();
    let mut folder = base.clone();
    for marker in want {
        folder = folder.join("d");
        touch(&folder.join(marker));
        folders.push(folder.clone());
    }
    let file = folder.join("deep").join("main.py");
    touch(&file);
    assert_eq!(server_root(&file, None, &markers), folders[0]);
    // Without the outer ones, the next marker wins.
    for (i, marker) in want.iter().enumerate().take(want.len() - 1) {
        fs::remove_file(folders[i].join(marker)).unwrap();
        assert_eq!(
            server_root(&file, None, &markers),
            folders[i + 1],
            "{marker}"
        );
    }
}

/// Holds launch `held` (counting from 1) until released, signalling
/// once it's asked.
struct HoldLaunch {
    fake: Arc<crate::fake::FakeLauncher>,
    held: usize,
    launches: Mutex<usize>,
    entered: Mutex<Sender<()>>,
    release: Mutex<Receiver<()>>,
}

impl HoldLaunch {
    /// Holding launch `held`, with the receiver of its signal and the
    /// sender of its release.
    fn new(fake: &FakeServer, held: usize) -> (Arc<Self>, Receiver<()>, Sender<()>) {
        let (entered, entering) = channel();
        let (release, released) = channel();
        let launcher = Arc::new(Self {
            fake: fake.launcher(),
            held,
            launches: Mutex::new(0),
            entered: Mutex::new(entered),
            release: Mutex::new(released),
        });
        (launcher, entering, release)
    }
}

impl ServerLauncher for HoldLaunch {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        let n = {
            let mut launches = self.launches.lock().unwrap();
            *launches += 1;
            *launches
        };
        if n == self.held {
            let _ = self.entered.lock().unwrap().send(());
            let _ = self.release.lock().unwrap().recv_timeout(WAIT);
        }
        self.fake.launch(spec, root, cancel)
    }
}

/// The Python environment of each `Starting` pyrefly reports, as its
/// session has it when the host hears: once `session` names the session.
type Heard = Arc<Mutex<Vec<Option<PathBuf>>>>;
type Watched = Arc<Mutex<Option<Arc<Mutex<crate::server::Session>>>>>;

fn listen(events: &Events) -> (Heard, Watched) {
    let heard: Heard = Arc::default();
    let watched: Watched = Arc::default();
    let (seen, session) = (heard.clone(), watched.clone());
    events.probe(move |event| {
        if let LspEvent::Status {
            name,
            status: ServerStatus::Starting,
            ..
        } = event
            && name == "pyrefly"
        {
            let env = (session.lock().unwrap().as_ref())
                .and_then(|session| session.lock().unwrap().python_env.clone());
            seen.lock()
                .unwrap()
                .push(env.map(|env| env.dir().to_owned()));
        }
    });
    (heard, watched)
}

fn open_main(manager: &mut LspManager, root: &Path, buffer: &mut Buffer) -> ServerId {
    manager
        .open(
            Some(root),
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            buffer.subscribe(),
            &root.join("main.py"),
            "python",
        )
        .unwrap()
        .unwrap()
}

/// T683 F4: once a launch's environment is chosen, and before a launch
/// that may block, the host hears a status again if it isn't the one it
/// last heard with, on a first launch and on an automatic restart.
#[test]
fn a_new_environment_is_announced_before_a_blocked_launch() {
    // A first launch, held: `Starting` before the environment is chosen,
    // then again once it is, which the host reads it with.
    let project = Project::new(true);
    let fake = FakeServer::new(Script::default());
    let (launcher, entering, release) = HoldLaunch::new(&fake, 1);
    let (mut manager, events) = managed(launcher, Some(Arc::new(ManualClock::new())));
    manager.update_settings(&settings(None, &[]));
    let (heard, _) = listen(&events);
    let mut buffer = Buffer::new("x = 1\n");
    let server = open_main(&mut manager, &project.root, &mut buffer);
    entering.recv_timeout(WAIT).unwrap();
    let starts = heard.lock().unwrap().clone();
    assert_eq!(starts.len(), 2, "{starts:?}");
    assert_eq!(
        manager.python_env(server).map(|env| env.dir().to_owned()),
        Some(project.venv())
    );
    release.send(()).unwrap();
    fake.wait_initialized();

    // With no environment, nothing new to say: once.
    let project = Project::new(false);
    let fake = FakeServer::new(Script::default());
    let (launcher, entering, release) = HoldLaunch::new(&fake, 2);
    let clock = Arc::new(ManualClock::new());
    let (mut manager, events) = managed(launcher, Some(clock.clone()));
    manager.update_settings(&settings(None, &[]));
    let (heard, watched) = listen(&events);
    let mut buffer = Buffer::new("x = 1\n");
    let server = open_main(&mut manager, &project.root, &mut buffer);
    fake.wait_initialized();
    assert_eq!(heard.lock().unwrap().len(), 1);
    assert_eq!(manager.python_env(server), None);

    // One made since: the restart after a crash finds it, and says so
    // with it before its launch, held here, goes ahead.
    *watched.lock().unwrap() = manager.shared_session(server);
    touch(&project.venv().join(PYTHON));
    heard.lock().unwrap().clear();
    fake.crash();
    let deadline = Instant::now() + WAIT;
    loop {
        clock.advance(Duration::from_millis(100));
        if entering.try_recv().is_ok() {
            break;
        }
        assert!(Instant::now() < deadline, "no second launch");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        *heard.lock().unwrap(),
        [None, Some(project.venv())],
        "the restart's Starting, then the new environment's"
    );
    assert_eq!(
        manager.python_env(server).map(|env| env.dir().to_owned()),
        Some(project.venv())
    );
    release.send(()).unwrap();
    fake.wait_initialized_at(2);
}
