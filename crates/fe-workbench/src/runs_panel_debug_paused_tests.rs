//! A stopped debug run's row in the Runs panel: the Pause icon, tinted with
//! the accent colour.

use std::fs;

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_ui::{ActiveTheme as _, Icon};
use gpui::{Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    Launch,
    runs::{DebugPhase, RunKind, RunSummary},
    tests::open,
};

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// C2: a debug run that's running draws the bug; once it stops, its row
/// draws the Pause icon in the accent colour.
#[gpui::test]
fn a_stopped_debug_run_draws_the_pause_icon_in_the_accent(cx: &mut TestAppContext) {
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
    workbench.update(cx, |workbench, cx| workbench.set_root_trust(true, cx));
    draw(cx);
    let stack = ContextStack::new().with(ContextFrame::new(crate::KEY_CONTEXT));
    let show = RunCommand(Invocation::of("runs.show").unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(show), cx));
    draw(cx);
    let runs = workbench.read_with(cx, |workbench, _| workbench.runs().clone());
    let panel = workbench.read_with(cx, |workbench, _| workbench.runs_panel().clone());
    let drawn = |cx: &mut VisualTestContext| {
        panel.read_with(cx, |panel, cx| {
            panel
                .list()
                .read(cx)
                .delegate()
                .drawn_icons
                .get(&0)
                .copied()
        })
    };
    let accent = cx.update(|_, cx| cx.theme().colors.accent.base);

    let run = runs.update(cx, |runs, cx| runs.begin(RunKind::Debug, "main", cx));
    let update = |phase, cx: &mut VisualTestContext| {
        runs.update(cx, |runs, cx| {
            runs.update(run, None, RunSummary::Debug(phase), cx)
        });
        draw(cx);
    };
    update(DebugPhase::Running, cx);
    let (icon, _) = drawn(cx).expect("the row is drawn");
    assert_eq!(icon, Icon::Bug);

    update(DebugPhase::Stopped, cx);
    assert_eq!(drawn(cx), Some((Icon::Pause, accent)));
}
