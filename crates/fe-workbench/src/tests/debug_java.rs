//! Java debugging through jdtls (`debug_java.rs`, debugger card D21b):
//! java-debug's jar found in order, jdtls's main classes as configurations,
//! a start that asks jdtls for the classpath and java-debug's port and
//! connects there, launch.json's `java` type, a Gradle task before the
//! launch, and the explanation when there's no jdtls. jdtls is fe-lsp's
//! fake server; java-debug is fe-dap's fake adapter, listening on a real
//! port on 127.0.0.1 that the session connects to through fe-dap's
//! `Port` launch kind.

use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    io::{Read as _, Write as _},
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    thread,
};

use fe_config::debug_settings::{DebugAdapterSettings, DebugSettings};
use fe_dap::{
    AdapterTransport, Message,
    fake::{self, FakeAdapter},
    registry::{JAVA, JAVA_PORT},
};
use fe_lsp::{
    LoginPath,
    fake::{InitializeReply, Record, Reply, Script, Scripted, initialize_result},
};
use futures::{AsyncReadExt as _, AsyncWriteExt as _};
use gpui::TestAppContext;
use kiln_l10n::tr;
use lsp_types::{ExecuteCommandOptions, PositionEncodingKind, ServerCapabilities};
use serde_json::{Map, Value, json};

use crate::{
    TerminalPanel,
    debug_java::{
        Jar, JavaDebugHome, JavaLaunch, Place, RESOLVE_CLASSPATH, RESOLVE_MAIN_CLASS,
        START_DEBUG_SESSION, find_jar, set_java_debug_home, well_known,
    },
    debug_session::Debugger,
    debug_session::launch::{Resolve, session_options_with},
    launch_defs::{
        self, AdapterRef, Cwd, LAUNCH_FILE, LAUNCH_JSON, LaunchConfig, Source, variables::Values,
    },
    task_commands::TestFiles,
    task_defs::ProjectFiles,
    tests::lsp::Lsp,
    workbench::toast::Level,
};
use fe_terminal_view::TerminalViewEvent;
use gpui::Entity;

const EXECUTE: &str = "workspace/executeCommand";

/// The project's files, read from disk.
struct Disk;

impl ProjectFiles for Disk {
    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn list_dir(&self, dir: &Path) -> Vec<PathBuf> {
        (fs::read_dir(dir).into_iter().flatten().flatten())
            .map(|entry| entry.path())
            .collect()
    }
}

fn touch(path: &Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "").unwrap();
}

fn java_path(paths: &[&str]) -> DebugSettings {
    let mut settings = DebugSettings::default();
    settings.adapters.insert(
        JAVA.to_owned(),
        DebugAdapterSettings {
            path: Some(paths.iter().map(|path| (*path).to_owned()).collect()),
            ..DebugAdapterSettings::default()
        },
    );
    settings
}

const JAR: &str = "com.microsoft.java.debug.plugin-";

