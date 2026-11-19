//! Launch configurations over a fake [`ProjectFiles`]: nothing here reads
//! the disk (card D4, docs/decisions/r1b9dw3g0g-debugger.md §4).

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use fe_dap::{HostOs, Registry};
use fe_lsp::LoginPath;
use kiln_l10n::tr;
use serde_json::{Map, Value, json};

use crate::launch_defs::variables::{self, ActiveFile, Values};
use crate::launch_defs::{
    AdapterRef, Console, Cwd, LAUNCH_FILE, LAUNCH_JSON, LaunchConfig, LaunchDiagnostic, PreLaunch,
    Program, Project, ProjectFiles, Request, Source, load,
};
use crate::task_defs;

const ROOT: &str = "/project";

/// A project's files, by their names under [`ROOT`].
#[derive(Default)]
struct FakeFiles {
    texts: BTreeMap<PathBuf, String>,
    unreadable: BTreeSet<PathBuf>,
}

impl FakeFiles {
    fn with(files: &[(&str, &str)]) -> Self {
        Self {
            texts: files
                .iter()
                .map(|(name, text)| (Path::new(ROOT).join(name), (*text).to_owned()))
                .collect(),
            unreadable: BTreeSet::new(),
        }
    }

    /// Its configurations with its own tasks, on `os`, with the active
    /// editor's language.
    fn load_as(
        &self,
        os: HostOs,
        language: Option<&str>,
    ) -> (Vec<LaunchConfig>, Vec<LaunchDiagnostic>) {
        let root = Path::new(ROOT);
        let (tasks, _) = task_defs::load(self, root);
        let adapters = adapters();
        let project = Project::new(self, root, &adapters)
            .with_tasks(&tasks)
            .with_active_language(language)
            .with_os(os);
        load(&project)
    }

    fn load(&self) -> (Vec<LaunchConfig>, Vec<LaunchDiagnostic>) {
        self.load_as(HostOs::Linux, None)
    }

    fn names(&self) -> Vec<String> {
        names(&self.load().0)
    }
}

impl ProjectFiles for FakeFiles {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        if self.unreadable.contains(path) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        self.texts
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn exists(&self, path: &Path) -> bool {
        self.texts.contains_key(path) || self.unreadable.contains(path)
    }

    fn list_dir(&self, dir: &Path) -> Vec<PathBuf> {
        let mut entries = BTreeSet::new();
        for path in self.texts.keys() {
            if let Ok(rest) = path.strip_prefix(dir)
                && let Some(first) = rest.components().next()
            {
                entries.insert(dir.join(first));
            }
        }
        entries.into_iter().collect()
    }
}

/// fe-dap's built-in adapters. Nothing is looked up: [`Registry::name_for`]
/// resolves nothing, so the login shell never runs.
fn adapters() -> Registry {
    Registry::new(Arc::new(LoginPath::new("/bin/sh")))
}

fn names(configs: &[LaunchConfig]) -> Vec<String> {
    configs.iter().map(|config| config.name.clone()).collect()
}

fn config<'a>(configs: &'a [LaunchConfig], name: &str) -> &'a LaunchConfig {
    configs
        .iter()
        .find(|config| config.name == name)
        .unwrap_or_else(|| panic!("no configuration {name} in {:?}", names(configs)))
}

fn launch_file(text: &str) -> FakeFiles {
    FakeFiles::with(&[(LAUNCH_FILE, text)])
}

fn launch_json(text: &str) -> FakeFiles {
    FakeFiles::with(&[(LAUNCH_JSON, text)])
}

fn root() -> PathBuf {
    PathBuf::from(ROOT)
}

fn strings(words: &[&str]) -> Vec<String> {
    words.iter().map(|&word| word.to_owned()).collect()
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("not an object: {other}"),
    }
}

fn named(written: &str, name: &str) -> AdapterRef {
    AdapterRef::Named {
        written: written.to_owned(),
        name: Some(name.to_owned()),
    }
}

/// A launch file configuration with every field at its default.
fn base(name: &str, adapter: AdapterRef) -> LaunchConfig {
    LaunchConfig {
        name: name.to_owned(),
        source: Source::File,
        adapter,
        request: Request::Launch,
        program: None,
        args: Vec::new(),
        cwd: Cwd::At(root()),
        env: BTreeMap::new(),
        stop_on_entry: false,
        pre_launch: None,
        console: None,
        adapter_args: Map::new(),
        blockers: Vec::new(),
    }
}

/// The message a diagnostic about `entry` of `file` has, for `problem`.
fn entry_message(file: &str, entry: &str, problem: &str) -> String {
    tr!(
        "workbench-debug-launch-entry-problem",
        file = file,
        entry = entry,
        problem = problem
    )
    .to_string()
}

/// The message for an entry left out for `problem`.
fn left_out(file: &str, entry: &str, problem: &str) -> String {
    let problem = tr!("workbench-debug-launch-left-out", problem = problem).to_string();
    entry_message(file, entry, &problem)
}

/// The message for a listed entry that can't start for `problem`.
fn cannot_start(file: &str, entry: &str, problem: &str) -> String {
    let problem = tr!("workbench-debug-launch-cannot-start", problem = problem).to_string();
    entry_message(file, entry, &problem)
}

fn file_message(file: &str, problem: &str) -> String {
    tr!(
        "workbench-debug-launch-file-problem",
        file = file,
        problem = problem
    )
    .to_string()
}

fn messages(diagnostics: &[LaunchDiagnostic]) -> Vec<String> {
    diagnostics.iter().map(|d| d.message.clone()).collect()
}

/// The diagnostics about `entry`, each naming it.
fn diagnostics_for(diagnostics: &[LaunchDiagnostic], entry: &str) -> Vec<String> {
    let found: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.entry.as_deref() == Some(entry))
        .collect();
    for diagnostic in &found {
        assert!(
            diagnostic.message.contains(entry),
            "{} doesn't name {entry}",
            diagnostic.message
        );
    }
    messages(&found.into_iter().cloned().collect::<Vec<_>>())
}

