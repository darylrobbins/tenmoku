//! The Runs tool window ([`crate::runs_panel`]): the workbench's runs, newest
//! first, each a row with its status icon, title, summary and age; a running
//! row's progress line; the empty state; a click or Enter opening the run's
//! task tab; and the rows' accessible names. Task sessions here are fake: no
//! process starts.

use std::{collections::BTreeMap, fs, path::Path, time::Duration, time::Instant};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_terminal_view::{TerminalView, TerminalViewEvent};
use fe_ui::{ActiveTheme as _, Role};
use gpui::{
    Bounds, Entity, Focusable as _, Hsla, Modifiers, Pixels, ScrollDelta, ScrollWheelEvent,
    TestAppContext, VisualTestContext, point, px,
};
use kiln_l10n::tr;
use kiln_shell::{DockView as _, PanelId};

use crate::{
    Launch, TerminalPanel, Workbench,
    runs::{RunEnd, RunId, RunKind, RunStatus, RunSummary, Runs},
    runs_panel::{
        EMPTY_SELECTOR, PROGRESS_HEIGHT, Row, RunsPanel, StatusIcon, UNKNOWN_PROGRESS_ALPHA,
        age_label, age_name, progress_selector, row_selector, status_name, summary_text,
    },
    task_defs::{Source, Task, TaskCommand},
    tests::open,
};

const RUNS: PanelId = PanelId("runs");
const TERMINAL: PanelId = PanelId("terminal");

/// A trusted workbench on a temporary project, focused, in an active window,
/// whose terminal panel starts no process.
fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "a\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("a\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    terminal(&workbench, cx).update(cx, |terminal, _| terminal.fake_sessions = true);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    (workbench, cx, dir)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Runs `id` as a key would in the workbench.
fn run(id: &str, cx: &mut VisualTestContext) {
    let stack = ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT));
    let run = RunCommand(Invocation::of(id).unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn runs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Runs> {
    workbench.read_with(cx, |workbench, _| workbench.runs().clone())
}

fn panel(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<RunsPanel> {
    workbench.read_with(cx, |workbench, _| workbench.runs_panel().clone())
}

/// The rows the panel lists.
fn rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Row> {
    let panel = panel(workbench, cx);
    panel.read_with(cx, |panel, cx| {
        panel.list().read(cx).delegate().rows().to_vec()
    })
}

/// The tool window the bottom dock shows, if it shows.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PanelId> {
    let bottom = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
    bottom.read_with(cx, |dock, _| dock.dock().rail_highlighted())
}

fn terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    workbench.read_with(cx, |workbench, _| workbench.terminal().clone())
}

fn views(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<Entity<TerminalView>> {
    terminal(workbench, cx).read_with(cx, |terminal, _| terminal.views().cloned().collect())
}

fn task(label: &str, cwd: &Path) -> Task {
    Task {
        label: label.to_owned(),
        command: TaskCommand::Shell("true".to_owned()),
        cwd: cwd.to_owned(),
        env: BTreeMap::new(),
        group: None,
        default: false,
        source: Source::File,
        problem_matcher: None,
    }
}

fn run_task(workbench: &Entity<Workbench>, task: Task, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        workbench.update(cx, |workbench, cx| workbench.run_task(task, window, cx));
    });
    draw(cx);
}

/// Ends `view`'s fake session.
fn exit(view: &Entity<TerminalView>, code: Option<i32>, cx: &mut VisualTestContext) {
    view.update(cx, |_, cx| cx.emit(TerminalViewEvent::Exited(code)));
    draw(cx);
}

/// Begins a run of `kind` named `title`.
fn begin(runs: &Entity<Runs>, kind: RunKind, title: &str, cx: &mut VisualTestContext) -> RunId {
    runs.update(cx, |runs, cx| runs.begin(kind, title, cx))
}

fn bounds(selector: &str, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    // `debug_bounds` takes a `&'static str`.
    cx.debug_bounds(selector.to_owned().leak())
}

/// Clicks row `ix` of the drawn list.
fn click_row(ix: usize, cx: &mut VisualTestContext) {
    let row = bounds(&row_selector(ix), cx).expect("the row is drawn");
    cx.simulate_click(row.center(), Modifiers::none());
    draw(cx);
}

fn accent(cx: &mut VisualTestContext) -> Hsla {
    cx.update(|_, cx| cx.theme().colors.accent.base)
}

