//! fastedit's command palette: cmd-shift-p lists the workbench's and the
//! editor's commands that apply in the focused editor, with their keys; a
//! pick runs in the editor, from the palette.

use fe_buffer::Buffer;
use fe_command::Source;
use fe_gpui::{EditorSettings, EditorTheme};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::{
    Picker,
    command_palette::{COMMAND_PALETTE, CommandPalette},
};

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
}

fn palette(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<Picker<CommandPalette>>> {
    workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        (layer.modal() == Some(COMMAND_PALETTE))
            .then(|| layer.modal_view().unwrap().clone().downcast().unwrap())
    })
}

fn rows(
    picker: &Entity<Picker<CommandPalette>>,
    cx: &mut VisualTestContext,
) -> Vec<(String, Option<String>)> {
    picker.read_with(cx, |picker, cx| {
        let palette = &picker.delegate;
        (palette.matches().iter().enumerate())
            .map(|(ix, entry)| {
                let key = palette.key_label(ix, cx).map(|key| key.to_string());
                (entry.title.to_string(), key)
            })
            .collect()
    })
}

#[gpui::test]
fn the_palette_runs_a_command_in_the_editor(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    let editor = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let editor = editor.unwrap();
    cx.simulate_keystrokes("cmd-shift-p");
    draw(cx);
    let picker = palette(&workbench, cx).expect("the palette shows");
    let rows = rows(&picker, cx);
    let cmd_w = fe_keymap::KeySequence::parse("cmd-w").unwrap();
    let close_tab = (
        "Close Tab".to_owned(),
        Some(fe_keymap::shortcut_label(&cmd_w)),
    );
    assert!(rows.contains(&close_tab), "{rows:?}");
    assert!(rows.iter().any(|(title, _)| title == "Split Right"));
    // The editor's own commands apply in it; the project panel's don't.
    assert!(
        rows.iter().any(|(title, _)| title == "Select All"),
        "{rows:?}"
    );

    cx.simulate_input("split right");
    draw(cx);
    let first = picker.read_with(cx, |picker, _| picker.delegate.matches()[0].title.clone());
    assert_eq!(first.as_ref(), "Split Right");
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(palette(&workbench, cx).is_none());
    let panes = workbench.read_with(cx, |workbench, cx| workbench.pane_group(cx).panes().len());
    assert_eq!(panes, 2);
    let run = workbench.read_with(cx, |workbench, _| workbench.commands_run.last().cloned());
    let run = run.unwrap();
    assert_eq!(run.command.as_str(), "pane.split_right");
    assert_eq!(run.source, Source::Palette);
    assert!(
        run.context
            .frames()
            .iter()
            .any(|frame| frame.name == "editor")
    );

    // Escape gives focus back to the editor it was in.
    let right = workbench.read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned());
    let right = right.unwrap();
    assert_ne!(right, editor);
    cx.simulate_keystrokes("cmd-shift-p");
    draw(cx);
    assert!(palette(&workbench, cx).is_some());
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(palette(&workbench, cx).is_none());
    assert!(cx.update(|window, cx| right.focus_handle(cx).is_focused(window)));
}