/// §4's example, word for word.
const RECORD_EXAMPLE: &str = r#"
[[launch]]
name = "server"                    # required, unique in the file
adapter = "lldb-dap"               # an adapter name or a VS Code `type` alias (§3)
request = "launch"                 # the default; "attach" is parsed but refused until D20
program = "${workspaceFolder}/target/debug/server"
args = ["--port", "8080"]          # an array only, never a shell string
cwd = "crates/server"              # relative to the root, the default
env = { RUST_LOG = "debug" }       # strings only
stop_on_entry = false
pre_launch_task = "build server"   # a task label, file or detected (§7)
console = "integrated"             # "integrated" (a terminal tab) or "internal"

[launch.adapter_args]              # passthrough: merged last into the launch arguments
sourceMap = { "/rustc/abc" = "/home/me/.rustup/toolchains/stable/lib/rustlib/src/rust" }

[[launch]]
name = "this file"
adapter = "debugpy"
program = "${file}"

[launch.adapter_args]
justMyCode = false
"#;

#[test]
fn the_records_example_file_parses_whole() {
    let files = FakeFiles::with(&[
        (LAUNCH_FILE, RECORD_EXAMPLE),
        (
            ".tenmoku/tasks.toml",
            "[[task]]\nlabel = \"build server\"\ncommand = \"cargo build -p server\"\n",
        ),
    ]);
    let (configs, diagnostics) = files.load();
    assert_eq!(diagnostics, []);
    assert_eq!(
        configs,
        [
            LaunchConfig {
                program: Some(Program::Written(
                    "${workspaceFolder}/target/debug/server".to_owned()
                )),
                args: strings(&["--port", "8080"]),
                cwd: Cwd::At(root().join("crates/server")),
                env: BTreeMap::from([("RUST_LOG".to_owned(), "debug".to_owned())]),
                pre_launch: Some(PreLaunch::Task("build server".to_owned())),
                console: Some(Console::Integrated),
                adapter_args: object(json!({
                    "sourceMap": {
                        "/rustc/abc": "/home/me/.rustup/toolchains/stable/lib/rustlib/src/rust"
                    }
                })),
                ..base("server", named("lldb-dap", "lldb-dap"))
            },
            LaunchConfig {
                program: Some(Program::Written("${file}".to_owned())),
                adapter_args: object(json!({ "justMyCode": false })),
                ..base("this file", named("debugpy", "debugpy"))
            },
        ]
    );
    assert!(configs.iter().all(LaunchConfig::can_start));
}

#[test]
fn every_field_parses() {
    let (configs, diagnostics) = launch_file(
        r#"
        [[launch]]
        name = "by type"
        adapter = "lldb"
        request = "launch"
        stop_on_entry = true
        console = "internal"
        cwd = "a/../b/./c"

        [launch.adapter_args]
        nested = { list = [1, 2.5, "three", true], empty = {} }

        [[launch]]
        name = "python by type"
        adapter = "python"
        cwd = ""

        [[launch]]
        name = "workspace cwd"
        adapter = "delve"
        cwd = "${workspaceFolder}"

        [[launch]]
        name = "under workspace"
        adapter = "go"
        cwd = "${workspaceFolder}/cmd/tool"

        [[launch]]
        name = "file cwd"
        adapter = "codelldb"
        cwd = "${fileDirname}"

        [[launch]]
        name = "glued cwd"
        adapter = "codelldb"
        cwd = "${workspaceFolder}x"
        "#,
    )
    .load();
    assert_eq!(diagnostics, []);
    assert_eq!(
        configs,
        [
            LaunchConfig {
                stop_on_entry: true,
                console: Some(Console::Internal),
                cwd: Cwd::At(root().join("b/c")),
                adapter_args: object(json!({
                    "nested": {
                        "list": [1, 2.5, "three", true],
                        "empty": {}
                    }
                })),
                ..base("by type", named("lldb", "codelldb"))
            },
            base("python by type", named("python", "debugpy")),
            base("workspace cwd", named("delve", "delve")),
            LaunchConfig {
                cwd: Cwd::At(root().join("cmd/tool")),
                ..base("under workspace", named("go", "delve"))
            },
            LaunchConfig {
                cwd: Cwd::Expand("${fileDirname}".to_owned()),
                ..base("file cwd", named("codelldb", "codelldb"))
            },
            LaunchConfig {
                cwd: Cwd::Expand("${workspaceFolder}x".to_owned()),
                ..base("glued cwd", named("codelldb", "codelldb"))
            },
        ]
    );
}