/// Four runs, oldest first: a build that passed, a test run that failed, a
/// script cancelled, and a test run still going, a quarter of the way.
fn four_runs(runs: &Entity<Runs>, cx: &mut VisualTestContext) -> [RunId; 4] {
    let build = begin(runs, RunKind::Build, "cargo build", cx);
    let tests = begin(runs, RunKind::Test, "cargo test -p fe-text", cx);
    let lint = begin(runs, RunKind::Script, "just lint", cx);
    let going = begin(runs, RunKind::Test, "cargo test", cx);
    runs.update(cx, |runs, cx| {
        let built = RunSummary::Build {
            crates: 12,
            warnings: 1,
            errors: 0,
        };
        runs.finish(build, RunEnd::Passed, Some(built), cx);
        let counted = RunSummary::Tests {
            passed: 40,
            failed: 2,
            total: Some(42),
        };
        runs.finish(tests, RunEnd::Failed { code: Some(101) }, Some(counted), cx);
        runs.finish(lint, RunEnd::Cancelled, None, cx);
        let so_far = RunSummary::Tests {
            passed: 3,
            failed: 0,
            total: None,
        };
        runs.update(going, Some(0.25), so_far, cx);
    });
    draw(cx);
    [build, tests, lint, going]
}

#[gpui::test]
fn lists_runs_newest_first_with_icon_title_summary_and_age(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let runs = runs(&workbench, cx);
    let [build, tests, lint, going] = four_runs(&runs, cx);
    run("runs.show", cx);
    assert_eq!(shown(&workbench, cx), Some(RUNS));

    let rows = rows(&workbench, cx);
    let listed: Vec<_> = (rows.iter())
        .map(|row| (row.id, row.title.to_string(), StatusIcon::of(row.status)))
        .collect();
    assert_eq!(
        listed,
        [
            (going, "cargo test".to_owned(), StatusIcon::Spinner),
            (lint, "just lint".to_owned(), StatusIcon::Cancelled),
            (
                tests,
                "cargo test -p fe-text".to_owned(),
                StatusIcon::Failed
            ),
            (build, "cargo build".to_owned(), StatusIcon::Passed),
        ]
    );
    let summaries: Vec<_> = rows.iter().map(|row| row.summary.to_string()).collect();
    assert_eq!(
        summaries,
        [
            tr!(
                "workbench-runs-summary-tests",
                passed = 3usize,
                failed = 0usize
            ),
            tr!("workbench-runs-summary-cancelled"),
            tr!(
                "workbench-runs-summary-tests-total",
                passed = 40usize,
                failed = 2usize,
                total = 42usize
            ),
            tr!(
                "workbench-runs-summary-build",
                crates = 12usize,
                warnings = 1usize,
                errors = 0usize
            ),
        ]
        .map(|text| text.to_string())
    );
    assert_eq!(summaries[3], "12 crates, 1 warning, 0 errors");

    // The icons: a spinner, a muted dash, a cross in the error colour, a
    // check in the success colour.
    let icons: Vec<_> = (rows.iter())
        .map(|row| StatusIcon::of(row.status).icon())
        .collect();
    assert_eq!(
        icons,
        [
            fe_ui::Icon::LoaderCircle,
            fe_ui::Icon::Minus,
            fe_ui::Icon::X,
            fe_ui::Icon::Check
        ]
    );
    let colours = cx.update(|_, cx| {
        let c = cx.theme().colors;
        (
            StatusIcon::Passed.colour(&c) == c.status.success,
            StatusIcon::Failed.colour(&c) == c.status.error,
            StatusIcon::Cancelled.colour(&c) == c.text.muted,
        )
    });
    assert_eq!(colours, (true, true, true));

    // Each is a second or so old: `now`.
    let now = Instant::now();
    for row in &rows {
        assert_eq!(age_label(row.age(now)), tr!("workbench-runs-age-now"));
    }

    // 24 px rows, one under the next.
    let first = bounds(&row_selector(0), cx).expect("drawn");
    let second = bounds(&row_selector(1), cx).expect("drawn");
    assert_eq!(first.size.height, px(24.));
    assert_eq!(second.top(), first.bottom());

    // A new run goes on top, the selection staying on its run.
    let panel = panel(&workbench, cx);
    let list = panel.read_with(cx, |panel, _| panel.list().clone());
    list.update(cx, |list, cx| list.set_selected(Some(1), cx));
    let newest = begin(&runs, RunKind::Script, "make", cx);
    draw(cx);
    let ids: Vec<_> = rows_of(&workbench, cx);
    assert_eq!(ids, [newest, going, lint, tests, build]);
    assert_eq!(list.read_with(cx, |list, _| list.selected()), Some(2));
}

