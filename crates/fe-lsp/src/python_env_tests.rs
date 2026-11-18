//! The Python environment of a Python server: the order it's found in,
//! how the launcher runs a server in it, the interpreter its settings get,
//! and what a server not found there says.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use fe_buffer::Buffer;
use fe_config::settings::LspSettings;
use lsp_types::request::WorkspaceConfiguration;
use lsp_types::{ConfigurationItem, ConfigurationParams};
use serde_json::{Value, json};

use super::{
    PythonEnv, PythonEnvChoice, find_python_env, python_env, serves_python, with_interpreter,
};
use crate::fake::{FakeServer, Script};
use crate::registry::{Registry, builtin_specs};
use crate::{
    LaunchCancel, LocalLauncher, LspEvent, LspManager, ManualClock, ServerIo, ServerLauncher,
    ServerSpec, ServerStatus,
};

const WAIT: Duration = Duration::from_secs(10);

#[cfg(not(windows))]
const PYTHON: &str = "bin/python";
#[cfg(windows)]
const PYTHON: &str = r"Scripts\python.exe";

/// A file at `file`, and its folders.
fn touch(file: &Path) {
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, "").unwrap();
}

/// An environment at `dir`: one with an interpreter.
fn make_env(dir: &Path) {
    touch(&dir.join(PYTHON));
}

fn env(dir: impl Into<PathBuf>) -> Option<PythonEnv> {
    Some(PythonEnv { dir: dir.into() })
}

/// The candidates of the order, each a folder in its own place: the
/// setting's, absolute, under `~` or relative to the root, `$VIRTUAL_ENV`,
/// `.venv` and `venv`.
#[derive(Clone, Copy, Debug, PartialEq)]
enum At {
    Absolute,
    Home,
    Relative,
    VirtualEnv,
    DotVenv,
    Venv,
}

/// The folders of the table's tests: a root, a home, and elsewhere.
struct Places {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    elsewhere: PathBuf,
}

impl Places {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(dir.path()).unwrap();
        let (root, home, elsewhere) = (base.join("root"), base.join("home"), base.join("else"));
        for dir in [&root, &home, &elsewhere] {
            fs::create_dir_all(dir).unwrap();
        }
        Self {
            _dir: dir,
            root,
            home,
            elsewhere,
        }
    }

    fn dir(&self, at: At) -> PathBuf {
        match at {
            At::Absolute => self.elsewhere.join("abs"),
            At::Home => self.home.join("envs/app"),
            At::Relative => self.root.join("envs/app"),
            At::VirtualEnv => self.elsewhere.join("active"),
            At::DotVenv => self.root.join(".venv"),
            At::Venv => self.root.join("venv"),
        }
    }

    /// The setting naming `at`, as a user would write it.
    fn setting(&self, at: At) -> PathBuf {
        match at {
            At::Absolute => self.dir(At::Absolute),
            At::Home => PathBuf::from("~/envs/app"),
            At::Relative => PathBuf::from("envs/app"),
            _ => unreachable!("not a setting"),
        }
    }
}