#[test]
fn an_entry_that_cant_be_used_is_left_out_with_a_diagnostic() {
    // An entry's body after `[[launch]]`, the name it's reported by, and why.
    let cases: Vec<(&str, &str, String)> = vec![
        (
            "adapter = \"debugpy\"",
            "#1",
            tr!("workbench-debug-launch-missing", key = "name").into(),
        ),
        (
            "name = 3\nadapter = \"debugpy\"",
            "#1",
            tr!("workbench-debug-launch-not-string", key = "name").into(),
        ),
        (
            "name = \"  \"\nadapter = \"debugpy\"",
            "#1",
            tr!("workbench-debug-launch-empty-name").into(),
        ),
        (
            "name = \"x\"",
            "x",
            tr!("workbench-debug-launch-missing", key = "adapter").into(),
        ),
        (
            "name = \"x\"\nadapter = 1",
            "x",
            tr!("workbench-debug-launch-not-string", key = "adapter").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nrequest = \"spawn\"",
            "x",
            tr!("workbench-debug-launch-unknown-request", request = "spawn").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nrequest = 1",
            "x",
            tr!("workbench-debug-launch-not-string", key = "request").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nprogram = 1",
            "x",
            tr!("workbench-debug-launch-not-string", key = "program").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nargs = \"--port 8080\"",
            "x",
            tr!("workbench-debug-launch-not-strings", key = "args").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nargs = [\"a\", 1]",
            "x",
            tr!("workbench-debug-launch-not-strings", key = "args").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\ncwd = 1",
            "x",
            tr!("workbench-debug-launch-not-string", key = "cwd").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\ncwd = \"../sibling\"",
            "x",
            tr!("workbench-debug-launch-cwd-outside", cwd = "../sibling").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\ncwd = \"a/../../b\"",
            "x",
            tr!("workbench-debug-launch-cwd-outside", cwd = "a/../../b").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\ncwd = \"/tmp\"",
            "x",
            tr!("workbench-debug-launch-cwd-outside", cwd = "/tmp").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\ncwd = \"${workspaceFolder}/..\"",
            "x",
            tr!(
                "workbench-debug-launch-cwd-outside",
                cwd = "${workspaceFolder}/.."
            )
            .into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nenv = \"A=1\"",
            "x",
            tr!("workbench-debug-launch-not-table", key = "env").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nenv = { \"A=B\" = \"1\" }",
            "x",
            tr!("workbench-debug-launch-env-bad-name", name = "A=B").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nenv = { \"\" = \"1\" }",
            "x",
            tr!("workbench-debug-launch-env-bad-name", name = "").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nenv = { A = 1 }",
            "x",
            tr!("workbench-debug-launch-env-not-string", name = "A").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nstop_on_entry = \"yes\"",
            "x",
            tr!("workbench-debug-launch-not-boolean", key = "stop_on_entry").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\npre_launch_task = 1",
            "x",
            tr!("workbench-debug-launch-not-string", key = "pre_launch_task").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nconsole = \"external\"",
            "x",
            tr!(
                "workbench-debug-launch-unknown-console",
                console = "external"
            )
            .into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nconsole = true",
            "x",
            tr!("workbench-debug-launch-not-string", key = "console").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nadapter_args = \"justMyCode\"",
            "x",
            tr!("workbench-debug-launch-not-table", key = "adapter_args").into(),
        ),
        // TOML's dates and times are no string, and JSON has none to pass.
        (
            "name = 1979-05-27\nadapter = \"debugpy\"",
            "#1",
            tr!("workbench-debug-launch-not-string", key = "name").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nprogram = 2026-11-15",
            "x",
            tr!("workbench-debug-launch-not-string", key = "program").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nargs = [2026-11-15]",
            "x",
            tr!("workbench-debug-launch-not-strings", key = "args").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nenv = { DATE = 2026-11-15 }",
            "x",
            tr!("workbench-debug-launch-env-not-string", name = "DATE").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\ncwd = 07:32:00",
            "x",
            tr!("workbench-debug-launch-not-string", key = "cwd").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nstop_on_entry = 2026-11-15",
            "x",
            tr!("workbench-debug-launch-not-boolean", key = "stop_on_entry").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nadapter_args = { when = 1979-05-27T07:32:00Z }",
            "x",
            tr!("workbench-debug-launch-not-json", key = "adapter_args.when").into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nadapter_args = { deep = { list = [1, nan] } }",
            "x",
            tr!(
                "workbench-debug-launch-not-json",
                key = "adapter_args.deep.list[1]"
            )
            .into(),
        ),
        (
            "name = \"x\"\nadapter = \"debugpy\"\nadapter_args = { limit = -inf }",
            "x",
            tr!(
                "workbench-debug-launch-not-json",
                key = "adapter_args.limit"
            )
            .into(),
        ),
    ];
    for (body, entry, problem) in cases {
        let text =
            format!("[[launch]]\n{body}\n\n[[launch]]\nname = \"kept\"\nadapter = \"debugpy\"\n");
        let (configs, diagnostics) = launch_file(&text).load();
        assert_eq!(names(&configs), ["kept"], "{body}");
        assert_eq!(
            diagnostics_for(&diagnostics, entry),
            [left_out(LAUNCH_FILE, entry, &problem)],
            "{body}"
        );
        assert_eq!(diagnostics.len(), 1, "{body}: {diagnostics:?}");
    }

    let (configs, diagnostics) = launch_file("launch = [\"x\"]").load();
    assert_eq!(configs, []);
    let not_table = tr!("workbench-debug-launch-not-a-table").to_string();
    assert_eq!(
        messages(&diagnostics),
        [left_out(LAUNCH_FILE, "#1", &not_table)]
    );
}

#[test]
fn a_file_that_cant_be_read_or_parsed_loses_only_its_own() {
    let cargo = "[package]\nname = \"app\"\n";
    let mut files = FakeFiles::with(&[
        (LAUNCH_FILE, "[[launch]"),
        ("Cargo.toml", cargo),
        ("src/main.rs", ""),
    ]);
    let (configs, diagnostics) = files.load();
    let app = tr!("workbench-debug-launch-detected-bin", bin = "app").to_string();
    assert_eq!(names(&configs), std::slice::from_ref(&app));
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].file, LAUNCH_FILE);
    assert_eq!(diagnostics[0].entry, None);
    assert!(
        diagnostics[0].message.starts_with(LAUNCH_FILE),
        "{diagnostics:?}"
    );

    files.texts.remove(&root().join(LAUNCH_FILE));
    files.unreadable.insert(root().join(LAUNCH_FILE));
    let (configs, diagnostics) = files.load();
    assert_eq!(names(&configs), std::slice::from_ref(&app));
    let denied = io::Error::from(io::ErrorKind::PermissionDenied).to_string();
    assert_eq!(messages(&diagnostics), [file_message(LAUNCH_FILE, &denied)]);

    let (configs, diagnostics) = launch_file("launch = 1").load();
    assert_eq!(configs, []);
    let not_array = tr!("workbench-debug-launch-not-an-array").to_string();
    assert_eq!(
        messages(&diagnostics),
        [file_message(LAUNCH_FILE, &not_array)]
    );

    let (configs, diagnostics) = launch_file("[other]\nkey = 1\n").load();
    assert_eq!((configs, diagnostics), (Vec::new(), Vec::new()));
}

#[test]
fn an_unknown_key_is_reported_and_the_entry_kept() {
    let (configs, diagnostics) = launch_file(
        r#"
        [[launch]]
        name = "typo"
        adapter = "debugpy"
        stop_on_entyr = true

        [[launch]]
        name = "attach keys"
        adapter = "debugpy"
        request = "attach"
        pid = 42
        host = "localhost"
        port = 5678
        "#,
    )
    .load();
    assert_eq!(names(&configs), ["typo", "attach keys"]);
    assert!(!config(&configs, "typo").stop_on_entry);
    let unknown = tr!("workbench-debug-launch-unknown-key", key = "stop_on_entyr").to_string();
    assert_eq!(
        diagnostics_for(&diagnostics, "typo"),
        [entry_message(LAUNCH_FILE, "typo", &unknown)]
    );
    // Attach's keys are known: only the attach itself is reported.
    let attach = tr!("workbench-debug-launch-attach-unsupported").to_string();
    assert_eq!(
        diagnostics_for(&diagnostics, "attach keys"),
        [cannot_start(LAUNCH_FILE, "attach keys", &attach)]
    );
}

#[test]
fn a_later_repeated_name_is_left_out() {
    let (configs, diagnostics) = launch_file(
        r#"
        [[launch]]
        name = "twice"
        adapter = "debugpy"
        program = "first.py"

        [[launch]]
        name = "twice"
        adapter = "lldb-dap"
        "#,
    )
    .load();
    assert_eq!(names(&configs), ["twice"]);
    assert_eq!(
        config(&configs, "twice").program,
        Some(Program::Written("first.py".to_owned()))
    );
    let repeated = tr!("workbench-debug-launch-repeated-name").to_string();
    assert_eq!(
        messages(&diagnostics),
        [entry_message(LAUNCH_FILE, "twice", &repeated)]
    );
}

#[test]
fn an_attach_entry_is_listed_and_cannot_start() {
    let (configs, diagnostics) = launch_file(
        r#"
        [[launch]]
        name = "attach to server"
        adapter = "lldb-dap"
        request = "attach"
        pid = 1234
        "#,
    )
    .load();
    let attach = config(&configs, "attach to server");
    assert_eq!(attach.request, Request::Attach);
    let reason = tr!("workbench-debug-launch-attach-unsupported").to_string();
    assert_eq!(reason, "attach isn't supported yet");
    assert_eq!(attach.blockers, std::slice::from_ref(&reason));
    assert!(!attach.can_start());
    assert_eq!(
        messages(&diagnostics),
        [cannot_start(LAUNCH_FILE, "attach to server", &reason)]
    );

    // The same from launch.json.
    let (configs, _) = launch_json(
        r#"{ "configurations": [
            { "name": "attach", "type": "python", "request": "attach", "processId": "${command:pickProcess}" }
        ] }"#,
    )
    .load();
    let attach = config(&configs, "attach");
    assert_eq!(attach.request, Request::Attach);
    assert!(attach.blockers.contains(&reason), "{:?}", attach.blockers);
    assert!(!attach.can_start());
}

