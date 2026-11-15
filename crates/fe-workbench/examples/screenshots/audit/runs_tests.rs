//! A test run's detail in the Runs tool window (T1576): a finished run of
//! ten tests, recorded through the workbench's public API, selected in the
//! list. Eight passed, one failed and one was ignored, so the split bar is
//! eight tenths celadon, a tenth shinsha and a tenth the track. Its tree
//! has `demo::count` open, as its test failed, and the other modules
//! closed. `count::empty_input` passed and is flaky: ▲, its bars
//! outlined in kiseto. `count::counts_lines` failed after seven passes,
//! which also counts as flaky: ✕, its bars outlined.
//!
//! The bottom panel's rows start at y = 360, from x = 282 to the window's
//! right edge at 1000 (`runs.rs`): 718 px, wide enough for the detail
//! beside the list, which keeps two fifths. The detail's title row is
//! 360–384, the split bar 384–388 from 12 px inside its edges to 988, and
//! the tree's 24 px rows from 392. A test row's bars end 12 px from the
//! right, at 988: eight 3 px bars a pixel apart inside a pixel of padding
//! and a pixel of outline, 953–988.

use std::time::Duration;

use fe_runlog::{History, Outcome, TestId};
use fe_workbench::{
    runs::{RunEnd, RunKind, RunSummary},
    test_runner::{TestOutcome, TestResult, TestTrack, Tracks},
};

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, idle, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "runs_tests",
    steps: |_| vec![runs(), run("runs.show"), select(), idle(), idle()],
    probes: PROBES,
    ..DEFAULT
};

/// The split bar's middle row, and its ends inside the detail.
const SPLIT_Y: f32 = 386.;

/// The middle of tree row `ix`.
const fn row_y(ix: usize) -> f32 {
    392. + 24. * ix as f32 + 12.
}

/// Bar `ix` of a test row's eight, its middle column.
const fn bar_x(ix: usize) -> f32 {
    953. + 2. + 4. * ix as f32 + 1.5
}

const PROBES: &[Probe] = &[
    // The split bar: passed, failed, then the track for the one ignored.
    probe(620., SPLIT_Y, "status.success"),
    probe(927., SPLIT_Y, "status.error"),
    probe(968., SPLIT_Y, "border.control"),
    // `count::counts_words` (row 1) has run twice before: six empty bars,
    // then two passes.
    probe(bar_x(0), row_y(1), "border.control"),
    probe(bar_x(7), row_y(1), "status.success"),
    // `count::counts_lines` (row 2) failed this time, after seven passes.
    probe(bar_x(7), row_y(2), "status.error"),
    // `count::empty_input` (row 3) is flaky: its bars' outline is kiseto.
    probe(953.5, row_y(3), "status.warning"),
    // The panel below the tree's last row.
    probe(640., 570., "surface.panel"),
];

fn result(module: &str, name: &str, outcome: TestOutcome, ms: u64) -> TestResult {
    TestResult {
        id: TestId::new("demo", format!("{module}::{name}")),
        outcome,
        output: String::new(),
        secs: Some(Duration::from_millis(ms).as_secs_f64()),
    }
}

/// `id`'s last results as a history keeps them.
fn track(last: &[Outcome]) -> TestTrack {
    let mut history = History::in_memory();
    let id = TestId::new("", "");
    for outcome in last {
        history.record(&[(id.clone(), *outcome)]).ok();
    }
    TestTrack {
        last: history.last8(&id),
        flaky: history.flaky(&id),
    }
}

/// Makes a build and, newest, the test run.
fn runs() -> Step {
    use Outcome::{Failed as F, Passed as P};
    use TestOutcome::{Failed, Ignored, Passed};
    Box::new(|workbench, _, cx| {
        (workbench.runs().clone()).update(cx, |runs, cx| {
            let build = runs.begin(RunKind::Build, "cargo build", cx);
            let built = RunSummary::Build {
                crates: 14,
                warnings: 0,
                errors: 0,
            };
            runs.finish(build, RunEnd::Passed, Some(built), cx);
        });
        let results = vec![
            result("count", "counts_words", Passed, 4),
            result("count", "counts_lines", Failed, 12),
            result("count", "empty_input", Passed, 2),
            result("count", "unicode", Passed, 31),
            result("split", "on_spaces", Passed, 1),
            result("split", "on_tabs", Passed, 1),
            result("split", "keeps_empty", Passed, 1),
            result("parse", "args", Passed, 3),
            result("parse", "help", Passed, 2),
            result("parse", "slow_fixture", Ignored, 0),
        ];
        let id = |name: &str| TestId::new("demo", name);
        let tracks: Tracks = [
            (id("count::counts_words"), track(&[P, P])),
            (id("count::counts_lines"), track(&[P, P, P, P, P, P, P, F])),
            (id("count::empty_input"), track(&[P, F, P, P, F, P, P, P])),
            (id("count::unicode"), track(&[P, P, P, P, P])),
        ]
        .into_iter()
        .collect();
        workbench.record_test_run("cargo test -p demo", results, tracks, cx);
    })
}

/// Selects the newest run, the test run, in the Runs list.
fn select() -> Step {
    Box::new(|workbench, _, cx| {
        let newest = workbench.runs().read(cx).list().next().map(|run| run.id);
        let selected = newest.is_some_and(|run| workbench.select_run(run, cx));
        if !selected {
            fail("no test run to select in the Runs list".to_owned());
        }
    })
}
