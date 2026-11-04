//! Project tasks over a fake [`ProjectFiles`]: nothing here reads the disk.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use crate::task_defs::{Group, ProjectFiles, Source, Task, TaskCommand, TaskDiagnostic, load};

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

    fn load(&self) -> (Vec<Task>, Vec<TaskDiagnostic>) {
        load(self, Path::new(ROOT))
    }

    fn labels(&self) -> Vec<String> {
        labels(&self.load().0)
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
}

fn labels(tasks: &[Task]) -> Vec<String> {
    tasks.iter().map(|task| task.label.clone()).collect()
}

fn task<'a>(tasks: &'a [Task], label: &str) -> &'a Task {
    tasks
        .iter()
        .find(|task| task.label == label)
        .unwrap_or_else(|| panic!("no task {label} in {:?}", labels(tasks)))
}

fn argv(words: &[&str]) -> TaskCommand {
    TaskCommand::Argv(words.iter().map(|&word| word.to_owned()).collect())
}

/// The one diagnostic about `entry`.
fn diagnostic_for<'a>(diagnostics: &'a [TaskDiagnostic], entry: &str) -> &'a TaskDiagnostic {
    let found: Vec<_> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.entry.as_deref() == Some(entry))
        .collect();
    assert_eq!(found.len(), 1, "for {entry} in {diagnostics:?}");
    assert!(
        found[0].message.contains(entry),
        "{} doesn't name {entry}",
        found[0].message
    );
    found[0]
}

fn tasks_file(text: &str) -> FakeFiles {
    FakeFiles::with(&[(".tenmoku/tasks.toml", text)])
}

#[test]
fn tasks_file_parses() {
    let (tasks, diagnostics) = tasks_file(
        r#"
        [[task]]
        label = "release"
        command = "cargo build --release && strip target/release/app"
        cwd = "crates/app"
        env = { RUST_LOG = "debug", PROFILE = "release" }
        group = "build"
        default = true
        problem_matcher = "rustc"

        [[task]]
        label = "unit"
        command = ["cargo", "nextest", "run", ""]
        group = "test"

        [[task]]
        label = "serve"
        command = "./serve"
        cwd = "a/../b/./c"
        group = "run"
        default = true

        [[task]]
        label = "plain"
        command = "true"
        cwd = ""

        [[task]]
        label = "release"
        command = "echo the second one"

        [[task]]
        label = "no words"
        command = "   "

        [[task]]
        label = "no args"
        command = []

        [[task]]
        label = "no program"
        command = ["", "arg"]

        [[task]]
        label = "up and out"
        command = "true"
        cwd = "../sibling"

        [[task]]
        label = "down, up and out"
        command = "true"
        cwd = "a/../../sibling"

        [[task]]
        label = "absolute"
        command = "true"
        cwd = "/etc"
        "#,
    )
    .load();

    assert_eq!(labels(&tasks), ["release", "unit", "serve", "plain"]);

    let release = task(&tasks, "release");
    assert_eq!(
        release.command,
        TaskCommand::Shell("cargo build --release && strip target/release/app".to_owned())
    );
    assert_eq!(release.cwd, Path::new("/project/crates/app"));
    assert_eq!(
        release.env,
        BTreeMap::from([
            ("PROFILE".to_owned(), "release".to_owned()),
            ("RUST_LOG".to_owned(), "debug".to_owned()),
        ])
    );
    assert_eq!(release.group, Some(Group::Build));
    assert!(release.default);
    assert_eq!(release.source, Source::File);
    assert_eq!(release.problem_matcher.as_deref(), Some("rustc"));

    let unit = task(&tasks, "unit");
    assert_eq!(unit.command, argv(&["cargo", "nextest", "run", ""]));
    assert_eq!(unit.cwd, Path::new(ROOT));
    assert_eq!(unit.group, Some(Group::Test));
    assert!(!unit.default);
    assert!(unit.env.is_empty());
    assert_eq!(unit.problem_matcher, None);

    let serve = task(&tasks, "serve");
    assert_eq!(serve.cwd, Path::new("/project/b/c"));
    assert_eq!(serve.group, Some(Group::Run));
    assert!(serve.default);

    let plain = task(&tasks, "plain");
    assert_eq!(plain.cwd, Path::new(ROOT));
    assert_eq!(plain.group, None);

    assert_eq!(diagnostics.len(), 7, "{diagnostics:?}");
    assert!(
        diagnostic_for(&diagnostics, "release")
            .message
            .contains("label")
    );
    for empty in ["no words", "no args", "no program"] {
        assert!(
            diagnostic_for(&diagnostics, empty)
                .message
                .contains("empty command")
        );
    }
    for escaping in ["up and out", "down, up and out", "absolute"] {
        assert!(
            diagnostic_for(&diagnostics, escaping)
                .message
                .contains("outside the project root")
        );
    }
}