fn rows_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<RunId> {
    rows(workbench, cx).iter().map(|row| row.id).collect()
}

#[gpui::test]
fn a_running_row_draws_its_progress_line(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let runs = runs(&workbench, cx);
    let known = begin(&runs, RunKind::Test, "cargo test", cx);
    let unknown = begin(&runs, RunKind::Build, "cargo build", cx);
    let done = begin(&runs, RunKind::Script, "make", cx);
    runs.update(cx, |runs, cx| {
        runs.update(known, Some(0.25), RunSummary::None, cx);
        runs.finish(done, RunEnd::Passed, None, cx);
    });
    run("runs.show", cx);
    assert_eq!(
        rows_of(&workbench, cx),
        [done, unknown, known],
        "newest first"
    );

    // The finished run draws none.
    assert_eq!(bounds(&progress_selector(0), cx), None);
    let row = bounds(&row_selector(1), cx).expect("drawn");
    let whole = bounds(&progress_selector(1), cx).expect("a line while it runs");
    let part = bounds(&progress_selector(2), cx).expect("a line while it runs");
    // Along the bottom edge, 2 px tall.
    for (line, ix) in [(whole, 1), (part, 2)] {
        let row = bounds(&row_selector(ix), cx).unwrap();
        assert_eq!(line.size.height, PROGRESS_HEIGHT);
        assert_eq!(line.bottom(), row.bottom(), "{ix}");
        assert_eq!(line.left(), row.left(), "{ix}");
    }
    // Progress unknown: the whole width; known: filled to it.
    assert_eq!(whole.size.width, row.size.width);
    let quarter = row.size.width * 0.25;
    assert!(
        (part.size.width - quarter).abs() < px(1.),
        "{part:?} of {row:?}"
    );

    // The accent, at 40% where the progress isn't known.
    let accent = accent(cx);
    let panel = panel(&workbench, cx);
    let drawn = panel.read_with(cx, |panel, cx| {
        panel.list().read(cx).delegate().drawn_lines.clone()
    });
    assert_eq!(drawn.get(&0), None);
    assert_eq!(drawn.get(&1), Some(&(1., accent.opacity(0.4))));
    assert_eq!(UNKNOWN_PROGRESS_ALPHA, 0.4);
    assert_eq!(drawn.get(&2), Some(&(0.25, accent)));

    // As it moves on, so does the line; once it ends, it's gone.
    runs.update(cx, |runs, cx| {
        runs.update(known, Some(0.75), RunSummary::None, cx)
    });
    draw(cx);
    let part = bounds(&progress_selector(2), cx).unwrap();
    assert!((part.size.width - row.size.width * 0.75).abs() < px(1.));
    runs.update(cx, |runs, cx| {
        runs.finish(known, RunEnd::Failed { code: Some(1) }, None, cx)
    });
    draw(cx);
    assert_eq!(bounds(&progress_selector(2), cx), None);
}

#[gpui::test]
fn empty_state_names_the_command(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    run("runs.show", cx);
    assert_eq!(shown(&workbench, cx), Some(RUNS));
    assert_eq!(rows(&workbench, cx), []);
    let empty = bounds(EMPTY_SELECTOR, cx).expect("the empty state is drawn");
    assert_eq!(bounds(&row_selector(0), cx), None);

    // Its hint names the command that runs a task, by its title.
    let command = fe_command::l10n::command_label("tasks.run", "");
    assert_eq!(command, "Tasks: Run Task…");
    let hint = tr!("workbench-runs-empty-hint", command = &command);
    assert!(hint.contains(command.as_ref()), "{hint}");

    // It's what shows, focused, as the panel is.
    let focused = panel(&workbench, cx);
    assert!(cx.update(|window, cx| focused.focus_handle(cx).is_focused(window)));
    assert!(empty.size.height > px(0.));

    // A run takes its place.
    begin(&runs(&workbench, cx), RunKind::Script, "make", cx);
    draw(cx);
    assert_eq!(bounds(EMPTY_SELECTOR, cx), None);
    assert!(bounds(&row_selector(0), cx).is_some());
}

