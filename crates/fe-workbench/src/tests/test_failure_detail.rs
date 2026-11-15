//! A failed test's detail in the Runs tool window
//! ([`crate::test_run_view`]): the assertion's left and right in blocks on
//! soft grounds with glyphs, the output alone for another failure, the flaky
//! note, and `Open test`. The runs are recorded
//! ([`Workbench::record_test_run`]); they ran nowhere.

use std::{fs, path::PathBuf, thread, time::Duration};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_runlog::{Flaky, History, Outcome, TestId};
use fe_ui::{ActiveTheme as _, Icon};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln_l10n::tr;

use crate::{
    Launch, Workbench,
    runs::RunId,
    test_run_view::{
        ACTIONS_SELECTOR, Block, FLAKY_SELECTOR, LEFT_SELECTOR, OPEN_TEST_SELECTOR,
        OUTPUT_SELECTOR, PANIC_SELECTOR, RIGHT_SELECTOR, TestRunView,
    },
    test_runner::{TestOutcome, TestResult, TestTrack, Tracks, set_history_dir},
    tests::open,
};

const SOURCE: &str = "pub fn add(a: u32, b: u32) -> u32 {\n    a + b\n}\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn adds() {\n        assert_eq!(1, 2);\n    }\n}\n";

const ASSERTION: &str = "thread 'tests::adds' panicked at src/lib.rs:9:9:\nassertion `left == right` failed: sums differ\n  left: 1\n right: 2\nnote: run with `RUST_BACKTRACE=1`\n";

struct Project<'a> {
    workbench: Entity<Workbench>,
    cx: &'a mut VisualTestContext,
    dir: tempfile::TempDir,
    _state: tempfile::TempDir,
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// A trusted workbench on package `p`, showing the Runs tool window.
fn project(cx: &mut TestAppContext) -> Project<'_> {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    let path = dir.path().join("src/lib.rs");
    fs::write(&path, SOURCE).unwrap();
    // Its Rust file would start rust-analyzer.
    let settings = fe_config::project_settings_path(dir.path());
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(settings, "[lsp.servers.rust-analyzer]\nenabled = false\n").unwrap();
    let state = tempfile::tempdir().unwrap();
    cx.update(|cx| set_history_dir(cx, state.path().to_owned()));
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of("runs.show").unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    Project {
        workbench,
        cx,
        dir,
        _state: state,
    }
}

fn failed(name: &str, output: &str) -> TestResult {
    TestResult {
        id: TestId::new("p", name),
        outcome: TestOutcome::Failed,
        output: output.to_owned(),
        secs: Some(0.01),
    }
}

fn flaky(failed: usize, of: usize) -> TestTrack {
    let mut history = History::in_memory();
    let id = TestId::new("", "");
    let results = (0..of).map(|ix| match ix < failed {
        true => Outcome::Failed,
        false => Outcome::Passed,
    });
    for outcome in results {
        history.record(&[(id.clone(), outcome)]).unwrap();
    }
    TestTrack {
        last: history.last8(&id),
        flaky: history.flaky(&id),
    }
}

impl Project<'_> {
    /// Records a run of `results`, shows it and selects its test `name`.
    fn show(&mut self, results: Vec<TestResult>, tracks: Tracks, name: &str) -> RunId {
        let run = self.workbench.update(self.cx, |workbench, cx| {
            workbench.record_test_run("cargo test", results, tracks, cx)
        });
        draw(self.cx);
        let panel = (self.workbench).read_with(self.cx, |wb, _| wb.runs_panel().clone());
        panel.update(self.cx, |panel, cx| {
            panel.list().update(cx, |list, cx| list.select(0, cx))
        });
        draw(self.cx);
        let view = self.view();
        let tree = view.read_with(self.cx, |view, _| view.tree().clone());
        let at = tree.read_with(self.cx, |tree, _| {
            let rows = tree.delegate().rows();
            rows.iter()
                .position(|row| row.label == name.rsplit("::").next().unwrap())
        });
        tree.update(self.cx, |tree, cx| tree.select(at.expect("a row"), cx));
        draw(self.cx);
        run
    }

    fn view(&mut self) -> Entity<TestRunView> {
        let panel = (self.workbench).read_with(self.cx, |wb, _| wb.runs_panel().clone());
        panel.read_with(self.cx, |panel, _| panel.detail().clone())
    }

    fn blocks(&mut self) -> Vec<Block> {
        let view = self.view();
        view.read_with(self.cx, |view, _| view.drawn_blocks.clone())
    }

    fn drawn(&mut self, selector: &'static str) -> bool {
        self.cx.debug_bounds(selector).is_some()
    }
}

