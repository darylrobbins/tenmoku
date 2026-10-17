//! The command palette opened over a picker leaves out Open Recent in New
//! Window: the command is the recent picker's key, which the palette
//! replaces, so choosing it there would do nothing.

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::{
    Picker,
    command_palette::{COMMAND_PALETTE, CommandPalette},
};

use crate::{Launch, Workbench, workbench::recent_picker::set_recent_file};

const TITLE: &str = "Open Recent in New Window";

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn window(
    cx: &mut TestAppContext,
    root: &std::path::Path,
) -> (Entity<Workbench>, VisualTestContext) {
    let state = std::env::temp_dir().join(format!("palette_recent_{}", std::process::id()));
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        set_recent_file(cx, state.join("recent.toml"));
    });
    let root = root.to_owned();
    let window = cx.add_window(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        let launch = Launch {
            root: Some(root),
            file: None,
        };
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let cx = &mut vcx;
    cx.update(|window, _| window.activate_window());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, vcx)
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// The titles the palette lists, opened with its key over whatever shows.
fn palette_titles(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.simulate_keystrokes("cmd-shift-p");
    draw(cx);
    let picker: Entity<Picker<CommandPalette>> = workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        assert_eq!(layer.modal(), Some(COMMAND_PALETTE), "the palette shows");
        layer.modal_view().unwrap().clone().downcast().unwrap()
    });
    picker.read_with(cx, |picker, _| {
        (picker.delegate.matches().iter())
            .map(|entry| entry.title.to_string())
            .collect()
    })
}

#[gpui::test]
fn the_palette_over_a_picker_leaves_out_open_recent_in_new_window(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut cx) = window(cx, dir.path());
    let cx = &mut cx;

    run("workbench.open_recent", cx);
    let titles = palette_titles(&workbench, cx);
    assert!(
        titles.iter().any(|title| title == "Open Recent…"),
        "{titles:?}"
    );
    assert!(!titles.iter().any(|title| title == TITLE), "{titles:?}");

    cx.simulate_keystrokes("escape");
    draw(cx);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    assert!(!workbench.read_with(cx, |workbench, _| workbench.palette_open()));
    cx.simulate_keystrokes("cmd-p");
    draw(cx);
    assert!(workbench.read_with(cx, |workbench, _| workbench.palette_open()));
    let titles = palette_titles(&workbench, cx);
    assert!(!titles.iter().any(|title| title == TITLE), "{titles:?}");
}