/// A row of the order's table: the setting, whether `$VIRTUAL_ENV` is set,
/// the candidates with an interpreter, whether there's a home, and the
/// environment found.
type Row = (Option<At>, bool, &'static [At], bool, Option<At>);

#[test]
fn env_order() {
    use At::*;
    const ALL: [At; 6] = [Absolute, Home, Relative, VirtualEnv, DotVenv, Venv];
    let all = ALL;
    let table: &[Row] = &[
        // The setting wins, however it's written.
        (Some(Absolute), true, &ALL, true, Some(Absolute)),
        (Some(Home), true, &ALL, true, Some(Home)),
        (Some(Relative), true, &ALL, true, Some(Relative)),
        // Then $VIRTUAL_ENV, then .venv, then venv.
        (None, true, &ALL, true, Some(VirtualEnv)),
        (None, false, &ALL, true, Some(DotVenv)),
        (None, true, &[DotVenv, Venv], true, Some(DotVenv)),
        (None, false, &[Venv], true, Some(Venv)),
        // A candidate with no interpreter doesn't count.
        (Some(Absolute), true, &[DotVenv], true, Some(DotVenv)),
        (
            Some(Relative),
            true,
            &[VirtualEnv, Venv],
            true,
            Some(VirtualEnv),
        ),
        (None, true, &[Venv], true, Some(Venv)),
        // `~` with no home is nowhere.
        (Some(Home), false, &ALL, false, Some(DotVenv)),
        // None at all: the PATH, as before.
        (None, true, &[], true, None),
        (Some(Absolute), true, &[], true, None),
    ];
    for &(setting, virtual_env, with, home, want) in table {
        let places = Places::new();
        for &at in with {
            make_env(&places.dir(at));
        }
        // Folders of every candidate exist; only `with` have Python.
        for at in all {
            fs::create_dir_all(places.dir(at)).unwrap();
        }
        let setting = setting.map(|at| places.setting(at));
        let active = places.dir(VirtualEnv);
        let found = find_python_env(
            setting.as_deref(),
            virtual_env.then_some(active.as_os_str()),
            &places.root,
            home.then_some(places.home.as_path()),
        );
        let row = (setting, virtual_env, with, home);
        assert_eq!(found, want.and_then(|at| env(places.dir(at))), "{row:?}");
    }
}

#[test]
fn empty_and_relative_values_are_skipped() {
    let places = Places::new();
    make_env(&places.root.join(".venv"));
    make_env(&places.root.join("rel"));
    let found = |setting: Option<&Path>, virtual_env: Option<&str>| {
        find_python_env(
            setting,
            virtual_env.map(OsStr::new),
            &places.root,
            Some(&places.home),
        )
    };
    let dot_venv = env(places.root.join(".venv"));
    // An empty setting is no setting; a relative $VIRTUAL_ENV is skipped,
    // never taken under the root.
    assert_eq!(found(Some(Path::new("")), None), dot_venv);
    assert_eq!(found(None, Some("rel")), dot_venv);
    assert_eq!(found(None, Some("")), dot_venv);
    assert_eq!(
        found(Some(Path::new("rel")), None),
        env(places.root.join("rel"))
    );
}

#[test]
fn the_env_names_its_parts() {
    let env = PythonEnv {
        dir: PathBuf::from("/p/.venv"),
    };
    assert_eq!(env.name(), ".venv");
    assert_eq!(env.python(), Path::new("/p/.venv").join(PYTHON));
    assert_eq!(env.python().parent(), Some(env.bin().as_path()));
}

#[test]
fn only_servers_of_python_get_one() {
    let places = Places::new();
    make_env(&places.root.join(".venv"));
    let python = ServerSpec {
        languages: vec!["python".into()],
        env: vec![("VIRTUAL_ENV".into(), String::new())],
        ..ServerSpec::new("ty", "ty")
    };
    let rust = ServerSpec {
        languages: vec!["rust".into()],
        ..python.clone()
    };
    assert!(serves_python(&python) && !serves_python(&rust));
    assert_eq!(
        python_env(&python, &places.root),
        env(places.root.join(".venv"))
    );
    assert_eq!(python_env(&rust, &places.root), None);

    // The spec's own VIRTUAL_ENV is $VIRTUAL_ENV; its setting beats it.
    let active = places.dir(At::VirtualEnv);
    make_env(&active);
    let activated = ServerSpec {
        env: vec![("VIRTUAL_ENV".into(), active.display().to_string())],
        ..python.clone()
    };
    assert_eq!(python_env(&activated, &places.root), env(&active));
    let set = ServerSpec {
        python_environment: Some("venv".into()),
        ..activated
    };
    make_env(&places.root.join("venv"));
    assert_eq!(
        python_env(&set, &places.root),
        env(places.root.join("venv"))
    );
}

#[test]
fn the_registry_hands_python_servers_the_setting() {
    let mut settings = LspSettings {
        python_environment: Some("~/envs/app".into()),
        ..LspSettings::default()
    };
    settings.servers.insert(
        "ty".into(),
        serde_json::from_value(json!({ "command": "ty", "languages": ["python"] })).unwrap(),
    );
    let mut registry = Registry::default();
    registry.set_settings(settings);
    let specs = registry.specs();
    let of = |name: &str| specs.iter().find(|spec| spec.name == name).unwrap();
    let set = Some(PathBuf::from("~/envs/app"));
    assert_eq!(of("pyright").python_environment, set);
    assert_eq!(of("ty").python_environment, set);
    assert_eq!(of("rust-analyzer").python_environment, None);

    // Only pyright's built-in has an interpreter setting.
    let hooked: Vec<_> = builtin_specs()
        .into_iter()
        .filter_map(|spec| Some((spec.name, spec.interpreter_setting?)))
        .collect();
    assert_eq!(hooked, [("pyright".into(), "python.pythonPath".into())]);
    assert_eq!(of("ty").interpreter_setting, None);
}

#[test]
fn the_interpreter_goes_in_at_the_hook() {
    let env = PythonEnv {
        dir: PathBuf::from("/p/.venv"),
    };
    let python = env.python().display().to_string();
    let key = Some("python.pythonPath");
    let cases = [
        (Value::Null, json!({ "python": { "pythonPath": python } })),
        (json!({}), json!({ "python": { "pythonPath": python } })),
        (
            json!({ "python": { "analysis": { "typeCheckingMode": "strict" } } }),
            json!({ "python": {
                "analysis": { "typeCheckingMode": "strict" },
                "pythonPath": python,
            } }),
        ),
        // The user's own value stays, and so does a value that isn't a
        // table on the way.
        (
            json!({ "python": { "pythonPath": "/usr/bin/python3" } }),
            json!({ "python": { "pythonPath": "/usr/bin/python3" } }),
        ),
        (json!({ "python": 3 }), json!({ "python": 3 })),
    ];
    for (settings, want) in cases {
        assert_eq!(
            with_interpreter(settings.clone(), key, Some(&env)),
            want,
            "{settings}"
        );
    }
    // No hook, or no environment: nothing added.
    assert_eq!(with_interpreter(json!({}), None, Some(&env)), json!({}));
    assert_eq!(with_interpreter(json!({}), key, None), json!({}));
    assert_eq!(with_interpreter(Value::Null, None, Some(&env)), Value::Null);
}

/// The configuration a fake server at launch 1 gets for `section`.
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

/// A manager on `launcher` whose statuses go to the receiver.
fn manager(launcher: Arc<dyn ServerLauncher>) -> (LspManager, Receiver<(String, ServerStatus)>) {
    let (sender, statuses) = channel();
    let manager = LspManager::with_clock(launcher, Arc::new(ManualClock::new()), move |event| {
        if let LspEvent::Status { name, status, .. } = event {
            let _ = sender.send((name, status));
        }
    });
    (manager, statuses)
}

#[test]
fn pyright_is_configured_with_the_envs_python() {
    let places = Places::new();
    let venv = places.root.join(".venv");
    make_env(&venv);
    let python = venv.join(PYTHON).display().to_string();
    let fake = FakeServer::new(Script::default());
    let (mut manager, _statuses) = manager(fake.launcher());
    let mut settings = LspSettings::default();
    settings.servers.insert(
        "pyright".into(),
        serde_json::from_value(json!({
            "env": { "VIRTUAL_ENV": "" },
            "settings": { "python": { "analysis": { "diagnosticMode": "workspace" } } },
        }))
        .unwrap(),
    );
    manager.update_settings(&settings);
    let mut buffer = Buffer::new("import missing\n");
    let path = places.root.join("main.py");
    manager
        .open(
            Some(&places.root),
            buffer.id(),
            buffer.text().clone(),
            buffer.version(),
            buffer.subscribe(),
            &path,
            "python",
        )
        .unwrap()
        .unwrap();
    fake.wait_initialized();
    assert_eq!(
        configuration(&fake, "python"),
        json!({
            "analysis": { "diagnosticMode": "workspace" },
            "pythonPath": python,
        })
    );

    // New settings keep it.
    settings.servers.insert(
        "pyright".into(),
        serde_json::from_value(json!({
            "env": { "VIRTUAL_ENV": "" },
            "settings": { "python": { "analysis": { "diagnosticMode": "openFilesOnly" } } },
        }))
        .unwrap(),
    );
    manager.update_settings(&settings);
    let want = json!({
        "analysis": { "diagnosticMode": "openFilesOnly" },
        "pythonPath": python,
    });
    let deadline = std::time::Instant::now() + WAIT;
    while configuration(&fake, "python") != want {
        assert!(
            std::time::Instant::now() < deadline,
            "the new settings never came"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(buffer);
}

#[test]
fn no_hook_adds_nothing() {
    let places = Places::new();
    make_env(&places.root.join(".venv"));
    let fake = FakeServer::new(Script::default());
    let (mut manager, _statuses) = manager(fake.launcher());
    let spec = ServerSpec {
        languages: vec!["python".into()],
        env: vec![("VIRTUAL_ENV".into(), String::new())],
        settings: json!({}),
        ..ServerSpec::new("ty", "ty")
    };
    manager.start(spec, &places.root).unwrap();
    fake.wait_initialized();
    assert_eq!(configuration(&fake, "python"), Value::Null);
}

/// Notes the environment each launch is handed, and runs `before` (once)
/// as the launch begins: after fe-lsp chose the environment, before the
/// server runs.
struct Recording {
    inner: Arc<dyn ServerLauncher>,
    handed: std::sync::Mutex<Vec<PythonEnvChoice>>,
    before: std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl ServerLauncher for Recording {
    fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
        cancel: &LaunchCancel,
    ) -> io::Result<ServerIo> {
        self.handed
            .lock()
            .unwrap()
            .push(spec.python_env_choice.clone());
        if let Some(before) = self.before.lock().unwrap().take() {
            before();
        }
        self.inner.launch(spec, root, cancel)
    }
}

#[test]
fn each_launch_has_one_env() {
    let places = Places::new();
    let root = places.root.clone();
    let venv = root.join(".venv");
    // A host's spec, with an interpreter setting.
    let spec = ServerSpec {
        languages: vec!["python".into()],
        env: vec![("VIRTUAL_ENV".into(), String::new())],
        interpreter_setting: Some("python.pythonPath".into()),
        settings: json!({}),
        ..ServerSpec::new("ty", "ty")
    };
    let fake = FakeServer::new(Script::default());
    // An environment made while the first launch is under way counts from
    // the next.
    let made = venv.clone();
    let recording = Arc::new(Recording {
        inner: fake.launcher(),
        handed: Default::default(),
        before: std::sync::Mutex::new(Some(Box::new(move || make_env(&made)))),
    });
    let (mut manager, _statuses) = manager(recording.clone());
    let first = manager.start(spec.clone(), &root).unwrap();
    fake.wait_initialized();
    assert_eq!(manager.python_env(first), None);
    assert_eq!(configuration(&fake, "python"), Value::Null);

    // Restarted, the launcher, the session and the settings have it.
    let second = manager.restart("ty", &root).unwrap().unwrap();
    fake.wait_initialized_at(2);
    assert_eq!(manager.python_env(second), env(&venv));
    let python = venv.join(PYTHON).display().to_string();
    let items = vec![ConfigurationItem {
        scope_uri: None,
        section: Some("python".into()),
    }];
    let answers = fake
        .request_at::<WorkspaceConfiguration>(2, ConfigurationParams { items })
        .unwrap();
    assert_eq!(answers[0], json!({ "pythonPath": python }));
    assert_eq!(
        *recording.handed.lock().unwrap(),
        [
            PythonEnvChoice::Chosen(None),
            PythonEnvChoice::Chosen(env(&venv)),
        ]
    );

    // Not a server of Python's: none, though there's one at its root.
    let rust = ServerSpec {
        languages: vec!["rust".into()],
        ..ServerSpec::new("ra", "ra")
    };
    let third = manager.start(rust, &root).unwrap();
    fake.wait_initialized_at(3);
    assert_eq!(manager.python_env(third), None);
    assert_eq!(
        recording.handed.lock().unwrap()[2],
        PythonEnvChoice::Chosen(None)
    );
}

/// Refuses every launch as not found, as a host's launcher finding
/// nothing to run does.
struct NotFound;

impl ServerLauncher for NotFound {
    fn launch(&self, _: &ServerSpec, _: &Path, _: &LaunchCancel) -> io::Result<ServerIo> {
        Err(io::ErrorKind::NotFound.into())
    }
}

/// The status a server of `spec` at `root` stops with on `launcher`.
fn stopped_with(
    launcher: Arc<dyn ServerLauncher>,
    spec: ServerSpec,
    root: &Path,
) -> Option<String> {
    let (mut manager, statuses) = manager(launcher);
    manager.start(spec, root).unwrap();
    loop {
        match statuses.recv_timeout(WAIT).unwrap() {
            (_, ServerStatus::Stopped(why)) => return why,
            _ => continue,
        }
    }
}

fn pyright() -> ServerSpec {
    let mut spec = builtin_specs()
        .into_iter()
        .find(|spec| spec.name == "pyright")
        .unwrap();
    spec.env = vec![
        ("PATH".into(), "/nonexistent".into()),
        ("VIRTUAL_ENV".into(), String::new()),
    ];
    spec
}

#[test]
fn a_python_server_not_found_says_where_it_looked() {
    let places = Places::new();
    let root = &places.root;
    let local = || Arc::new(LocalLauncher) as Arc<dyn ServerLauncher>;
    // What the launcher's error and the server's status say.
    let said = |spec: ServerSpec| {
        let err = LocalLauncher
            .launch(&spec, root, &LaunchCancel::new())
            .err()
            .unwrap();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        (err.to_string(), stopped_with(local(), spec, root))
    };

    // No environment: the PATH, and the root that has none.
    let no_env = format!(
        "pyright-langserver not found on PATH, and {} has no Python environment (.venv or venv)",
        root.display()
    );
    assert_eq!(said(pyright()), (no_env.clone(), Some(no_env)));

    make_env(&root.join(".venv"));
    let bin = root.join(".venv").join(PYTHON);
    let in_env = format!(
        "pyright-langserver not found in {} or on PATH",
        bin.parent().unwrap().display()
    );
    assert_eq!(said(pyright()), (in_env.clone(), Some(in_env)));

    // A host's launcher, whose search fe-lsp doesn't know: the name, as
    // before.
    let host = stopped_with(Arc::new(NotFound), pyright(), root);
    assert_eq!(host.as_deref(), Some("pyright not found"));

    // Any other server's text is as it was.
    make_env(&root.join("venv"));
    let rust = ServerSpec {
        languages: vec!["rust".into()],
        env: vec![("PATH".into(), "/nonexistent".into())],
        ..ServerSpec::new("rust-analyzer", "rust-analyzer")
    };
    let rust = said(rust);
    let want = "rust-analyzer not found".to_owned();
    assert_eq!(rust, (want.clone(), Some(want)));
}

/// The launcher's side, with real processes: a script that writes what it
/// was run with.
#[cfg(unix)]
mod launched {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// `body` as an executable `/bin/sh` script at `file`.
    fn script(file: &Path, body: &str) {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, format!("#!/bin/sh\n{body}")).unwrap();
        fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Writes its PATH and VIRTUAL_ENV to `ran.txt` in its working
    /// directory, with shell builtins only.
    const WRITES_ITS_ENV: &str = "printf '%s|%s' \"$PATH\" \"${VIRTUAL_ENV-unset}\" > ran.txt\n";

    /// Runs `spec` at `root`, and what it wrote.
    fn run(spec: &ServerSpec, root: &Path) -> String {
        let mut io = LocalLauncher
            .launch(spec, root, &LaunchCancel::new())
            .unwrap();
        async_io::block_on(io.process.exited());
        fs::read_to_string(root.join("ran.txt")).unwrap()
    }

    fn spec(languages: &[&str], path: &str) -> ServerSpec {
        ServerSpec {
            languages: languages.iter().map(|&l| l.into()).collect(),
            env: vec![
                ("PATH".into(), path.into()),
                ("VIRTUAL_ENV".into(), String::new()),
            ],
            ..ServerSpec::new("ty", "ty")
        }
    }

    #[test]
    fn a_python_server_runs_in_the_env() {
        let places = Places::new();
        let root = &places.root;
        let venv = root.join(".venv");
        make_env(&venv);
        script(&venv.join("bin/ty"), WRITES_ITS_ENV);
        // On the spec's PATH too, but the environment's comes first.
        let other = places.elsewhere.join("bin");
        script(&other.join("ty"), "printf other > ran.txt\n");
        let path = format!("/nonexistent:{}", other.display());
        let ran = run(&spec(&["python"], &path), root);
        let bin = venv.join("bin");
        assert_eq!(ran, format!("{}:{path}|{}", bin.display(), venv.display()));
    }

    #[test]
    fn a_chosen_env_is_the_one_run_in() {
        let places = Places::new();
        let root = &places.root;
        let venv = root.join(".venv");
        make_env(&venv);
        script(&venv.join("bin/ty"), "printf venv > ran.txt\n");
        let chosen = places.dir(At::VirtualEnv);
        make_env(&chosen);
        script(&chosen.join("bin/ty"), WRITES_ITS_ENV);
        let path = "/nonexistent";
        let spec = |choice| ServerSpec {
            python_env_choice: choice,
            ..spec(&["python"], path)
        };
        // The chosen one, though .venv is at the root.
        let ran = run(&spec(PythonEnvChoice::Chosen(env(&chosen))), root);
        let bin = chosen.join("bin");
        let want = format!("{}:{path}|{}", bin.display(), chosen.display());
        assert_eq!(ran, want);
        // None chosen: none looked for.
        let err = LocalLauncher
            .launch(
                &spec(PythonEnvChoice::Chosen(None)),
                root,
                &LaunchCancel::new(),
            )
            .err()
            .unwrap();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn with_no_env_the_path_is_as_before() {
        let places = Places::new();
        let root = &places.root;
        let other = places.elsewhere.join("bin");
        script(&other.join("ty"), WRITES_ITS_ENV);
        let path = other.display().to_string();
        assert_eq!(run(&spec(&["python"], &path), root), format!("{path}|"));
    }

    #[test]
    fn other_servers_are_unchanged() {
        let places = Places::new();
        let root = &places.root;
        let venv = root.join(".venv");
        make_env(&venv);
        script(&venv.join("bin/ty"), "printf venv > ran.txt\n");
        let other = places.elsewhere.join("bin");
        script(&other.join("ty"), WRITES_ITS_ENV);
        let path = other.display().to_string();
        assert_eq!(run(&spec(&["rust"], &path), root), format!("{path}|"));

        // Only in the environment: not found, by its old text.
        fs::remove_file(other.join("ty")).unwrap();
        let err = LocalLauncher
            .launch(&spec(&["rust"], &path), root, &LaunchCancel::new())
            .err()
            .unwrap();
        assert_eq!(err.to_string(), "ty not found");
    }
}
