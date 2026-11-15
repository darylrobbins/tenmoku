//! A test run's detail in the Runs tool window ([`crate::test_run_view`]):
//! the split bar, the test tree by module, each test's last 8 results as
//! bars with a flaky test's outlined in kiseto, the rows' glyphs and
//! accessible names, and `runs.rerun_failed`. The runs recorded here ran
//! nowhere ([`Workbench::record_test_run`]); those that run are a fake
//! `cargo` (`tests/fixtures/fake_cargo_test.sh`) on the `PATH` they're given.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_runlog::{Flaky, History, Outcome, TestId};
use fe_ui::{ActiveTheme as _, Icon};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln_l10n::tr;

use crate::{
    Launch, Workbench,
    run_process::RunSpec,
    runs::{Run, RunId, RunKind, RunSummary},
    runs_panel::RunsPanel,
    task_commands::TestFiles,
    task_defs::ProjectFiles,
    test_run_view::{
        BAR_COUNT, Bar, Glyph, RowKey, TestRunView, TreeRow, bars, flaky_outline, split,
        tree_row_selector,
    },
    test_runner::{CargoBinaries, CargoTarget, filterset, reruns, unknown_target},
    test_runner::{RunnerPath, TestOutcome, TestResult, TestTrack, Tracks, set_history_dir},
    tests::open,
};

/// The disk.
struct Disk;

impl ProjectFiles for Disk {
    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.is_file()
    }
}

const LIB: &str = "pub fn add(a: u32, b: u32) -> u32 {\n    a + b\n}\n";

struct Project<'a> {
    workbench: Entity<Workbench>,
    cx: &'a mut VisualTestContext,
    dir: tempfile::TempDir,
    /// The fake `cargo`'s folder, first on the runs' `PATH`.
    bin: tempfile::TempDir,
    /// The state directory the test histories go under.
    state: tempfile::TempDir,
}

/// A trusted workbench on package `p` in a temporary folder, the fake
/// `cargo` (`tests/fixtures/fake_cargo_test.sh`) first on its runs'
/// `PATH`, showing the Runs tool window.
fn project(cx: &mut TestAppContext) -> Project<'_> {
    project_with(cx, Layout::Single)
}

/// What a test's project is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// Package `p`, with `src/lib.rs` open and the fake `cargo` of
    /// `tests/fixtures/fake_cargo_test.sh`, which names no targets.
    Single,
    /// A workspace of packages `p` and `q`, and a root package `root` if
    /// `root_package`, with `p/src/util.rs` open and [`FILTERING`] for its
    /// `cargo`.
    Workspace { root_package: bool },
}

/// `p/src/util.rs`.
const UTIL: &str = "#[cfg(test)]\nmod tests {}\n";

/// A fake `cargo` that picks what it runs as cargo and nextest do, for a
/// workspace of packages `p` and `q`, and `root` if the root manifest has a
/// `[package]`:
///
/// - `p`'s library (`src/lib.rs`, executable `p-1111…`): `util::tests::adds`
///   and `util::tests::shared`;
/// - `p`'s binary `p` (`src/cli.rs`, `p-2222…`), a library's namesake:
///   `util::tests::shared`;
/// - `p`'s integration test `my-more` (`checks/more.rs`, `my_more-3333…`):
///   `util::tests::shared` and `other`;
/// - `q`'s integration test `my-more` (`tests/more.rs`, `my_more-4444…`), a
///   namesake of `p`'s: `util::tests::shared`;
/// - `root`'s library (`src/lib.rs`, `root-5555…`): `util::tests::shared`.
///
/// It records its arguments, a run a line, in `args` beside it. `-p` picks
/// a package; `--workspace`, or no `-p` in a virtual workspace, all of
/// them; else the root package. `--lib`, `--bin <name>` and `--test
/// <name>` pick targets. Name filters, before `--` and after it, pick the
/// tests whose names have them in, or, with `--exact` after `--`, whose
/// names they are; nextest's `--exact` reaches only those after `--`.
/// Under nextest, `-E` picks the tests its `(binary_id(=…) & test(=…))`
/// terms name. With `--message-format json-render-diagnostics` it prints a
/// `compiler-artifact` message for each test executable first. A test
/// fails if `fail/<executable> <name>` is beside it, its captured output
/// `output` beside it, if that's there. With `color` beside
/// it, its `Running` lines are coloured; with `gate`, it waits for `go`
/// after a binary's first result.
const FILTERING: &str = r#"#!/bin/sh
bin=$(dirname "$0")
echo "$*" >> "$bin/args"
nextest=; [ "$1" = "nextest" ] && nextest=1
root=; grep -q '^\[package\]' Cargo.toml 2>/dev/null && root=1
color=; [ -e "$bin/color" ] && color=1
json=; prev=; after=; exact=; before=; names=; targets=; expr=; package=; workspace=
for arg in "$@"; do
    if [ -n "$after" ]; then
        if [ "$arg" = "--exact" ]; then exact=1; else names="$names<$arg>"; fi
    elif [ "$arg" = "--" ]; then after=1
    elif [ "$prev" = "-p" ]; then package=$arg
    elif [ "$prev" = "--bin" ]; then targets="$targets<bin:$arg>"
    elif [ "$prev" = "--test" ]; then targets="$targets<test:$arg>"
    elif [ "$prev" = "-E" ]; then expr=$arg
    elif [ "$prev" = "--message-format" ]; then [ "$arg" = json-render-diagnostics ] && json=1
    elif [ "$arg" = "--lib" ]; then targets="$targets<lib>"
    elif [ "$arg" = "--workspace" ]; then workspace=1
    else
        case "$arg" in
        -*|test|nextest|run) ;;
        *) before="$before<$arg>";;
        esac
    fi
    prev=$arg
