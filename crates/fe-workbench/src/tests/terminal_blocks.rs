//! Terminal command blocks, drawn (`docs/design/design-system.md#terminal`,
//! T1595): each command the shell marks with OSC 133 has a 2 px edge along
//! its rows down the grid's left edge, under the text, celadon for exit 0
//! and shinsha for any other code, none while it runs or without a code; and
//! its exit code and duration right-aligned on its prompt row, dropped where
//! the row has no room. `terminal.command_blocks = false` draws neither, and
//! neither moves the text: the grid starts at the session's left edge and
//! fills its width, as before blocks were drawn.

use std::{
    fs, thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::TerminalSettings;
use fe_editor::lane::LaneId;
use fe_gpui::commands::RunCommand;
use fe_terminal_view::{CommandBlock, PaintReport, TerminalOptions, TerminalView};
use fe_ui::ActiveTheme as _;
use gpui::{Bounds, Entity, Focusable as _, Hsla, Pixels, TestAppContext, VisualTestContext, px};
use kiln_l10n::tr;

use crate::tests::open;
use crate::{
    Launch, TerminalPanel, Workbench,
    terminal_panel::{block_label, duration_text},
};

/// A workbench on a temporary project, focused, in an active window.
fn opened(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, tempfile::TempDir) {
    // The pty's threads wake the terminal's event task.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "a\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("a\n").into()))),
    };
    let (workbench, cx, _) = open(cx, launch);
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, cx, dir)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn terminal(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TerminalPanel> {
    workbench.read_with(cx, |workbench, _| workbench.terminal().clone())
}

/// What the session writes last: once it shows, all of it has.
const READY: &str = "ready";

/// A prompt (`A`), then `$ ` and the command line's start (`B`).
const PROMPT: &str = "\x1b]133;A\x07$ \x1b]133;B\x07";

/// A command, `C`, its output and `D` with `end` (`;<code>`, or empty for
/// none), ending its output on a line of its own.
fn command(line: &str, output: &str, end: &str) -> String {
    format!("{PROMPT}{line}\r\n\x1b]133;C\x07{output}\x1b]133;D{end}\x07")
}

/// Opens a session that writes `blocks` and then the live prompt, shows the
/// terminal and waits for it all, then records what each frame paints.
fn session(
    workbench: &Entity<Workbench>,
    blocks: &str,
    cx: &mut VisualTestContext,
) -> Entity<TerminalView> {
    let bytes = format!("{blocks}\x1b]133;A\x07{READY} $ ");
    written(workbench, &bytes, cx)
}

/// Opens a session that writes `bytes`, which must show [`READY`], shows
/// the terminal and waits for it all, then records what each frame paints.
fn written(
    workbench: &Entity<Workbench>,
    bytes: &str,
    cx: &mut VisualTestContext,
) -> Entity<TerminalView> {
    let panel = terminal(workbench, cx);
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| panel.open_with_session(bytes, window, cx))
    });
    let stack = ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT));
    let toggle = RunCommand(Invocation::of("terminal.toggle").unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(toggle), cx));
    draw(cx);
    let view = panel.read_with(cx, |panel, _| panel.view().unwrap().clone());
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        draw(cx);
        let done = view.read_with(cx, |view, _| {
            view.terminal()
                .is_some_and(|terminal| terminal.snapshot().text().contains(READY))
        });
        if done {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for the session"
        );
        thread::sleep(Duration::from_millis(10));
    }
    view.update(cx, |view, _| view.record_paint(true));
    draw(cx);
    view
}