#[test]
fn the_java_debug_jar_is_found_in_order() {
    let dir = tempfile::tempdir().unwrap();
    let home = dunce::canonicalize(dir.path()).unwrap();
    let data = home.join("data");
    let at = JavaDebugHome {
        home: Some(home.clone()),
        data_home: Some(data.clone()),
    };
    // The places, as the decision file lists them.
    let mason = data.join("nvim/mason/packages/java-debug-adapter/extension/server");
    assert_eq!(
        well_known(&at),
        [
            Place::Extensions(home.join(".vscode/extensions")),
            Place::Extensions(home.join(".vscode-insiders/extensions")),
            Place::Extensions(home.join(".vscode-oss/extensions")),
            Place::Folder(mason.clone()),
        ]
    );

    // Each install, newest version first: 0.10.0's extension over 0.9.1's,
    // and its newest jar.
    let extensions = home.join(".vscode/extensions");
    let older = extensions.join("vscjava.vscode-java-debug-0.9.1/server");
    let newer = extensions.join("vscjava.vscode-java-debug-0.10.0/server");
    touch(&older.join(format!("{JAR}0.52.0.jar")));
    touch(&newer.join(format!("{JAR}0.9.0.jar")));
    touch(&newer.join(format!("{JAR}0.10.1.jar")));
    // Not a Debugger for Java extension, nor its jar.
    touch(&extensions.join("vscjava.vscode-java-test-0.40.0/server/x.jar"));
    let vscode = newer.join(format!("{JAR}0.10.1.jar"));
    let oss = home.join(".vscode-oss/extensions/vscjava.vscode-java-debug-0.58.0/server");
    let oss_jar = oss.join(format!("{JAR}0.53.0.jar"));
    touch(&oss_jar);
    let mason_jar = mason.join(format!("{JAR}0.53.1.jar"));
    touch(&mason_jar);
    let custom = home.join("custom");
    let custom_jar = custom.join(format!("{JAR}0.1.0.jar"));
    touch(&custom_jar);
    let direct = home.join("java-debug.jar");
    touch(&direct);

    // [debug.adapters.java] path first, each entry in order: a folder
    // without a jar is passed over, `~/` is the home, a jar is itself.
    let found = |settings: &DebugSettings| find_jar(settings, &at);
    assert_eq!(
        found(&java_path(&["~/missing", "~/custom", "~/java-debug.jar"])),
        Jar::Found(custom_jar)
    );
    assert_eq!(
        found(&java_path(&[direct.to_str().unwrap()])),
        Jar::Found(direct.clone())
    );
    // Then the well-known installs, in order.
    assert_eq!(found(&java_path(&["~/missing"])), Jar::Found(vscode));
    assert_eq!(found(&DebugSettings::default()), found(&java_path(&[])));
    fs::remove_dir_all(home.join(".vscode")).unwrap();
    assert_eq!(found(&DebugSettings::default()), Jar::Found(oss_jar));
    fs::remove_dir_all(home.join(".vscode-oss")).unwrap();
    assert_eq!(found(&DebugSettings::default()), Jar::Found(mason_jar));
    fs::remove_dir_all(&data).unwrap();
    // Else not found, with the hint.
    assert_eq!(
        found(&DebugSettings::default()),
        Jar::NotFound {
            hint: tr!("workbench-debug-java-no-jar").into()
        }
    );
    assert_eq!(
        find_jar(&DebugSettings::default(), &JavaDebugHome::default()),
        found(&DebugSettings::default())
    );
}

/// jdtls's fake: java-debug's three commands listed, answering as
/// `answers` says.
fn jdtls(answers: &[(&str, Value)]) -> Script {
    let mut init = initialize_result(Some(PositionEncodingKind::UTF8));
    init.capabilities = ServerCapabilities {
        execute_command_provider: Some(ExecuteCommandOptions {
            commands: [RESOLVE_MAIN_CLASS, RESOLVE_CLASSPATH, START_DEBUG_SESSION]
                .map(str::to_owned)
                .into(),
            ..Default::default()
        }),
        ..init.capabilities
    };
    Script {
        initialize: InitializeReply::Result(Box::new(init)),
        commands: (answers.iter())
            .map(|(command, answer)| {
                let reply = Scripted::now(Reply::Result(answer.clone()));
                ((*command).to_owned(), reply)
            })
            .collect(),
        ..Script::default()
    }
}

/// The main classes jdtls lists.
fn mains() -> Value {
    json!([
        { "mainClass": "com.example.App", "projectName": "demo", "filePath": "/x/App.java" },
        { "mainClass": "com.example.Server", "projectName": "demo" },
        { "mainClass": "com.example.Server", "projectName": "tools" },
    ])
}

/// A Maven project's App.java.
const APP: &str = "src/main/java/com/example/App.java";

/// A workbench on a Java project (`files`, then `App.java`), trusted, its
/// fake jdtls following `script`; the well-known installs looked for in
/// an empty home, and java-debug's jar `jar.jar` in the project's folder
/// given by `[debug.adapters.java] path`, found before jdtls starts. With
/// `open`, App.java is open and jdtls running.
struct Java {
    s: Lsp,
    jar: PathBuf,
    _home: tempfile::TempDir,
}

