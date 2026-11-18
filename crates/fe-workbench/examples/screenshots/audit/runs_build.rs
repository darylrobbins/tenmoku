//! A build run's detail in the Runs tool window (T1580): a finished build of
//! the demo project, recorded through the workbench's public API, selected
//! in the list, its crate waterfall shown. Four registry crates were fresh
//! (2 px lines at the start), three were rebuilt (bars in `text.muted` at
//! half alpha), and `demo`, the local crate, was rebuilt last, from 6.2 s to
//! 9.8 s, its bar in the accent, with two warnings in `src/main.rs`.
//!
//! The bottom panel's rows start at y = 360, from x = 282 to the window's
//! right edge at 1000 (`runs.rs`): the detail is beside the list, which
//! keeps two fifths, from x = 570. Its title row is 360–388, the axis
//! 388–406, and the crates' 18 px rows from 406: `demo` is the eighth,
//! 532–550, its bar 5 px down its row and 8 px tall. A row has 12 px of
//! padding each side, then the name's 120 px, the track, the counts' 56 px
//! and the duration's 48 px, 8 px apart: the track is 570 + 12 + 120 + 8 =
//! 710 to 1000 - 12 - 48 - 8 - 56 - 8 = 868, 158 px for 9.8 s, so `demo`'s
//! bar is 710 + 158 × 6.2 / 9.8 ≈ 810 to the track's end.

use std::{path::Path, time::Duration};

use fe_runlog::{BuildMessage, BuildTimeline, CrateSpan};
use fe_workbench::runs::RunEnd;

use crate::{
    audit_scenes::{DEFAULT, Step, Surface, fail, idle, run},
    token_probe::{Probe, probe},
};

pub(crate) const SURFACE: Surface = Surface {
    name: "runs_build",
    steps: |root| vec![record(root), run("runs.show"), select(), idle(), idle()],
    probes: PROBES,
    ..DEFAULT
};

/// The middle of crate row `ix`'s mark.
const fn mark_y(ix: usize) -> f32 {
    406. + 18. * ix as f32 + 9.
}

const PROBES: &[Probe] = &[
    // `demo`'s bar, in the accent, near its end.
    probe(850., mark_y(7), "accent.base"),
    // Its row's track before the bar: the panel's ground.
    probe(760., mark_y(7), "surface.panel"),
    // The chosen segment, Timeline, on its pressed ground: near its top
    // left corner, inside the outline and clear of its label (the switch
    // is 853–988, its segments 364–386).
    probe(DETAIL_RIGHT - 141., 367., "state.selected_inactive"),
    // The panel below the last row.
    probe(640., 565., "surface.panel"),
];

/// The detail's right edge: the window's.
const DETAIL_RIGHT: f32 = 1000.;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn span(name: &str, start: u64, end: u64, fresh: bool, dir: Option<&Path>) -> CrateSpan {
    CrateSpan {
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        start: ms(start),
        end: ms(end),
        fresh,
        local: dir.is_some(),
        dir: dir.map(Path::to_owned),
        running: false,
    }
}

fn warning(line: usize, text: &str) -> BuildMessage {
    BuildMessage {
        package: String::new(),
        level: "warning".to_owned(),
        file: Some("src/main.rs".to_owned()),
        line: Some(line),
        column: Some(9),
        rendered: format!("warning: {text}\n --> src/main.rs:{line}:9"),
    }
}

/// Records the build of the project at `root`, whose crate `demo` is.
fn record(root: &Path) -> Step {
    let root = root.to_owned();
    Box::new(move |workbench, _, cx| {
        let timeline = BuildTimeline {
            spans: vec![
                span("cfg-if", 10, 10, true, None),
                span("itoa", 12, 12, true, None),
                span("memchr", 14, 14, true, None),
                span("ryu", 16, 16, true, None),
                span("serde_core", 120, 2400, false, None),
                span("serde", 2400, 4100, false, None),
                span("serde_json", 4100, 6200, false, None),
                span("demo", 6200, 9800, false, Some(&root)),
            ],
            messages: vec![
                warning(7, "unused variable: `counts`"),
                warning(8, "unused variable: `line`"),
            ],
            warnings: 2,
            errors: 0,
            success: true,
        };
        let output = vec![
            "   Compiling serde_core v1.0.0".to_owned(),
            "   Compiling serde v1.0.0".to_owned(),
            "   Compiling serde_json v1.0.0".to_owned(),
            format!("   Compiling demo v0.1.0 ({})", root.display()),
            "warning: unused variable: `counts`".to_owned(),
            "    Finished `dev` profile [unoptimized + debuginfo] target(s) in 9.80s".to_owned(),
        ];
        workbench.record_build_run("cargo build", timeline, output, RunEnd::Passed, cx);
    })
}

/// Selects the newest run, the build, in the Runs list.
fn select() -> Step {
    Box::new(|workbench, _, cx| {
        let newest = workbench.runs().read(cx).list().next().map(|run| run.id);
        let selected = newest.is_some_and(|run| workbench.select_run(run, cx));
        if !selected {
            fail("no build run to select in the Runs list".to_owned());
        }
    })
}
