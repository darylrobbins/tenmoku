//! A build run's detail in the Runs tool window
//! ([`crate::build_timeline_view`]): the crate waterfall's layout as a pure
//! function, its rows as drawn (fresh lines, rebuilt bars, the local crate in
//! the accent, counts and accessible names), the axis's steps, the Timeline |
//! Output switch, a crate's row selecting its messages in the Problems panel,
//! and a running build's bars growing. The builds recorded here ran nowhere
//! ([`Workbench::record_build_run`]); those that run are a fake `cargo` on
//! the `PATH` they're given.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_runlog::{BuildMessage, BuildTimeline, CrateSpan};
use fe_ui::ActiveTheme as _;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext, px};
use kiln_l10n::tr;
use kiln_shell::DockView as _;

use crate::{
    Launch, Workbench,
    build_timeline_view::{
        BuildTimelineView, CrateRow, FRESH_WIDTH, LABEL_GAP, LIVE_TICK, MIN_BAR_WIDTH, Mark,
        OTHER_ALPHA, Tab, Tone, crate_counts, layout, mark_selector, row_selector, tick_label,
        tick_step,
    },
    dock::problems_panel::ShownRow,
    runs::{RunEnd, RunId},
    task_commands::TestFiles,
    task_defs::ProjectFiles,
    test_runner::RunnerPath,
    tests::{open, test_runner::install_fake_cargo},
};

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

/// A crate `name` that started at `start` and ended at `end`: local if it
/// has a folder.
fn span(name: &str, start: Duration, end: Duration, fresh: bool, dir: Option<&Path>) -> CrateSpan {
    CrateSpan {
        name: name.to_owned(),
        version: "1.0.0".to_owned(),
        start,
        end,
        fresh,
        local: dir.is_some(),
        dir: dir.map(Path::to_owned),
        running: false,
    }
}

fn message(level: &str, file: &str, line: usize, text: &str) -> BuildMessage {
    BuildMessage {
        package: String::new(),
        level: level.to_owned(),
        file: Some(file.to_owned()),
        line: Some(line),
        column: Some(1),
        rendered: format!("{level}: {text}\n --> {file}:{line}:1"),
    }
}

/// A build of `app` (local, in `dir`) on `serde` (rebuilt) and `itoa`
/// (fresh): ten seconds long, `serde` first, then `app`.
fn timeline(dir: &Path) -> BuildTimeline {
    BuildTimeline {
        spans: vec![
            span("app", secs(6), secs(10), false, Some(dir)),
            span("itoa", ms(0), ms(0), true, None),
            span("serde", ms(500), secs(6), false, None),
        ],
        messages: vec![
            message("warning", "src/main.rs", 2, "unused variable: `x`"),
            message("warning", "src/main.rs", 3, "unused variable: `y`"),
            message("error", "src/main.rs", 4, "mismatched types"),
        ],
        warnings: 2,
        errors: 1,
        success: false,
    }
}

const MAIN: &str = "fn main() {\n    let x = 1;\n    let y = 2;\n    let z: u32 = \"z\";\n}\n";

/// The output a recorded build's Output view shows.
fn output() -> Vec<String> {
    vec![
        "   Compiling serde v1.0.0".to_owned(),
        "   Compiling app v1.0.0 (/w/app)".to_owned(),
        "warning: unused variable: `x`".to_owned(),
    ]
}

/// The disk.
struct Disk;

impl ProjectFiles for Disk {
    fn read_to_string(&self, path: &Path) -> std::io::Result<String> {
        fs::read_to_string(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.is_file()
    }
}

struct Project<'a> {
    workbench: Entity<Workbench>,
    cx: &'a mut VisualTestContext,
    dir: tempfile::TempDir,
    /// The fake `cargo`'s folder, first on the runs' `PATH`.
    bin: tempfile::TempDir,
}

