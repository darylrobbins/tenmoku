// SPDX-License-Identifier: Apache-2.0

//! The adapter registry (§3, §13; card D3): its layers, `type` aliases,
//! "not found" with hints, the Rust default, the login shell's `PATH`, the
//! Python interpreter, Rust's LLDB formatters, the macOS `xcrun` lookup as
//! data, and the host-given port of a `Port` adapter.
//!
//! Programs are found on a fake login shell's `PATH`, a temporary folder
//! of stand-in scripts, never on the `PATH` this test runs with.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use fe_config::debug_settings::{DebugAdapterSettings, DebugSettings, DebugTransport};
use fe_lsp::LoginPath;
use kiln_l10n::tr;

use crate::launcher::{
    AdapterIo, AdapterLauncher, AdapterSpec, AdapterTransport, LaunchCancel, LocalLauncher,
    PORT_PLACEHOLDER,
};
use crate::registry::{
    Context, HostOs, Lookup, Registry, Runner, Source, Status, XCRUN, lldb_dap_lookups,
    python_interpreter, rust_init_commands,
};

/// A folder of programs and a login shell whose `PATH` is `bin` (after a
/// directory that doesn't exist), with a project root beside them.
struct Fixture {
    _dir: tempfile::TempDir,
    dir: PathBuf,
    bin: PathBuf,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tempdir = tempfile::tempdir().unwrap();
        let dir = tempdir.path().canonicalize().unwrap();
        let bin = dir.join("bin");
        let root = dir.join("root");
        std::fs::create_dir(&bin).unwrap();
        std::fs::create_dir(&root).unwrap();
        Self {
            _dir: tempdir,
            dir,
            bin,
            root,
        }
    }

    /// A program in `bin`, on the login shell's `PATH`.
    fn on_path(&self, name: &str) -> PathBuf {
        program(&self.bin.join(name))
    }

    /// The login shell's `PATH`: `/nonexistent:<bin>`.
    fn login(&self) -> Arc<LoginPath> {
        self.login_with(&format!("/nonexistent:{}", self.bin.display()))
    }

    /// A login shell whose `PATH` is `path`. It runs at once, so a shell
    /// starved past fe-lsp's 2 s limit on a loaded machine (which isn't
    /// what is tested) is tried again.
    fn login_with(&self, path: &str) -> Arc<LoginPath> {
        let shell = self.dir.join("login-shell");
        write_script(&shell, &format!("PATH='{path}' /bin/sh -c \"$3\""));
        for _ in 0..5 {
            let login = Arc::new(LoginPath::new(&shell));
            if login.path().is_some() {
                return login;
            }
        }
        panic!("the stand-in login shell never gave its PATH");
    }

    fn registry(&self) -> Registry {
        Registry::new(self.login()).with_os(HostOs::Linux)
    }

    fn context(&self) -> Context {
        Context {
            root: self.root.clone(),
            language: None,
            virtual_env: None,
        }
    }
}

/// Writes a shell script at `path` running `body`, made executable.
fn write_script(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// A stand-in program at `path`, never run.
fn program(path: &Path) -> PathBuf {
    write_script(path, "exit 0");
    path.to_owned()
}

/// A [`Runner`] answering from a table, by the program's file name, and
/// recording what it was asked.
#[derive(Default)]
struct FakeRunner {
    answers: BTreeMap<String, String>,
    calls: Mutex<Vec<(PathBuf, Vec<String>, PathBuf)>>,
}

impl FakeRunner {
    fn answering(answers: &[(&str, &str)]) -> Arc<Self> {
        Arc::new(Self {
            answers: answers
                .iter()
                .map(|(program, out)| ((*program).to_owned(), (*out).to_owned()))
                .collect(),
            calls: Mutex::default(),
        })
    }

    fn calls(&self) -> Vec<(PathBuf, Vec<String>, PathBuf)> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Runner for FakeRunner {
    fn stdout(&self, program: &Path, args: &[&str], cwd: &Path) -> Option<String> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((
                program.to_owned(),
                args.iter().map(|arg| (*arg).to_owned()).collect(),
                cwd.to_owned(),
            ));
        let name = program.file_name()?.to_str()?;
        self.answers.get(name).cloned()
    }
}

fn settings(adapters: &[(&str, DebugAdapterSettings)]) -> DebugSettings {
    DebugSettings {
        adapters: adapters
            .iter()
            .map(|(name, adapter)| ((*name).to_owned(), adapter.clone()))
            .collect(),
        ..DebugSettings::default()
    }
}

/// (host, user, extension) → source, program, arguments.
type OrderRow<'a> = ((bool, bool, bool), Source, &'a Path, &'a [&'a str]);

