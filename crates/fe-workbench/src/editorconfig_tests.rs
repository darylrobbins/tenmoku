//! A file's `.editorconfig` applies when it opens: its indent style and
//! size over the detected ones, its tab width over `editor.tab_size`, and
//! its line ending.

use std::sync::Arc;

use fe_buffer::{Buffer, LineEnding};
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::{Settings, SettingsEvent};
use fe_gpui::commands::RunCommand;
use fe_gpui::{EditorSettings, EditorTheme, EditorView};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::ItemHandle as _;

use crate::{Launch, Workbench};

/// A workbench in a temporary project with `.editorconfig` set to `config`
/// (none if empty), showing `name` with `text`.
fn opened<'a>(
    config: &str,
    name: &str,
    text: &str,
    cx: &'a mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    if !config.is_empty() {
        std::fs::write(dir.path().join(".editorconfig"), config).unwrap();
    }
    let path = dir.path().join(name);
    std::fs::write(&path, text).unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::from_file_contents(text).into()))),
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.run_until_parked();
    let editor = editor(&workbench, cx);
    (dir, workbench, editor, cx)
}

fn editor(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<EditorView> {
    workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane_group(cx).panes()[0].read(cx);
        pane.tabs()[0].editor().unwrap().clone()
    })
}

fn status(editor: &Entity<EditorView>, id: &str, cx: &mut VisualTestContext) -> String {
    let items = cx.update(|_, cx| editor.status_items(cx));
    let item = items.iter().find(|item| item.id == id);
    item.expect("a status item").text.to_string()
}

const SPACED: &str = "a {\n    b\n}\n";

#[gpui::test]
fn indent_and_tab_width(cx: &mut TestAppContext) {
    let config = "[*.go]\nindent_style = tab\n\n[*.js]\nindent_size = 2\n\n[*.rs]\ntab_width = 8\n";
    let text = SPACED.repeat(5);

    let (_dir, _workbench, go, cx) = opened(config, "a.go", &text, cx);
    assert_eq!(status(&go, "indent", cx), "Tab Size: 4");

    let (_dir, _workbench, js, cx) = opened(config, "a.js", &text, cx);
    assert_eq!(status(&js, "indent", cx), "Spaces: 2");

    let (_dir, workbench, rs, cx) = opened(config, "a.rs", &text, cx);
    assert_eq!(rs.read_with(cx, |e, _| e.model().tab_size()), 8);
    let mut settings = Settings::default();
    settings.editor.tab_size = 3;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    cx.run_until_parked();
    assert_eq!(rs.read_with(cx, |e, _| e.model().tab_size()), 8);
}

#[gpui::test]
fn line_ending_and_none(cx: &mut TestAppContext) {
    let (dir, _workbench, editor, cx) =
        opened("[*.txt]\nend_of_line = crlf\n", "a.txt", "a\nb\n", cx);
    assert_eq!(status(&editor, "line_ending", cx), "CRLF");
    let saved = cx.update(|window, cx| editor.update(cx, |e, cx| e.save(window, cx)));
    cx.run_until_parked();
    drop(saved);
    assert_eq!(
        std::fs::read(dir.path().join("a.txt")).unwrap(),
        b"a\r\nb\r\n"
    );

    let (_dir, _workbench, editor, cx) =
        opened("", "b.txt", &SPACED.replace('\n', "\r\n").repeat(5), cx);
    assert_eq!(status(&editor, "indent", cx), "Spaces: 4");
    editor.read_with(cx, |e, _| {
        assert_eq!(e.model().buffer().line_ending(), LineEnding::CrLf);
        assert_eq!(e.model().tab_size(), EditorSettings::default().tab_size);
    });
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    cx.run_until_parked();
}

fn tab_sizes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<usize> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group(cx).panes().iter())
            .flat_map(|pane| pane.read(cx).tabs().to_vec())
            .map(|tab| tab.editor().unwrap().read(cx).model().tab_size())
            .collect()
    })
}