done
picked() {
    if [ -n "$package" ]; then [ "$package" = "$1" ]
    elif [ -n "$workspace" ] || [ -z "$root" ]; then return 0
    else [ "$1" = root ]
    fi
}
aimed() { [ -z "$targets" ] || case "$targets" in *"<$1>"*) return 0;; *) return 1;; esac; }
# Whether name filter list $1 (`<a><b>`) picks test $2, exactly if $3.
picks() {
    rest=$1
    while [ -n "$rest" ]; do
        filter=${rest#<}; filter=${filter%%>*}; rest=${rest#*>}
        if [ -n "$3" ]; then [ "$filter" = "$2" ] && return 0
        else case "$2" in *"$filter"*) return 0;; esac
        fi
    done
    return 1
}
# Whether test $2 of binary id $1 runs.
wanted() {
    if [ -n "$expr" ]; then
        case "$expr" in *"(binary_id(=$1) & test(=$2))"*) ;; *) return 1;; esac
    fi
    [ -z "$before$names" ] && return 0
    if [ -n "$nextest" ]; then
        picks "$before" "$2" "" && return 0
        picks "$names" "$2" "$exact"
    else
        picks "$before$names" "$2" "$exact"
    fi
}
artifact() { # package, target, executable
    picked "$1" || return 0
    aimed "$2" || return 0
    manifest="$PWD/$1/Cargo.toml"; package_id="path+file://$PWD/$1#0.1.0"
    if [ "$1" = root ]; then manifest="$PWD/Cargo.toml"; package_id="path+file://$PWD#root@0.1.0"; fi
    kind=${2%%:*}; name=${2#*:}; [ "$kind" = lib ] && name=$1
    printf '{"reason":"compiler-artifact","package_id":"%s","manifest_path":"%s","target":{"kind":["%s"],"name":"%s","test":true},"profile":{"test":true},"executable":"%s/target/debug/deps/%s","fresh":true}\n' "$package_id" "$manifest" "$kind" "$name" "$PWD" "$3"
}
failed=
binary() { # package, target, executable, nextest id, source, tests…
    picked "$1" || return 0
    aimed "$2" || return 0
    exe=$3; id=$4; source=$5; shift 5
    count=0
    for test in "$@"; do wanted "$id" "$test" && count=$((count + 1)); done
    if [ -z "$nextest" ]; then
        if [ -n "$color" ]; then
            printf '\033[1m\033[92m     Running\033[0m %s (target/debug/deps/%s)\n' "$source" "$exe" >&2
        else
            echo "     Running $source (target/debug/deps/$exe)" >&2
        fi
        echo "running $count tests"
    fi
    first=1; failed_here=
    for test in "$@"; do
        wanted "$id" "$test" || continue
        outcome=ok
        if [ -e "$bin/fail/$exe $test" ]; then
            outcome=FAILED; failed=1; failed_here="$failed_here $test"
        fi
        if [ -n "$nextest" ]; then
            event=ok; [ "$outcome" = FAILED ] && event=failed
            printf '{"type":"test","event":"started","name":"%s$%s"}\n' "$id" "$test"
            printf '{"type":"test","event":"%s","name":"%s$%s","exec_time":0.001}\n' "$event" "$id" "$test"
        else
            echo "test $test ... $outcome"
        fi
        if [ -n "$first" ] && [ -e "$bin/gate" ]; then
            while [ ! -e "$bin/go" ]; do sleep 0.01; done
        fi
        first=
    done
    if [ -z "$nextest" ] && [ -n "$failed_here" ]; then
        echo ''; echo 'failures:'; echo ''
        for test in $failed_here; do
            echo "---- $test stdout ----"
            if [ -e "$bin/output" ]; then cat "$bin/output"; fi
            echo ''
        done
        echo 'failures:'
        for test in $failed_here; do echo "    $test"; done
        echo ''
    fi
    [ -z "$nextest" ] && echo 'test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s'
    return 0
}
each() {
    "$1" p lib p-1111111111111111 p 'unittests src/lib.rs' util::tests::adds util::tests::shared
    "$1" p bin:p p-2222222222222222 p::bin/p 'unittests src/cli.rs' util::tests::shared
    "$1" p test:my-more my_more-3333333333333333 p::my-more 'checks/more.rs' util::tests::shared other
    "$1" q test:my-more my_more-4444444444444444 q::my-more 'tests/more.rs' util::tests::shared
    if [ -n "$root" ]; then
        "$1" root lib root-5555555555555555 root 'unittests src/lib.rs' util::tests::shared
    fi
}
[ -n "$json" ] && [ -z "$nextest" ] && each artifact
each binary
[ -n "$failed" ] && exit 101
exit 0
"#;

/// A trusted workbench on a temporary project laid out as `layout` says,
/// its fake `cargo` first on its runs' `PATH`, showing the Runs tool
/// window.
fn project_with(cx: &mut TestAppContext, layout: Layout) -> Project<'_> {
    // The runs' programs are real processes, read on threads of their own.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let at = |path: &str| dir.path().join(path);
    let (path, text) = match layout {
        Layout::Single => {
            fs::write(at("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
            fs::create_dir(at("src")).unwrap();
            (at("src/lib.rs"), LIB)
        }
        Layout::Workspace { root_package } => {
            let mut manifest = "[workspace]\nmembers = [\"p\", \"q\"]\n".to_owned();
            if root_package {
                manifest.push_str("\n[package]\nname = \"root\"\n");
                fs::create_dir(at("src")).unwrap();
                fs::write(at("src/lib.rs"), "").unwrap();
            }
            fs::write(at("Cargo.toml"), manifest).unwrap();
            for package in ["p", "q"] {
                fs::create_dir_all(at(&format!("{package}/src"))).unwrap();
                let manifest = format!("[package]\nname = \"{package}\"\n");
                fs::write(at(&format!("{package}/Cargo.toml")), manifest).unwrap();
            }
            fs::write(at("p/src/lib.rs"), "pub mod util;\n").unwrap();
            (at("p/src/util.rs"), UTIL)
        }
    };
    fs::write(&path, text).unwrap();
    // Its Rust file would start rust-analyzer.
    let settings = fe_config::project_settings_path(dir.path());
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(settings, "[lsp.servers.rust-analyzer]\nenabled = false\n").unwrap();

    let bin = tempfile::tempdir().unwrap();
    let fake = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_cargo_test.sh");
    let cargo = bin.path().join("cargo");
    match layout {
        Layout::Workspace { .. } => fs::write(&cargo, FILTERING).unwrap(),
        Layout::Single => drop(fs::copy(fake, &cargo).unwrap()),
    }
    executable(&cargo);
    fs::create_dir(bin.path().join("fail")).unwrap();
    let path_dirs = [bin.path(), Path::new("/usr/bin"), Path::new("/bin")];
    let runner_path = std::env::join_paths(path_dirs).unwrap();
    let state = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        cx.set_global(TestFiles(Arc::new(Disk)));
        cx.set_global(RunnerPath(runner_path));
        set_history_dir(cx, state.path().to_owned());
    });

    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new(text).into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    let mut project = Project {
        workbench,
        cx,
        dir,
        bin,
        state,
    };
    project.run("runs.show");
    project
}

