//! Quick open's `@` prefix hands over to the symbol picker; `:` still goes
//! to a line.

use std::{fs, ops::Range};

use fe_gpui::EditorView;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    Picker, Workbench,
    symbol_picker::SymbolPicker,
    tests::quick_open::{picker as quick_open, workbench_at},
    workbench::Palette,
};

const TEXT: &str = "fn alpha() {}\nfn beta() {}\nfn gamma() {}\n";

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn symbols(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<Picker<SymbolPicker>>> {
    workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        (layer.modal() == Some(Palette::DocumentSymbols.id()))
            .then(|| layer.modal_view().unwrap().clone().downcast().unwrap())
    })
}

fn rows(picker: &Entity<Picker<SymbolPicker>>, cx: &mut VisualTestContext) -> Vec<String> {
    picker.read_with(cx, |picker, _| picker.delegate.row_names())
}

fn selection(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Range<usize> {
    editor.read_with(cx, |editor, _| editor.model().primary_selection().range)
}

fn at(name: &str) -> Range<usize> {
    let start = TEXT.find(name).unwrap();
    start..start + name.len()
}

/// A workbench on a project showing `sym.rs`, its outline landed.
fn showing<'a>(
    dir: &tempfile::TempDir,
    cx: &'a mut TestAppContext,
) -> (
    Entity<Workbench>,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    let path = dir.path().join("sym.rs");
    fs::write(&path, TEXT).unwrap();
    let (workbench, cx) = workbench_at(cx, Some(dir.path()));
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    draw(cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown");
    (workbench, editor, cx)
}

#[gpui::test]
fn at_prefix_opens_symbols(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, editor, cx) = showing(&dir, cx);

    // `@` alone lists them all, quick open gone.
    cx.simulate_keystrokes("cmd-p");
    draw(cx);
    cx.simulate_input("@");
    draw(cx);
    let picker = symbols(&workbench, cx).expect("the symbol picker shows");
    assert!(!workbench.read_with(cx, |workbench, _| workbench.palette_open()));
    assert_eq!(rows(&picker, cx), ["alpha", "beta", "gamma"]);
    cx.update(|window, cx| {
        assert!(picker.focus_handle(cx).is_focused(window), "typing goes on");
    });

    // Typing goes on in the symbol picker.
    cx.simulate_input("bt");
    draw(cx);
    assert_eq!(rows(&picker, cx), ["beta"]);

    // Escape goes back to the editor, as the symbol picker's own does.
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(symbols(&workbench, cx).is_none());
    cx.update(|window, cx| assert!(editor.focus_handle(cx).is_focused(window)));

    // What follows the `@` is the symbol picker's query, pasted whole.
    cx.simulate_keystrokes("cmd-p");
    draw(cx);
    cx.simulate_input("@gmm");
    draw(cx);
    let picker = symbols(&workbench, cx).expect("the symbol picker shows");
    let input = picker.read_with(cx, |picker, cx| picker.input().read(cx).text().to_owned());
    assert_eq!(input, "gmm", "without the @");
    assert_eq!(rows(&picker, cx), ["gamma"]);
    cx.simulate_keystrokes("enter");
    draw(cx);
    assert!(symbols(&workbench, cx).is_none());
    assert_eq!(selection(&editor, cx), at("gamma"));
    cx.update(|window, cx| assert!(editor.focus_handle(cx).is_focused(window)));

    // Quick open is itself again next time, and `:` still goes to a line.
    cx.simulate_keystrokes("cmd-p");
    draw(cx);
    assert!(workbench.read_with(cx, |workbench, _| workbench.palette_open()));
    cx.simulate_input("sym.rs:2:4");
    draw(cx);
    assert!(symbols(&workbench, cx).is_none());
    assert!(workbench.read_with(cx, |workbench, _| workbench.palette_open()));
    cx.simulate_keystrokes("enter");
    draw(cx);
    let second_row = TEXT.find('\n').unwrap() + 1;
    assert_eq!(selection(&editor, cx), second_row + 3..second_row + 3);
}

#[gpui::test]
fn at_only_leads(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a@b.rs"), "").unwrap();
    let (workbench, _, cx) = showing(&dir, cx);
    cx.simulate_keystrokes("cmd-p");
    draw(cx);
    cx.simulate_input("a@b");
    draw(cx);
    assert!(symbols(&workbench, cx).is_none());
    let found = quick_open(&workbench, cx).read_with(cx, |picker, _| {
        (picker.delegate.matches().iter())
            .map(|found| found.path.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(found, ["a@b.rs"]);
}

#[gpui::test]
fn at_with_no_folder_still_lists_the_files_symbols(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sym.rs");
    fs::write(&path, TEXT).unwrap();
    let (workbench, cx) = workbench_at(cx, None);
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    draw(cx);
    cx.simulate_keystrokes("cmd-p");
    draw(cx);
    cx.simulate_input("@");
    draw(cx);
    let picker = symbols(&workbench, cx).expect("the symbol picker shows");
    assert_eq!(rows(&picker, cx), ["alpha", "beta", "gamma"]);
}

#[gpui::test]
fn picking_a_file_for_something_else_keeps_the_at(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, _, cx) = showing(&dir, cx);
    cx.simulate_keystrokes("cmd-p");
    draw(cx);
    let picker = quick_open(&workbench, cx);
    picker.update(cx, |picker, _| {
        picker.delegate.set_pick(Box::new(|_, _, _| {}))
    });
    cx.simulate_input("@sym");
    draw(cx);
    assert!(symbols(&workbench, cx).is_none(), "it filters files");
    assert!(workbench.read_with(cx, |workbench, _| workbench.palette_open()));
}