fn java(cx: &mut TestAppContext, script: Script, files: &[(&str, &str)], open: bool) -> Java {
    let mut s = Lsp::with_scripts(cx, script, Script::default());
    let home = tempfile::tempdir().unwrap();
    let at = JavaDebugHome {
        home: Some(home.path().to_owned()),
        data_home: Some(home.path().join("data")),
    };
    s.cx.update(|_, cx| {
        set_java_debug_home(cx, at);
        cx.set_global(TestFiles(Arc::new(Disk)));
    });
    for (name, text) in files {
        let path = s.path(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let app = s.path(APP);
    fs::create_dir_all(app.parent().unwrap()).unwrap();
    fs::write(
        &app,
        "class App { public static void main(String[] a) {} }\n",
    )
    .unwrap();
    let jar = home.path().join("jar.jar");
    fs::write(&jar, "").unwrap();
    let settings = java_path(&[jar.to_str().unwrap()]);
    let workbench = s.workbench.clone();
    workbench.update(&mut s.cx, |workbench, cx| {
        workbench.reload_debug_settings(&settings, cx)
    });
    let wanted = Some(Jar::Found(jar.clone()));
    s.until("java-debug's jar", |s| jar_of(s) == wanted);
    s.set_trust(true);
    let mut java = Java {
        s,
        jar,
        _home: home,
    };
    if open {
        java.open_app();
    }
    java
}

impl Java {
    /// Opens App.java, which starts jdtls.
    fn open_app(&mut self) {
        self.s.open(APP);
        let uri = self.s.uri(APP);
        (self.s.fake).wait_for_document(&uri, "didOpen", |r| r.is_some_and(|r| r.open));
    }

    fn workbench(&self) -> Entity<crate::Workbench> {
        self.s.workbench.clone()
    }

    /// The root's configurations, as the picker reads them.
    fn configs(&mut self) -> Vec<LaunchConfig> {
        let slot = Rc::new(RefCell::new(None));
        let workbench = self.workbench();
        let put = slot.clone();
        self.s.cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                let reading = workbench.read_launch_configs(cx);
                cx.spawn_in(window, async move |_, _| {
                    *put.borrow_mut() = Some(reading.await.configs);
                })
                .detach();
            });
        });
        self.s
            .until("the configurations", |_| slot.borrow().is_some());
        slot.take().unwrap()
    }

    /// Starts the configuration `name`, as `debug.rerun_last` does.
    fn start(&mut self, name: &str) {
        let workbench = self.workbench();
        let name = name.to_owned();
        self.s.cx.update(|window, cx| {
            workbench.update(cx, |workbench, cx| {
                workbench.rerun_debug_named(name, window, cx)
            });
        });
        self.s.draw();
    }

    /// The commands jdtls was asked to run, in order, with their arguments.
    fn commands(&self) -> Vec<(String, Value)> {
        (self.s.fake.records().into_iter())
            .filter_map(|record| match record {
                Record::Request { method, params, .. } if method == EXECUTE => Some((
                    params["command"].as_str().unwrap_or_default().to_owned(),
                    params.get("arguments").cloned().unwrap_or(Value::Null),
                )),
                _ => None,
            })
            .collect()
    }

    fn asked(&self, command: &str) -> bool {
        self.commands().iter().any(|(asked, _)| asked == command)
    }

    /// Each toast showing: level, message and detail.
    fn toasts(&mut self) -> Vec<(Level, String, Option<String>)> {
        let layer = (self.workbench()).read_with(&self.s.cx, |w, _| w.toasts().clone());
        layer.read_with(&self.s.cx, |layer, _| {
            (layer.queue().shown().iter())
                .map(|entry| {
                    let toast = &entry.toast;
                    (toast.level, toast.message.clone(), toast.detail.clone())
                })
                .collect()
        })
    }

    fn debugger_idle(&mut self) -> bool {
        let debugger = debugger(&mut self.s);
        debugger.read_with(&self.s.cx, |debugger, _| {
            debugger.state().is_none() && debugger.starting().is_none()
        })
    }

    /// Whether a toast shows `wanted`'s level, message and detail.
    fn shows(&mut self, wanted: &(Level, String, Option<String>)) -> bool {
        self.toasts().contains(wanted)
    }

    fn terminal(&self) -> Entity<TerminalPanel> {
        (self.workbench()).read_with(&self.s.cx, |w, _| w.terminal().clone())
    }
}