#[test]
fn command_and_input_variables_give_a_diagnostic_and_cannot_start() {
    let (configs, diagnostics) = launch_file(
        r#"
        [[launch]]
        name = "picks"
        adapter = "debugpy"
        program = "${command:python.interpreterPath}"
        args = ["--level", "${input:level}"]
        env = { TOKEN = "${input:token}" }

        [launch.adapter_args]
        deep = { list = ["${command:pickProcess}", "${command:python.interpreterPath}"] }

        [[launch]]
        name = "fine"
        adapter = "debugpy"
        program = "${file}"
        args = ["${workspaceFolder}", "${workspaceFolderBasename}", "${fileDirname}",
                "${fileBasename}", "${fileBasenameNoExtension}", "${relativeFile}",
                "${lineNumber}", "${env:HOME}", "${userHome}", "${ unclosed"]
        cwd = "${workspaceFolder}"
        "#,
    )
    .load();
    let unsupported = |variable: &str| {
        tr!(
            "workbench-debug-launch-variable-unsupported",
            variable = variable
        )
        .to_string()
    };
    // Each variable once, in the order of the fields.
    let reasons = [
        unsupported("${command:python.interpreterPath}"),
        unsupported("${input:level}"),
        unsupported("${input:token}"),
        unsupported("${command:pickProcess}"),
    ];
    let picks = config(&configs, "picks");
    assert_eq!(picks.blockers, reasons);
    assert!(!picks.can_start());
    let expected: Vec<String> = (reasons.iter())
        .map(|reason| cannot_start(LAUNCH_FILE, "picks", reason))
        .collect();
    assert_eq!(messages(&diagnostics), expected);
    assert!(config(&configs, "fine").can_start());

    // Neither is one of §4's, so neither expands.
    let values = Values::default();
    assert_eq!(
        variables::expand("x ${input:level}", &values),
        Err(unsupported("${input:level}"))
    );
    assert_eq!(
        variables::problems("${command:x}"),
        [unsupported("${command:x}")]
    );
}

#[test]
fn an_unknown_variable_cannot_start() {
    let (configs, diagnostics) = launch_file(
        r#"
        [[launch]]
        name = "config"
        adapter = "debugpy"
        cwd = "${config:python.root}"
        "#,
    )
    .load();
    let reason = tr!(
        "workbench-debug-launch-variable-unknown",
        variable = "${config:python.root}"
    )
    .to_string();
    let entry = config(&configs, "config");
    assert_eq!(entry.cwd, Cwd::Expand("${config:python.root}".to_owned()));
    assert_eq!(entry.blockers, std::slice::from_ref(&reason));
    assert_eq!(
        messages(&diagnostics),
        [cannot_start(LAUNCH_FILE, "config", &reason)]
    );
}

