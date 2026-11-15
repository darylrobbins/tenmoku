//! A test run's module rows ([`crate::test_run_view`]) keyed by binary and
//! module path apart: a library's `p` module `checks` and package `p`'s
//! integration target `p::checks` are two rows, though they read alike.

use std::fs;

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln_l10n::tr;

use crate::{
    Launch,
    test_run_view::{TestRunView, TreeRow, tree_row_selector},
    test_runner::{TestOutcome, TestResult, Tracks, set_history_dir},
    tests::open,
};

use fe_runlog::TestId;

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn result(binary: &str, name: &str, outcome: TestOutcome) -> TestResult {
    TestResult {
        id: TestId::new(binary, name),
        outcome,
        output: String::new(),
        secs: None,
    }
}

fn rows(view: &Entity<TestRunView>, cx: &mut VisualTestContext) -> Vec<TreeRow> {
    view.read_with(cx, |view, cx| {
        view.tree().read(cx).delegate().rows().to_vec()
    })
}

fn outline(view: &Entity<TestRunView>, cx: &mut VisualTestContext) -> Vec<String> {
    rows(view, cx)
        .into_iter()
        .map(|row| match row.expanded {
            Some(true) => format!("+ {}", row.label),
            Some(false) => format!("- {}", row.label),
            None => format!("    {}", row.label),
        })
        .collect()
}

#[gpui::test]
fn a_library_module_and_an_integration_target_keep_separate_rows(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"p\"\n").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "pub fn add() {}\n").unwrap();
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
    let show = RunCommand(Invocation::of("runs.show").unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(show), cx));
    draw(cx);

    // The library's `checks::shared` and the integration target `p::checks`'s
    // `shared`; the library's fails.
    let results = vec![
        result("p", "checks::shared", TestOutcome::Failed),
        result("p::checks", "shared", TestOutcome::Passed),
        result("p::checks", "other", TestOutcome::Passed),
    ];
    let run = workbench.update(cx, |workbench, cx| {
        workbench.record_test_run("cargo test", results, Tracks::default(), cx)
    });
    draw(cx);
    let panel = workbench.read_with(cx, |workbench, _| workbench.runs_panel().clone());
    let runs = workbench.read_with(cx, |workbench, _| workbench.runs().clone());
    let at = runs.read_with(cx, |runs, _| {
        runs.list().position(|listed| listed.id == run)
    });
    let at = at.expect("the run is listed");
    panel.update(cx, |panel, cx| {
        panel.list().update(cx, |list, cx| list.select(at, cx))
    });
    draw(cx);
    let view = panel.read_with(cx, |panel, _| panel.detail().clone());

    // Two rows: the failed library module open, the integration target not.
    let rows_now = rows(&view, cx);
    assert_eq!(rows_now.len(), 3, "{:?}", outline(&view, cx));
    assert_eq!(
        outline(&view, cx),
        ["+ p::checks", "    shared", "- p::checks"]
    );
    assert_ne!(rows_now[0].key, rows_now[2].key);
    let count = |n| Some(tr!("workbench-test-runs-module-count", count = n));
    assert_eq!(rows_now[0].trailing, count(1));
    assert_eq!(rows_now[2].trailing, count(2));
    assert_ne!(rows_now[0].glyph, rows_now[2].glyph);

    // Opening the integration target leaves the library module as it was,
    // and shows only its own tests.
    let target = cx.debug_bounds(tree_row_selector(2).leak()).unwrap();
    cx.simulate_click(target.center(), Modifiers::none());
    draw(cx);
    assert_eq!(
        outline(&view, cx),
        [
            "+ p::checks",
            "    shared",
            "+ p::checks",
            "    shared",
            "    other"
        ]
    );

    // Closing the library module leaves the target open.
    let library = cx.debug_bounds(tree_row_selector(0).leak()).unwrap();
    cx.simulate_click(library.center(), Modifiers::none());
    draw(cx);
    assert_eq!(
        outline(&view, cx),
        ["- p::checks", "+ p::checks", "    shared", "    other"]
    );
}