fn debugger(s: &mut Lsp) -> Entity<Debugger> {
    let workbench = s.workbench.clone();
    s.cx.update(|_, cx| workbench.update(cx, |workbench, cx| workbench.debugger(cx)))
        .expect("the window's debugger")
}

/// What the debugger last found of java-debug's jar.
fn jar_of(s: &mut Lsp) -> Option<Jar> {
    let debugger = debugger(s);
    debugger.read_with(&s.cx, |debugger, _| debugger.java.jar.clone())
}

/// java-debug: fe-dap's fake adapter, serving one connection on a port of
/// its own on 127.0.0.1.
struct JavaDebug {
    port: u16,
    fake: Arc<std::sync::Mutex<Option<FakeAdapter>>>,
}

impl JavaDebug {
    fn listen() -> Self {
        Self::listen_with(fe_dap::fake::Script::default())
    }

    fn listen_with(script: fe_dap::fake::Script) -> Self {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let fake = Arc::new(std::sync::Mutex::new(None));
        let served = fake.clone();
        thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            // The connection's two ways, each a thread copying between the
            // socket and the fake's pipes.
            let (fake_reads, mut to_fake) = fake::pipe();
            let (mut from_fake, fake_writes) = fake::pipe();
            let mut reading = stream.try_clone().unwrap();
            thread::spawn(move || {
                let mut buffer = [0_u8; 8192];
                while let Ok(n) = reading.read(&mut buffer) {
                    let sent = futures::executor::block_on(to_fake.write_all(&buffer[..n]));
                    if n == 0 || sent.is_err() {
                        break;
                    }
                }
            });
            let mut writing = stream;
            thread::spawn(move || {
                let mut buffer = [0_u8; 8192];
                while let Ok(n) = futures::executor::block_on(from_fake.read(&mut buffer)) {
                    if n == 0 || writing.write_all(&buffer[..n]).is_err() {
                        break;
                    }
                }
            });
            let adapter = fake::serve(script, fake_reads, fake_writes);
            *served.lock().unwrap() = Some(adapter);
        });
        Self { port, fake }
    }

    fn connected(&self) -> Option<FakeAdapter> {
        self.fake.lock().unwrap().clone()
    }
}

/// The `launch` request's arguments `fake` received.
fn launched(fake: &FakeAdapter) -> Option<Map<String, Value>> {
    fake.received()
        .into_iter()
        .find_map(|message| match message {
            Message::Request(request) if request.command == "launch" => {
                request.arguments.and_then(|arguments| match arguments {
                    Value::Object(arguments) => Some(arguments),
                    _ => None,
                })
            }
            _ => None,
        })
}

#[gpui::test]
fn main_classes_from_jdtls_are_detected_configurations(cx: &mut TestAppContext) {
    let script = jdtls(&[(RESOLVE_MAIN_CLASS, mains())]);
    let mut j = java(cx, script, &[("pom.xml", "<project/>")], true);
    // jdtls launched with java-debug's jar among its bundles.
    let (spec, _) = (j.s.fake.launched().into_iter())
        .find(|(spec, _)| spec.name == "jdtls")
        .expect("jdtls launched");
    let options = spec.initialization_options.expect("options");
    assert_eq!(options["bundles"], json!([j.jar.to_str().unwrap()]));

    let configs = j.configs();
    assert_eq!(j.commands(), [(RESOLVE_MAIN_CLASS.to_owned(), json!([]))]);
    let java: Vec<&LaunchConfig> = (configs.iter())
        .filter(|config| config.adapter_args.contains_key("mainClass"))
        .collect();
    let names: Vec<&str> = java.iter().map(|config| config.name.as_str()).collect();
    let named = |class: &str| tr!("workbench-debug-launch-detected-java", class = class);
    let named_in = |class: &str, project: &str| {
        tr!(
            "workbench-debug-launch-detected-java-in",
            class = class,
            project = project
        )
    };
    assert_eq!(
        names,
        [
            named("App").to_string(),
            named_in("com.example.Server", "demo").to_string(),
            named_in("com.example.Server", "tools").to_string(),
        ]
    );
    let app = java[0];
    assert_eq!(app.source, Source::Detected);
    assert_eq!(
        app.adapter,
        AdapterRef::Named {
            written: JAVA.to_owned(),
            name: Some(JAVA.to_owned())
        }
    );
    assert_eq!(app.adapter_args["mainClass"], "com.example.App");
    assert_eq!(app.adapter_args["projectName"], "demo");
    assert_eq!(app.cwd, Cwd::At(j.s.root.clone()));
    assert!(app.can_start(), "{:?}", app.blockers);
    assert_eq!(java[2].adapter_args["projectName"], "tools");
}