#[cfg(unix)]
fn executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(not(unix))]
fn executable(_: &Path) {}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn result(binary: &str, name: &str, outcome: TestOutcome, secs: Option<f64>) -> TestResult {
    TestResult {
        id: TestId::new(binary, name),
        outcome,
        output: String::new(),
        secs,
    }
}

fn track(last: &[Outcome]) -> TestTrack {
    let mut history = History::in_memory();
    let id = TestId::new("", "");
    for outcome in last {
        history.record(&[(id.clone(), *outcome)]).unwrap();
    }
    TestTrack {
        last: history.last8(&id),
        flaky: history.flaky(&id),
    }
}

use Outcome::{Failed as F, Passed as P};

impl Project<'_> {
    fn run(&mut self, id: &str) {
        let context = ContextStack::new().with(ContextFrame::new("workbench"));
        let run = RunCommand(Invocation::of(id).unwrap().context(context));
        self.cx
            .update(|window, cx| window.dispatch_action(Box::new(run), cx));
        draw(self.cx);
    }

    fn root(&mut self) -> PathBuf {
        (self.workbench).read_with(self.cx, |workbench, _| {
            workbench.root().expect("a root").to_owned()
        })
    }

    /// The workbench's runs, newest first.
    fn runs(&mut self) -> Vec<Run> {
        let runs = (self.workbench).read_with(self.cx, |workbench, _| workbench.runs().clone());
        runs.read_with(self.cx, |runs, _| runs.list().cloned().collect())
    }

    fn panel(&mut self) -> Entity<RunsPanel> {
        (self.workbench).read_with(self.cx, |workbench, _| workbench.runs_panel().clone())
    }

    fn view(&mut self) -> Entity<TestRunView> {
        let panel = self.panel();
        panel.read_with(self.cx, |panel, _| panel.detail().clone())
    }

    /// Records a finished test run of `results`, with `tracks`.
    fn record(&mut self, results: Vec<TestResult>, tracks: Tracks) -> RunId {
        let run = self.workbench.update(self.cx, |workbench, cx| {
            workbench.record_test_run("cargo test", results, tracks, cx)
        });
        draw(self.cx);
        run
    }

    /// Selects `run` in the Runs list, as a click would.
    fn select(&mut self, run: RunId) {
        let at = self.runs().iter().position(|listed| listed.id == run);
        let at = at.expect("the run is listed");
        let panel = self.panel();
        panel.update(self.cx, |panel, cx| {
            panel.list().update(cx, |list, cx| list.select(at, cx))
        });
        draw(self.cx);
    }

    /// The test tree's rows.
    fn rows(&mut self) -> Vec<TreeRow> {
        let view = self.view();
        view.read_with(self.cx, |view, cx| {
            view.tree().read(cx).delegate().rows().to_vec()
        })
    }

    /// The tree rows' labels, a module's after a `+` if it's expanded, a `-`
    /// if not, a test's indented.
    fn outline(&mut self) -> Vec<String> {
        (self.rows().into_iter())
            .map(|row| match row.expanded {
                Some(true) => format!("+ {}", row.label),
                Some(false) => format!("- {}", row.label),
                None => format!("    {}", row.label),
            })
            .collect()
    }

    /// Runs the app until `done`, which real threads bring about, for up to
    /// 60 s.
    fn wait_until(&mut self, what: &str, mut done: impl FnMut(&mut Self) -> bool) {
        let give_up = Instant::now() + Duration::from_secs(60);
        loop {
            draw(self.cx);
            if done(self) {
                return;
            }
            assert!(Instant::now() < give_up, "gave up waiting: {what}");
            thread::sleep(Duration::from_millis(5));
        }
    }

    /// Waits for `count` runs, the newest ended, and gives the newest.
    fn finished(&mut self, count: usize) -> Run {
        self.wait_until("the run to end", |project| {
            let runs = project.runs();
            runs.len() == count && !runs[0].status.is_running()
        });
        self.runs().remove(0)
    }

    /// The fake `cargo`'s arguments, a run a line.
    fn args(&self) -> Vec<String> {
        let args = fs::read_to_string(self.bin.path().join("args")).unwrap_or_default();
        args.lines().map(str::to_owned).collect()
    }

    fn toasts(&mut self) -> Vec<String> {
        let layer = (self.workbench).read_with(self.cx, |workbench, _| workbench.toasts().clone());
        layer.read_with(self.cx, |layer, _| {
            (layer.queue().shown().iter())
                .map(|entry| entry.toast.message.clone())
                .collect()
        })
    }
}

#[gpui::test]
fn split_bar_is_proportional_to_total(cx: &mut TestAppContext) {
    assert_eq!(split(3, 1, 4, Some(10)), (0.3, 0.1));
    // Until the total's known, the tests ended so far.
    assert_eq!(split(3, 1, 5, None), (0.6, 0.2));
    assert_eq!(split(0, 0, 0, None), (0., 0.));

    let mut project = project(cx);
    // A finished run: its total is its tests.
    let results = vec![
        result("p", "tests::a", TestOutcome::Passed, None),
        result("p", "tests::b", TestOutcome::Passed, None),
        result("p", "tests::c", TestOutcome::Failed, None),
        result("p", "tests::d", TestOutcome::Ignored, None),
    ];
    let run = project.record(results, Tracks::default());
    project.select(run);
    let view = project.view();
    let drawn = view.read_with(project.cx, |view, _| view.drawn_split);
    assert_eq!(drawn, Some((0.5, 0.25)));
    assert!(project.cx.debug_bounds("test-run-split").is_some());

    // A run under way: 2 passed and 1 failed of 10, the rest the track.
    let runs = (project.workbench).read_with(project.cx, |workbench, _| workbench.runs().clone());
    let going = runs.update(project.cx, |runs, cx| {
        let going = runs.begin(RunKind::Test, "cargo test --workspace", cx);
        let summary = RunSummary::Tests {
            passed: 2,
            failed: 1,
            total: Some(10),
        };
        runs.update(going, Some(0.3), summary, cx);
        going
    });
    draw(project.cx);
    project.select(going);
    let drawn = view.read_with(project.cx, |view, _| view.drawn_split);
    assert_eq!(drawn, Some((0.2, 0.1)));
    let split = project.cx.debug_bounds("test-run-split").unwrap();
    assert_eq!(split.size.height, crate::test_run_view::SPLIT_HEIGHT);

    // A run that isn't a test run shows no detail.
    let build = runs.update(project.cx, |runs, cx| {
        runs.begin(RunKind::Build, "cargo build", cx)
    });
    draw(project.cx);
    project.select(build);
    assert_eq!(view.read_with(project.cx, |view, _| view.run()), None);
    assert!(project.cx.debug_bounds("test-run-split").is_none());
}

