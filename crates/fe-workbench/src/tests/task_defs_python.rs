//! Python projects' detected tasks (T688) over a fake [`ProjectFiles`]:
//! the main file and pytest by the project's interpreter, its
//! `[project.scripts]`, and all of them through `uv run` with a `uv.lock`.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use crate::task_defs::{Group, ProjectFiles, Source, Task, TaskCommand, load};

const ROOT: &str = "/project";

/// A project's files, by their names under [`ROOT`]; a folder is listed by
/// what's under it.
struct FakeFiles(BTreeMap<PathBuf, String>);

impl FakeFiles {
    fn with(files: &[(&str, &str)]) -> Self {
        Self(
            files
                .iter()
                .map(|(name, text)| (Path::new(ROOT).join(name), (*text).to_owned()))
                .collect(),
        )
    }

    fn tasks(&self) -> Vec<Task> {
        let (tasks, diagnostics) = load(self, Path::new(ROOT));
        assert_eq!(diagnostics, [], "no tasks file, so nothing to report");
        tasks
    }

    fn python_tasks(&self) -> Vec<Task> {
        (self.tasks().into_iter())
            .filter(|task| matches!(task.source, Source::Python | Source::Uv))
            .collect()
    }

    fn labels(&self) -> Vec<String> {
        self.python_tasks()
            .into_iter()
            .map(|task| task.label)
            .collect()
    }
}

impl ProjectFiles for FakeFiles {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
    }

    fn exists(&self, path: &Path) -> bool {
        self.0.contains_key(path)
    }

    fn list_dir(&self, dir: &Path) -> Vec<PathBuf> {
        let mut entries: Vec<PathBuf> = (self.0.keys())
            .filter_map(|path| {
                let rest = path.strip_prefix(dir).ok()?;
                let first = rest.components().next()?;
                Some(dir.join(first))
            })
            .collect();
        entries.dedup();
        entries
    }
}

/// The `.venv`'s interpreter, as a fake file name under the root.
fn venv_python() -> &'static str {
    if cfg!(windows) {
        ".venv/Scripts/python.exe"
    } else {
        ".venv/bin/python"
    }
}

/// The interpreter on the PATH that a root without a `.venv` gets.
fn bare_python() -> &'static str {
    if cfg!(windows) { "python" } else { "python3" }
}

/// `rel` under the root, joined a component at a time as production does
/// (one `join("a/b")` would mix separators on Windows).
fn by_components(rel: &str) -> String {
    let mut path = PathBuf::from(ROOT);
    path.extend(rel.split('/'));
    path.to_str().unwrap().to_owned()
}

/// The `.venv`'s interpreter, on its full path.
fn venv_python_path() -> String {
    by_components(venv_python())
}

/// Where a `.venv` holds the script `name`, under the root.
fn venv_script(name: &str) -> String {
    if cfg!(windows) {
        format!(".venv/Scripts/{name}.exe")
    } else {
        format!(".venv/bin/{name}")
    }
}

fn argv(words: &[&str]) -> TaskCommand {
    TaskCommand::Argv(words.iter().map(|&word| word.to_owned()).collect())
}

fn only<'a>(tasks: &'a [Task], label: &str) -> &'a Task {
    let found: Vec<&Task> = tasks.iter().filter(|task| task.label == label).collect();
    assert_eq!(found.len(), 1, "one {label} in {tasks:?}");
    found[0]
}

