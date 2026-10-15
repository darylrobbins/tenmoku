//! The pane commands that resize, even out and cycle: `pane.grow_width`,
//! `shrink_width`, `grow_height`, `shrink_height`, `equalize`, `focus_next`
//! and `focus_previous`, run through the workbench's dispatch, and listed in
//! the palette without a key.

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::command_palette::{COMMAND_PALETTE, CommandPalette};

use super::RESIZE_STEP;
use crate::{Launch, Workbench};

fn opened(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("alpha\n").into()))),
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (dir, workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn ratios(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<f32> {
    workbench.read_with(cx, |workbench, _| {
        workbench.pane_group().ratios(&[]).unwrap_or_default()
    })
}

/// Where the active editor is among the panes.
fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<usize> {
    workbench.read_with(cx, |workbench, cx| {
        let editor = workbench.active_editor(cx)?;
        (workbench.pane_group().panes().iter()).position(|pane| {
            let pane = pane.read(cx);
            (pane.active()).is_some_and(|ix| pane.tabs()[ix].editor() == Some(editor))
        })
    })
}

fn pane_count(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, _| workbench.pane_group().panes().len())
}

/// Each pane's size in pixels, in the panes' order.
fn sizes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(f32, f32)> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group().panes().iter())
            .map(|pane| {
                let size = pane.read(cx).bounds().size;
                (f32::from(size.width), f32::from(size.height))
            })
            .collect()
    })
}

/// Runs `id` and checks pane `grown` got `step` px bigger along the axis
/// (width if `wide`), and `taken` that much smaller, the others unchanged.
fn resized(
    workbench: &Entity<Workbench>,
    id: &str,
    wide: bool,
    (grown, taken): (usize, usize),
    step: f32,
    cx: &mut VisualTestContext,
) {
    let along = |size: (f32, f32)| if wide { size.0 } else { size.1 };
    let before = sizes(workbench, cx);
    run(id, cx);
    let after = sizes(workbench, cx);
    for (ix, (before, after)) in before.iter().zip(&after).enumerate() {
        let expected = if ix == grown {
            step
        } else if ix == taken {
            -step
        } else {
            0.
        };
        let moved = along(*after) - along(*before);
        assert!(
            (moved - expected).abs() <= 1.,
            "{id}: pane {ix} moved {moved}, not {expected}: {before:?} -> {after:?}"
        );
    }
}

#[gpui::test]
fn resize_and_equalize_commands_move_the_borders(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    let step = f32::from(RESIZE_STEP);
    assert_eq!(step, 32.);
    run("pane.split_right", cx);
    run("pane.focus_left", cx);

    // The first pane takes from its next sibling; the last, its previous.
    resized(&workbench, "pane.grow_width", true, (0, 1), step, cx);
    resized(&workbench, "pane.shrink_width", true, (1, 0), step, cx);
    run("pane.focus_right", cx);
    resized(&workbench, "pane.grow_width", true, (1, 0), step, cx);
    resized(&workbench, "pane.shrink_width", true, (0, 1), step, cx);
    // There's no vertical axis yet.
    let before = ratios(&workbench, cx);
    run("pane.grow_height", cx);
    run("pane.shrink_height", cx);
    assert_eq!(ratios(&workbench, cx), before);

    // Right | (top / bottom), the bottom active.
    run("pane.split_down", cx);
    assert_eq!(pane_count(&workbench, cx), 3);
    resized(&workbench, "pane.grow_height", false, (2, 1), step, cx);
    resized(&workbench, "pane.shrink_height", false, (1, 2), step, cx);
    run("pane.focus_up", cx);
    resized(&workbench, "pane.grow_height", false, (1, 2), step, cx);
    resized(&workbench, "pane.shrink_height", false, (2, 1), step, cx);
    // Its width is the outer axis's.
    let before = sizes(&workbench, cx);
    run("pane.grow_width", cx);
    let after = sizes(&workbench, cx);
    let moved: Vec<f32> = before.iter().zip(&after).map(|(b, a)| a.0 - b.0).collect();
    assert!(
        (moved[0] + step).abs() <= 1.
            && (moved[1] - step).abs() <= 1.
            && (moved[2] - step).abs() <= 1.,
        "{moved:?}"
    );

    run("pane.equalize", cx);
    assert_eq!(ratios(&workbench, cx), vec![0.5, 0.5]);
    let inner = workbench.read_with(cx, |workbench, _| workbench.pane_group().ratios(&[1]));
    assert_eq!(inner, Some(vec![0.5, 0.5]));
}

#[gpui::test]
fn focus_next_and_previous_step_through_the_panes(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    run("pane.focus_next", cx);
    assert_eq!(pane_count(&workbench, cx), 1);
    assert_eq!(active(&workbench, cx), Some(0));

    run("pane.split_right", cx);
    run("pane.split_right", cx);
    assert_eq!(pane_count(&workbench, cx), 3);
    assert_eq!(active(&workbench, cx), Some(2));
    run("pane.focus_next", cx);
    assert_eq!(active(&workbench, cx), Some(0));
    run("pane.focus_next", cx);
    assert_eq!(active(&workbench, cx), Some(1));
    run("pane.focus_previous", cx);
    run("pane.focus_previous", cx);
    assert_eq!(active(&workbench, cx), Some(2));
}

#[gpui::test]
fn the_palette_lists_the_commands_without_keys(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    cx.simulate_keystrokes("cmd-shift-p");
    draw(cx);
    let picker = workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        assert_eq!(layer.modal(), Some(COMMAND_PALETTE));
        layer
            .modal_view()
            .unwrap()
            .clone()
            .downcast::<kiln_shell::Picker<CommandPalette>>()
            .unwrap()
    });
    let rows: Vec<_> = picker.read_with(cx, |picker, cx| {
        let palette = &picker.delegate;
        (palette.matches().iter().enumerate())
            .map(|(ix, entry)| (entry.title.to_string(), palette.key_label(ix, cx)))
            .collect()
    });
    for title in [
        "Grow Pane Width",
        "Shrink Pane Width",
        "Grow Pane Height",
        "Shrink Pane Height",
        "Equalize Panes",
        "Focus Next Pane",
        "Focus Previous Pane",
    ] {
        let row = rows.iter().find(|(row, _)| row == title);
        assert!(matches!(row, Some((_, None))), "{title}: {row:?}");
    }
}