/// A trusted workbench on package `app` in a temporary folder, its
/// `src/main.rs` open ([`MAIN`]), `cargo` the fake `script` on its runs'
/// `PATH`, showing the Runs tool window.
fn project<'a>(cx: &'a mut TestAppContext, script: &str) -> Project<'a> {
    // The runs' programs are real processes, read on threads of their own.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"app\"\n").unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    let path = dir.path().join("src/main.rs");
    fs::write(&path, MAIN).unwrap();
    // Its Rust file would start rust-analyzer.
    let settings = fe_config::project_settings_path(dir.path());
    fs::create_dir_all(settings.parent().unwrap()).unwrap();
    fs::write(settings, "[lsp.servers.rust-analyzer]\nenabled = false\n").unwrap();

    let bin = tempfile::tempdir().unwrap();
    let runner_path = install_fake_cargo(bin.path(), script);
    cx.update(|cx| {
        cx.set_global(TestFiles(Arc::new(Disk)));
        cx.set_global(RunnerPath(runner_path));
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new(MAIN).into()))),
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
    };
    project.run("runs.show");
    project
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

impl Project<'_> {
    fn run(&mut self, id: &str) {
        let context = ContextStack::new().with(ContextFrame::new("workbench"));
        let run = RunCommand(Invocation::of(id).unwrap().context(context));
        self.cx
            .update(|window, cx| window.dispatch_action(Box::new(run), cx));
        draw(self.cx);
    }

    /// Records [`timeline`]'s build, failed, and selects it in the Runs list.
    fn record(&mut self) -> RunId {
        let timeline = timeline(self.dir.path());
        let end = RunEnd::Failed { code: Some(101) };
        let run = self.workbench.update(self.cx, |workbench, cx| {
            workbench.record_build_run("cargo build", timeline, output(), end, cx)
        });
        draw(self.cx);
        // Once the Runs list has it.
        let selected =
            (self.workbench).update(self.cx, |workbench, cx| workbench.select_run(run, cx));
        assert!(selected);
        draw(self.cx);
        // The track's measured, so drawn again at its width.
        draw(self.cx);
        run
    }

    fn view(&mut self) -> Entity<BuildTimelineView> {
        (self.workbench).read_with(self.cx, |workbench, cx| {
            workbench.runs_panel().read(cx).build_detail().clone()
        })
    }

    /// The waterfall's rows as last made.
    fn rows(&mut self) -> Vec<CrateRow> {
        let view = self.view();
        let list = view.read_with(self.cx, |view, _| view.rows().clone());
        list.read_with(self.cx, |list, _| list.delegate().rows().to_vec())
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

    fn newest_run(&mut self) -> Option<crate::runs::Run> {
        let runs = (self.workbench).read_with(self.cx, |workbench, _| workbench.runs().clone());
        runs.read_with(self.cx, |runs, _| runs.list().next().cloned())
    }
}