#[test]
fn main_py_runs_with_the_project_python() {
    let venv = FakeFiles::with(&[("main.py", "print('hi')\n"), (venv_python(), "")]);
    let python = venv_python_path();
    let label = format!("{python} main.py");
    assert_eq!(venv.labels(), std::slice::from_ref(&label));
    let tasks = venv.python_tasks();
    let main = only(&tasks, &label);
    assert_eq!(main.command, argv(&[&python, "main.py"]));
    assert_eq!(main.group, Some(Group::Run));
    assert_eq!(main.source, Source::Python);
    assert_eq!(main.cwd, Path::new(ROOT));
    assert!(!main.default, "the run group has no by-name default");

    let bare = FakeFiles::with(&[("main.py", "print('hi')\n")]);
    let label = format!("{} main.py", bare_python());
    assert_eq!(bare.labels(), std::slice::from_ref(&label));
    let tasks = bare.python_tasks();
    assert_eq!(
        only(&tasks, &label).command,
        argv(&[bare_python(), "main.py"])
    );

    // Any marker makes a Python project, but only main.py gives the task.
    for marker in ["pyproject.toml", "setup.py", "requirements.txt"] {
        let project = FakeFiles::with(&[(marker, ""), ("tests/test_x.py", "")]);
        assert_eq!(
            project.labels(),
            [format!("{} -m pytest", bare_python())],
            "{marker} marks a Python project"
        );
    }
    let uv = FakeFiles::with(&[("uv.lock", ""), ("tests/test_x.py", "")]);
    assert_eq!(uv.labels(), ["uv run pytest"], "uv.lock marks one too");

    // No marker: a .venv, tests or a stray script aren't a Python project.
    let none = FakeFiles::with(&[
        (venv_python(), ""),
        ("app.py", ""),
        ("tests/test_x.py", ""),
        ("conftest.py", ""),
        ("pytest.ini", ""),
        ("README.md", ""),
    ]);
    assert_eq!(none.labels(), Vec::<String>::new());
    assert_eq!(none.tasks(), []);

    // Beside another build system, Python's tasks come after .NET's.
    let mixed = FakeFiles::with(&[("Cargo.toml", ""), ("main.py", "")]);
    let labels: Vec<String> = mixed.tasks().into_iter().map(|task| task.label).collect();
    assert_eq!(
        labels,
        [
            "cargo build".to_owned(),
            "cargo test".to_owned(),
            "cargo run".to_owned(),
            "cargo clippy".to_owned(),
            format!("{} main.py", bare_python()),
        ]
    );
}

#[test]
fn pytest_when_tests_exist() {
    let pytest = format!("{} -m pytest", venv_python_path());
    let with = |files: &[(&str, &str)]| {
        let mut all = vec![("main.py", ""), (venv_python(), "")];
        all.extend_from_slice(files);
        FakeFiles::with(&all)
    };
    let found = [
        vec![("tests/test_app.py", "")],
        vec![("tests/unit/helpers.py", "")],
        vec![("test_app.py", "")],
        vec![("app_test.py", "")],
        vec![("conftest.py", "")],
        vec![("pytest.ini", "[pytest]\n")],
        vec![(
            "pyproject.toml",
            "[project]\nname = \"app\"\n\n[tool.pytest.ini_options]\naddopts = \"-q\"\n",
        )],
        vec![(
            "setup.cfg",
            "[metadata]\nname = app\n\n[tool:pytest]\ntestpaths = t\n",
        )],
        vec![(
            "setup.cfg",
            "[metadata]\nname = app\n\n[tool:pytest] # pytest settings\ntestpaths = t\n",
        )],
        vec![(
            "setup.cfg",
            "[tool:pytest]   ; settings\r\ntestpaths = t\r\n",
        )],
    ];
    for files in &found {
        let project = with(files);
        let tasks = project.python_tasks();
        let task = only(&tasks, &pytest);
        assert_eq!(
            task.command,
            argv(&[&venv_python_path(), "-m", "pytest"]),
            "{files:?}"
        );
        assert_eq!(task.group, Some(Group::Test), "{files:?}");
        assert_eq!(task.source, Source::Python, "{files:?}");
        assert!(task.default, "the test group's default by name: {files:?}");
    }

    let not_found = [
        vec![],
        vec![("tests_old.txt", "")],
        vec![("app.py", "")],
        vec![("testing.py", "")],
        vec![("test_app.txt", "")],
        vec![(
            "pyproject.toml",
            "[project]\nname = \"app\"\n[tool.ruff]\nline-length = 90\n",
        )],
        vec![("pyproject.toml", "[tool.pytest]\nini_options = 1\n")],
        vec![("setup.cfg", "[metadata]\nname = app\n# [tool:pytest]\n")],
        vec![("setup.cfg", "[metadata]\nname = app\n  [tool:pytest]\n")],
        vec![("setup.cfg", "[metadata]\nnote = [tool:pytest]\n")],
        vec![("setup.cfg", "[tool:pytest.ini]\n[ tool:pytest ]\n")],
    ];
    for files in &not_found {
        let project = with(files);
        assert_eq!(
            project.labels(),
            [format!("{} main.py", venv_python_path())],
            "no pytest for {files:?}"
        );
    }

    // An earlier source's test task is the default, by the same order.
    let cargo = with(&[("Cargo.toml", ""), ("tests/test_app.py", "")]);
    let tasks = cargo.tasks();
    assert!(only(&tasks, "cargo test").default);
    assert!(!only(&tasks, &pytest).default);

    // So is the tasks file's.
    let marked = with(&[
        ("tests/test_app.py", ""),
        (
            ".tenmoku/tasks.toml",
            "[[task]]\nlabel = \"t\"\ncommand = \"true\"\ngroup = \"test\"\ndefault = true\n",
        ),
    ]);
    let tasks = marked.tasks();
    assert!(only(&tasks, "t").default);
    assert!(!only(&tasks, &pytest).default);
}