#[test]
fn a_bad_entry_of_any_kind_is_named_and_the_rest_survive() {
    let (tasks, diagnostics) = tasks_file(
        r#"
        [[task]]
        command = "no label"

        [[task]]
        label = "no command"

        [[task]]
        label = ""
        command = "true"

        [[task]]
        label = 7
        command = "true"

        [[task]]
        label = "numeric command"
        command = 7

        [[task]]
        label = "mixed argv"
        command = ["cargo", 7]

        [[task]]
        label = "numeric env"
        command = "true"
        env = { JOBS = 4 }

        [[task]]
        label = "bad env name"
        command = "true"
        env = { "A=B" = "c" }

        [[task]]
        label = "bad group"
        command = "true"
        group = "deploy"

        [[task]]
        label = "bad default"
        command = "true"
        default = "yes"

        [[task]]
        label = "bad matcher"
        command = "true"
        problem_matcher = 7

        [[task]]
        label = "good"
        command = "true"
        "#,
    )
    .load();

    assert_eq!(labels(&tasks), ["good"]);
    assert_eq!(diagnostics.len(), 11, "{diagnostics:?}");
    for entry in [
        "#1",
        "no command",
        "#3",
        "#4",
        "numeric command",
        "mixed argv",
        "numeric env",
        "bad env name",
        "bad group",
        "bad default",
        "bad matcher",
    ] {
        diagnostic_for(&diagnostics, entry);
    }
}

#[test]
fn an_invalid_entry_does_not_claim_its_label() {
    let (tasks, diagnostics) = tasks_file(
        r#"
        [[task]]
        label = "build"
        command = ""

        [[task]]
        label = "build"
        command = "make"
        "#,
    )
    .load();

    assert_eq!(
        task(&tasks, "build").command,
        TaskCommand::Shell("make".to_owned())
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics[0].message.contains("empty command"));
}

#[test]
fn an_unknown_key_is_reported_and_keeps_its_entry() {
    let (tasks, diagnostics) = tasks_file(
        r#"
        [[task]]
        label = "build"
        command = "make"
        defualt = true
        "#,
    )
    .load();

    assert_eq!(labels(&tasks), ["build"]);
    assert!(!tasks[0].default);
    assert!(
        diagnostic_for(&diagnostics, "build")
            .message
            .contains("defualt")
    );
}

#[test]
fn a_tasks_file_that_cannot_be_used_keeps_the_detected_tasks() {
    for text in ["[[task", "task = 3", "[task]\nlabel = \"x\""] {
        let mut files = tasks_file(text);
        files
            .texts
            .insert(Path::new(ROOT).join("Makefile"), "all:\n".to_owned());
        let (tasks, diagnostics) = files.load();
        assert_eq!(labels(&tasks), ["make all"], "for {text}");
        assert_eq!(diagnostics.len(), 1, "for {text}: {diagnostics:?}");
        assert_eq!(diagnostics[0].entry, None);
        assert!(diagnostics[0].message.contains(".tenmoku/tasks.toml"));
    }
}

#[test]
fn a_missing_tasks_file_is_no_problem_but_an_unreadable_one_is() {
    let (tasks, diagnostics) = FakeFiles::default().load();
    assert!(tasks.is_empty());
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    let (_, diagnostics) = tasks_file("").load();
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    let mut files = FakeFiles::default();
    files
        .unreadable
        .insert(Path::new(ROOT).join(".tenmoku/tasks.toml"));
    let (tasks, diagnostics) = files.load();
    assert!(tasks.is_empty());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].entry, None);
}

