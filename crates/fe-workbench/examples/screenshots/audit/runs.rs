//! The Runs tool window (T1571): five runs of the workbench's, newest
//! first, made through its public API: a test run three fifths of the way
//! (its accent line filled to there), a release build whose progress isn't
//! known (its line the whole width, faint), a script cancelled, a test run
//! that failed and a build that passed. All are moments old: `now`.
//!
//! The bottom panel is 250 px over the 24 px status bar, its tab strip
//! 326–360 and its rows 24 px each from 360; it starts at the centre's
//! left, x = 282 after the rail and the sidebar, and runs to the window's
//! right edge at 1000.

use fe_workbench::runs::{RunEnd, RunKind, RunSummary};

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, idle, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "runs",
    steps: |_| vec![runs(), run("runs.show"), idle()],
    probes: PROBES,
    ..DEFAULT
};

/// Just inside the panel's left edge, and the first row's progress line,
/// 2 px along its bottom edge (382–384).
const LEFT: f32 = 290.;
const LINE_Y: f32 = 383.;

const PROBES: &[Probe] = &[
    // The first row's line, near its start, in the accent.
    probe(LEFT + 20., LINE_Y, "accent.base"),
    // Past three fifths of the panel's width, the row's own ground.
    probe(LEFT + 0.6 * (1000. - LEFT) + 40., LINE_Y, "surface.panel"),
    // The panel, below the rows.
    probe(640., 540., "surface.panel"),
];

/// Makes the scene's runs, oldest first.
fn runs() -> Step {
    Box::new(|workbench, _, cx| {
        (workbench.runs().clone()).update(cx, |runs, cx| {
            let build = runs.begin(RunKind::Build, "cargo build", cx);
            let tests = runs.begin(RunKind::Test, "cargo test -p demo", cx);
            let lint = runs.begin(RunKind::Script, "just lint", cx);
            runs.begin(RunKind::Build, "cargo build --release", cx);
            let going = runs.begin(RunKind::Test, "cargo test", cx);
            let built = RunSummary::Build {
                crates: 14,
                warnings: 2,
                errors: 0,
            };
            runs.finish(build, RunEnd::Passed, Some(built), cx);
            let counted = RunSummary::Tests {
                passed: 41,
                failed: 1,
                total: Some(42),
            };
            runs.finish(tests, RunEnd::Failed { code: Some(101) }, Some(counted), cx);
            runs.finish(lint, RunEnd::Cancelled, None, cx);
            let so_far = RunSummary::Tests {
                passed: 25,
                failed: 0,
                total: Some(42),
            };
            runs.update(going, Some(0.6), so_far, cx);
        });
    })
}