#[gpui::test]
fn starting_one_connects_to_the_port_jdtls_gives(cx: &mut TestAppContext) {
    let java_debug = JavaDebug::listen();
    let script = jdtls(&[
        (RESOLVE_MAIN_CLASS, mains()),
        (
            RESOLVE_CLASSPATH,
            json!([["/m/mods.jar"], ["/c/classes", "/c/lib.jar"]]),
        ),
        (START_DEBUG_SESSION, json!(java_debug.port)),
    ]);
    let mut j = java(cx, script, &[("pom.xml", "<project/>")], true);
    let name = tr!("workbench-debug-launch-detected-java", class = "App").to_string();
    j.start(&name);
    j.s.until("java-debug's launch", |_| {
        java_debug
            .connected()
            .is_some_and(|fake| launched(&fake).is_some())
    });
    let fake = java_debug.connected().unwrap();
    let arguments = launched(&fake).unwrap();
    assert_eq!(arguments["mainClass"], "com.example.App");
    assert_eq!(arguments["projectName"], "demo");
    assert_eq!(arguments["classPaths"], json!(["/c/classes", "/c/lib.jar"]));
    assert_eq!(arguments["modulePaths"], json!(["/m/mods.jar"]));
    // java-debug reads `args` as one string.
    assert_eq!(arguments["args"], "");
    assert_eq!(arguments["name"], name.as_str());
    assert_eq!(
        arguments["cwd"],
        j.s.root.to_string_lossy().into_owned().as_str()
    );
    // Listing, then the start: the classpath for the class and project,
    // then java-debug's port.
    let commands = j.commands();
    let asked: Vec<&str> = commands
        .iter()
        .map(|(command, _)| command.as_str())
        .collect();
    assert_eq!(
        asked,
        [RESOLVE_MAIN_CLASS, RESOLVE_CLASSPATH, START_DEBUG_SESSION]
    );
    assert_eq!(commands[1].1, json!(["com.example.App", "demo"]));
    j.s.until("the session", |s| {
        let debugger = debugger(s);
        debugger.read_with(&s.cx, |debugger, _| debugger.state().is_some())
    });
    let stop = j.workbench();
    stop.update(&mut j.s.cx, |workbench, cx| workbench.debug_stop(cx));
    j.s.draw();
}

#[gpui::test]
fn a_launch_json_java_entry_imports(_cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let launch_json = r#"{
        // VS Code's, as vscode-java-debug writes them.
        "configurations": [
            {
                "type": "java",
                "name": "Launch App",
                "request": "launch",
                "mainClass": "com.example.App",
                "projectName": "demo",
                "args": "--port 8080 \"a b\"",
                "vmArgs": ["-Xmx1g", "-Dname=a b"],
            },
            {
                "type": "java",
                "name": "Listed args",
                "request": "launch",
                "mainClass": "com.example.App",
                "args": ["one", "two words"],
            },
        ],
    }"#;
    let launch_toml = r#"
[[launch]]
name = "From the file"
adapter = "java"