#[gpui::test]
fn assertion_blocks_use_shinsha_left_celadon_right_with_glyphs(cx: &mut TestAppContext) {
    let mut project = project(cx);
    let results = vec![failed("tests::adds", ASSERTION)];
    project.show(results, Tracks::default(), "tests::adds");
    let colors = project.cx.update(|_, cx| cx.theme().colors);
    let blocks = project.blocks();
    let [left, right] = blocks.as_slice() else {
        panic!("two blocks, got {blocks:?}");
    };
    assert_eq!(
        (left.selector, left.icon, left.value.as_str()),
        (LEFT_SELECTOR, Icon::Minus, "1")
    );
    assert_eq!(
        (right.selector, right.icon, right.value.as_str()),
        (RIGHT_SELECTOR, Icon::Plus, "2")
    );
    assert_eq!(left.glyph, colors.status.error);
    assert_eq!(right.glyph, colors.status.success);
    assert_ne!(left.ground, right.ground);
    for selector in [
        LEFT_SELECTOR,
        RIGHT_SELECTOR,
        PANIC_SELECTOR,
        OUTPUT_SELECTOR,
        ACTIONS_SELECTOR,
    ] {
        assert!(project.drawn(selector), "{selector} is drawn");
    }
    assert!(!project.drawn(FLAKY_SELECTOR));
}

#[gpui::test]
fn a_non_assertion_failure_shows_output_only(cx: &mut TestAppContext) {
    let mut project = project(cx);
    let output = "thread 'tests::adds' panicked at src/lib.rs:9:9:\nboom\n";
    project.show(
        vec![failed("tests::adds", output)],
        Tracks::default(),
        "tests::adds",
    );
    assert_eq!(project.blocks(), []);
    assert!(project.drawn(OUTPUT_SELECTOR));
    for selector in [LEFT_SELECTOR, RIGHT_SELECTOR, PANIC_SELECTOR] {
        assert!(!project.drawn(selector), "{selector} isn't drawn");
    }
}

#[gpui::test]
fn the_flaky_note_reads_failed_3_of_its_last_8_runs_likely_flaky(cx: &mut TestAppContext) {
    let mut project = project(cx);
    let id = TestId::new("p", "tests::adds");
    let tracks: Tracks = [(id, flaky(3, 8))].into_iter().collect();
    project.show(
        vec![failed("tests::adds", ASSERTION)],
        tracks,
        "tests::adds",
    );
    assert!(project.drawn(FLAKY_SELECTOR));
    let note = tr!("workbench-test-runs-failure-flaky", failed = 3, of = 8);
    assert_eq!(note, "failed 3 of its last 8 runs, likely flaky");
    // The plural variants.
    let one = tr!("workbench-test-runs-failure-flaky", failed = 1, of = 1);
    assert_eq!(one, "failed 1 of its last 1 run, likely flaky");
    let two = tr!("workbench-test-runs-failure-flaky", failed = 1, of = 2);
    assert_eq!(two, "failed 1 of its last 2 runs, likely flaky");
    assert_eq!(Flaky { failed: 3, of: 8 }, flaky(3, 8).flaky.unwrap());
}

#[gpui::test]
fn open_test_jumps_to_the_function(cx: &mut TestAppContext) {
    let mut project = project(cx);
    project.show(
        vec![failed("tests::adds", ASSERTION)],
        Tracks::default(),
        "tests::adds",
    );
    assert!(project.drawn(OPEN_TEST_SELECTOR));
    let bounds = project.cx.debug_bounds(OPEN_TEST_SELECTOR).unwrap();
    project
        .cx
        .simulate_click(bounds.center(), Modifiers::none());
    let path: PathBuf = project.dir.path().join("src/lib.rs");
    for _ in 0..1000 {
        draw(project.cx);
        let active = (project.workbench).read_with(project.cx, |wb, cx| {
            let editor = wb.active_editor(cx)?;
            let editor = editor.read(cx);
            let row = editor
                .model()
                .text()
                .offset_to_point(crate::pane_toolbar::caret(editor))
                .row;
            Some((editor.model().path()?.to_owned(), row))
        });
        if let Some((opened, row)) = active {
            assert_eq!(opened, path);
            assert_eq!(row, 7, "the function's row, past its attribute");
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("the file never opened");
}