fn reload_tab_size(workbench: &Entity<Workbench>, size: usize, cx: &mut VisualTestContext) {
    let mut settings = Settings::default();
    settings.editor.tab_size = size;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    cx.run_until_parked();
}

const WIDE: &str = "[*.rs]\ntab_width = 8\n";

/// The ids of the editors in pane order, with their tab widths.
fn editors(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(gpui::EntityId, usize)> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane_group(cx).panes().iter())
            .flat_map(|pane| pane.read(cx).tabs().to_vec())
            .map(|tab| {
                let editor = tab.editor().unwrap();
                (editor.entity_id(), editor.read(cx).model().tab_size())
            })
            .collect()
    })
}

#[gpui::test]
fn both_split_editors_keep_their_tab_width(cx: &mut TestAppContext) {
    let (_dir, workbench, _editor, cx) = opened(WIDE, "a.rs", "a\n", cx);
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    run("pane.split_right", cx);
    let before = editors(&workbench, cx);
    assert_eq!(before.len(), 2, "{before:?}");
    assert_ne!(before[0].0, before[1].0);
    assert!(before.iter().all(|&(_, size)| size == 8), "{before:?}");

    reload_tab_size(&workbench, 3, cx);
    let after = editors(&workbench, cx);
    assert_eq!(
        after.iter().map(|e| e.0).collect::<Vec<_>>(),
        before.iter().map(|e| e.0).collect::<Vec<_>>()
    );
    assert!(after.iter().all(|&(_, size)| size == 8), "{after:?}");
}

#[gpui::test]
fn a_moved_editor_keeps_its_identity_and_tab_width(cx: &mut TestAppContext) {
    let (dir, workbench, _editor, cx) = opened(WIDE, "a.rs", "a\n", cx);
    let b = dir.path().join("b.rs");
    std::fs::write(&b, "b\n").unwrap();
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    run("pane.split_right", cx);
    // The right pane closes its copy of a.rs and shows b.rs alone.
    let right = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| right.update(cx, |pane, cx| pane.open(b, window, cx)));
    cx.run_until_parked();
    let moved = right.read_with(cx, |pane, _| pane.tabs()[pane.active().unwrap()].id());

    run("pane.move_tab_left", cx);
    let target = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let ids = target.read_with(cx, |pane, _| {
        pane.tabs().iter().map(|tab| tab.id()).collect::<Vec<_>>()
    });
    assert!(ids.contains(&moved), "{ids:?} lacks {moved:?}");
    let left_first = workbench.read_with(cx, |workbench, cx| {
        workbench.pane_group(cx).panes()[0].clone()
    });
    assert!(Entity::eq(&target, &left_first));

    reload_tab_size(&workbench, 3, cx);
    let sizes = tab_sizes(&workbench, cx);
    assert!(
        sizes.len() >= 2 && sizes.iter().all(|&size| size == 8),
        "{sizes:?}"
    );
}

#[gpui::test]
fn a_reload_while_the_read_is_pending_ends_at_the_tab_width(cx: &mut TestAppContext) {
    let (dir, workbench, _editor, cx) = opened(WIDE, "a.rs", "a\n", cx);
    let b = dir.path().join("b.rs");
    std::fs::write(&b, "b\n").unwrap();
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(b, window, cx)));
    // No parking yet: b.rs is still being read.
    reload_tab_size(&workbench, 3, cx);
    let sizes = tab_sizes(&workbench, cx);
    assert!(sizes.iter().all(|&size| size == 8), "{sizes:?}");
}

#[gpui::test]
fn a_restored_setting_leaves_the_tab_width(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(WIDE, "a.rs", "a\n", cx);
    reload_tab_size(&workbench, 3, cx);
    reload_tab_size(&workbench, EditorSettings::default().tab_size, cx);
    assert_eq!(editor.read_with(cx, |e, _| e.model().tab_size()), 8);
}
