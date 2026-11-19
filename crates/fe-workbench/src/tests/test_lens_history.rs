//! The test history on a test's lens: plain text after Run test and Debug, from the
//! root's `fe-runlog` history, read again when a run's results are saved.

use std::fs;

use fe_runlog::{History, Outcome, TestId};
use gpui::TestAppContext;
use kiln_l10n::tr;

use crate::{
    test_lens_tests::{LIB, Project, project},
    test_runner::{HistorySaved, set_history_dir},
};

/// The test `adds` of `LIB`, as a run of package `p` records it.
fn adds() -> TestId {
    TestId::new("p", "tests::adds")
}

/// A project over `LIB` whose history has `runs` of `adds`, oldest first.
fn with_runs<'a>(cx: &'a mut TestAppContext, runs: &[Outcome]) -> (Project<'a>, tempfile::TempDir) {
    let state = tempfile::tempdir().unwrap();
    cx.update(|cx| set_history_dir(cx, state.path().to_owned()));
    let project = project(cx, "src/lib.rs", LIB, true);
    let mut this = (project, state);
    record(&mut this, runs);
    this
}

/// Records `runs` of `adds` one run each, as a finished run's save does,
/// then tells the lenses.
fn record((project, state): &mut (Project, tempfile::TempDir), runs: &[Outcome]) {
    let root = project.workbench.read_with(project.cx, |workbench, _| {
        workbench.root().unwrap().to_owned()
    });
    let mut history = History::load(state.path(), &root);
    for outcome in runs {
        history.record(&[(adds(), *outcome)]).unwrap();
    }
    project.cx.update(|_, cx| {
        let saved = cx.try_global::<HistorySaved>().map_or(0, |saved| saved.0);
        cx.set_global(HistorySaved(saved + 1));
    });
}

/// The titles of the lens over the test's row (the second lens row), and
/// whether each is a click target.
fn items(project: &mut Project) -> Vec<(String, bool)> {
    let lenses = project.lenses();
    assert_eq!(lenses.len(), 2, "{lenses:?}");
    (lenses[1].items.iter())
        .map(|item| (item.title.to_string(), item.invocation.is_some()))
        .collect()
}

fn run_test() -> (String, bool) {
    (tr!("workbench-lens-run-test").to_string(), true)
}

/// The Debug item after Run test (debug_test.rs).
fn debug() -> (String, bool) {
    (tr!("workbench-debug-test-lens").to_string(), true)
}

/// Where the history's text is among a test lens's items: after Run test
/// and Debug.
const HISTORY: usize = 2;

#[gpui::test]
fn passed_in_the_last_8_runs(cx: &mut TestAppContext) {
    let mut this = with_runs(cx, &[Outcome::Passed; 8]);
    assert_eq!(
        items(&mut this.0),
        [
            run_test(),
            debug(),
            (tr!("workbench-lens-passed", count = 8).to_string(), false)
        ]
    );
    assert_eq!(
        tr!("workbench-lens-passed", count = 8).to_string(),
        "passed in the last 8 runs"
    );
    // The module's lens has none.
    let lenses = this.0.lenses();
    assert_eq!(lenses[0].items.len(), 1);
}

#[gpui::test]
fn failed_in_3_of_the_last_8_runs(cx: &mut TestAppContext) {
    use Outcome::{Failed as F, Passed as P};
    let mut this = with_runs(cx, &[P, P, F, P, F, P, P, F]);
    let shown = items(&mut this.0);
    assert_eq!(
        shown[HISTORY],
        (
            tr!("workbench-lens-failed-some", failed = 3, count = 8).to_string(),
            false
        )
    );
    assert_eq!(shown[HISTORY].0, "failed in 3 of the last 8 runs");
    // More than 8 runs: only the last 8 count.
    record(&mut this, &[F, P, P, P, P, P, P, F]);
    assert_eq!(
        items(&mut this.0)[HISTORY].0,
        "failed in 2 of the last 8 runs"
    );
}

#[gpui::test]
fn flaky_is_kiseto(cx: &mut TestAppContext) {
    use Outcome::{Failed as F, Passed as P};
    // It has failed, and its last run passed: "flaky", in the warning
    // colour, as the lens row draws it.
    let mut this = with_runs(cx, &[F, P]);
    let flaky = tr!("workbench-lens-flaky").to_string();
    assert_eq!(items(&mut this.0)[HISTORY].0, flaky);
    assert_eq!(flaky, "flaky");
    assert!(this.0.lenses()[1].items[HISTORY].warning);
    assert!(!this.0.lenses()[1].items[0].warning);
    let (drawn, warning) = this.0.editor.read_with(this.0.cx, |editor, _| {
        let drawn = editor.drawn_annotations().expect("drawn").lenses.clone();
        (drawn, editor.theme().warning)
    });
    let flaky_color = drawn.iter().find(|t| t.text == flaky).map(|t| t.color);
    assert_eq!(flaky_color, Some(warning));
    let run = drawn.iter().find(|t| t.text.starts_with('▷')).unwrap();
    assert_ne!(run.color, warning);
    // Every one of its results a failure: not "of".
    record(&mut this, &[F; 8]);
    assert_eq!(items(&mut this.0)[HISTORY].0, "failed in the last 8 runs");
}

#[gpui::test]
fn no_results_shows_nothing(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    cx.update(|cx| set_history_dir(cx, state.path().to_owned()));
    let mut project = project(cx, "src/lib.rs", LIB, true);
    assert_eq!(items(&mut project), [run_test(), debug()]);
    fs::remove_dir_all(state.path()).ok();
}

#[gpui::test]
fn it_updates_when_a_run_finishes(cx: &mut TestAppContext) {
    let mut this = with_runs(cx, &[Outcome::Passed]);
    assert_eq!(items(&mut this.0)[HISTORY].0, "passed in the last run");
    record(&mut this, &[Outcome::Failed]);
    assert_eq!(
        items(&mut this.0)[HISTORY].0,
        "failed in 1 of the last 2 runs"
    );
}

#[gpui::test]
fn another_modules_or_packages_test_of_the_same_name_is_not_this_one(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    cx.update(|cx| set_history_dir(cx, state.path().to_owned()));
    let project = project(cx, "src/lib.rs", LIB, true);
    let mut this = (project, state);
    let root = this.0.workbench.read_with(this.0.cx, |workbench, _| {
        workbench.root().unwrap().to_owned()
    });
    let mut history = History::load(this.1.path(), &root);
    // `shapes::tests::adds` of this package, and `tests::adds` of another.
    for id in [
        TestId::new("p", "shapes::tests::adds"),
        TestId::new("other", "tests::adds"),
    ] {
        history.record(&[(id, Outcome::Passed)]).unwrap();
    }
    this.0.cx.update(|_, cx| cx.set_global(HistorySaved(1)));
    assert_eq!(items(&mut this.0), [run_test(), debug()]);
    // Its own, once it has run.
    record(&mut this, &[Outcome::Passed]);
    assert_eq!(items(&mut this.0).len(), HISTORY + 1);
}