const SCRIPTS: &str = r#"
[project]
name = "app"

[project.scripts]
zeta = "app.cli:zeta"
alpha = "app.cli:alpha"
serve = "app.web:main"
"not-a-string" = 3
"../escape" = "app:x"
"#;

#[test]
fn project_scripts_and_uv() {
    // With a .venv, each script it holds, in the file's order.
    let venv = FakeFiles::with(&[
        ("pyproject.toml", SCRIPTS),
        (venv_python(), ""),
        (&venv_script("zeta"), ""),
        (&venv_script("serve"), ""),
        (&venv_script("not-a-string"), ""),
    ]);
    let script = |name: &str| by_components(&venv_script(name));
    assert_eq!(venv.labels(), [script("zeta"), script("serve")]);
    let tasks = venv.python_tasks();
    let zeta = only(&tasks, &script("zeta"));
    assert_eq!(zeta.command, argv(&[&script("zeta")]));
    assert_eq!(zeta.source, Source::Python);
    assert_eq!(zeta.group, None);

    // With no .venv, no scripts: they aren't installed anywhere known.
    let bare = FakeFiles::with(&[("pyproject.toml", SCRIPTS)]);
    assert_eq!(bare.labels(), Vec::<String>::new());

    // With uv.lock, everything runs through uv, in the file's order.
    let uv = FakeFiles::with(&[
        ("pyproject.toml", SCRIPTS),
        ("uv.lock", ""),
        ("main.py", ""),
        ("tests/test_app.py", ""),
        (venv_python(), ""),
    ]);
    assert_eq!(
        uv.labels(),
        [
            "uv run main.py",
            "uv run pytest",
            "uv run zeta",
            "uv run alpha",
            "uv run serve",
        ]
    );
    let tasks = uv.python_tasks();
    let main = only(&tasks, "uv run main.py");
    assert_eq!(main.command, argv(&["uv", "run", "main.py"]));
    assert_eq!((main.group, main.source), (Some(Group::Run), Source::Uv));
    let pytest = only(&tasks, "uv run pytest");
    assert_eq!(pytest.command, argv(&["uv", "run", "pytest"]));
    assert_eq!(
        (pytest.group, pytest.source),
        (Some(Group::Test), Source::Uv)
    );
    assert!(
        pytest.default,
        "uv's test task is its group's default by name"
    );
    let alpha = only(&tasks, "uv run alpha");
    assert_eq!(alpha.command, argv(&["uv", "run", "alpha"]));
    assert_eq!((alpha.group, alpha.source), (None, Source::Uv));

    // A script named like an option is passed after `--`.
    let dashed = FakeFiles::with(&[
        ("pyproject.toml", "[project.scripts]\n\"-x\" = \"app:x\"\n"),
        ("uv.lock", ""),
    ]);
    let tasks = dashed.python_tasks();
    assert_eq!(
        only(&tasks, "uv run -x").command,
        argv(&["uv", "run", "--", "-x"])
    );

    // A pyproject.toml that doesn't parse leaves only the file-based tasks.
    for broken in [
        "[project\nname = ",
        "[project]\nscripts = \"app:main\"\n",
        "[tool.pytest.ini_options\n",
    ] {
        let project = FakeFiles::with(&[
            ("pyproject.toml", broken),
            ("uv.lock", ""),
            ("main.py", ""),
            ("conftest.py", ""),
        ]);
        assert_eq!(
            project.labels(),
            ["uv run main.py", "uv run pytest"],
            "for {broken:?}"
        );
        let project = FakeFiles::with(&[("pyproject.toml", broken), ("main.py", "")]);
        assert_eq!(
            project.labels(),
            [format!("{} main.py", bare_python())],
            "for {broken:?}"
        );
    }
}