#[test]
fn variables_expand_from_the_active_editor_and_the_environment() {
    let values = Values {
        workspace_folder: root(),
        active: Some(ActiveFile {
            path: root().join("src/pkg/tool.py"),
            line: 12,
        }),
        user_home: Some(PathBuf::from("/home/me")),
        env: BTreeMap::from([("NAME".to_owned(), "value".to_owned())]),
    };
    let file = root()
        .join("src/pkg/tool.py")
        .to_string_lossy()
        .into_owned();
    let dirname = root().join("src/pkg").to_string_lossy().into_owned();
    let cases = [
        ("${workspaceFolder}/bin", "/project/bin"),
        ("${workspaceFolderBasename}", "project"),
        ("${file}", file.as_str()),
        ("${fileDirname}", dirname.as_str()),
        ("${fileBasename}", "tool.py"),
        ("${fileBasenameNoExtension}", "tool"),
        ("${relativeFile}", "src/pkg/tool.py"),
        ("line ${lineNumber}", "line 12"),
        ("${env:NAME}-${env:UNSET}", "value-"),
        ("${userHome}/.cargo", "/home/me/.cargo"),
        ("plain $ {text} ${unclosed", "plain $ {text} ${unclosed"),
    ];
    for (text, expanded) in cases {
        assert_eq!(
            variables::expand(text, &values).as_deref(),
            Ok(expanded),
            "{text}"
        );
    }

    let outside = Values {
        active: Some(ActiveFile {
            path: PathBuf::from("/elsewhere/a.py"),
            line: 1,
        }),
        ..values.clone()
    };
    assert_eq!(
        variables::expand("${relativeFile}", &outside).as_deref(),
        Ok("/elsewhere/a.py")
    );

    let none = Values {
        active: None,
        user_home: None,
        ..values
    };
    assert_eq!(
        variables::expand("${lineNumber}", &none),
        Err(tr!(
            "workbench-debug-launch-variable-no-editor",
            variable = "${lineNumber}"
        )
        .into())
    );
    assert_eq!(
        variables::expand("${userHome}", &none),
        Err(tr!(
            "workbench-debug-launch-variable-no-home",
            variable = "${userHome}"
        )
        .into())
    );
    assert_eq!(
        variables::expand("${nope}", &none),
        Err(tr!(
            "workbench-debug-launch-variable-unknown",
            variable = "${nope}"
        )
        .into())
    );
}

#[test]
fn an_unknown_adapter_is_listed_and_cannot_start() {
    let (configs, diagnostics) =
        launch_file("[[launch]]\nname = \"gdb\"\nadapter = \"gdb\"\n").load();
    let gdb = config(&configs, "gdb");
    assert_eq!(
        gdb.adapter,
        AdapterRef::Named {
            written: "gdb".to_owned(),
            name: None
        }
    );
    let reason = tr!("workbench-debug-launch-no-adapter-named", adapter = "gdb").to_string();
    assert_eq!(gdb.blockers, std::slice::from_ref(&reason));
    assert_eq!(
        messages(&diagnostics),
        [cannot_start(LAUNCH_FILE, "gdb", &reason)]
    );
}

#[test]
fn a_pre_launch_task_is_a_file_or_detected_task() {
    let files = FakeFiles::with(&[
        (
            LAUNCH_FILE,
            r#"
            [[launch]]
            name = "detected task"
            adapter = "lldb-dap"
            pre_launch_task = "cargo build"

            [[launch]]
            name = "file task"
            adapter = "lldb-dap"
            pre_launch_task = "bundle"

            [[launch]]
            name = "no such task"
            adapter = "lldb-dap"
            pre_launch_task = "missing"
            "#,
        ),
        (
            ".tenmoku/tasks.toml",
            "[[task]]\nlabel = \"bundle\"\ncommand = \"true\"\n",
        ),
        ("Cargo.toml", "[workspace]\n"),
    ]);
    let (configs, diagnostics) = files.load();
    assert!(config(&configs, "detected task").can_start());
    assert!(config(&configs, "file task").can_start());
    let reason = tr!("workbench-debug-launch-task-not-found", task = "missing").to_string();
    assert_eq!(
        config(&configs, "no such task").blockers,
        std::slice::from_ref(&reason)
    );
    assert_eq!(
        messages(&diagnostics),
        [cannot_start(LAUNCH_FILE, "no such task", &reason)]
    );
}

/// A launch.json as VS Code writes them: comments, trailing commas, and
/// platform sub-objects.
const LAUNCH_JSON_TEXT: &str = r#"{
    // Use IntelliSense to learn about possible attributes.
    "version": "0.2.0",
    "configurations": [
        {
            "name": "Debug server", /* the main one */
            "type": "lldb",
            "request": "launch",
            "program": "${workspaceFolder}/target/debug/server",
            "args": ["--port", "8080",],
            "cwd": "${workspaceFolder}",
            "env": { "RUST_LOG": "info", "SHARED": "base", },
            "stopOnEntry": true,
            "preLaunchTask": "cargo build",
            "console": "integratedTerminal",
            "sourceLanguages": ["rust"],
            "linux": {
                "program": "${workspaceFolder}/target/debug/server-linux",
                "env": { "RUST_LOG": "debug" },
            },
            "osx": {
                "program": "${workspaceFolder}/target/debug/server-mac",
                "env": { "DYLD_PRINT_LIBRARIES": "1" },
                "sourceMap": { "/rustc/abc": "/src" },
            },
            "windows": { "program": "server.exe" },
        },
        {
            "name": "Python: module",
            "type": "debugpy",
            "request": "launch",
            "module": "pkg.main", // a passthrough key
            "justMyCode": false,
        },
    ],
}"#;