/// Host adapters, the user's settings and extension adapters → which
/// adapter `lldb` names, from which layer.
type CollisionRow<'a> = (
    &'a [&'a str],
    &'a [(&'a str, DebugAdapterSettings)],
    &'a [&'a str],
    (&'a str, Source),
);

/// (.venv's, $VIRTUAL_ENV's, python3 on the PATH) → the interpreter.
type PythonRow<'a> = ((bool, bool, bool), Option<&'a Path>);

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[cfg(unix)]
#[test]
fn resolution_order_is_host_user_extension_builtin() {
    let fixture = Fixture::new();
    let builtin = fixture.on_path("codelldb");
    let host = program(&fixture.dir.join("host/codelldb"));
    let user = program(&fixture.dir.join("user/codelldb"));
    let extension = program(&fixture.dir.join("extension/codelldb"));
    let spec = |command: &Path, arg: &str| AdapterSpec {
        args: strings(&[arg]),
        types: strings(&["lldb"]),
        ..AdapterSpec::new("codelldb", command)
    };
    // The user's settings set only the command: an extension's arguments
    // stay under them.
    let user_settings = DebugAdapterSettings {
        command: Some(user.display().to_string()),
        env: Some(BTreeMap::from([("CODELLDB_LOG".into(), "debug".into())])),
        ..DebugAdapterSettings::default()
    };
    // (host, user, extension) → source, program, arguments.
    let table: [OrderRow<'_>; 8] = [
        ((false, false, false), Source::Builtin, &builtin, &[]),
        (
            (false, false, true),
            Source::Extension,
            &extension,
            &["--ext"],
        ),
        ((false, true, false), Source::User, &user, &[]),
        ((false, true, true), Source::User, &user, &["--ext"]),
        ((true, false, false), Source::Host, &host, &["--host"]),
        ((true, false, true), Source::Host, &host, &["--host"]),
        ((true, true, false), Source::Host, &host, &["--host"]),
        ((true, true, true), Source::Host, &host, &["--host"]),
    ];
    for ((has_host, has_user, has_extension), source, command, args) in table {
        let row = format!("host {has_host}, user {has_user}, extension {has_extension}");
        let mut registry = fixture.registry();
        if has_host {
            registry.register(spec(&host, "--host"));
        }
        if has_user {
            registry.set_settings(&settings(&[("codelldb", user_settings.clone())]));
        }
        if has_extension {
            registry.set_extension_specs(vec![spec(&extension, "--ext")]);
        }
        let context = fixture.context();
        for by in ["codelldb", "lldb"] {
            let adapter = registry.find(by, &context).unwrap();
            assert_eq!(adapter.source, source, "{row} by {by}");
            assert_eq!(adapter.status, Status::Found, "{row}");
            assert_eq!(adapter.spec.command, command, "{row}");
            assert_eq!(adapter.spec.args, strings(args), "{row}");
            let env = adapter
                .spec
                .env
                .iter()
                .any(|(name, _)| name == "CODELLDB_LOG");
            assert_eq!(env, source == Source::User, "{row}");
        }
        // Each name once, in the layer that won it: any layer above the
        // built-ins puts codelldb first.
        let names = registry.names();
        let expected = if has_host || has_user || has_extension {
            ["codelldb", "lldb-dap", "debugpy", "delve"]
        } else {
            ["lldb-dap", "codelldb", "debugpy", "delve"]
        };
        assert_eq!(names, expected, "{row}");
    }

    // Adapters of a layer's own, with no built-in of their name, are
    // listed highest layer first, then the built-ins; and a `type` they
    // share goes to the highest.
    let lldb = |name: &str| AdapterSpec {
        types: strings(&["lldb"]),
        ..AdapterSpec::new(name, &host)
    };
    let layered = |(has_host, has_user, has_extension): (bool, bool, bool)| {
        let mut registry = fixture.registry();
        if has_extension {
            registry.set_extension_specs(vec![lldb("extension-lldb"), lldb("extension-lldb")]);
        }
        if has_user {
            registry.set_settings(&settings(&[(
                "user-lldb",
                DebugAdapterSettings {
                    command: Some(user.display().to_string()),
                    types: Some(strings(&["lldb"])),
                    ..DebugAdapterSettings::default()
                },
            )]));
        }
        if has_host {
            registry.register(lldb("host-lldb"));
        }
        registry
    };
    let registry = layered((true, true, true));
    assert_eq!(
        registry.names(),
        [
            "host-lldb",
            "user-lldb",
            "extension-lldb",
            "lldb-dap",
            "codelldb",
            "debugpy",
            "delve"
        ]
    );
    let context = fixture.context();
    for (layers, first) in [
        ((true, true, true), "host-lldb"),
        ((false, true, true), "user-lldb"),
        ((false, false, true), "extension-lldb"),
        ((false, false, false), "codelldb"),
    ] {
        let found = layered(layers).find("lldb", &context).unwrap();
        assert_eq!(found.spec.name, first, "{layers:?}");
    }

    // Differently named adapters sharing the `lldb` alias, where settings
    // or an extension change a spec of a lower layer: each counts in the
    // layer that won it, so the alias goes by layer, not by where the spec
    // started.
    let args = |arg: &str| DebugAdapterSettings {
        args: Some(strings(&[arg])),
        ..DebugAdapterSettings::default()
    };
    let relocated = DebugAdapterSettings {
        command: Some(user.display().to_string()),
        ..DebugAdapterSettings::default()
    };
    let own = DebugAdapterSettings {
        types: Some(strings(&["lldb"])),
        ..relocated.clone()
    };
    let collisions: [CollisionRow<'_>; 8] = [
        // A user's codelldb beats an extension's adapter.
        (
            &[],
            &[("codelldb", relocated.clone())],
            &["extension-lldb"],
            ("codelldb", Source::User),
        ),
        // But not the host's.
        (
            &["host-lldb"],
            &[("codelldb", relocated.clone())],
            &["extension-lldb"],
            ("host-lldb", Source::Host),
        ),
        // A user's change to an extension's adapter beats an unchanged
        // extension's.
        (
            &[],
            &[("extension-b", args("-v"))],
            &["extension-a", "extension-b"],
            ("extension-b", Source::User),
        ),
        // Within the user's layer, by name.
        (
            &[],
            &[("zz-lldb", own.clone()), ("extension-a", args("-v"))],
            &["extension-a"],
            ("extension-a", Source::User),
        ),
        (
            &[],
            &[("aa-lldb", own.clone()), ("extension-a", args("-v"))],
            &["extension-a"],
            ("aa-lldb", Source::User),
        ),
        // An extension's codelldb is the extensions' layer, in their
        // order.
        (
            &[],
            &[],
            &["codelldb", "extension-lldb"],
            ("codelldb", Source::Extension),
        ),
        (
            &[],
            &[],
            &["extension-lldb", "codelldb"],
            ("extension-lldb", Source::Extension),
        ),
        // And the user's settings lift it again.
        (
            &[],
            &[("codelldb", args("-v"))],
            &["extension-lldb", "codelldb"],
            ("codelldb", Source::User),
        ),
    ];
    for (host_names, user_settings, extension_names, (name, source)) in collisions {
        let row = format!("{host_names:?} {user_settings:?} {extension_names:?}");
        let mut registry = fixture.registry();
        registry.set_extension_specs(extension_names.iter().map(|name| lldb(name)).collect());
        registry.set_settings(&settings(user_settings));
        for name in host_names {
            registry.register(lldb(name));
        }
        let found = registry.find("lldb", &context).unwrap();
        assert_eq!(
            (found.spec.name.as_str(), found.source),
            (name, source),
            "{row}"
        );
        // `names` is that order, and `list` too.
        assert_eq!(registry.names()[0], name, "{row}");
        assert_eq!(registry.list(&context)[0].spec.name, name, "{row}");
    }
}