#[gpui::test]
fn the_tree_groups_by_module_and_expands_failed_ones(cx: &mut TestAppContext) {
    let mut project = project(cx);
    let results = vec![
        result("p", "tests::adds", TestOutcome::Passed, None),
        result("p", "util::splits", TestOutcome::Passed, None),
        result("p", "tests::fails", TestOutcome::Failed, None),
        result("more", "subtracts", TestOutcome::Passed, None),
        result(
            "Doc-tests p",
            "src/lib.rs - add::sum (line 3)",
            TestOutcome::Passed,
            None,
        ),
    ];
    let run = project.record(results, Tracks::default());
    project.select(run);
    // Modules in the order their first tests came; the failed one open.
    assert_eq!(
        project.outline(),
        [
            "+ p::tests",
            "    adds",
            "    fails",
            "- p::util",
            "- more",
            "- Doc-tests p",
        ]
    );
    let count = tr!("workbench-test-runs-module-count", count = 2);
    assert_eq!(project.rows()[0].trailing.as_ref(), Some(&count));

    // A click on a closed module's row opens it; on an open one's, closes it.
    let util = project
        .cx
        .debug_bounds(tree_row_selector(3).leak())
        .unwrap();
    project.cx.simulate_click(util.center(), Modifiers::none());
    draw(project.cx);
    let tests = project
        .cx
        .debug_bounds(tree_row_selector(0).leak())
        .unwrap();
    project.cx.simulate_click(tests.center(), Modifiers::none());
    draw(project.cx);
    assert_eq!(
        project.outline(),
        [
            "- p::tests",
            "+ p::util",
            "    splits",
            "- more",
            "- Doc-tests p",
        ]
    );

    // Left closes the selected module, right opens it, from the keyboard.
    let view = project.view();
    let tree = view.read_with(project.cx, |view, _| view.tree().clone());
    project.cx.update(|window, cx| {
        tree.update(cx, |tree, cx| tree.select(4, cx));
        window.focus(&tree.focus_handle(cx), cx);
    });
    draw(project.cx);
    project.cx.simulate_keystrokes("right");
    draw(project.cx);
    assert_eq!(
        project.outline()[4..],
        ["+ Doc-tests p", "    src/lib.rs - add::sum (line 3)"]
    );
    project.cx.simulate_keystrokes("left");
    draw(project.cx);
    assert_eq!(project.outline()[4..], ["- Doc-tests p"]);

    // Selecting another run shows its modules as they start.
    let again = project.record(
        vec![result("p", "util::splits", TestOutcome::Passed, None)],
        Tracks::default(),
    );
    project.select(again);
    assert_eq!(project.outline(), ["- p::util"]);
}

#[gpui::test]
fn bars_show_the_last_eight_oldest_left(cx: &mut TestAppContext) {
    // Eight at most, the newest at the right; those not had yet empty, left.
    let empty = Bar::Empty;
    assert_eq!(bars(&[]), [empty; BAR_COUNT]);
    assert_eq!(
        bars(&[P, F]),
        [
            empty,
            empty,
            empty,
            empty,
            empty,
            empty,
            Bar::Passed,
            Bar::Failed
        ]
    );
    let ten = [F, F, P, P, P, P, P, P, P, F];
    let mut want = [Bar::Passed; BAR_COUNT];
    want[BAR_COUNT - 1] = Bar::Failed;
    assert_eq!(bars(&ten), want);

    // A real run: `tests::fails` had nine passes in the root's history, and
    // fails now; `tests::adds` had none and passes now.
    let mut project = project(cx);
    let root = project.root();
    let mut history = History::load(project.state.path(), &root);
    for _ in 0..9 {
        let fails = TestId::new("p", "tests::fails");
        history.record(&[(fails, P)]).unwrap();
    }
    project.run("runs.test_project");
    let run = project.finished(1);
    project.select(run.id);
    let view = project.view();
    let tree = view.read_with(project.cx, |view, _| view.tree().clone());
    project.wait_until("the history to be read", |project| {
        let rows = project.rows();
        let fails = rows.iter().find(|row| row.label.as_ref() == "fails");
        fails.is_some_and(|row| row.bars.unwrap().0[BAR_COUNT - 1] == Bar::Failed)
    });
    let rows = project.rows();
    let bars_of = |label: &str| {
        let row = rows.iter().find(|row| row.label.as_ref() == label).unwrap();
        row.bars.unwrap()
    };
    let mut fails = [Bar::Passed; BAR_COUNT];
    fails[BAR_COUNT - 1] = Bar::Failed;
    assert_eq!(bars_of("fails"), (fails, true));
    // Its first failure after passes reads as flaky, but its glyph says it
    // failed.
    let fails_row = rows.iter().find(|row| row.label.as_ref() == "fails");
    assert_eq!(fails_row.unwrap().glyph, Glyph::Failed);
    let mut adds = [Bar::Empty; BAR_COUNT];
    adds[BAR_COUNT - 1] = Bar::Passed;
    assert_eq!(bars_of("adds"), (adds, false));

    // Drawn: eight 3 x 10 bars a row, in celadon, shinsha and the track.
    let colors = project.cx.update(|_, cx| cx.theme().colors);
    let drawn = tree.read_with(project.cx, |tree, _| tree.delegate().drawn_bars.clone());
    let adds_row = rows.iter().position(|row| row.label.as_ref() == "adds");
    let drawn_adds = &drawn[&adds_row.unwrap()];
    let mut want = [colors.border.control; BAR_COUNT];
    want[BAR_COUNT - 1] = colors.status.success;
    assert_eq!(drawn_adds.fills, want);
    assert_eq!(drawn_adds.outline, None);
}