#[test]
fn launch_json_with_comments_trailing_commas_and_an_osx_or_linux_override_imports() {
    let files = FakeFiles::with(&[
        (LAUNCH_JSON, LAUNCH_JSON_TEXT),
        ("Cargo.toml", "[workspace]\n"),
    ]);
    let server = |os| {
        let (configs, diagnostics) = files.load_as(os, None);
        assert_eq!(diagnostics, [], "{os:?}");
        assert_eq!(names(&configs), ["Debug server", "Python: module"]);
        assert!(
            configs
                .iter()
                .all(|config| config.source == Source::LaunchJson)
        );
        assert!(configs.iter().all(LaunchConfig::can_start));
        assert_eq!(
            config(&configs, "Python: module"),
            &LaunchConfig {
                source: Source::LaunchJson,
                adapter_args: object(json!({ "module": "pkg.main", "justMyCode": false })),
                ..base("Python: module", named("debugpy", "debugpy"))
            }
        );
        config(&configs, "Debug server").clone()
    };
    let shared = LaunchConfig {
        source: Source::LaunchJson,
        args: strings(&["--port", "8080"]),
        cwd: Cwd::At(root()),
        stop_on_entry: true,
        pre_launch: Some(PreLaunch::Task("cargo build".to_owned())),
        ..base("Debug server", named("lldb", "codelldb"))
    };
    let env = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        (pairs.iter())
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    };

    assert_eq!(
        server(HostOs::Linux),
        LaunchConfig {
            program: Some(Program::Written(
                "${workspaceFolder}/target/debug/server-linux".to_owned()
            )),
            env: env(&[("RUST_LOG", "debug"), ("SHARED", "base")]),
            adapter_args: object(json!({
                "console": "integratedTerminal",
                "sourceLanguages": ["rust"],
            })),
            ..shared.clone()
        }
    );
    assert_eq!(
        server(HostOs::MacOs),
        LaunchConfig {
            program: Some(Program::Written(
                "${workspaceFolder}/target/debug/server-mac".to_owned()
            )),
            env: env(&[
                ("DYLD_PRINT_LIBRARIES", "1"),
                ("RUST_LOG", "info"),
                ("SHARED", "base"),
            ]),
            adapter_args: object(json!({
                "console": "integratedTerminal",
                "sourceLanguages": ["rust"],
                "sourceMap": { "/rustc/abc": "/src" },
            })),
            ..shared
        }
    );
}

#[test]
fn launch_json_problems_are_reported() {
    let (configs, diagnostics) = launch_json(
        r#"{
            "configurations": [
                { "name": "ignored keys", "type": "debugpy", "postDebugTask": "x",
                  "presentation": { "group": "a" }, "serverReadyAction": {} },
                { "name": "bad platform", "type": "debugpy", "linux": "nope" },
                { "name": "shell args", "type": "debugpy", "args": "--port 1" },
                { "name": "env", "type": "debugpy", "env": [] },
                { "type": "debugpy" },
                { "name": "no type" },
                "not an object",
                { "name": "twice", "type": "debugpy" },
                { "name": "twice", "type": "python" },
            ],
            "compounds": [{ "name": "both", "configurations": ["a", "b"] }],
        }"#,
    )
    .load();
    assert_eq!(names(&configs), ["ignored keys", "bad platform", "twice"]);
    let ignored = |key: &str| {
        let problem = tr!("workbench-debug-launch-ignored-key", key = key).to_string();
        entry_message(LAUNCH_JSON, "ignored keys", &problem)
    };
    assert_eq!(
        diagnostics_for(&diagnostics, "ignored keys"),
        [
            ignored("serverReadyAction"),
            ignored("postDebugTask"),
            ignored("presentation")
        ]
    );
    assert_eq!(config(&configs, "ignored keys").adapter_args, Map::new());
    let platform = tr!("workbench-debug-launch-platform-not-object", key = "linux").to_string();
    assert_eq!(
        diagnostics_for(&diagnostics, "bad platform"),
        [entry_message(LAUNCH_JSON, "bad platform", &platform)]
    );
    let left = |entry: &str, problem: String| {
        assert_eq!(
            diagnostics_for(&diagnostics, entry),
            [left_out(LAUNCH_JSON, entry, &problem)]
        );
    };
    left(
        "shell args",
        tr!("workbench-debug-launch-not-strings", key = "args").into(),
    );
    left(
        "env",
        tr!("workbench-debug-launch-not-object", key = "env").into(),
    );
    left(
        "#5",
        tr!("workbench-debug-launch-missing", key = "name").into(),
    );
    left(
        "no type",
        tr!("workbench-debug-launch-missing", key = "type").into(),
    );
    left("#7", tr!("workbench-debug-launch-not-an-object").into());
    let repeated = tr!("workbench-debug-launch-repeated-name").to_string();
    assert_eq!(
        diagnostics_for(&diagnostics, "twice"),
        [entry_message(LAUNCH_JSON, "twice", &repeated)]
    );
    assert_eq!(
        config(&configs, "twice").adapter,
        named("debugpy", "debugpy")
    );
    let compounds = tr!("workbench-debug-launch-ignored-key", key = "compounds").to_string();
    let file_level: Vec<_> = diagnostics.iter().filter(|d| d.entry.is_none()).collect();
    assert_eq!(file_level.len(), 1, "{diagnostics:?}");
    assert_eq!(file_level[0].message, file_message(LAUNCH_JSON, &compounds));
    assert_eq!(file_level[0].file, LAUNCH_JSON);

    for (text, problem) in [
        (
            "[]",
            tr!("workbench-debug-launch-json-not-object").to_string(),
        ),
        (
            r#"{ "configurations": {} }"#,
            tr!("workbench-debug-launch-not-array", key = "configurations").to_string(),
        ),
    ] {
        let (configs, diagnostics) = launch_json(text).load();
        assert_eq!(configs, [], "{text}");
        assert_eq!(
            messages(&diagnostics),
            [file_message(LAUNCH_JSON, &problem)],
            "{text}"
        );
    }
    let (configs, diagnostics) = launch_json("{ \"configurations\": [ }").load();
    assert_eq!(configs, []);
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].entry, None);
    assert!(
        diagnostics[0].message.starts_with(LAUNCH_JSON),
        "{diagnostics:?}"
    );
    assert_eq!(
        launch_json(r#"{ "version": "0.2.0" }"#).load(),
        (Vec::new(), Vec::new())
    );
}