fn painted(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> PaintReport {
    view.read_with(cx, |view, _| view.painted().clone())
}

fn blocks(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> Vec<CommandBlock> {
    view.read_with(cx, |view, _| view.terminal().unwrap().command_blocks())
}

/// The grid's top-left corner and cell size, from the first prompt's `$`,
/// in the first column of the first row.
fn grid(
    view: &Entity<TerminalView>,
    cx: &mut VisualTestContext,
) -> (gpui::Point<Pixels>, gpui::Size<Pixels>) {
    let report = painted(view, cx);
    let dollar = (report.text.iter())
        .filter(|text| text.text == "$")
        .min_by_key(|text| text.origin.y)
        .expect("a prompt was painted");
    let style = view.read_with(cx, |view, _| view.style().clone());
    let cell = cx.update(|window, _| fe_terminal_view::cell_size(&style, window));
    (dollar.origin, cell)
}

/// The edge a block over grid `rows` has, down the grid's left edge.
fn edge(
    origin: gpui::Point<Pixels>,
    cell: gpui::Size<Pixels>,
    rows: std::ops::Range<usize>,
) -> Bounds<Pixels> {
    Bounds::new(
        gpui::point(origin.x, origin.y + cell.height * rows.start as f32),
        gpui::size(TerminalView::BLOCK_EDGE, cell.height * rows.len() as f32),
    )
}

fn colours(cx: &mut VisualTestContext) -> (Hsla, Hsla) {
    cx.update(|_, cx| {
        let status = &cx.theme().colors.status;
        (status.success, status.error)
    })
}

fn set_command_blocks(workbench: &Entity<Workbench>, on: bool, cx: &mut VisualTestContext) {
    let settings = TerminalSettings {
        command_blocks: on,
        ..TerminalSettings::default()
    };
    workbench.update(cx, |workbench, cx| {
        workbench.set_terminal_settings(&settings, cx)
    });
    draw(cx);
}

/// A block that ran `duration` and ended with `code`.
fn finished(code: Option<i32>, duration: Duration) -> CommandBlock {
    let started = Instant::now();
    CommandBlock {
        id: 0,
        prompt_row: 0,
        input_row: Some(0),
        output_start_row: Some(1),
        end_row: Some(1),
        exit_code: code,
        started,
        finished: Some(started + duration),
    }
}

#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn the_edge_is_celadon_for_exit_0_and_shinsha_otherwise(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    // `true` on row 0, its output none; `false` on row 1, its output on
    // row 2; `exit 2` on row 3, its output on rows 4 and 5.
    let bytes = [
        command("true", "", ";0"),
        command("false", "oops\r\n", ";1"),
        command("exit 2", "one\r\ntwo\r\n", ";2"),
    ]
    .concat();
    let view = session(&workbench, &bytes, cx);
    let (origin, cell) = grid(&view, cx);
    let (celadon, shinsha) = colours(cx);
    assert_ne!(celadon, shinsha);

    assert_eq!(
        painted(&view, cx).block_edges,
        [
            (edge(origin, cell, 0..1), celadon),
            (edge(origin, cell, 1..3), shinsha),
            (edge(origin, cell, 3..6), shinsha),
        ]
    );

    // The failures' labels, right-aligned to the grid's right edge on their
    // prompt rows; a quick success has none.
    let blocks = blocks(&view, cx);
    let expected: Vec<_> = (blocks.iter())
        .filter_map(|block| Some((block_label(block)?.to_string(), block.prompt_row)))
        .collect();
    assert_eq!(expected.len(), 2, "{expected:?}");
    let cols = view.read_with(cx, |view, _| view.grid_size().unwrap().cols);
    let right = origin.x + cell.width * f32::from(cols);
    let labels: Vec<_> = (painted(&view, cx).block_labels.iter())
        .map(|label| {
            let end = label.origin.x + label.width;
            assert!(
                (end - right).abs() < px(0.01),
                "{label:?} ends at {end:?}, not {right:?}"
            );
            let row = ((label.origin.y - origin.y) / cell.height).round() as isize;
            (label.text.clone(), row)
        })
        .collect();
    assert_eq!(labels, expected);
    assert_eq!(labels[0].1, 1);
    assert_eq!(labels[1].1, 3);

    // The labels the catalogue gives: a success only past a second.
    let ms = Duration::from_millis;
    assert_eq!(block_label(&finished(Some(0), ms(900))), None);
    assert_eq!(
        block_label(&finished(Some(0), ms(2_340))),
        Some(tr!(
            "workbench-terminal-block-passed",
            duration = duration_text(ms(2_340))
        ))
    );
    assert_eq!(
        block_label(&finished(Some(1), ms(2_340))),
        Some(tr!(
            "workbench-terminal-block-failed",
            code = 1,
            duration = duration_text(ms(2_340))
        ))
    );
    assert_eq!(block_label(&finished(None, ms(2_340))), None);
    assert_eq!(
        duration_text(ms(2_340)),
        tr!(
            "workbench-terminal-duration-seconds",
            seconds = 2,
            tenths = 3
        )
    );
    assert_eq!(
        duration_text(ms(65_000)),
        tr!(
            "workbench-terminal-duration-minutes",
            minutes = 1,
            seconds = 5
        )
    );
    assert_eq!(
        duration_text(ms(3_720_000)),
        tr!("workbench-terminal-duration-hours", hours = 1, minutes = 2)
    );
}

#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn a_running_or_unknown_block_has_no_edge(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    // A `D` with no code; a prompt with no command, ended by the next with
    // no `D`; and a command still running (no prompt after it), whose output
    // is the last thing written.
    let bytes = [
        command("sleep 1", "", ""),
        format!("{PROMPT}\r\n"),
        format!("{PROMPT}serve\r\n\x1b]133;C\x07listening, {READY}\r\n"),
    ]
    .concat();
    let view = written(&workbench, &bytes, cx);
    let blocks = blocks(&view, cx);
    assert_eq!(blocks.len(), 3, "{blocks:?}");
    for ended in &blocks[..2] {
        assert!(ended.finished.is_some(), "{ended:?}");
        assert_eq!(ended.exit_code, None, "{ended:?}");
    }
    let running = &blocks[2];
    assert!(running.running(), "{running:?}");
    assert_eq!(running.finished, None);
    assert_eq!(running.end_row, None);
    let report = painted(&view, cx);
    assert_eq!(report.block_edges, []);
    assert_eq!(report.block_labels, []);
}