#[test]
fn layout_scales_bars_to_the_build_length() {
    let dir = PathBuf::from("/w/app");
    let timeline = timeline(&dir);
    let width = px(400.);
    let laid = layout(&timeline, None, width);
    // A pure function: the same timeline and width, the same layout.
    assert_eq!(laid, layout(&timeline, None, width));
    assert_eq!(laid.length, secs(10));

    // In the order they started: itoa, serde, app; each by its span, in
    // proportion to the ten seconds 400 px stands for.
    let spans: Vec<usize> = laid.rows.iter().map(|row| row.span).collect();
    assert_eq!(spans, [1, 2, 0]);
    let marks: Vec<Mark> = laid.rows.iter().map(|row| row.mark).collect();
    assert_eq!(
        marks,
        [
            Mark::Line { x: px(0.) },
            Mark::Bar {
                x: px(20.),
                width: px(220.)
            },
            Mark::Bar {
                x: px(240.),
                width: px(160.)
            },
        ]
    );
    let durations: Vec<Option<Duration>> = laid.rows.iter().map(|row| row.duration).collect();
    assert_eq!(durations, [None, Some(ms(5500)), Some(secs(4))]);

    // Twice as wide, twice as far and long.
    let wide = layout(&timeline, None, px(800.));
    assert_eq!(
        wide.rows[2].mark,
        Mark::Bar {
            x: px(480.),
            width: px(320.)
        }
    );

    // A quick rebuild is still wider than a fresh line, and every mark
    // stays on the track, even at the end.
    let mut quick = timeline.clone();
    quick
        .spans
        .push(span("tiny", secs(10), secs(10), false, None));
    quick
        .spans
        .push(span("late", secs(10), secs(10), true, None));
    let laid = layout(&quick, None, width);
    let tiny = laid.rows.iter().find(|row| row.span == 3).unwrap();
    assert_eq!(
        tiny.mark,
        Mark::Bar {
            x: px(400.) - MIN_BAR_WIDTH,
            width: MIN_BAR_WIDTH
        }
    );
    let late = laid.rows.iter().find(|row| row.span == 4).unwrap();
    assert_eq!(
        late.mark,
        Mark::Line {
            x: px(400.) - FRESH_WIDTH
        }
    );

    // A running build is as long as it's been running, and the crates being
    // compiled reach the time.
    let mut running = timeline.clone();
    running.spans[0].running = true;
    let laid = layout(&running, Some(secs(20)), width);
    assert_eq!(laid.length, secs(20));
    let app = laid.rows.iter().find(|row| row.span == 0).unwrap();
    assert_eq!(
        app.mark,
        Mark::Bar {
            x: px(120.),
            width: px(280.)
        }
    );
    assert!(app.running);
    assert_eq!(app.duration, Some(secs(14)));
    // Once it's ended, a crate is never running.
    assert!(!layout(&running, None, width).rows[2].running);

    // No crates, or none that took time: a second's track.
    let empty = layout(&BuildTimeline::default(), None, width);
    assert_eq!((empty.length, empty.rows.len()), (secs(1), 0));
}

#[test]
fn axis_ticks_step_1_5_10_30_seconds_by_length() {
    for (length, step) in [
        (ms(300), 1),
        (secs(10), 1),
        (ms(10_500), 5),
        (secs(50), 5),
        (secs(51), 10),
        (secs(100), 10),
        (secs(101), 30),
        (secs(600), 30),
        (secs(3600), 30),
    ] {
        assert_eq!(tick_step(length), secs(step), "{length:?}");
    }

    // From the start to the end, a step apart, placed as the bars are.
    let timeline = BuildTimeline {
        spans: vec![span("a", ms(0), secs(40), false, None)],
        ..BuildTimeline::default()
    };
    let laid = layout(&timeline, None, px(400.));
    assert_eq!(laid.step, secs(5));
    let ticks: Vec<(Duration, f32)> = (laid.ticks.iter())
        .map(|tick| (tick.at, tick.x.as_f32()))
        .collect();
    let want: Vec<(Duration, f32)> = (0..=8).map(|n| (secs(5 * n), 50. * n as f32)).collect();
    assert_eq!(ticks, want);
    // 50 px apart, all labelled; on a track a quarter as wide, only every
    // third, so labels are LABEL_GAP apart.
    assert!(laid.ticks.iter().all(|tick| tick.labelled));
    let narrow = layout(&timeline, None, px(100.));
    let labelled: Vec<u64> = (narrow.ticks.iter())
        .filter(|tick| tick.labelled)
        .map(|tick| tick.at.as_secs())
        .collect();
    assert_eq!(labelled, [0, 15, 30]);
    assert!(px(12.5) * 3. >= LABEL_GAP);

    // Labels: seconds, then minutes and seconds.
    let label = |minutes: u64, seconds: u64| {
        tr!(
            "workbench-build-runs-tick",
            minutes = minutes,
            seconds = seconds
        )
    };
    assert_eq!(tick_label(secs(5)), label(0, 5));
    assert_eq!(tick_label(secs(90)), label(1, 30));
    assert_eq!(tick_label(secs(120)), label(2, 0));
    assert_eq!(tick_label(secs(5)), "5s");
    assert_eq!(tick_label(secs(90)), "1m 30s");
    assert_eq!(tick_label(secs(120)), "2m");
}