#[test]
fn cargo_targets_are_detected() {
    for manifest in [
        "[package]\nname = \"app\"\n",
        "[workspace]\nmembers = [\"crates/*\"]\n",
    ] {
        let (tasks, diagnostics) = FakeFiles::with(&[("Cargo.toml", manifest)]).load();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            labels(&tasks),
            ["cargo build", "cargo test", "cargo run", "cargo clippy"]
        );
        for task in &tasks {
            assert_eq!(task.source, Source::Cargo);
            assert_eq!(task.cwd, Path::new(ROOT));
        }

        let build = task(&tasks, "cargo build");
        assert_eq!(build.command, argv(&["cargo", "build"]));
        assert_eq!(build.group, Some(Group::Build));
        assert!(build.default);

        let test = task(&tasks, "cargo test");
        assert_eq!(test.command, argv(&["cargo", "test"]));
        assert_eq!(test.group, Some(Group::Test));
        assert!(test.default);

        let run = task(&tasks, "cargo run");
        assert_eq!(run.group, Some(Group::Run));
        assert!(!run.default);

        let clippy = task(&tasks, "cargo clippy");
        assert_eq!(clippy.group, None);
        assert!(!clippy.default);
    }

    assert!(FakeFiles::default().load().0.is_empty());
    // Shallow: a member's manifest isn't the root's.
    assert!(
        FakeFiles::with(&[("crates/app/Cargo.toml", "[package]\n")])
            .load()
            .0
            .is_empty()
    );
}