[launch.adapter_args]
mainClass = "com.example.Tool"
"#;
    touch(&root.join(LAUNCH_JSON));
    touch(&root.join(LAUNCH_FILE));
    fs::write(root.join(LAUNCH_JSON), launch_json).unwrap();
    fs::write(root.join(LAUNCH_FILE), launch_toml).unwrap();
    let registry = crate::debug_session::launch::registry(
        &[],
        &DebugSettings::default(),
        Some(Arc::new(LoginPath::new(PathBuf::new()))),
    );
    let project = launch_defs::Project::new(&Disk, &root, &registry);
    let (configs, diagnostics) = launch_defs::load(&project);
    assert_eq!(diagnostics, []);
    let java = AdapterRef::Named {
        written: JAVA.to_owned(),
        name: Some(JAVA.to_owned()),
    };
    let config = |name: &str| {
        (configs.iter())
            .find(|config| config.name == name)
            .unwrap_or_else(|| panic!("{name} in {configs:?}"))
            .clone()
    };
    let app = config("Launch App");
    assert_eq!(app.source, Source::LaunchJson);
    assert_eq!(app.adapter, java);
    assert!(app.can_start(), "{:?}", app.blockers);
    assert_eq!(app.adapter_args["mainClass"], "com.example.App");
    // A string `args` is passed as written; an array is a typed field.
    assert_eq!(app.adapter_args["args"], "--port 8080 \"a b\"");
    assert!(app.args.is_empty());
    let listed = config("Listed args");
    assert_eq!(listed.args, ["one", "two words"]);
    assert_eq!(config("From the file").adapter, java);

    // Started with jdtls's port and resolution, as java-debug reads it.
    let options = |config: LaunchConfig, java: Option<JavaLaunch>| {
        let resolve = Resolve {
            config,
            root: root.clone(),
            values: Values {
                workspace_folder: root.clone(),
                active: None,
                user_home: None,
                env: BTreeMap::new(),
            },
            program: None,
            specs: Vec::new(),
            login: Some(Arc::new(LoginPath::new(PathBuf::new()))),
            settings: DebugSettings::default(),
            breakpoints: BTreeMap::new(),
        };
        session_options_with(resolve, java).unwrap()
    };
    let mut resolved = Map::new();
    resolved.insert("mainClass".into(), json!("com.example.Resolved"));
    resolved.insert("classPaths".into(), json!(["/c"]));
    resolved.insert("modulePaths".into(), json!([]));
    let jdtls = JavaLaunch {
        port: 40123,
        arguments: resolved,
    };
    let started = options(app, Some(jdtls.clone()));
    assert_eq!(started.adapter.name, JAVA);
    assert_eq!(started.adapter.transport, AdapterTransport::Port(40123));
    let arguments = &started.arguments;
    // The configuration's own over jdtls's.
    assert_eq!(arguments["mainClass"], "com.example.App");
    assert_eq!(arguments["classPaths"], json!(["/c"]));
    assert_eq!(arguments["args"], "--port 8080 \"a b\"");
    assert_eq!(arguments["vmArgs"], "-Xmx1g \"-Dname=a b\"");
    let listed = options(listed, Some(jdtls));
    assert_eq!(listed.arguments["args"], "one \"two words\"");
    assert_eq!(listed.arguments["mainClass"], "com.example.App");
    // Without jdtls's answers, the port is still the placeholder.
    let unasked = options(config("From the file"), None);
    assert_eq!(unasked.adapter.transport, AdapterTransport::Port(JAVA_PORT));
}