#[gpui::test]
fn fresh_crates_are_2px_lines(cx: &mut TestAppContext) {
    let mut project = project(cx, "#!/bin/sh\n");
    project.record();
    let rows = project.rows();
    let fresh: Vec<bool> = (rows.iter())
        .map(|row| matches!(row.layout.mark, Mark::Line { .. }))
        .collect();
    assert_eq!(fresh, [true, false, false]);
    assert_eq!(rows[0].name, "itoa");
    assert_eq!(rows[0].duration, None);

    // Drawn so: the fresh line 2 px wide, the bars as their spans have them,
    // on a track that's been measured.
    let line = project.cx.debug_bounds(mark_selector(0).leak()).unwrap();
    assert_eq!(line.size.width, FRESH_WIDTH);
    let serde = project.cx.debug_bounds(mark_selector(1).leak()).unwrap();
    let app = project.cx.debug_bounds(mark_selector(2).leak()).unwrap();
    assert!(serde.size.width > MIN_BAR_WIDTH, "{serde:?}");
    // serde takes 5.5 s of the 10, app 4.
    let ratio = serde.size.width / app.size.width;
    assert!((ratio - 5.5 / 4.).abs() < 0.01, "{ratio}");
    assert!(
        (app.left() - serde.right()).as_f32().abs() < 0.5,
        "{serde:?} {app:?}"
    );
    let duration = tr!("workbench-build-runs-duration-secs", secs = 5.5);
    assert_eq!(rows[1].duration.as_ref(), Some(&duration));
}

#[gpui::test]
fn the_local_crate_is_accent(cx: &mut TestAppContext) {
    let mut project = project(cx, "#!/bin/sh\n");
    project.record();
    let rows = project.rows();
    let tones: Vec<Tone> = rows.iter().map(|row| row.layout.tone).collect();
    assert_eq!(tones, [Tone::Other, Tone::Other, Tone::Local]);
    let colors = project.cx.update(|_, cx| cx.theme().colors);
    let view = project.view();
    let list = view.read_with(project.cx, |view, _| view.rows().clone());
    let drawn = list.read_with(project.cx, |list, _| list.delegate().drawn_marks.clone());
    let other = colors.text.muted.opacity(OTHER_ALPHA);
    assert_eq!(drawn[&0].1, other);
    assert_eq!(drawn[&1].1, other);
    assert_eq!(drawn[&2].1, colors.accent.base);

    // Its messages are counted beside it: its file is in its folder.
    let counts: Vec<(usize, usize)> = rows.iter().map(|row| (row.warnings, row.errors)).collect();
    assert_eq!(counts, [(0, 0), (0, 0), (2, 1)]);
    let dir = project.dir.path().to_owned();
    assert_eq!(
        crate_counts(&timeline(&dir), std::slice::from_ref(&dir)),
        [(2, 1), (0, 0), (0, 0)]
    );
    // Cargo's resolved folders aren't the root as opened: no crate has
    // them then.
    assert_eq!(
        crate_counts(&timeline(Path::new("/elsewhere")), &[dir]),
        [(0, 0); 3]
    );
}