#[cfg(unix)]
#[test]
fn type_aliases_resolve() {
    let fixture = Fixture::new();
    let registry = fixture.registry();
    let context = fixture.context();
    for (ty, name) in [
        ("lldb-dap", "lldb-dap"),
        ("lldb", "codelldb"),
        ("debugpy", "debugpy"),
        ("python", "debugpy"),
        ("go", "delve"),
        // By name, too.
        ("codelldb", "codelldb"),
        ("delve", "delve"),
    ] {
        let adapter = registry.find(ty, &context);
        assert_eq!(adapter.map(|a| a.spec.name).as_deref(), Some(name), "{ty}");
    }
    // js-debug's spec waits for multi-session support (D23).
    for ty in ["pwa-node", "node", "js-debug", "java", ""] {
        assert_eq!(registry.find(ty, &context), None, "{ty}");
    }
    // The built-ins' data, as §3's table has it.
    let spec = |name: &str| registry.find(name, &context).unwrap().spec;
    let delve = spec("delve");
    assert_eq!(delve.command, Path::new("dlv"));
    assert_eq!(
        delve.args,
        strings(&["dap", "--listen", &format!("127.0.0.1:{PORT_PLACEHOLDER}")])
    );
    assert_eq!(delve.transport, AdapterTransport::Tcp);
    assert_eq!(delve.languages, ["go"]);
    let debugpy = spec("debugpy");
    assert_eq!(debugpy.args, ["-m", "debugpy.adapter"]);
    assert_eq!(debugpy.transport, AdapterTransport::Stdio);
    assert_eq!(debugpy.languages, ["python"]);
    for name in ["lldb-dap", "codelldb"] {
        assert_eq!(spec(name).languages, ["rust", "c", "cpp"], "{name}");
        assert_eq!(spec(name).transport, AdapterTransport::Stdio, "{name}");
    }

    // A name beats a `type`: a host adapter named "python" is the one
    // "python" names.
    let mut registry = fixture.registry();
    registry.register(AdapterSpec::new("python", "/bin/true"));
    assert_eq!(
        registry.find("python", &context).unwrap().source,
        Source::Host
    );
    assert_eq!(
        registry.find("debugpy", &context).unwrap().spec.name,
        "debugpy"
    );
}