#[gpui::test]
fn clicking_a_row_opens_its_task_tab(cx: &mut TestAppContext) {
    let (workbench, cx, dir) = opened(cx);
    run_task(&workbench, task("first", dir.path()), cx);
    run_task(&workbench, task("second", dir.path()), cx);
    let active = |cx: &mut VisualTestContext| {
        terminal(&workbench, cx).read_with(cx, |terminal, _| terminal.active())
    };
    assert_eq!(active(cx), 1);

    run("runs.show", cx);
    assert_eq!(shown(&workbench, cx), Some(RUNS));
    let titles: Vec<_> = (rows(&workbench, cx).iter())
        .map(|row| row.title.to_string())
        .collect();
    assert_eq!(titles, ["second", "first"]);

    // The first's row: its tab, shown and focused.
    click_row(1, cx);
    assert_eq!(shown(&workbench, cx), Some(TERMINAL));
    assert_eq!(active(cx), 0);
    let first = views(&workbench, cx).remove(0);
    assert!(cx.update(|window, cx| first.focus_handle(cx).is_focused(window)));

    // Enter on the selected row, from the keyboard.
    run("runs.show", cx);
    cx.simulate_keystrokes("up");
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert_eq!(shown(&workbench, cx), Some(TERMINAL));
    assert_eq!(active(cx), 1);

    // A rerun in the first's tab: the earlier run's row opens it still.
    exit(&first, Some(0), cx);
    run_task(&workbench, task("first", dir.path()), cx);
    assert_eq!(views(&workbench, cx).len(), 2, "the same tab");
    run("runs.show", cx);
    let titles: Vec<_> = (rows(&workbench, cx).iter())
        .map(|row| row.title.to_string())
        .collect();
    assert_eq!(titles, ["first", "second", "first"]);
    terminal(&workbench, cx).update_in(cx, |terminal, window, cx| terminal.activate(1, window, cx));
    run("runs.show", cx);
    click_row(2, cx);
    assert_eq!(shown(&workbench, cx), Some(TERMINAL));
    assert_eq!(active(cx), 0);

    // A run whose tab is closed opens nothing: the list stays.
    terminal(&workbench, cx).update_in(cx, |terminal, window, cx| {
        terminal.activate(1, window, cx);
        terminal.close_active(window, cx);
    });
    draw(cx);
    run("runs.show", cx);
    let second = (rows(&workbench, cx).iter())
        .position(|row| row.title == "second")
        .unwrap();
    click_row(second, cx);
    assert_eq!(shown(&workbench, cx), Some(RUNS));
}

#[gpui::test]
fn its_rail_button_shows_and_hides_it(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let sidebar = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    let button = |cx: &mut VisualTestContext| {
        sidebar.read_with(cx, |dock, _| {
            dock.state()
                .drawn()
                .rail_button_bounds(RUNS)
                .expect("drawn")
        })
    };
    let lit = |cx: &mut VisualTestContext| {
        sidebar.read_with(cx, |dock, _| dock.state().drawn().rail_highlighted())
    };
    let at = button(cx).center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    assert_eq!(shown(&workbench, cx), Some(RUNS));
    assert!(lit(cx).contains(&RUNS), "{:?}", lit(cx));
    let focus = panel(&workbench, cx);
    assert!(cx.update(|window, cx| focus.focus_handle(cx).contains_focused(window, cx)));

    // Lit, it hides the bottom panel, focus going to the editor.
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    assert_eq!(shown(&workbench, cx), None);
    assert!(!lit(cx).contains(&RUNS));
    let editor = workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx));
    assert!(cx.update(|window, cx| editor.contains_focused(window, cx)));
}

#[gpui::test]
fn rows_have_accessible_names(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let runs = runs(&workbench, cx);
    four_runs(&runs, cx);
    run("runs.show", cx);
    cx.update(|_, cx| fe_ui::clear_nodes(cx));
    draw(cx);

    let nodes = cx.update(|window, cx| fe_ui::nodes(window, cx));
    let list = (nodes.iter()).find(|node| node.role == Role::ListBox && node.label == "Runs");
    assert!(list.is_some(), "{nodes:#?}");
    let named: Vec<String> = (nodes.iter())
        .filter(|node| node.role == Role::ListBoxOption)
        .map(|node| node.label.to_string())
        .collect();
    let now = tr!("workbench-runs-age-name-now");
    let expected = [
        (
            "cargo test",
            RunStatus::Running {
                progress: Some(0.25),
            },
        ),
        ("just lint", RunStatus::Cancelled),
        (
            "cargo test -p fe-text",
            RunStatus::Failed { code: Some(101) },
        ),
        ("cargo build", RunStatus::Passed),
    ]
    .map(|(title, status)| {
        tr!(
            "workbench-runs-row-name",
            title = title,
            status = status_name(status),
            age = &now
        )
        .to_string()
    });
    for name in &expected {
        assert!(named.contains(name), "{name:?} in {named:#?}");
    }
    assert!(
        named.contains(&"cargo test, Running, 25% done, just now".to_owned()),
        "{named:#?}"
    );
    assert!(
        named.contains(&"cargo build, Passed, just now".to_owned()),
        "{named:#?}"
    );
}