#[gpui::test]
fn flaky_tests_have_kiseto_outlined_bars(cx: &mut TestAppContext) {
    let mut project = project(cx);
    let flaky = TestId::new("p", "tests::flaky");
    let steady = TestId::new("p", "tests::steady");
    let tracks: Tracks = [
        (flaky.clone(), track(&[P, F, P, P, F, P])),
        (steady.clone(), track(&[P, P, P])),
    ]
    .into_iter()
    .collect();
    assert_eq!(tracks[&flaky].flaky, Some(Flaky { failed: 2, of: 6 }));
    let results = vec![
        result("p", "tests::flaky", TestOutcome::Passed, None),
        result("p", "tests::steady", TestOutcome::Passed, None),
        result("p", "tests::broken", TestOutcome::Failed, None),
    ];
    let run = project.record(results, tracks);
    project.select(run);
    let rows = project.rows();
    let at = |label: &str| {
        rows.iter()
            .position(|row| row.label.as_ref() == label)
            .unwrap()
    };
    let (flaky_row, steady_row) = (at("flaky"), at("steady"));
    assert_eq!(rows[flaky_row].glyph, Glyph::Flaky);
    assert!(rows[flaky_row].bars.unwrap().1);
    assert!(!rows[steady_row].bars.unwrap().1);

    let colors = project.cx.update(|_, cx| cx.theme().colors);
    let view = project.view();
    let tree = view.read_with(project.cx, |view, _| view.tree().clone());
    let drawn = tree.read_with(project.cx, |tree, _| tree.delegate().drawn_bars.clone());
    assert_eq!(drawn[&flaky_row].outline, Some(colors.status.warning));
    assert_eq!(drawn[&flaky_row].outline, Some(flaky_outline(&colors)));
    assert_eq!(drawn[&steady_row].outline, None);
    let fills = drawn[&flaky_row].fills;
    let (passed, failed) = (colors.status.success, colors.status.error);
    assert_eq!(fills[2..], [passed, failed, passed, passed, failed, passed]);
    // The module's glyph says it's failed; its flaky test's says flaky.
    let glyphs = tree.read_with(project.cx, |tree, _| tree.delegate().drawn_glyphs.clone());
    assert_eq!(glyphs[&0], (Icon::X, colors.status.error));
    assert_eq!(glyphs[&flaky_row], (Icon::Triangle, colors.status.warning));
}

/// The workspace's test executables (see [`FILTERING`]).
const P_LIB: &str = "p-1111111111111111";
const P_BIN: &str = "p-2222222222222222";
const P_MORE: &str = "my_more-3333333333333333";
const Q_MORE: &str = "my_more-4444444444444444";

const SHARED: &str = "util::tests::shared";

impl Project<'_> {
    /// Makes test `name` of executable `executable` fail from now on.
    fn fail(&self, executable: &str, name: &str) {
        let fail = self
            .bin
            .path()
            .join("fail")
            .join(format!("{executable} {name}"));
        fs::write(fail, "").unwrap();
    }

    /// Makes every test pass from now on.
    fn pass(&self) {
        let fail = self.bin.path().join("fail");
        fs::remove_dir_all(&fail).unwrap();
        fs::create_dir(&fail).unwrap();
    }

    /// `run`'s results: each test and how it ended, sorted.
    fn outcomes(&mut self, run: &Run) -> Vec<(TestId, TestOutcome)> {
        let results = (self.workbench).read_with(self.cx, |workbench, cx| {
            workbench.test_results(run.id, cx).unwrap()
        });
        let mut outcomes: Vec<(TestId, TestOutcome)> =
            (results.into_iter()).map(|r| (r.id, r.outcome)).collect();
        outcomes.sort_by(|a, b| (&a.0.binary, &a.0.name).cmp(&(&b.0.binary, &b.0.name)));
        outcomes
    }

    /// Waits for `count` runs, all ended.
    fn all_ended(&mut self, count: usize) -> Vec<Run> {
        self.wait_until("the runs to end", |project| {
            let runs = project.runs();
            runs.len() == count && runs.iter().all(|run| !run.status.is_running())
        });
        self.runs()
    }
}

/// `cargo test`'s flags in every run, before what picks its tests.
const CARGO: &str = "test --no-fail-fast --message-format json-render-diagnostics";