#[cfg(unix)]
#[test]
fn a_missing_program_is_listed_not_found_with_a_hint() {
    let fixture = Fixture::new();
    let not_a_program = fixture.bin.join("not-a-program");
    std::fs::write(&not_a_program, "").unwrap();
    let mut registry = fixture.registry();
    let command = |command: &str| DebugAdapterSettings {
        command: Some(command.to_owned()),
        ..DebugAdapterSettings::default()
    };
    registry.set_settings(&settings(&[
        ("mine", command("fe-dap-no-such-adapter")),
        ("absolute", command("/nonexistent/adapter")),
        ("unrunnable", command(&not_a_program.display().to_string())),
    ]));
    let listed: Vec<(String, PathBuf, Status)> = registry
        .list(&fixture.context())
        .into_iter()
        .map(|adapter| (adapter.spec.name, adapter.spec.command, adapter.status))
        .collect();
    fn not_found(hint: impl ToString) -> Status {
        Status::NotFound {
            hint: hint.to_string(),
        }
    }
    let generic = |name: &str, command: &str| {
        not_found(tr!(
            "dap-hint-not-found",
            name = name,
            command = command.to_owned()
        ))
    };
    let expected: Vec<(String, PathBuf, Status)> = [
        (
            "absolute",
            "/nonexistent/adapter",
            generic("absolute", "/nonexistent/adapter"),
        ),
        (
            "mine",
            "fe-dap-no-such-adapter",
            generic("mine", "fe-dap-no-such-adapter"),
        ),
        (
            "unrunnable",
            &not_a_program.display().to_string(),
            generic("unrunnable", &not_a_program.display().to_string()),
        ),
        ("lldb-dap", "lldb-dap", not_found(tr!("dap-hint-lldb-dap"))),
        ("codelldb", "codelldb", not_found(tr!("dap-hint-codelldb"))),
        ("debugpy", "python3", not_found(tr!("dap-hint-debugpy"))),
        ("delve", "dlv", not_found(tr!("dap-hint-delve"))),
    ]
    .into_iter()
    .map(|(name, command, status)| (name.to_owned(), PathBuf::from(command), status))
    .collect();
    assert_eq!(listed, expected);
    for (_, _, status) in &listed {
        let Status::NotFound { hint } = status else {
            unreachable!()
        };
        assert!(!hint.is_empty());
    }

    // A built-in whose command the user set gets the generic hint.
    registry.set_settings(&settings(&[("codelldb", command("/opt/codelldb"))]));
    let codelldb = registry.find("codelldb", &fixture.context()).unwrap();
    assert_eq!(codelldb.status, generic("codelldb", "/opt/codelldb"));

    // Once there, it's found.
    let found = fixture.on_path("fe-dap-no-such-adapter");
    registry.set_settings(&settings(&[("mine", command("fe-dap-no-such-adapter"))]));
    let mine = registry.find("mine", &fixture.context()).unwrap();
    assert_eq!((mine.spec.command, mine.status), (found, Status::Found));
}