#[test]
fn a_byte_order_mark_is_read_past() {
    let (configs, diagnostics) = launch_json(
        "\u{feff}{ // saved with a BOM\n \"configurations\": [{ \"name\": \"bom\", \"type\": \"debugpy\" },] }",
    )
    .load();
    assert_eq!(diagnostics, []);
    assert_eq!(names(&configs), ["bom"]);
    let (configs, diagnostics) =
        launch_file("\u{feff}[[launch]]\nname = \"bom\"\nadapter = \"debugpy\"\n").load();
    assert_eq!(diagnostics, []);
    assert_eq!(names(&configs), ["bom"]);
}

#[test]
fn a_launch_toml_name_wins_over_an_imported_one() {
    let files = FakeFiles::with(&[
        (
            LAUNCH_FILE,
            "[[launch]]\nname = \"server\"\nadapter = \"lldb-dap\"\nprogram = \"ours\"\n",
        ),
        (
            LAUNCH_JSON,
            r#"{ "configurations": [
                { "name": "server", "type": "lldb", "program": "theirs" },
                { "name": "client", "type": "lldb", "program": "theirs too" },
            ] }"#,
        ),
    ]);
    let (configs, diagnostics) = files.load();
    assert_eq!(names(&configs), ["server", "client"]);
    let server = config(&configs, "server");
    assert_eq!(server.source, Source::File);
    assert_eq!(server.program, Some(Program::Written("ours".to_owned())));
    assert_eq!(config(&configs, "client").source, Source::LaunchJson);
    let shadowed = tr!("workbench-debug-launch-shadowed", file = LAUNCH_FILE).to_string();
    assert_eq!(
        messages(&diagnostics),
        [entry_message(LAUNCH_JSON, "server", &shadowed)]
    );
    assert_eq!(diagnostics[0].file, LAUNCH_JSON);
}

#[test]
fn an_unknown_type_lists_no_adapter_for_type_x() {
    let (configs, diagnostics) = launch_json(
        r#"{ "configurations": [
            { "name": "node", "type": "pwa-node", "request": "launch", "program": "index.js" },
            { "name": "known", "type": "go" },
        ] }"#,
    )
    .load();
    assert_eq!(names(&configs), ["node", "known"]);
    let node = config(&configs, "node");
    assert_eq!(
        node.adapter,
        AdapterRef::Named {
            written: "pwa-node".to_owned(),
            name: None
        }
    );
    let reason = tr!(
        "workbench-debug-launch-no-adapter-for-type",
        kind = "pwa-node"
    )
    .to_string();
    assert_eq!(reason, "no adapter for type `pwa-node`");
    assert_eq!(node.blockers, std::slice::from_ref(&reason));
    assert!(!node.can_start());
    assert_eq!(config(&configs, "known").adapter, named("go", "delve"));
    assert_eq!(
        messages(&diagnostics),
        [cannot_start(LAUNCH_JSON, "node", &reason)]
    );
}

fn detected_bin(bin: &str) -> LaunchConfig {
    let name = tr!("workbench-debug-launch-detected-bin", bin = bin).to_string();
    LaunchConfig {
        source: Source::Detected,
        program: Some(Program::CargoArtifact {
            bin: bin.to_owned(),
        }),
        pre_launch: Some(PreLaunch::CargoBuild(strings(&[
            "cargo",
            "build",
            "--bin",
            bin,
            "--message-format=json-render-diagnostics",
        ]))),
        ..base(&name, AdapterRef::DefaultFor("rust".to_owned()))
    }
}