#[test]
fn age_buckets_read_now_12s_3m_2h_4d() {
    let secs = Duration::from_secs;
    let cases = [
        (secs(0), "now", "just now"),
        (secs(4), "now", "just now"),
        (secs(12), "12s", "12 seconds ago"),
        (secs(59), "59s", "59 seconds ago"),
        (secs(60), "1m", "1 minute ago"),
        (secs(3 * 60 + 59), "3m", "3 minutes ago"),
        (secs(2 * 3600 + 59 * 60), "2h", "2 hours ago"),
        (secs(23 * 3600 + 3599), "23h", "23 hours ago"),
        (secs(24 * 3600), "1d", "1 day ago"),
        (secs(4 * 24 * 3600 + 3600), "4d", "4 days ago"),
    ];
    for (age, label, name) in cases {
        assert_eq!(age_label(age), label, "{age:?}");
        assert_eq!(age_name(age), name, "{age:?}");
    }
    // Through the catalogue, as the rows build them.
    assert_eq!(
        age_label(secs(12)),
        tr!("workbench-runs-age-seconds", count = 12u64)
    );
    assert_eq!(
        age_label(secs(3 * 60)),
        tr!("workbench-runs-age-minutes", count = 3u64)
    );
    assert_eq!(
        age_label(secs(2 * 3600)),
        tr!("workbench-runs-age-hours", count = 2u64)
    );
    assert_eq!(
        age_label(secs(4 * 86400)),
        tr!("workbench-runs-age-days", count = 4u64)
    );
    // A summary with nothing counted says what the status does.
    assert_eq!(
        summary_text(RunStatus::Failed { code: None }, RunSummary::None),
        tr!("workbench-runs-summary-no-code")
    );
}