#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn the_label_is_dropped_when_the_prompt_row_is_full(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    // Wider than any test window: the command line fills its prompt row
    // and wraps onto the next.
    let long = "x".repeat(400);
    let bytes = [command(&long, "", ";1"), command("false", "", ";1")].concat();
    let view = session(&workbench, &bytes, cx);
    let (origin, cell) = grid(&view, cx);
    let blocks = blocks(&view, cx);
    let report = painted(&view, cx);
    // Both have edges, only the short one a label: on its prompt row, past
    // its text and a blank cell.
    assert_eq!(report.block_edges.len(), 2);
    let [label] = &report.block_labels[..] else {
        panic!("one label: {:?}", report.block_labels);
    };
    assert_eq!(label.text, block_label(&blocks[1]).unwrap().to_string());
    assert_eq!(
        label.origin.y,
        origin.y + cell.height * blocks[1].prompt_row as f32
    );
    let typed = origin.x + cell.width * "$ false".len() as f32;
    assert!(label.origin.x >= typed + cell.width, "{label:?}");
    // No text was painted under it.
    let row_y = label.origin.y;
    assert!(
        (report.text.iter())
            .filter(|text| text.origin.y == row_y)
            .all(|text| text.origin.x < typed),
        "{:?}",
        report.text
    );
}

#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn terminal_command_blocks_false_draws_neither(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let bytes = [command("true", "", ";0"), command("false", "", ";1")].concat();
    let view = session(&workbench, &bytes, cx);
    let report = painted(&view, cx);
    assert_eq!(report.block_edges.len(), 2);
    assert_eq!(report.block_labels.len(), 1);

    set_command_blocks(&workbench, false, cx);
    let report = painted(&view, cx);
    assert_eq!(report.block_edges, []);
    assert_eq!(report.block_labels, []);

    set_command_blocks(&workbench, true, cx);
    assert_eq!(painted(&view, cx).block_edges.len(), 2);

    // An agent's terminal is never marked here, whatever the setting.
    let options = TerminalOptions {
        shell: Some((
            "/bin/sh".to_owned(),
            vec![
                "-c".to_owned(),
                "printf '%s' \"$1\"; exec cat >/dev/null".to_owned(),
                "sh".to_owned(),
                format!("{bytes}{READY}"),
            ],
        )),
        ..TerminalOptions::new(std::env::temp_dir())
    };
    let panel = terminal(&workbench, cx);
    let lane = LaneId::from_number(2).unwrap();
    cx.update(|window, cx| {
        panel.update(cx, |panel, cx| {
            panel.add_lane_session(lane, true, "agent", options, window, cx)
        })
    });
    draw(cx);
    let agent = panel.read_with(cx, |panel, _| panel.view().unwrap().clone());
    assert_ne!(agent, view);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !agent.read_with(cx, |view, _| {
        view.terminal()
            .is_some_and(|terminal| terminal.snapshot().text().contains(READY))
    }) {
        assert!(Instant::now() < deadline, "timed out waiting for the agent");
        thread::sleep(Duration::from_millis(10));
        draw(cx);
    }
    agent.update(cx, |view, _| view.record_paint(true));
    draw(cx);
    assert_eq!(blocks(&agent, cx).len(), 2);
    let report = painted(&agent, cx);
    assert_eq!(report.block_edges, []);
    assert_eq!(report.block_labels, []);
    // Its grid is as wide as the marked session's (the strip two sessions
    // draw takes a row, not a column).
    let cols = |view: &Entity<TerminalView>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.grid_size().map(|size| size.cols))
    };
    assert_eq!(cols(&agent, cx), cols(&view, cx));
}

#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn the_text_grid_does_not_move(cx: &mut TestAppContext) {
    let (workbench, cx, _dir) = opened(cx);
    let bytes = [
        command("true", "", ";0"),
        command("false", "nope\r\n", ";1"),
    ]
    .concat();
    let view = session(&workbench, &bytes, cx);
    let size = |cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.grid_size());
    let marked = painted(&view, cx);
    let marked_size = size(cx);
    assert_eq!(marked.block_edges.len(), 2);

    set_command_blocks(&workbench, false, cx);
    let plain = painted(&view, cx);
    assert_eq!(plain.block_edges, []);
    // Every glyph where it was, and the pty's size the same.
    assert_eq!(marked.text, plain.text);
    assert_eq!(marked.cursor, plain.cursor);
    assert_eq!(marked_size, size(cx));

    // Where the grid was before blocks were drawn: its first cell at the
    // session's top left, and as many columns as fit the session's width.
    let (origin, cell) = grid(&view, cx);
    let bounds = cx
        .debug_bounds("terminal-session")
        .expect("the session was drawn");
    assert_eq!(origin, bounds.origin);
    let cols = (f32::from(bounds.size.width) / f32::from(cell.width)).floor() as u16;
    assert_eq!(marked_size.map(|size| size.cols), Some(cols));
    let left = marked
        .text
        .iter()
        .map(|text| text.origin.x)
        .fold(origin.x, Pixels::min);
    assert_eq!(left, origin.x);
    // The edges run down that left edge, 2 px wide.
    for (edge, _) in &marked.block_edges {
        assert_eq!(edge.origin.x, origin.x, "{edge:?}");
        assert_eq!(edge.size.width, px(2.));
    }
}