#[gpui::test]
fn a_gradle_or_maven_task_runs_as_the_pre_launch_task(cx: &mut TestAppContext) {
    let java_debug = JavaDebug::listen();
    let script = jdtls(&[
        (RESOLVE_CLASSPATH, json!([[], ["/c/classes"]])),
        (START_DEBUG_SESSION, json!(java_debug.port)),
    ]);
    // Gradle's wrapper beside its build, so T1660 detects `./gradlew build`.
    let launch = r#"
[[launch]]
name = "App"
adapter = "java"
pre_launch_task = "./gradlew build"

[launch.adapter_args]
mainClass = "com.example.App"
projectName = "demo"
"#;
    let files = [
        ("build.gradle", ""),
        ("gradlew", "#!/bin/sh\n"),
        (LAUNCH_FILE, launch),
    ];
    let mut j = java(cx, script, &files, true);
    let terminal = j.terminal();
    terminal.update(&mut j.s.cx, |terminal, _| terminal.fake_sessions = true);
    let tasks = crate::task_defs::load(&Disk, &j.s.root).0;
    assert!(
        (tasks.iter()).any(|task| task.label == "./gradlew build"
            && task.source == crate::task_defs::Source::Gradle),
        "{tasks:?}"
    );
    j.start("App");
    j.s.until("the build's tab", |s| {
        let terminal = terminal.clone();
        s.cx.update(|_, cx| !terminal.read(cx).started_options.is_empty())
    });
    let started = terminal.read_with(&j.s.cx, |terminal, _| {
        (terminal.started_options.iter())
            .map(|options| options.shell.clone())
            .collect::<Vec<_>>()
    });
    let (program, args) = started[0].clone().expect("an argv");
    assert_eq!(
        (program.as_str(), args),
        ("./gradlew", vec!["build".to_owned()])
    );
    // Nothing asked of jdtls for the start until the build is done.
    assert!(!j.asked(RESOLVE_CLASSPATH));
    assert!(!j.asked(START_DEBUG_SESSION));
    let view = terminal
        .read_with(&j.s.cx, |terminal, _| terminal.view().cloned())
        .expect("the build's tab");
    view.update(&mut j.s.cx, |_, cx| {
        cx.emit(TerminalViewEvent::Exited(Some(0)))
    });
    j.s.until("java-debug's launch", |_| {
        java_debug
            .connected()
            .is_some_and(|fake| launched(&fake).is_some())
    });
    let arguments = launched(&java_debug.connected().unwrap()).unwrap();
    assert_eq!(arguments["mainClass"], "com.example.App");
    assert_eq!(arguments["classPaths"], json!(["/c/classes"]));
    let asked: Vec<String> = j
        .commands()
        .into_iter()
        .map(|(command, _)| command)
        .collect();
    // The listing's main classes, then the start's: no main class to look
    // for, as the configuration names it.
    assert_eq!(
        asked,
        [RESOLVE_MAIN_CLASS, RESOLVE_CLASSPATH, START_DEBUG_SESSION]
    );
    let stop = j.workbench();
    stop.update(&mut j.s.cx, |workbench, cx| workbench.debug_stop(cx));
    j.s.draw();
}

