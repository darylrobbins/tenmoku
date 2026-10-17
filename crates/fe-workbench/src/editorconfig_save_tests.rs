//! A file's `.editorconfig` trims trailing whitespace and adds a final
//! newline when it's saved.

use fe_buffer::Buffer;
use fe_gpui::{EditorSettings, EditorTheme, EditorView};
use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::{Launch, Workbench};

/// A workbench in a temporary project with `.editorconfig` set to `config`
/// (none if empty), showing `a.txt`, which has been typed over with `typed`.
fn opened<'a>(
    config: &str,
    typed: &str,
    cx: &'a mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    if !config.is_empty() {
        std::fs::write(dir.path().join(".editorconfig"), config).unwrap();
    }
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "x\n").unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::from_file_contents("x\n").into()))),
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.run_until_parked();
    let editor = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane_group(cx).panes()[0].read(cx);
        pane.tabs()[0].editor().unwrap().clone()
    });
    editor.update(cx, |editor, _| {
        editor.model_mut().select_all();
        editor.model_mut().insert(typed);
    });
    (dir, editor, cx)
}

fn saved(
    editor: &Entity<EditorView>,
    dir: &tempfile::TempDir,
    cx: &mut VisualTestContext,
) -> String {
    let saved = cx.update(|window, cx| editor.update(cx, |e, cx| e.save(window, cx)));
    cx.run_until_parked();
    drop(saved);
    std::fs::read_to_string(dir.path().join("a.txt")).unwrap()
}

#[gpui::test]
fn from_the_editorconfig(cx: &mut TestAppContext) {
    let typed = "a  \nb\t";

    let config = "[*.txt]\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n";
    let (dir, editor, cx) = opened(config, typed, cx);
    assert_eq!(saved(&editor, &dir, cx), "a\nb\n");

    let config = "[*.txt]\ntrim_trailing_whitespace = false\ninsert_final_newline = false\n";
    let (dir, editor, cx) = opened(config, typed, cx);
    assert_eq!(saved(&editor, &dir, cx), typed);

    let (dir, editor, cx) = opened("", typed, cx);
    assert_eq!(saved(&editor, &dir, cx), typed);
}

/// Appends a blank-padded line, as a formatter might.
struct Pads;

impl fe_gpui::SaveHook for Pads {
    fn before_save(&self, editor: &Entity<EditorView>, cx: &mut gpui::App) -> gpui::Task<()> {
        let editor = editor.clone();
        cx.spawn(async move |cx| {
            editor.update(cx, |view, cx| {
                view.model_mut().select_all();
                view.model_mut().insert("formatted  ");
                cx.notify();
            });
        })
    }
}

#[gpui::test]
fn after_format_on_save(cx: &mut TestAppContext) {
    let config = "[*.txt]\ntrim_trailing_whitespace = true\ninsert_final_newline = true\n";
    let (dir, editor, cx) = opened(config, "typed", cx);
    editor.update(cx, |view, _| {
        view.set_save_hook(Some(std::rc::Rc::new(Pads)))
    });
    assert_eq!(saved(&editor, &dir, cx), "formatted\n");
}