/// The list's delegate's spinners as last drawn, by row.
fn spins(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> BTreeMap<usize, f32> {
    let panel = panel(workbench, cx);
    panel.read_with(cx, |panel, cx| {
        panel.list().read(cx).delegate().drawn_spins.clone()
    })
}

/// How many frames the spinners have asked for.
fn frames_asked(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    let panel = panel(workbench, cx);
    panel.read_with(cx, |panel, cx| {
        panel.list().read(cx).delegate().frames_asked.get()
    })
}

/// Delivers the next frame, as the display would, then draws.
fn next_frame(cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
    draw(cx);
}

#[gpui::test]
fn the_spinner_turns_while_drawn_and_stops_when_done_or_hidden(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let runs = runs(&workbench, cx);
    let going = begin(&runs, RunKind::Test, "cargo test", cx);
    run("runs.show", cx);
    let first = spins(&workbench, cx)[&0];
    let asked = frames_asked(&workbench, cx);
    assert!(asked > 0, "it asks for the next frame");

    // The frame it asked for draws it turned on, and asks for another.
    std::thread::sleep(Duration::from_millis(40));
    next_frame(cx);
    let second = spins(&workbench, cx)[&0];
    assert_ne!(first, second);
    assert!(frames_asked(&workbench, cx) > asked);

    // Over a minute old, past the ages' ticks, it turns still.
    let panel = panel(&workbench, cx);
    let list = panel.read_with(cx, |panel, _| panel.list().clone());
    list.update(cx, |list, cx| {
        list.delegate_mut().age_by(Duration::from_secs(120));
        cx.notify();
    });
    draw(cx);
    let row = rows(&workbench, cx).remove(0);
    let two_minutes = tr!("workbench-runs-age-minutes", count = 2u64);
    assert_eq!(age_label(row.age(Instant::now())), two_minutes);
    std::thread::sleep(Duration::from_millis(40));
    next_frame(cx);
    assert_ne!(spins(&workbench, cx)[&0], second);

    // Once it ends: no spinner, and no more frames once the last one asked
    // for has come.
    runs.update(cx, |runs, cx| runs.finish(going, RunEnd::Passed, None, cx));
    draw(cx);
    next_frame(cx);
    assert_eq!(spins(&workbench, cx), BTreeMap::new());
    let asked = frames_asked(&workbench, cx);
    next_frame(cx);
    next_frame(cx);
    assert_eq!(frames_asked(&workbench, cx), asked);

    // A running one in a hidden panel asks for none.
    begin(&runs, RunKind::Build, "cargo build", cx);
    draw(cx);
    assert_eq!(spins(&workbench, cx).len(), 1);
    workbench.update(cx, |workbench, cx| workbench.hide_bottom_panel(cx));
    draw(cx);
    next_frame(cx);
    let asked = frames_asked(&workbench, cx);
    next_frame(cx);
    next_frame(cx);
    assert_eq!(frames_asked(&workbench, cx), asked);

    // With reduced motion it stands still, asking for none.
    cx.update(|_, cx| cx.set_reduce_motion(true));
    run("runs.show", cx);
    next_frame(cx);
    assert_eq!(shown(&workbench, cx), Some(RUNS));
    assert_eq!(spins(&workbench, cx), BTreeMap::new());
    let row = rows(&workbench, cx).remove(0);
    assert_eq!(StatusIcon::of(row.status), StatusIcon::Spinner);
    let asked = frames_asked(&workbench, cx);
    next_frame(cx);
    assert_eq!(frames_asked(&workbench, cx), asked);
}

#[gpui::test]
fn a_background_update_never_scrolls_to_the_selection(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let runs = runs(&workbench, cx);
    let ids: Vec<RunId> = (0..30)
        .map(|n| begin(&runs, RunKind::Script, &format!("job {n}"), cx))
        .collect();
    run("runs.show", cx);
    let panel = panel(&workbench, cx);
    let list = panel.read_with(cx, |panel, _| panel.list().clone());
    list.update(cx, |list, cx| list.set_selected(Some(0), cx));
    draw(cx);
    let selected = bounds(&row_selector(0), cx).expect("drawn");

    // Scrolled away from the selected row, ten rows down.
    cx.simulate_event(ScrollWheelEvent {
        position: selected.center(),
        delta: ScrollDelta::Pixels(point(px(0.), px(-240.))),
        ..Default::default()
    });
    draw(cx);
    let first_drawn =
        |cx: &mut VisualTestContext| (0..32).find(|ix| bounds(&row_selector(*ix), cx).is_some());
    let top = first_drawn(cx);
    assert!(top > Some(0), "{top:?}");
    let still = |cx: &mut VisualTestContext| {
        let selected = list.read_with(cx, |list, _| list.selected());
        let at = selected.map(|ix| rows(&workbench, cx)[ix].id);
        (selected, at, first_drawn(cx))
    };

    // Another run moving on, then ending, leaves the list where it is.
    runs.update(cx, |runs, cx| {
        runs.update(ids[5], Some(0.5), RunSummary::None, cx)
    });
    draw(cx);
    assert_eq!(still(cx), (Some(0), Some(ids[29]), top));
    runs.update(cx, |runs, cx| runs.finish(ids[3], RunEnd::Passed, None, cx));
    draw(cx);
    assert_eq!(still(cx), (Some(0), Some(ids[29]), top));

    // A new run above it: the selection stays on its run, still unscrolled.
    begin(&runs, RunKind::Script, "newest", cx);
    draw(cx);
    assert_eq!(still(cx), (Some(1), Some(ids[29]), top));
    assert_eq!(bounds(&row_selector(1), cx), None);
}

#[gpui::test]
fn vim_keys_leave_the_panel_empty_or_not(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    cx.update(|_, cx| fe_gpui::set_vim_enabled(cx, true));
    draw(cx);
    let panel = panel(&workbench, cx);
    for populated in [false, true] {
        if populated {
            begin(&runs(&workbench, cx), RunKind::Script, "make", cx);
            draw(cx);
        }
        for keys in ["escape", "ctrl-w k", "ctrl-w p", "ctrl-k"] {
            run("runs.show", cx);
            cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
            draw(cx);
            assert_eq!(shown(&workbench, cx), Some(RUNS));
            let in_panel = cx.update(|window, cx| panel.focus_handle(cx).is_focused(window));
            assert!(in_panel, "{keys}, populated: {populated}");
            cx.simulate_keystrokes(keys);
            draw(cx);
            let editor = workbench.read_with(cx, |workbench, cx| workbench.focus_handle(cx));
            let back = cx.update(|window, cx| editor.contains_focused(window, cx));
            assert!(back, "{keys}, populated: {populated}");
        }
    }
}