#[gpui::test]
fn the_segmented_control_switches_to_output(cx: &mut TestAppContext) {
    let mut project = project(cx, "#!/bin/sh\n");
    project.record();
    let view = project.view();
    assert_eq!(
        view.read_with(project.cx, |view, _| view.tab()),
        Tab::Timeline
    );
    assert!(project.cx.debug_bounds("build-run-output").is_none());
    assert!(project.cx.debug_bounds("build-run-axis").is_some());

    let segment = format!("segment:{}", tr!("workbench-build-runs-output"));
    let segment = project.cx.debug_bounds(segment.leak()).unwrap();
    project
        .cx
        .simulate_click(segment.center(), Modifiers::none());
    draw(project.cx);
    assert_eq!(
        view.read_with(project.cx, |view, _| view.tab()),
        Tab::Output
    );
    assert!(project.cx.debug_bounds("build-run-output").is_some());
    assert!(project.cx.debug_bounds("build-run-axis").is_none());
    let shown: Vec<String> = view.read_with(project.cx, |view, _| {
        view.output().iter().map(ToString::to_string).collect()
    });
    assert_eq!(shown, output());

    let timeline = format!("segment:{}", tr!("workbench-build-runs-timeline"));
    let segment = project.cx.debug_bounds(timeline.leak()).unwrap();
    project
        .cx
        .simulate_click(segment.center(), Modifiers::none());
    draw(project.cx);
    assert_eq!(
        view.read_with(project.cx, |view, _| view.tab()),
        Tab::Timeline
    );
    assert!(project.cx.debug_bounds("build-run-axis").is_some());
}

#[gpui::test]
fn rows_have_accessible_names(cx: &mut TestAppContext) {
    let mut project = project(cx, "#!/bin/sh\n");
    project.record();
    let view = project.view();
    let list = view.read_with(project.cx, |view, _| view.rows().clone());
    let names: Vec<String> = (0..3)
        .map(|ix| {
            list.read_with(project.cx, |list, cx| {
                fe_ui::ListDelegate::row_name(list.delegate(), ix, cx).to_string()
            })
        })
        .collect();
    let row = |name: &str, state, local: &str, warnings: usize, errors: usize| {
        tr!(
            "workbench-build-runs-crate-row",
            name = name,
            state = state,
            local = local,
            warnings = warnings,
            errors = errors
        )
        .to_string()
    };
    let built = |secs: f64| {
        let duration = tr!("workbench-build-runs-duration-secs", secs = secs);
        tr!("workbench-build-runs-crate-built", duration = duration)
    };
    let want = [
        row("itoa", tr!("workbench-build-runs-crate-fresh"), "no", 0, 0),
        row("serde", built(5.5), "no", 0, 0),
        row("app", built(4.), "yes", 2, 1),
    ];
    assert_eq!(names, want);
    assert_eq!(names[0], "itoa, fresh");
    assert_eq!(names[2], "app, built in 4s, local, 2 warnings, 1 error");
    let name = list.read_with(project.cx, |list, cx| {
        fe_ui::ListDelegate::accessible_name(list.delegate(), cx)
    });
    assert_eq!(name, tr!("workbench-build-runs-crates"));
}

#[gpui::test]
fn a_crate_row_selects_its_messages_in_problems(cx: &mut TestAppContext) {
    let mut project = project(cx, "#!/bin/sh\n");
    project.record();
    let title = "cargo build".to_owned();
    let panel = (project.workbench).read_with(project.cx, |workbench, cx| {
        workbench.dock().read(cx).problems().clone()
    });
    project.wait_until("the build's problems", |project| {
        panel.read_with(project.cx, |panel, _| panel.shown_rows().len()) == 4
    });
    assert_eq!(
        panel.read_with(project.cx, |panel, _| panel.selected()),
        None
    );

    // serde has no messages: a click on its row does nothing but select it.
    let serde = project.cx.debug_bounds(row_selector(1).leak()).unwrap();
    project.cx.simulate_click(serde.center(), Modifiers::none());
    draw(project.cx);
    assert_eq!(
        panel.read_with(project.cx, |panel, _| panel.selected()),
        None
    );
    assert!(project.cx.debug_bounds("build-run-axis").is_some());

    // app's selects its first in the Problems panel, shown.
    let app = project.cx.debug_bounds(row_selector(2).leak()).unwrap();
    project.cx.simulate_click(app.center(), Modifiers::none());
    draw(project.cx);
    let (rows, selected) = panel.read_with(project.cx, |panel, _| {
        (panel.shown_rows(), panel.selected())
    });
    let first = ShownRow::Problem(
        "W",
        "unused variable: `x`".to_owned(),
        Some(title),
        "2:1".to_owned(),
    );
    assert_eq!(selected.map(|ix| rows[ix].clone()), Some(first));
    let bottom =
        (project.workbench).read_with(project.cx, |workbench, _| workbench.terminal_dock().clone());
    let shown = bottom.read_with(project.cx, |dock, _| dock.dock().rail_highlighted());
    assert_eq!(shown, Some(crate::DockPanel::Problems.id()));
}