#[gpui::test]
fn rerun_failed_runs_exactly_the_failed_names(cx: &mut TestAppContext) {
    // A nextest filterset names each test by its binary and name, its
    // matchers' `\`, `)`, `,` and `/` escaped.
    let odd = TestId::new("p::bin/x", r"a::b(c,d)\e");
    assert_eq!(
        filterset(&[odd, TestId::new("p", "t")]),
        r"(binary_id(=p::bin\/x) & test(=a::b(c\,d\)\\e)) | (binary_id(=p) & test(=t))"
    );

    // A library's doc tests rerun in its package, named by its crate.
    let mut binaries = CargoBinaries::default();
    binaries.read_artifact(
        r#"{"reason":"compiler-artifact","manifest_path":"/w/a/Cargo.toml","target":{"kind":["lib"],"name":"my-lib"},"profile":{"test":false},"executable":null}"#,
    );
    let doc = CargoTarget {
        manifest: "/w/a/Cargo.toml".into(),
        options: vec!["--doc".to_owned()],
        id: None,
    };
    assert_eq!(binaries.target("Doc-tests my_lib"), Some(doc));
    assert_eq!(binaries.target("my_lib-0123456789abcdef"), None);
    // Two packages' libraries of one name: whose doc test failed can't be
    // told, so it isn't rerun.
    binaries.read_artifact(
        r#"{"reason":"compiler-artifact","manifest_path":"/w/b/Cargo.toml","target":{"kind":["lib"],"name":"my_lib"},"profile":{"test":false},"executable":null}"#,
    );
    assert_eq!(binaries.target("Doc-tests my_lib"), None);
    let base = RunSpec {
        kind: RunKind::Test,
        title: "cargo test".to_owned(),
        program: "cargo".into(),
        args: vec!["test".into()],
        cwd: "/w".into(),
        env: Vec::new(),
        merged: true,
    };
    let doc_test = TestId::new("Doc-tests my_lib", "src/lib.rs - f (line 1)");
    let failed = vec![(doc_test, Some("Doc-tests my_lib".to_owned()))];
    let refused = reruns(&base, false, failed, &binaries).map(|_| ());
    assert_eq!(refused, Err(unknown_target("Doc-tests my_lib")));

    let mut project = project_with(
        cx,
        Layout::Workspace {
            root_package: false,
        },
    );
    // Cargo's status lines coloured, as `term.color = "always"` has them.
    fs::write(project.bin.path().join("color"), "").unwrap();
    // `util::tests::shared` passes in `p`'s library and fails in its
    // namesake binary; `other` fails in `p`'s renamed integration test, in
    // `checks/`; `shared` fails in `q`'s integration test, a namesake of
    // `p`'s, where it passes.
    project.fail(P_BIN, SHARED);
    project.fail(P_MORE, "other");
    project.fail(Q_MORE, SHARED);
    project.run("runs.test_project");
    let first = project.finished(1);
    assert_eq!(
        project.args(),
        ["test --workspace --no-fail-fast --message-format json-render-diagnostics"]
    );
    assert_eq!(project.outcomes(&first).len(), 6);

    // Each target's tests are its own: named as nextest names their binary,
    // in modules, rows and histories of their own, though the library and
    // the binary are both `p` to cargo, and the integration tests both
    // `my_more`.
    project.select(first.id);
    assert_eq!(
        project.outline(),
        [
            "- p::util::tests",
            "+ p::bin/p::util::tests",
            "    shared",
            "- p::my-more::util::tests",
            "+ p::my-more",
            "    other",
            "+ q::my-more::util::tests",
            "    shared",
        ]
    );
    let row = project
        .cx
        .debug_bounds(tree_row_selector(2).leak())
        .unwrap();
    project.cx.simulate_click(row.center(), Modifiers::none());
    draw(project.cx);
    let view = project.view();
    let selected = view.read_with(project.cx, |view, _| {
        view.selected_test()
            .map(|test| (test.id.clone(), test.outcome))
    });
    assert_eq!(
        selected,
        Some((TestId::new("p::bin/p", SHARED), TestOutcome::Failed))
    );
    let root = project.root();
    let state = project.state.path().to_owned();
    project.wait_until("the run's history to be written", |_| {
        let history = History::load(&state, &root);
        history.last8(&TestId::new("q::my-more", SHARED)) == [F]
    });
    let history = History::load(&state, &root);
    assert_eq!(history.last8(&TestId::new("p", SHARED)), [P]);
    assert_eq!(history.last8(&TestId::new("p::bin/p", SHARED)), [F]);
    assert_eq!(history.last8(&TestId::new("p::my-more", SHARED)), [P]);

    // The command's title, from the catalogue.
    let title = fe_command::l10n::command_label("runs.rerun_failed", "").to_string();
    assert_eq!(title, tr!("command-runs-rerun_failed").to_string());

    // The run's failed tests: a run for each binary, each in its own
    // package and target, exactly.
    project.run("runs.rerun_failed");
    let runs = project.all_ended(4);
    let mut args = project.args()[1..].to_vec();
    args.sort();
    assert_eq!(
        args,
        [
            format!("{CARGO} -p p --bin p -- --exact {SHARED}"),
            format!("{CARGO} -p p --test my-more -- --exact other"),
            format!("{CARGO} -p q --test my-more -- --exact {SHARED}"),
        ]
    );
    let mut ran: Vec<(TestId, TestOutcome)> = Vec::new();
    for rerun in &runs[..3] {
        let outcomes = project.outcomes(rerun);
        assert_eq!(outcomes.len(), 1, "{}: {outcomes:?}", rerun.title);
        ran.extend(outcomes);
        assert_eq!(rerun.kind, RunKind::Test);
    }
    ran.sort_by(|a, b| (&a.0.binary, &a.0.name).cmp(&(&b.0.binary, &b.0.name)));
    assert_eq!(
        ran,
        [
            (TestId::new("p::bin/p", SHARED), TestOutcome::Failed),
            (TestId::new("p::my-more", "other"), TestOutcome::Failed),
            (TestId::new("q::my-more", SHARED), TestOutcome::Failed),
        ]
    );
    let mut titles: Vec<String> = runs[..3].iter().map(|run| run.title.clone()).collect();
    titles.sort();
    let titled = |target: &str| {
        let title = tr!(
            "workbench-test-runs-rerun-title-binary",
            title = &first.title,
            binary = target
        );
        title.to_string()
    };
    let want = [
        titled("p --bin p"),
        titled("p --test my-more"),
        titled("q --test my-more"),
    ];
    assert_eq!(titles, want);

    // One binary's failure: one rerun, titled for the run. Its captured
    // output says `Running` and `Doc-tests` as cargo's status lines do,
    // but not where cargo writes them: still the library's.
    project.pass();
    project.fail(P_LIB, SHARED);
    let output = "Running worker\nDoc-tests worker\n  Running unittests src/w.rs (target/debug/deps/p-9999999999999999)\n";
    fs::write(project.bin.path().join("output"), output).unwrap();
    project.run("runs.test_project");
    let library = project.finished(5);
    project.select(library.id);
    project.run("runs.rerun_failed");
    let rerun = project.finished(6);
    assert_eq!(
        project.args()[5],
        format!("{CARGO} -p p --lib -- --exact {SHARED}")
    );
    assert_eq!(
        project.outcomes(&rerun),
        [(TestId::new("p", SHARED), TestOutcome::Failed)]
    );
    let rerun_title = tr!("workbench-test-runs-rerun-title", title = &library.title);
    assert_eq!(rerun.title, rerun_title.as_ref());
    let results = (project.workbench).read_with(project.cx, |workbench, cx| {
        workbench.test_results(library.id, cx).unwrap()
    });
    let failure = results
        .iter()
        .find(|r| r.outcome == TestOutcome::Failed)
        .unwrap();
    assert_eq!(failure.id, TestId::new("p", SHARED));
    assert!(
        failure.output.contains("Running worker"),
        "{}",
        failure.output
    );
    fs::remove_file(project.bin.path().join("output")).unwrap();

    // A run with no failures: nothing runs.
    project.pass();
    project.run("runs.test_project");
    let passed = project.finished(7);
    project.select(passed.id);
    project.run("runs.rerun_failed");
    assert_eq!(project.args().len(), 7);
    let none = tr!("workbench-test-runs-rerun-no-failures");
    assert!(project.toasts().contains(&none.to_string()));
}