#[test]
fn cargo_binaries_are_detected_with_a_json_build_step() {
    let files = FakeFiles::with(&[
        (
            "Cargo.toml",
            r#"
            [package]
            name = "app"
            edition = "2021"

            [[bin]]
            name = "explicit"
            path = "tools/explicit.rs"

            [[bin]]
            name = "named-in-bin"
            "#,
        ),
        ("src/main.rs", "fn main() {}"),
        ("src/lib.rs", ""),
        ("src/bin/zeta.rs", "fn main() {}"),
        ("src/bin/alpha/main.rs", "fn main() {}"),
        ("src/bin/alpha/helper.rs", ""),
        ("src/bin/named-in-bin.rs", "fn main() {}"),
        ("src/bin/notes.txt", ""),
        ("src/bin/empty/mod.rs", ""),
        // Hidden, so cargo passes over them.
        ("src/bin/.backup.rs", "fn main() {}"),
        ("src/bin/.old/main.rs", "fn main() {}"),
    ]);
    let (configs, diagnostics) = files.load();
    assert_eq!(diagnostics, []);
    assert_eq!(
        configs,
        ["app", "explicit", "named-in-bin", "alpha", "zeta"].map(detected_bin)
    );
    let app = &configs[0];
    assert_eq!(app.name, "Debug app");
    assert!(app.can_start());
    // Nothing runs, so cargo's JSON names the program later (D7).
    assert_eq!(
        app.program,
        Some(Program::CargoArtifact {
            bin: "app".to_owned()
        })
    );

    // A [[bin]] can take src/main.rs, and autobins = false finds nothing by itself.
    let claimed = FakeFiles::with(&[
        (
            "Cargo.toml",
            "[package]\nname = \"app\"\nedition = \"2024\"\n\n[[bin]]\nname = \"server\"\npath = \"./src/main.rs\"\n",
        ),
        ("src/main.rs", ""),
        ("src/bin/extra.rs", ""),
    ]);
    assert_eq!(claimed.load().0, ["server", "extra"].map(detected_bin));
    let off = FakeFiles::with(&[
        (
            "Cargo.toml",
            "[package]\nname = \"app\"\nautobins = false\n\n[[bin]]\nname = \"only\"\n",
        ),
        ("src/main.rs", ""),
        ("src/bin/extra.rs", ""),
    ]);
    assert_eq!(off.load().0, [detected_bin("only")]);

    // As cargo does, the 2015 edition, the default, looks for no files once
    // there is a [[bin]], unless autobins says to.
    // `above` are manifests outside the project, by full path.
    let bins_with = |manifest: &str, above: &[(&str, &str)]| {
        let mut files = FakeFiles::with(&[
            ("Cargo.toml", manifest),
            ("src/main.rs", ""),
            ("src/bin/extra.rs", ""),
        ]);
        for (path, text) in above {
            files.texts.insert(PathBuf::from(path), (*text).to_owned());
        }
        let (configs, diagnostics) = files.load();
        assert_eq!(diagnostics, [], "{manifest}");
        configs
    };
    let bins = |manifest: &str| bins_with(manifest, &[]);
    let only = [detected_bin("only")];
    let all = ["app", "only", "extra"].map(detected_bin);
    let package = "[package]\nname = \"app\"\n";
    let bin = "\n[[bin]]\nname = \"only\"\npath = \"tools/only.rs\"\n";
    for (edition, with_bin) in [
        ("", &only[..]),
        ("edition = \"2015\"\n", &only[..]),
        ("edition = \"2015\"\nautobins = true\n", &all[..]),
        ("edition = \"2018\"\n", &all[..]),
    ] {
        assert_eq!(
            bins(&format!("{package}{edition}{bin}")),
            with_bin,
            "{edition}"
        );
        // With no [[bin]], every edition looks.
        assert_eq!(
            bins(&format!("{package}{edition}")),
            ["app", "extra"].map(detected_bin),
            "{edition}"
        );
    }
    // An edition inherited from the root's own workspace is that one.
    let inherited = format!(
        "{package}edition.workspace = true\n{bin}\n[workspace.package]\nedition = \"2015\"\n"
    );
    assert_eq!(bins(&inherited), only);

    // Or from the workspace above the root: the nearest manifest with a
    // [workspace], or the one package.workspace names.
    let member = format!("{package}edition.workspace = true\n{bin}");
    let workspace =
        |edition: &str| format!("[workspace]\n\n[workspace.package]\nedition = \"{edition}\"\n");
    let (old, new) = (workspace("2015"), workspace("2021"));
    assert_eq!(bins_with(&member, &[("/Cargo.toml", &old)]), only);
    assert_eq!(bins_with(&member, &[("/Cargo.toml", &new)]), all);
    let named_workspace =
        format!("{package}edition.workspace = true\nworkspace = \"../ws\"\n{bin}");
    let both = [
        ("/Cargo.toml", new.as_str()),
        ("/ws/Cargo.toml", old.as_str()),
    ];
    assert_eq!(bins_with(&named_workspace, &both), only);
    // With no edition to inherit cargo builds nothing, so none is offered.
    assert_eq!(bins_with(&member, &[]), []);
    let no_edition = "[workspace]\nmembers = [\"project\"]\n";
    assert_eq!(bins_with(&member, &[("/Cargo.toml", no_edition)]), []);

    // A workspace root without a package, a library, and a manifest that
    // doesn't parse give none, and say nothing.
    for files in [
        FakeFiles::with(&[
            ("Cargo.toml", "[workspace]\nmembers = [\"app\"]\n"),
            ("app/src/main.rs", ""),
        ]),
        FakeFiles::with(&[
            ("Cargo.toml", "[package]\nname = \"lib\"\n"),
            ("src/lib.rs", ""),
        ]),
        FakeFiles::with(&[("Cargo.toml", "[package"), ("src/main.rs", "")]),
    ] {
        assert_eq!(files.load(), (Vec::new(), Vec::new()));
    }
}

#[test]
fn a_file_name_replaces_a_detected_one() {
    let app = tr!("workbench-debug-launch-detected-bin", bin = "app").to_string();
    let text = format!("[[launch]]\nname = \"{app}\"\nadapter = \"codelldb\"\n");
    let files = FakeFiles::with(&[
        (LAUNCH_FILE, &text),
        ("Cargo.toml", "[package]\nname = \"app\"\n"),
        ("src/main.rs", ""),
        ("src/bin/other.rs", ""),
    ]);
    let (configs, diagnostics) = files.load();
    assert_eq!(diagnostics, []);
    let other = tr!("workbench-debug-launch-detected-bin", bin = "other").to_string();
    assert_eq!(names(&configs), [app.clone(), other]);
    assert_eq!(config(&configs, &app).source, Source::File);
}

#[test]
fn an_open_python_file_offers_debug_current_file() {
    let files = FakeFiles::with(&[
        ("Cargo.toml", "[package]\nname = \"app\"\n"),
        ("src/main.rs", ""),
    ]);
    let name = tr!("workbench-debug-launch-current-file").to_string();
    assert_eq!(name, "Debug current file");
    let (configs, diagnostics) = files.load_as(HostOs::Linux, Some("python"));
    assert_eq!(diagnostics, []);
    assert_eq!(
        configs,
        [
            detected_bin("app"),
            LaunchConfig {
                source: Source::Detected,
                program: Some(Program::Written("${file}".to_owned())),
                ..base(&name, named("debugpy", "debugpy"))
            },
        ]
    );
    assert!(configs[1].can_start());

    for language in [None, Some("rust"), Some("go")] {
        let (configs, _) = files.load_as(HostOs::Linux, language);
        assert_eq!(configs, [detected_bin("app")], "{language:?}");
    }
    assert_eq!(FakeFiles::default().names(), Vec::<String>::new());
}