#[cfg(unix)]
#[test]
fn the_rust_default_is_lldb_dap_when_found_else_codelldb() {
    // (lldb-dap on the PATH, codelldb on the PATH) → the default, found.
    for ((lldb_dap, codelldb), name, found) in [
        ((true, true), "lldb-dap", true),
        ((true, false), "lldb-dap", true),
        ((false, true), "codelldb", true),
        ((false, false), "lldb-dap", false),
    ] {
        let fixture = Fixture::new();
        if lldb_dap {
            fixture.on_path("lldb-dap");
        }
        if codelldb {
            fixture.on_path("codelldb");
        }
        let registry = fixture.registry();
        for language in ["rust", "c", "cpp"] {
            let row = format!("lldb-dap {lldb_dap}, codelldb {codelldb}, {language}");
            let adapter = registry.default_for(language, &fixture.context()).unwrap();
            assert_eq!(adapter.spec.name, name, "{row}");
            assert_eq!(adapter.status == Status::Found, found, "{row}");
            if found {
                assert_eq!(adapter.spec.command, fixture.bin.join(name), "{row}");
            }
        }
    }
    // A user's settings for codelldb put it in their layer, ahead of the
    // built-in lldb-dap, even when that is found.
    let fixture = Fixture::new();
    fixture.on_path("lldb-dap");
    let codelldb = program(&fixture.dir.join("codelldb/adapter/codelldb"));
    let mut registry = fixture.registry();
    registry.set_settings(&settings(&[(
        "codelldb",
        DebugAdapterSettings {
            command: Some(codelldb.display().to_string()),
            ..DebugAdapterSettings::default()
        },
    )]));
    let adapter = registry.default_for("rust", &fixture.context()).unwrap();
    assert_eq!(
        (
            adapter.spec.name.as_str(),
            adapter.source,
            adapter.spec.command
        ),
        ("codelldb", Source::User, codelldb)
    );

    // The other languages' defaults.
    let fixture = Fixture::new();
    let registry = fixture.registry();
    for (language, name) in [("python", "debugpy"), ("go", "delve")] {
        let adapter = registry.default_for(language, &fixture.context()).unwrap();
        assert_eq!(adapter.spec.name, name, "{language}");
    }
    assert_eq!(registry.default_for("cobol", &fixture.context()), None);
}

#[cfg(unix)]
#[test]
fn bare_commands_resolve_on_the_login_path() {
    let fixture = Fixture::new();
    let lldb_dap = fixture.on_path("lldb-dap");
    let mine = fixture.on_path("fe-dap-test-adapter");
    // In the root, which a `.` on the PATH would reach: never searched.
    program(&fixture.root.join("codelldb"));
    // `sh` is on this process's PATH, but not on the login shell's.
    let process_path = std::env::var_os("PATH").unwrap_or_default();
    assert!(
        std::env::split_paths(&process_path).any(|dir| dir.join("sh").is_file()),
        "{process_path:?}"
    );
    let login = fixture.login_with(&format!(".::/nonexistent:{}", fixture.bin.display()));
    let mut registry = Registry::new(login).with_os(HostOs::Linux);
    let command = |command: &str| DebugAdapterSettings {
        command: Some(command.to_owned()),
        ..DebugAdapterSettings::default()
    };
    registry.set_settings(&settings(&[
        ("mine", command("fe-dap-test-adapter")),
        ("shell", command("sh")),
    ]));
    let context = fixture.context();
    let found = |registry: &Registry, name: &str| {
        let adapter = registry.find(name, &context).unwrap();
        (adapter.status == Status::Found).then_some(adapter.spec.command)
    };
    assert_eq!(found(&registry, "lldb-dap"), Some(lldb_dap));
    assert_eq!(found(&registry, "mine"), Some(mine.clone()));
    assert_eq!(found(&registry, "shell"), None);
    assert_eq!(found(&registry, "codelldb"), None);

    // A `path` of the user's is looked in first, then the login shell's.
    let own = program(&fixture.dir.join("llvm/bin/lldb-dap"));
    registry.set_settings(&settings(&[
        (
            "lldb-dap",
            DebugAdapterSettings {
                path: Some(vec![fixture.dir.join("llvm/bin").display().to_string()]),
                ..DebugAdapterSettings::default()
            },
        ),
        (
            "mine",
            DebugAdapterSettings {
                path: Some(vec![fixture.dir.join("llvm/bin").display().to_string()]),
                ..command("fe-dap-test-adapter")
            },
        ),
    ]));
    assert_eq!(found(&registry, "lldb-dap"), Some(own));
    assert_eq!(found(&registry, "mine"), Some(mine));

    // An adapter's own PATH, in its `env`, is searched instead.
    let elsewhere = program(&fixture.dir.join("elsewhere/fe-dap-test-adapter"));
    registry.set_settings(&settings(&[(
        "mine",
        DebugAdapterSettings {
            env: Some(BTreeMap::from([(
                "PATH".into(),
                fixture.dir.join("elsewhere").display().to_string(),
            )])),
            ..command("fe-dap-test-adapter")
        },
    )]));
    assert_eq!(found(&registry, "mine"), Some(elsewhere));
}