/// A Test File run of a member, in a workspace whose root is a package
/// too, reruns its failed test in that member, not the root package, whose
/// library is also `src/lib.rs` to cargo's `Running` lines.
#[gpui::test]
fn a_member_run_beside_a_root_package_reruns_in_the_member(cx: &mut TestAppContext) {
    let mut project = project_with(cx, Layout::Workspace { root_package: true });
    project.fail(P_LIB, SHARED);
    project.run("runs.test_file");
    let run = project.finished(1);
    assert_eq!(
        project.args(),
        ["test -p p util:: --no-fail-fast --message-format json-render-diagnostics"]
    );
    assert_eq!(project.outcomes(&run).len(), 4);
    project.run("runs.rerun_failed");
    let rerun = project.finished(2);
    assert_eq!(
        project.args()[1],
        format!("{CARGO} -p p --lib -- --exact {SHARED}")
    );
    assert_eq!(
        project.outcomes(&rerun),
        [(TestId::new("p", SHARED), TestOutcome::Failed)]
    );
}

/// A module-filtered nextest run (Test File on `p/src/util.rs`) reruns its
/// failed test alone: the filter before its `--`, which nextest's `--exact`
/// wouldn't reach, isn't kept, and a filterset picks the test by binary and
/// name.
#[gpui::test]
fn a_filtered_nextest_run_reruns_only_its_failed_test(cx: &mut TestAppContext) {
    let mut project = project_with(
        cx,
        Layout::Workspace {
            root_package: false,
        },
    );
    let config = project.dir.path().join(".config");
    fs::create_dir(&config).unwrap();
    fs::write(config.join("nextest.toml"), "[profile.default]\n").unwrap();
    let nextest = project.bin.path().join("cargo-nextest");
    fs::write(&nextest, "#!/bin/sh\n").unwrap();
    executable(&nextest);
    project.fail(P_LIB, SHARED);

    project.run("runs.test_file");
    let run = project.finished(1);
    assert_eq!(
        project.args(),
        ["nextest run -p p util:: --no-fail-fast --message-format libtest-json"]
    );
    assert_eq!(
        project.outcomes(&run),
        [
            (TestId::new("p", "util::tests::adds"), TestOutcome::Passed),
            (TestId::new("p", SHARED), TestOutcome::Failed),
            (TestId::new("p::bin/p", SHARED), TestOutcome::Passed),
            (TestId::new("p::my-more", SHARED), TestOutcome::Passed),
        ]
    );

    project.run("runs.rerun_failed");
    let rerun = project.finished(2);
    assert_eq!(
        project.args()[1],
        "nextest run -p p --no-fail-fast --message-format libtest-json \
         -E (binary_id(=p) & test(=util::tests::shared))"
    );
    assert_eq!(
        project.outcomes(&rerun),
        [(TestId::new("p", SHARED), TestOutcome::Failed)]
    );
}

/// A rerun that couldn't pick only the failed tests isn't started: one
/// whose binaries cargo named no target for (the fixture's fake prints no
/// `compiler-artifact` messages), and one kept from a folder that's no
/// longer open, whose results would go in the open folder's history.
#[gpui::test]
fn a_rerun_that_cant_be_exact_is_refused(cx: &mut TestAppContext) {
    let mut project = project(cx);
    project.run("runs.test_project");
    project.finished(1);
    project.run("runs.rerun_failed");
    draw(project.cx);
    assert_eq!(project.args().len(), 1);
    assert_eq!(project.runs().len(), 1);
    let unknown = tr!("workbench-test-runs-rerun-unknown-target", binary = "p");
    assert!(project.toasts().contains(&unknown.to_string()));

    let other = tempfile::tempdir().unwrap();
    fs::write(other.path().join("Cargo.toml"), "[package]\nname = \"q\"\n").unwrap();
    let workbench = project.workbench.clone();
    project.cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| {
            workbench.set_root(other.path().to_owned(), window, cx);
        });
    });
    draw(project.cx);
    project.run("runs.rerun_failed");
    draw(project.cx);
    assert_eq!(project.args().len(), 1);
    let other_folder = tr!("workbench-test-runs-rerun-other-folder");
    assert!(project.toasts().contains(&other_folder.to_string()));
}