#[gpui::test]
fn without_jdtls_for_the_root_java_debugging_explains_why(cx: &mut TestAppContext) {
    let launch = r#"
[[launch]]
name = "App"
adapter = "java"

[launch.adapter_args]
mainClass = "com.example.App"
"#;
    let files = [("pom.xml", "<project/>"), (LAUNCH_FILE, launch)];
    // No Java file open, so no jdtls.
    let mut j = java(cx, jdtls(&[]), &files, false);
    j.start("App");
    let cant = tr!("workbench-debug-cant-start", name = "App").to_string();
    let why = tr!(
        "workbench-debug-java-no-jdtls",
        root = j.s.root.display().to_string()
    )
    .to_string();
    let wanted = (Level::Error, cant, Some(why));
    let mut shown = false;
    for _ in 0..100 {
        j.s.draw();
        shown = j.shows(&wanted);
        if shown {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(shown, "{:?}", j.toasts());
    assert!(j.debugger_idle());
    assert!(j.s.fake.launched().is_empty());
    assert!(j.commands().is_empty());
}

#[gpui::test]
fn jdtls_without_java_debug_says_where_to_get_it(cx: &mut TestAppContext) {
    let launch = r#"
[[launch]]
name = "App"
adapter = "java"

[launch.adapter_args]
mainClass = "com.example.App"
"#;
    let files = [("pom.xml", "<project/>"), (LAUNCH_FILE, launch)];
    // jdtls lists none of java-debug's commands; the jar isn't found.
    let mut bare = initialize_result(Some(PositionEncodingKind::UTF8));
    bare.capabilities.execute_command_provider = Some(ExecuteCommandOptions::default());
    let script = Script {
        initialize: InitializeReply::Result(Box::new(bare)),
        ..Script::default()
    };
    let mut j = java(cx, script, &files, false);
    let workbench = j.workbench();
    workbench.update(&mut j.s.cx, |workbench, cx| {
        workbench.reload_debug_settings(&DebugSettings::default(), cx)
    });
    let hint = tr!("workbench-debug-java-no-jar").to_string();
    let not_found = Some(Jar::NotFound { hint: hint.clone() });
    j.s.until("no jar", |s| jar_of(s) == not_found);
    j.open_app();
    j.start("App");
    let cant = tr!("workbench-debug-cant-start", name = "App").to_string();
    let wanted = (Level::Error, cant, Some(hint));
    let mut shown = false;
    for _ in 0..500 {
        j.s.draw();
        shown = j.shows(&wanted);
        if shown {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(shown, "{:?}", j.toasts());
    assert!(j.debugger_idle());
}

/// Where the active editor is: its file and its caret's row.
fn caret(s: &mut Lsp) -> Option<(PathBuf, u32)> {
    let editor = (s.workbench).read_with(&s.cx, |w, cx| w.active_editor(cx).cloned())?;
    editor.read_with(&s.cx, |editor, _| {
        let model = editor.model();
        let row = (model.text())
            .offset_to_point(model.primary_selection().head())
            .row;
        Some((model.path()?.to_owned(), row))
    })
}

fn has_tab(s: &mut Lsp, path: &Path) -> bool {
    let pane = s.workbench.read_with(&s.cx, |w, _| w.pane().clone());
    pane.read_with(&s.cx, |pane, _| pane.position(path).is_some())
}

/// The file the program stops in, never opened before the stop.
const HELD: &str = "src/main/java/com/example/Held.java";

/// T658 F19, through a Java session (any session's stops open alike): a
/// stop in a file a Save As holds opens it only once the Save As has let
/// go, at the stop's row, and only if the stop is still current then.
#[gpui::test]
fn a_stop_in_a_file_a_save_as_holds_waits_for_it(cx: &mut TestAppContext) {
    let launch = r#"
[[launch]]
name = "App"
adapter = "java"

[launch.adapter_args]
mainClass = "com.example.App"
classPaths = []
"#;
    let files = [
        ("pom.xml", "<project/>"),
        (LAUNCH_FILE, launch),
        (HELD, "class Held {}\n"),
    ];
    let mut j = java(cx, jdtls(&[]), &files, true);
    let held = j.s.path(HELD);
    let stopping = |j: &mut Java| {
        let mut script = fe_dap::fake::Script {
            stop_on_entry: true,
            ..fe_dap::fake::Script::default()
        };
        script.program.path = held.to_string_lossy().into_owned();
        let java_debug = JavaDebug::listen_with(script);
        let port = Scripted::now(Reply::Result(json!(java_debug.port)));
        j.s.fake.set_command_reply(START_DEBUG_SESSION, port);
        let identity = crate::open::file_identity(&held);
        let reservation =
            crate::open::reserve(fe_buffer::Buffer::new("").id(), held.clone(), identity);
        j.start("App");
        j.s.until("the stop", |s| {
            let debugger = debugger(s);
            debugger.read_with(&s.cx, |debugger, _| debugger.stop_location().is_some())
        });
        for _ in 0..5 {
            j.s.draw();
        }
        assert!(!has_tab(&mut j.s, &held), "opened while a Save As holds it");
        (java_debug, reservation)
    };

    let (_adapter, reservation) = stopping(&mut j);
    drop(reservation);
    j.s.until("the stop's file", |s| caret(s) == Some((held.clone(), 0)));
    let workbench = j.workbench();
    workbench.update(&mut j.s.cx, |workbench, cx| workbench.debug_stop(cx));
    j.s.until("the session's end", |s| {
        let debugger = debugger(s);
        debugger.read_with(&s.cx, |debugger, _| !debugger.is_active())
    });
    let pane = workbench.read_with(&j.s.cx, |w, _| w.pane().clone());
    j.s.cx.update(|window, cx| {
        pane.update(cx, |pane, cx| {
            let ix = pane.position(&held).expect("Held.java's tab");
            pane.activate(ix, window, cx);
            pane.close_active(window, cx);
        });
    });
    j.s.draw();
    assert!(!has_tab(&mut j.s, &held));

    // Continued before the Save As lets go: the stop is over, and nothing
    // opens.
    let (_adapter, reservation) = stopping(&mut j);
    j.s.cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.debug_start(window, cx))
    });
    j.s.until("the stop's end", |s| {
        let debugger = debugger(s);
        debugger.read_with(&s.cx, |debugger, _| debugger.stop_location().is_none())
    });
    drop(reservation);
    for _ in 0..5 {
        j.s.draw();
    }
    assert!(
        !has_tab(&mut j.s, &held),
        "an obsolete stop opened its file"
    );
    workbench.update(&mut j.s.cx, |workbench, cx| workbench.debug_stop(cx));
    j.s.draw();
}