#[cfg(unix)]
#[test]
fn python_interpreter_order() {
    let fixture = Fixture::new();
    let venv = fixture.root.join(".venv/bin/python");
    let env = fixture.dir.join("env");
    let env_python = env.join("bin/python");
    // (.venv's python, $VIRTUAL_ENV's python, python3 on the PATH) → the
    // interpreter, or not found.
    let table: [PythonRow<'_>; 6] = [
        ((true, true, true), Some(&venv)),
        ((true, false, false), Some(&venv)),
        ((false, true, true), Some(&env_python)),
        ((false, true, false), Some(&env_python)),
        ((false, false, true), Some(&fixture.bin.join("python3"))),
        ((false, false, false), None),
    ];
    for ((has_venv, has_env, has_python3), expected) in table {
        let row = format!(".venv {has_venv}, VIRTUAL_ENV {has_env}, python3 {has_python3}");
        for (exists, path) in [
            (has_venv, venv.clone()),
            (has_env, env_python.clone()),
            (has_python3, fixture.bin.join("python3")),
        ] {
            if exists {
                program(&path);
            } else {
                let _ = std::fs::remove_file(&path);
            }
        }
        let context = Context {
            virtual_env: Some(env.clone()),
            ..fixture.context()
        };
        let debugpy = fixture.registry().find("python", &context).unwrap();
        match expected {
            Some(python) => {
                assert_eq!(debugpy.status, Status::Found, "{row}");
                assert_eq!(debugpy.spec.command, python, "{row}");
            }
            None => {
                assert!(matches!(debugpy.status, Status::NotFound { .. }), "{row}");
                assert_eq!(debugpy.spec.command, Path::new("python3"), "{row}");
            }
        }
        assert_eq!(debugpy.spec.args, ["-m", "debugpy.adapter"], "{row}");
    }

    // The rule on its own: a .venv python that can't be run is passed
    // over, and no $VIRTUAL_ENV goes straight to python3.
    std::fs::write(&venv, "").unwrap();
    program(&env_python);
    assert_eq!(
        python_interpreter(&fixture.root, Some(&env), crate::launcher::is_runnable),
        env_python
    );
    assert_eq!(
        python_interpreter(&fixture.root, None, crate::launcher::is_runnable),
        Path::new("python3")
    );
    // The user's command for debugpy is used as it is.
    let mut registry = fixture.registry();
    registry.set_settings(&settings(&[(
        "debugpy",
        DebugAdapterSettings {
            command: Some(env_python.display().to_string()),
            ..DebugAdapterSettings::default()
        },
    )]));
    let context = fixture.context();
    let debugpy = registry.find("debugpy", &context).unwrap();
    assert_eq!(debugpy.spec.command, env_python);
}