/// A fake `cargo` that starts `app` (in the folder it runs in) after
/// `itoa`, fresh, then waits for `go` beside it before `app`'s artifact.
const GATED: &str = r#"#!/bin/sh
bin=$(dirname "$0")
here=${FAKE_CARGO_PWD:-$(pwd)}
printf '%s\n' '{"reason":"compiler-artifact","package_id":"registry+https://github.com/rust-lang/crates.io-index#itoa@1.0.11","fresh":true}'
echo "   Compiling app v0.1.0 ($here)" >&2
while [ ! -e "$bin/go" ]; do sleep 0.01; done
printf '%s\n' '{"reason":"compiler-artifact","package_id":"path+file://'"$here"'#app@0.1.0","fresh":false}'
printf '%s\n' '{"reason":"build-finished","success":true}'
"#;

#[gpui::test]
fn a_running_build_grows_its_bars(cx: &mut TestAppContext) {
    let mut project = project(cx, GATED);
    project.run("runs.build");
    project.wait_until("the build to start", |project| {
        project.newest_run().is_some()
    });
    let run = project.newest_run().unwrap().id;
    project.workbench.update(project.cx, |workbench, cx| {
        workbench.select_run(run, cx);
    });
    let view = project.view();
    project.wait_until("app to start", |project| {
        project.rows().iter().any(|row| row.layout.running)
    });
    let app = |project: &mut Project| -> (Duration, Mark) {
        let length = view.read_with(project.cx, |view, _| view.layout().unwrap().length);
        let rows = project.rows();
        let row = rows.iter().find(|row| row.name == "app").unwrap();
        assert!(row.layout.running);
        assert_eq!(row.layout.tone, Tone::Local);
        assert_eq!(
            row.a11y,
            tr!(
                "workbench-build-runs-crate-row",
                name = "app",
                state = tr!("workbench-build-runs-crate-building"),
                local = "yes",
                warnings = 0,
                errors = 0
            )
        );
        (length, row.layout.mark)
    };
    let (before, _) = app(&mut project);
    // It grows with the time, redrawn as it goes.
    project.wait_until("the waterfall to be redrawn", |project| {
        // The test executor's timers fire as its clock's moved on.
        project.cx.executor().advance_clock(LIVE_TICK);
        app(project).0 > before
    });
    // It reaches the track's end: the build's as long as it's run.
    let (_, Mark::Bar { x, width }) = app(&mut project) else {
        panic!("app is a bar");
    };
    let track = view.read_with(project.cx, |view, _| view.track_width());
    assert!(track > px(0.));
    assert!(
        ((x + width) - track).as_f32().abs() < 0.01,
        "{x:?} {width:?} {track:?}"
    );

    fs::write(project.bin.path().join("go"), "").unwrap();
    project.wait_until("the build to end", |project| {
        (project.newest_run()).is_some_and(|run| !run.status.is_running())
    });
    project.wait_until("the ended build's rows", |project| {
        let rows = project.rows();
        rows.len() == 2 && rows.iter().all(|row| !row.layout.running)
    });
    let rows = project.rows();
    // Rows are ordered by start time, and the fresh crate and the app begin
    // within the same moments, so look the app up by name.
    let app = rows.iter().find(|row| row.name == "app").unwrap();
    assert!(app.duration.is_some());
}