/// While a run's under way: the last results of each test are read as it
/// comes, and a module opened by hand stays open, its selection kept, when
/// its first test fails; once the run's ended, its own results join them.
#[gpui::test]
fn a_live_run_reads_history_as_tests_come_and_keeps_open_modules(cx: &mut TestAppContext) {
    let mut project = project_with(
        cx,
        Layout::Workspace {
            root_package: false,
        },
    );
    let root = project.root();
    let adds = TestId::new("p", "util::tests::adds");
    let mut history = History::load(project.state.path(), &root);
    for _ in 0..3 {
        history.record(&[(adds.clone(), P)]).unwrap();
    }
    let bin = project.bin.path().to_owned();
    fs::write(bin.join("gate"), "").unwrap();
    project.fail(P_LIB, SHARED);

    project.run("runs.test_project");
    project.wait_until("the run to begin", |project| !project.runs().is_empty());
    let run = project.runs()[0].clone();
    project.select(run.id);
    let view = project.view();
    project.wait_until("adds to pass", |project| {
        let results = view.read_with(project.cx, |view, _| view.results_for_tests());
        results.iter().any(|result| result.id == adds)
    });
    // Its module has no failure yet, so it's closed: open it, and select
    // `adds`.
    assert_eq!(project.outline(), ["- p::util::tests"]);
    let module = project
        .cx
        .debug_bounds(tree_row_selector(0).leak())
        .unwrap();
    project
        .cx
        .simulate_click(module.center(), Modifiers::none());
    draw(project.cx);
    let tree = view.read_with(project.cx, |view, _| view.tree().clone());
    tree.update(project.cx, |tree, cx| tree.select(1, cx));
    draw(project.cx);

    // Its last results show while the run's going.
    let passes = |count: usize| {
        let mut bars = [Bar::Empty; BAR_COUNT];
        bars[BAR_COUNT - count..].fill(Bar::Passed);
        Some((bars, false))
    };
    project.wait_until("adds's history to be read", |project| {
        let rows = project.rows();
        (rows.iter()).any(|row| row.label.as_ref() == "adds" && row.bars == passes(3))
    });
    assert!(project.runs()[0].status.is_running());

    // `shared` fails: the module stays open, `adds` selected; once the
    // run's ended, `adds` has this pass too.
    fs::write(bin.join("go"), "").unwrap();
    project.finished(1);
    project.wait_until("the ended run's history to be read", |project| {
        let rows = project.rows();
        (rows.iter()).any(|row| row.label.as_ref() == "adds" && row.bars == passes(4))
    });
    let outline = project.outline();
    assert_eq!(outline[..3], ["+ p::util::tests", "    adds", "    shared"]);
    let selected = view.read_with(project.cx, |view, _| {
        view.selected_test().map(|test| test.id.clone())
    });
    assert_eq!(selected, Some(adds));
}

#[gpui::test]
fn rows_have_glyphs_and_accessible_names(cx: &mut TestAppContext) {
    let mut project = project(cx);
    let tracks: Tracks = [(TestId::new("p", "tests::flaky"), track(&[P, F, P]))]
        .into_iter()
        .collect();
    let results = vec![
        result("p", "tests::adds", TestOutcome::Passed, Some(0.0042)),
        result("p", "tests::fails", TestOutcome::Failed, Some(1.26)),
        result("p", "tests::flaky", TestOutcome::Passed, None),
        result("p", "tests::later", TestOutcome::Ignored, None),
    ];
    let run = project.record(results, tracks);
    project.select(run);
    let rows = project.rows();
    let glyphs: Vec<Glyph> = rows.iter().map(|row| row.glyph).collect();
    assert_eq!(
        glyphs,
        [
            Glyph::Failed,
            Glyph::Passed,
            Glyph::Failed,
            Glyph::Flaky,
            Glyph::Ignored
        ]
    );
    let colors = project.cx.update(|_, cx| cx.theme().colors);
    let view = project.view();
    let tree = view.read_with(project.cx, |view, _| view.tree().clone());
    let drawn = tree.read_with(project.cx, |tree, _| tree.delegate().drawn_glyphs.clone());
    assert_eq!(drawn[&1], (Icon::Check, colors.status.success));
    assert_eq!(drawn[&2], (Icon::X, colors.status.error));
    assert_eq!(drawn[&3], (Icon::Triangle, colors.status.warning));
    assert_eq!(drawn[&4], (Icon::Minus, colors.text.muted));

    // Durations at the right: milliseconds under a second, else seconds.
    let ms = tr!("workbench-test-runs-duration-ms", ms = 4);
    let secs = tr!("workbench-test-runs-duration-secs", secs = 1.3);
    assert_eq!(rows[1].trailing.as_ref(), Some(&ms));
    assert_eq!(rows[2].trailing.as_ref(), Some(&secs));
    assert_eq!(rows[3].trailing, None);

    // The names a screen reader hears, row by row.
    let names: Vec<String> = (0..rows.len())
        .map(|ix| {
            tree.read_with(project.cx, |tree, cx| {
                fe_ui::ListDelegate::row_name(tree.delegate(), ix, cx).to_string()
            })
        })
        .collect();
    let want = [
        tr!(
            "workbench-test-runs-module-row-expanded",
            module = "p::tests",
            passed = 2,
            failed = 1
        ),
        tr!(
            "workbench-test-runs-test-row-timed",
            name = "tests::adds",
            outcome = tr!("workbench-test-runs-outcome-passed"),
            duration = &ms,
            passed = 0,
            failed = 0
        ),
        tr!(
            "workbench-test-runs-test-row-timed",
            name = "tests::fails",
            outcome = tr!("workbench-test-runs-outcome-failed"),
            duration = &secs,
            passed = 0,
            failed = 0
        ),
        tr!(
            "workbench-test-runs-test-row",
            name = "tests::flaky",
            outcome = tr!("workbench-test-runs-outcome-passed-flaky"),
            passed = 2,
            failed = 1
        ),
        tr!(
            "workbench-test-runs-test-row",
            name = "tests::later",
            outcome = tr!("workbench-test-runs-outcome-ignored"),
            passed = 0,
            failed = 0
        ),
    ];
    let want: Vec<String> = want.iter().map(ToString::to_string).collect();
    assert_eq!(names, want);
    let tree_name = tree.read_with(project.cx, |tree, cx| {
        fe_ui::ListDelegate::accessible_name(tree.delegate(), cx)
    });
    assert_eq!(tree_name, tr!("workbench-test-runs-tree"));

    // Selecting a test's row shows the detail slot, with its name.
    assert!(project.cx.debug_bounds("test-run-detail").is_none());
    let fails = project
        .cx
        .debug_bounds(tree_row_selector(2).leak())
        .unwrap();
    project.cx.simulate_click(fails.center(), Modifiers::none());
    draw(project.cx);
    let selected = view.read_with(project.cx, |view, _| {
        view.selected_test().map(|test| test.id.clone())
    });
    assert_eq!(selected, Some(TestId::new("p", "tests::fails")));
    assert!(project.cx.debug_bounds("test-run-detail").is_some());
    let key = view.read_with(project.cx, |view, cx| {
        let tree = view.tree().read(cx);
        tree.selected()
            .map(|ix| tree.delegate().rows()[ix].key.clone())
    });
    assert_eq!(key, Some(RowKey::Test(TestId::new("p", "tests::fails"))));
}
