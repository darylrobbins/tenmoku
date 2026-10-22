//! The chat's own commands that need an editor: Ask About Selection.

use std::{fs, path::Path, sync::Arc};

use fe_buffer::ResolvedSelection;
use fe_chat::ContextKind;
use fe_gpui::EditorView;
use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln::core::secrets::MemoryStore;

use super::{ChatPanel, chat, composer_focused, dispatch_in, draw, opened, set_source, shown};
use crate::{DockPanel, Workbench};

const MAIN: &str = "func main() {\n    x := 1\n}\n";

/// Opens `path` in the active pane and gives back its editor.
fn open_file(
    workbench: &Entity<Workbench>,
    path: &Path,
    cx: &mut VisualTestContext,
) -> Entity<EditorView> {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx)));
    draw(cx);
    pane.read_with(cx, |pane, _| pane.active_editor().cloned())
        .expect("an editor")
}

fn select(editor: &Entity<EditorView>, range: std::ops::Range<usize>, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| {
        let selection = ResolvedSelection {
            range,
            reversed: false,
        };
        editor.model_mut().set_selections([selection]);
        cx.notify();
    });
    draw(cx);
}

/// The chips waiting in the panel's composer: each label and text.
fn chips(panel: &Entity<ChatPanel>, cx: &mut VisualTestContext) -> Vec<(String, String)> {
    panel.read_with(cx, |panel, cx| {
        (panel.view().read(cx).chips().into_iter())
            .map(|chip| chip.item)
            .map(|item| {
                assert!(matches!(item.kind, ContextKind::Selection { .. }));
                (item.label, item.text)
            })
            .collect()
    })
}

#[gpui::test]
fn ask_about_selection_attaches_it(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.go"), MAIN).unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    set_source(Some("key"), &Arc::new(MemoryStore::new()), cx);
    assert!(!workbench.read_with(cx, |workbench, _| workbench.root_trusted()));
    let editor = open_file(&workbench, &dir.path().join("main.go"), cx);

    // A selection, in an untrusted root: the panel opens, with the
    // selection as a chip and the composer focused.
    select(&editor, 5..9, cx);
    dispatch_in(
        "chat.ask_about_selection",
        &[crate::KEY_CONTEXT, "editor"],
        cx,
    );
    cx.run_until_parked();
    assert_eq!(shown(&workbench, cx), (true, DockPanel::Chat));
    let panel = chat(&workbench, cx).expect("made");
    assert_eq!(
        chips(&panel, cx),
        [("main.go:1-1".to_owned(), "main".to_owned())]
    );
    assert!(composer_focused(&panel, cx));

    // Nothing selected: the caret's row, whole.
    select(&editor, 20..20, cx);
    dispatch_in(
        "chat.ask_about_selection",
        &[crate::KEY_CONTEXT, "editor"],
        cx,
    );
    cx.run_until_parked();
    assert_eq!(
        chips(&panel, cx).last(),
        Some(&("main.go:2-2".to_owned(), "    x := 1".to_owned()))
    );
    assert!(composer_focused(&panel, cx));
}

#[gpui::test]
fn ask_about_selection_closes_the_key_form(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.go"), MAIN).unwrap();
    let (workbench, cx) = opened(cx, dir.path());
    set_source(Some("key"), &Arc::new(MemoryStore::new()), cx);
    let editor = open_file(&workbench, &dir.path().join("main.go"), cx);
    dispatch_in("chat.set_api_key", &[crate::KEY_CONTEXT], cx);
    cx.run_until_parked();
    let panel = chat(&workbench, cx).expect("made");
    assert!(!composer_focused(&panel, cx));

    select(&editor, 5..9, cx);
    dispatch_in(
        "chat.ask_about_selection",
        &[crate::KEY_CONTEXT, "editor"],
        cx,
    );
    cx.run_until_parked();
    assert_eq!(
        chips(&panel, cx),
        [("main.go:1-1".to_owned(), "main".to_owned())]
    );
    assert!(composer_focused(&panel, cx));
    assert!(panel.read_with(cx, |panel, _| panel.key_form_closed()));
}