#[test]
fn npm_scripts_use_the_right_package_manager() {
    let package =
        r#"{ "name": "app", "scripts": { "test": "jest", "build": "tsc", "lint:fix": "eslint" } }"#;
    let cases: [(&[&str], &[&str]); 7] = [
        (&[], &["npm", "run"]),
        (&["package-lock.json"], &["npm", "run"]),
        (&["pnpm-lock.yaml"], &["pnpm", "run"]),
        (&["yarn.lock"], &["yarn"]),
        (&["bun.lockb"], &["bun", "run"]),
        (&["bun.lock"], &["bun", "run"]),
        (
            &["pnpm-lock.yaml", "yarn.lock", "bun.lock"],
            &["pnpm", "run"],
        ),
    ];
    for (lockfiles, runner) in cases {
        let mut files = vec![("package.json", package)];
        files.extend(lockfiles.iter().map(|&lockfile| (lockfile, "")));
        let (tasks, diagnostics) = FakeFiles::with(&files).load();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        // In the file's order, not sorted.
        let prefix = runner.join(" ");
        assert_eq!(
            labels(&tasks),
            [
                format!("{prefix} test"),
                format!("{prefix} build"),
                format!("{prefix} lint:fix"),
            ]
        );
        let lint = [runner, &["lint:fix"]].concat();
        assert_eq!(tasks[2].command, argv(&lint));
        assert_eq!(tasks[2].source, Source::Npm);

        assert_eq!(tasks[0].group, Some(Group::Test));
        assert!(tasks[0].default);
        assert_eq!(tasks[1].group, Some(Group::Build));
        assert!(tasks[1].default);
        assert_eq!(tasks[2].group, None);
        assert!(!tasks[2].default);
    }

    for no_tasks in [
        "{ not json",
        r#"[{ "build": "tsc" }]"#,
        r#"{ "scripts": { "": "unnamed" } }"#,
        "[]",
        "{}",
        r#"{ "name": "app" }"#,
        r#"{ "scripts": {} }"#,
        r#"{ "scripts": 3 }"#,
        r#"{ "scripts": { "build": 3 } }"#,
    ] {
        let (tasks, diagnostics) = FakeFiles::with(&[("package.json", no_tasks)]).load();
        assert!(tasks.is_empty(), "for {no_tasks}: {:?}", labels(&tasks));
        assert!(diagnostics.is_empty(), "for {no_tasks}: {diagnostics:?}");
    }

    assert_eq!(
        FakeFiles::with(&[("package.json", "\n  { \"scripts\": { \"a\": \"b\" } }")]).labels(),
        ["npm run a"]
    );

    // A lockfile alone is no project, and one script's bad value isn't all's.
    assert!(FakeFiles::with(&[("yarn.lock", "")]).load().0.is_empty());
    assert_eq!(
        FakeFiles::with(&[("package.json", r#"{ "scripts": { "a": 1, "b": "x" } }"#)]).labels(),
        ["npm run b"]
    );
}

#[test]
fn make_and_just_targets_are_listed() {
    let makefile = "\
# A comment: not a rule
CC = gcc
FLAGS := -O2
CACHE ?= a:b
LIBS += -lm
NOW != date +%H:%M
LONG = one \\
    two: three
export PREFIX := /usr
.PHONY: all clean
.DEFAULT_GOAL := all
.SUFFIXES:
all: app docs # the usual: both
\t@echo not: a target
\techo split \\
continued: too
app docs: main.o
%.o: %.c
\t$(CC) -c $<
lib%.a: x.o
$(BIN): main.o
*.tmp:
check:: all
debug: FLAGS = -g
inline: ; @echo A=B: done
deep/er.txt: all
ifeq ($(OS),linux:x)
linux-only:
else
other:
endif
define HELP
in-define: not a rule
endef
all: more
$(addprefix out/,one two) listed:
lib.a(one.o two.o):
lib.a(one.o):
(wrapped) mid(dle)x:
(foo(bar baz):
lib.a(one).o two.o):
spaced\\ name real:
export override define MULTI
phantom: not a rule
endef
private export define MULTI2
phantom-too: not a rule
endef
grouped outputs &:
both more&::
brace{ open( literal:
spaced & :
tight& :
escaped\\&:
clean:
";
    for name in ["Makefile", "GNUmakefile"] {
        let (tasks, diagnostics) = FakeFiles::with(&[(name, makefile)]).load();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            labels(&tasks),
            [
                "make all",
                "make app",
                "make docs",
                "make check",
                "make inline",
                "make deep/er.txt",
                "make linux-only",
                "make other",
                "make listed",
                "make (wrapped)",
                "make mid(dle)x",
                "make (foo(bar",
                "make baz)",
                "make real",
                "make grouped",
                "make outputs",
                "make both",
                "make more",
                "make brace{",
                "make open(",
                "make literal",
                "make spaced",
                "make &",
                "make tight&",
                "make clean",
            ],
            "from {name}"
        );
        assert_eq!(tasks[0].command, argv(&["make", "all"]));
        assert_eq!(tasks[0].source, Source::Make);
        assert_eq!(tasks[0].group, None);
        assert!(!tasks[0].default);
    }
    // `make` reads GNUmakefile over Makefile, and only one of them.
    assert_eq!(
        FakeFiles::with(&[("GNUmakefile", "gnu:\n"), ("Makefile", "plain:\n")]).labels(),
        ["make gnu"]
    );
    // Windows line endings, and a final line with no newline.
    assert_eq!(
        FakeFiles::with(&[("Makefile", "a:\r\n\tb: c\r\nd: a")]).labels(),
        ["make a", "make d"]
    );

    let justfile = "\
# A comment: not a recipe
set shell := [\"bash\", \"-c\"]
set dotenv-load
export JOBS := \"4\"
version := `git describe: --tags`
alias b := build
import 'other:file.just'
mod tools

default:
    @just --list

build: lint
    cargo build
    echo indented: not a recipe

@quiet:
    true

deploy env='prod:eu' *args: build (lint \"x\")
    ./deploy {{env}} {{args}}

_helper:
    true

@_quiet_helper:
    true

[private]
hidden:
    true

[private, no-cd]
hidden-too:
    true

[private]
alias h := hidden

[group('private')]
[no-cd]
kebab-case arg=\"a\\\"b:c\":
    true

notes := \"\"\"
in-string: not a recipe
\"\"\"

fence := '```'
after-fence:

commented: # a \"\"\" in a comment
after-comment:

[doc(\"example, private, text\")]
documented:

[private] # helper
hidden-by-attribute:

lint # a comment: not a colon for the recipe
";
    for name in ["justfile", "Justfile"] {
        let (tasks, diagnostics) = FakeFiles::with(&[(name, justfile)]).load();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            labels(&tasks),
            [
                "just default",
                "just build",
                "just quiet",
                "just deploy",
                "just kebab-case",
                "just after-fence",
                "just commented",
                "just after-comment",
                "just documented",
            ],
            "from {name}"
        );
        assert_eq!(tasks[3].command, argv(&["just", "deploy"]));
        assert_eq!(tasks[3].source, Source::Just);
        assert_eq!(tasks[3].group, None);
    }
    // One multi-line string closing and the next opening, on one line.
    assert_eq!(
        FakeFiles::with(&[(
            "justfile",
            "a := \"\"\"\nx\n\"\"\" + \"\"\"\nin-string:\n\"\"\"\nb:\n"
        )])
        .labels(),
        ["just b"]
    );
    // A string of any kind can run over lines, in an assignment, an attribute
    // or a parameter's default.
    assert_eq!(
        FakeFiles::with(&[("justfile", "notes := '\nphantom:\n'\nreal:\n")]).labels(),
        ["just real"]
    );
    assert_eq!(
        FakeFiles::with(&[("justfile", "note := \"\nphantom:\n\"\nreal:\n")]).labels(),
        ["just real"]
    );
    assert_eq!(
        FakeFiles::with(&[("justfile", "[doc(\"\"\"\nphantom:\n\"\"\")]\nreal:\n")]).labels(),
        ["just real"]
    );
    assert_eq!(
        FakeFiles::with(&[(
            "justfile",
            "[doc('\nphantom:\n'), private]\nhidden:\nreal:\n"
        )])
        .labels(),
        ["just real"]
    );
    assert_eq!(
        FakeFiles::with(&[("justfile", "deploy env='a\nb:c': dep\nreal:\n")]).labels(),
        ["just deploy", "just real"]
    );
    // `[private]` applies to what follows it, and only that.
    assert_eq!(
        FakeFiles::with(&[("justfile", "[private]\nalias a := b\nb:\n")]).labels(),
        ["just b"]
    );
}

#[test]
fn file_tasks_win_over_detected_ones() {
    let (tasks, diagnostics) = FakeFiles::with(&[
        (
            ".tenmoku/tasks.toml",
            r#"
            [[task]]
            label = "zebra"
            command = "./zebra"

            [[task]]
            label = "make all"
            command = "make -j8 all"

            [[task]]
            label = "alpha"
            command = "./alpha"

            [[task]]
            label = "cargo test"
            command = ["cargo", "nextest", "run"]
            "#,
        ),
        ("Cargo.toml", "[package]\n"),
        (
            "package.json",
            r#"{ "scripts": { "start": "a", "lint": "b" } }"#,
        ),
        ("Makefile", "docs:\nall:\nclean:\n"),
        ("justfile", "fmt:\ncheck:\n"),
    ])
    .load();

    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(
        labels(&tasks),
        [
            "zebra",
            "make all",
            "alpha",
            "cargo test",
            "cargo build",
            "cargo run",
            "cargo clippy",
            "npm run start",
            "npm run lint",
            "make docs",
            "make clean",
            "just fmt",
            "just check",
        ]
    );
    let replaced = task(&tasks, "make all");
    assert_eq!(replaced.source, Source::File);
    assert_eq!(
        replaced.command,
        TaskCommand::Shell("make -j8 all".to_owned())
    );
    let replaced = task(&tasks, "cargo test");
    assert_eq!(replaced.source, Source::File);
    assert_eq!(replaced.command, argv(&["cargo", "nextest", "run"]));
}

#[test]
fn each_group_has_at_most_one_default() {
    // Marked ones win over a detected name, the first over later ones, in
    // each group alone.
    let (tasks, diagnostics) = FakeFiles::with(&[
        (
            ".tenmoku/tasks.toml",
            r#"
            [[task]]
            label = "first build"
            command = "true"
            group = "build"
            default = true

            [[task]]
            label = "second build"
            command = "true"
            group = "build"
            default = true

            [[task]]
            label = "first run"
            command = "true"
            group = "run"
            default = true

            [[task]]
            label = "second run"
            command = "true"
            group = "run"
            default = true

            [[task]]
            label = "unmarked test"
            command = "true"
            group = "test"

            [[task]]
            label = "no group"
            command = "true"
            default = true
            "#,
        ),
        ("Cargo.toml", "[package]\n"),
    ])
    .load();

    let defaults: Vec<&str> = tasks
        .iter()
        .filter(|task| task.default)
        .map(|task| task.label.as_str())
        .collect();
    assert_eq!(defaults, ["first build", "first run", "cargo test"]);

    assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
    assert!(
        diagnostic_for(&diagnostics, "second build")
            .message
            .contains("first build")
    );
    assert!(
        diagnostic_for(&diagnostics, "second run")
            .message
            .contains("first run")
    );
    diagnostic_for(&diagnostics, "no group");

    // With no tasks file, Cargo's build comes before npm's, and only Cargo's
    // and npm's names count.
    let (tasks, _) = FakeFiles::with(&[
        ("Cargo.toml", "[package]\n"),
        ("package.json", r#"{ "scripts": { "build": "tsc" } }"#),
        ("Makefile", "test:\n"),
    ])
    .load();
    assert!(task(&tasks, "cargo build").default);
    assert!(!task(&tasks, "npm run build").default);
    assert!(!task(&tasks, "make test").default);

    let (tasks, _) = FakeFiles::with(&[("Makefile", "build:\ntest:\n")]).load();
    assert!(tasks.iter().all(|task| !task.default));
}