#[cfg(unix)]
#[test]
fn rust_lldb_init_commands_come_from_rustc_print_sysroot() {
    let fixture = Fixture::new();
    fixture.on_path("lldb-dap");
    fixture.on_path("codelldb");
    let rustc = fixture.on_path("rustc");
    let sysroot = "/opt/rust \"toolchain\"";
    let runner = FakeRunner::answering(&[("rustc", &format!("{sysroot}\n"))]);
    let registry = fixture.registry().with_runner(runner.clone());
    let rust = fixture.context().with_language("rust");

    let lldb_dap = registry.find("lldb-dap", &rust).unwrap();
    let etc = r#"/opt/rust \"toolchain\"/lib/rustlib/etc"#;
    assert_eq!(
        lldb_dap.spec.init_commands,
        [
            format!(r#"command script import "{etc}/lldb_lookup.py""#),
            format!(r#"command source "{etc}/lldb_commands""#),
        ]
    );
    assert_eq!(
        runner.calls(),
        [(
            rustc,
            strings(&["--print", "sysroot"]),
            fixture.root.clone()
        )]
    );
    assert_eq!(rust_init_commands(sysroot), lldb_dap.spec.init_commands);

    // Only for Rust, under lldb-dap: codelldb has its own support.
    let c = fixture.context().with_language("c");
    assert!(
        registry
            .find("lldb-dap", &c)
            .unwrap()
            .spec
            .init_commands
            .is_empty()
    );
    assert!(
        registry
            .find("lldb-dap", &fixture.context())
            .unwrap()
            .spec
            .init_commands
            .is_empty()
    );
    assert!(
        registry
            .find("codelldb", &rust)
            .unwrap()
            .spec
            .init_commands
            .is_empty()
    );
    assert_eq!(runner.calls().len(), 1);
    // The host's and an extension's own commands are theirs alone.
    let mut hosted = fixture.registry().with_runner(runner.clone());
    hosted.register(AdapterSpec {
        init_commands: strings(&["settings set x 1"]),
        ..AdapterSpec::new("lldb-dap", fixture.bin.join("lldb-dap"))
    });
    assert_eq!(
        hosted.find("lldb-dap", &rust).unwrap().spec.init_commands,
        ["settings set x 1"]
    );

    // No answer, or one that isn't an absolute path on one line: none.
    for answer in ["", "relative/sysroot\n", "/a\n/b\n"] {
        let registry = fixture
            .registry()
            .with_runner(FakeRunner::answering(&[("rustc", answer)]));
        let adapter = registry.find("lldb-dap", &rust).unwrap();
        assert_eq!(adapter.status, Status::Found, "{answer:?}");
        assert!(adapter.spec.init_commands.is_empty(), "{answer:?}");
    }
    let silent = fixture.registry().with_runner(FakeRunner::answering(&[]));
    assert!(
        silent
            .find("lldb-dap", &rust)
            .unwrap()
            .spec
            .init_commands
            .is_empty()
    );
    // No rustc on the PATH: it isn't run.
    std::fs::remove_file(fixture.bin.join("rustc")).unwrap();
    let runner = FakeRunner::answering(&[("rustc", "/opt/rust\n")]);
    let registry = fixture.registry().with_runner(runner.clone());
    assert!(
        registry
            .find("lldb-dap", &rust)
            .unwrap()
            .spec
            .init_commands
            .is_empty()
    );
    assert_eq!(runner.calls(), []);
}

#[cfg(unix)]
#[test]
fn the_macos_xcrun_lookup_is_data() {
    // The `cfg(target_os = "macos")` choice, and each OS's lookups as data.
    assert_eq!(
        HostOs::current() == HostOs::MacOs,
        cfg!(target_os = "macos")
    );
    assert_eq!(
        lldb_dap_lookups(HostOs::MacOs),
        [
            Lookup::OnPath,
            Lookup::Run {
                program: XCRUN,
                args: &["-f", "lldb-dap"],
            },
        ]
    );
    assert_eq!(XCRUN, "/usr/bin/xcrun");
    for os in [HostOs::Linux, HostOs::Windows] {
        assert_eq!(lldb_dap_lookups(os), [Lookup::OnPath], "{os:?}");
    }

    // (OS, lldb-dap on the PATH) → where it's found, run on every OS with
    // a stand-in xcrun.
    for os in [HostOs::MacOs, HostOs::Linux, HostOs::Windows] {
        for on_path in [true, false] {
            let row = format!("{os:?}, on the PATH {on_path}");
            let fixture = Fixture::new();
            let xcode = program(&fixture.dir.join("Xcode/usr/bin/lldb-dap"));
            let path = on_path.then(|| fixture.on_path("lldb-dap"));
            let runner = FakeRunner::answering(&[("xcrun", &format!("{}\n", xcode.display()))]);
            let registry = fixture.registry().with_os(os).with_runner(runner.clone());
            let adapter = registry.find("lldb-dap", &fixture.context()).unwrap();
            let expected = path.or_else(|| (os == HostOs::MacOs).then_some(xcode));
            match &expected {
                Some(program) => {
                    assert_eq!(adapter.status, Status::Found, "{row}");
                    assert_eq!(&adapter.spec.command, program, "{row}");
                }
                None => assert!(matches!(adapter.status, Status::NotFound { .. }), "{row}"),
            }
            let asked = runner.calls();
            if os == HostOs::MacOs && !on_path {
                assert_eq!(
                    asked,
                    [(
                        PathBuf::from(XCRUN),
                        strings(&["-f", "lldb-dap"]),
                        fixture.root.clone()
                    )],
                    "{row}"
                );
            } else {
                assert_eq!(asked, [], "{row}");
            }
        }
    }

    // What xcrun prints must be a program that is there.
    let fixture = Fixture::new();
    for answer in ["", "lldb-dap\n", "/nonexistent/lldb-dap\n"] {
        let registry = fixture
            .registry()
            .with_os(HostOs::MacOs)
            .with_runner(FakeRunner::answering(&[("xcrun", answer)]));
        let adapter = registry.find("lldb-dap", &fixture.context()).unwrap();
        assert!(
            matches!(adapter.status, Status::NotFound { .. }),
            "{answer:?}"
        );
    }
}

/// A user's adapter of their own over TCP gets that transport.
#[cfg(unix)]
#[test]
fn a_users_own_adapter_takes_its_transport() {
    let fixture = Fixture::new();
    let program = fixture.on_path("my-adapter");
    let mut registry = fixture.registry();
    registry.set_settings(&settings(&[(
        "mine",
        DebugAdapterSettings {
            command: Some("my-adapter".into()),
            args: Some(strings(&["--listen", PORT_PLACEHOLDER])),
            transport: Some(DebugTransport::Tcp),
            languages: Some(strings(&["zig"])),
            types: Some(strings(&["zig"])),
            ..DebugAdapterSettings::default()
        },
    )]));
    let adapter = registry.default_for("zig", &fixture.context()).unwrap();
    assert_eq!(adapter.source, Source::User);
    assert_eq!(adapter.spec.command, program);
    assert_eq!(adapter.spec.transport, AdapterTransport::Tcp);
    assert_eq!(adapter.spec.args, ["--listen", PORT_PLACEHOLDER]);
    assert_eq!(registry.find("zig", &fixture.context()), Some(adapter));
}

/// A Port adapter's connection that the host's end closes resolves
/// `exited` without a kill; one fe-dap closes resolves it too.
#[test]
fn a_port_connection_closed_by_either_end_has_exited() {
    use futures::AsyncReadExt as _;
    for remote in [true, false] {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = std::thread::spawn(move || listener.accept().unwrap().0);
        let spec = AdapterSpec {
            name: "hosted".into(),
            transport: AdapterTransport::Port(port),
            ..AdapterSpec::default()
        };
        let io = LocalLauncher::new()
            .launch(&spec, &std::env::temp_dir(), &LaunchCancel::new())
            .unwrap();
        let host_end = accepted.join().unwrap();
        let AdapterIo {
            mut output,
            mut process,
            ..
        } = io;
        async_io::block_on(async {
            if remote {
                drop(host_end);
                let mut rest = Vec::new();
                assert_eq!(output.read_to_end(&mut rest).await.unwrap(), 0);
            } else {
                process.kill();
                let mut rest = Vec::new();
                let _ = output.read_to_end(&mut rest).await;
                drop(host_end);
            }
            let exited = process.exited();
            let deadline = async_io::Timer::after(std::time::Duration::from_secs(10));
            match futures::future::select(exited, deadline).await {
                futures::future::Either::Left((exit, _)) => {
                    assert_eq!(exit, crate::launcher::ExitInfo::default());
                }
                futures::future::Either::Right(_) => panic!("never exited (remote {remote})"),
            }
        });
    }
}

/// [`SystemRunner`]'s one deadline covers its stdout as well as its exit:
/// a program whose descendant keeps its stdout open has run out of time,
/// and that descendant is killed with its group.
#[cfg(unix)]
#[test]
fn the_system_runner_stops_a_descendant_holding_its_stdout() {
    use crate::registry::SystemRunner;
    use std::time::{Duration, Instant};

    let fixture = Fixture::new();
    // Spawning a script can take a while on a loaded machine: the cases
    // that must finish get a generous deadline, only the cut-off ones a short one.
    let patient = SystemRunner::with_timeout(Duration::from_secs(10));
    let runner = SystemRunner::with_timeout(Duration::from_millis(500));
    let printing = fixture.dir.join("printing");
    write_script(&printing, "echo /opt/rust");
    // A sibling test forking while the script was still open for writing makes
    // Linux refuse the exec once (ETXTBSY), so allow a retry.
    let printed = (0..3).find_map(|_| patient.stdout(&printing, &[], &fixture.root));
    assert_eq!(printed.as_deref(), Some("/opt/rust\n"));
    let failing = fixture.dir.join("failing");
    write_script(&failing, "echo /opt/rust; exit 3");
    assert_eq!(patient.stdout(&failing, &[], &fixture.root), None);

    // It prints and exits at once, leaving a `sleep` with its stdout.
    let lingering = fixture.dir.join("lingering");
    let pid_file = fixture.dir.join("descendant.pid");
    write_script(
        &lingering,
        &format!(
            "echo /opt/rust\nsleep 30 &\necho $! > '{}'\nexit 0",
            pid_file.display()
        ),
    );
    let started = Instant::now();
    assert_eq!(runner.stdout(&lingering, &[], &fixture.root), None);
    let took = started.elapsed();
    assert!(took < Duration::from_secs(5), "{took:?}");
    let pid: u32 = std::fs::read_to_string(&pid_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    wait_gone(pid);

    // One that never ends is stopped at the deadline too.
    let hanging = fixture.dir.join("hanging");
    write_script(&hanging, "exec sleep 30");
    let started = Instant::now();
    assert_eq!(runner.stdout(&hanging, &[], &fixture.root), None);
    assert!(started.elapsed() < Duration::from_secs(5));
}

/// Waits up to 10 s for process `pid` to be gone: no such process, or (on
/// Linux) a zombie waiting for a parent that isn't us.
#[cfg(unix)]
fn wait_gone(pid: u32) {
    use std::time::{Duration, Instant};

    let gone = || {
        let Some(pid) = rustix::process::Pid::from_raw(pid as i32) else {
            return true;
        };
        if rustix::process::test_kill_process(pid).is_err() {
            return true;
        }
        std::fs::read_to_string(format!("/proc/{}/stat", pid.as_raw_nonzero()))
            .ok()
            .and_then(|stat| {
                stat.rsplit_once(')')
                    .map(|(_, rest)| rest.trim_start().starts_with('Z'))
            })
            .unwrap_or(false)
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while !gone() {
        assert!(Instant::now() < deadline, "process {pid} is still running");
        std::thread::sleep(Duration::from_millis(10));
    }
}
