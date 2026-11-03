//! Go to Symbol in File, through the default keymap: the rows are the
//! outline the panel shows, moving the selection previews, escape puts the
//! scroll back, enter selects the name, and a stale outline refuses the pick.

use std::{fs, ops::Range, path::Path};

use fe_gpui::EditorView;
use fe_syntax::{Outline, OutlineItem, OutlineKind};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::SymbolPicker;
use crate::{
    OutlinePanel, Picker, Workbench, outline_panel::symbol_icon, picker::PickerDelegate as _,
    tests::open_root, workbench::Palette,
};

/// `alpha`, `outer` holding `inner` and `beta`, far enough apart to scroll.
fn source() -> String {
    let filler = "// filler\n".repeat(100);
    format!(
        "fn alpha() {{}}\n{filler}mod outer {{\n    fn inner() {{}}\n}}\n{filler}fn beta() {{}}\n"
    )
}

/// A workbench showing the file `name` holding `text`, its outline landed.
fn opened<'a>(
    dir: &tempfile::TempDir,
    name: &str,
    text: &str,
    cx: &'a mut TestAppContext,
) -> (
    Entity<Workbench>,
    Entity<EditorView>,
    &'a mut VisualTestContext,
) {
    let path = dir.path().join(name);
    fs::write(&path, text).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.simulate_resize(gpui::size(gpui::px(800.), gpui::px(300.)));
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    open(&workbench, &path, cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown");
    (workbench, editor, cx)
}

fn open(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path.to_owned(), window, cx)));
    draw(cx);
    draw(cx);
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

fn picker(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Option<Entity<Picker<SymbolPicker>>> {
    workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        (layer.modal() == Some(Palette::DocumentSymbols.id()))
            .then(|| layer.modal_view().unwrap().clone().downcast().unwrap())
    })
}

/// The picker the key opens.
fn show(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Picker<SymbolPicker>> {
    press("cmd-shift-o", cx);
    picker(workbench, cx).expect("the picker shows")
}

fn rows(picker: &Entity<Picker<SymbolPicker>>, cx: &mut VisualTestContext) -> Vec<String> {
    picker.read_with(cx, |picker, _| picker.delegate.row_names())
}

fn status(picker: &Entity<Picker<SymbolPicker>>, cx: &mut VisualTestContext) -> Option<String> {
    picker.read_with(cx, |picker, _| {
        picker.delegate.status().map(|status| status.to_string())
    })
}

fn outline_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<OutlinePanel> {
    workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).outline().clone()
    })
}

fn selection(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Range<usize> {
    editor.read_with(cx, |editor, _| editor.model().primary_selection().range)
}

fn scroll_top(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    editor.read_with(cx, |editor, _| editor.scroll_top())
}

/// Whether the last frame laid out buffer `row`.
fn row_shown(editor: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> bool {
    editor.read_with(cx, |editor, _| editor.shows_buffer_row(row).expect("drawn"))
}

fn editor_focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

/// Where `name` is in `text`.
fn at(text: &str, name: &str) -> Range<usize> {
    let start = text.find(name).expect("in the text");
    start..start + name.len()
}

/// A top-level symbol as a server would give it.
fn served_item(name: &str, range: Range<usize>, row: u32) -> OutlineItem {
    OutlineItem {
        name: name.to_owned(),
        detail: String::new(),
        kind: OutlineKind::Function,
        range: range.clone(),
        name_range: range,
        row,
        depth: 0,
        parent: None,
    }
}

fn serve(
    workbench: &Entity<Workbench>,
    editor: &Entity<EditorView>,
    items: Vec<OutlineItem>,
    cx: &mut VisualTestContext,
) {
    let outline = outline_of(workbench, cx);
    let version = editor.read_with(cx, |editor, _| editor.model().snapshot().version());
    let id = editor.entity_id();
    outline.update(cx, |outline, cx| {
        let served = Outline {
            items,
            truncated: false,
            version,
        };
        outline.set_served(id, Some(served), cx)
    });
    draw(cx);
}

#[gpui::test]
fn lists_the_outline_and_filters_fuzzily(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, _, cx) = opened(&dir, "sym.rs", &source(), cx);
    let picker = show(&workbench, cx);

    let panel = outline_of(&workbench, cx).read_with(cx, |outline, _| {
        let names = outline.rows().into_iter().map(|(_, name)| name);
        names.collect::<Vec<_>>()
    });
    assert_eq!(panel, ["alpha", "outer", "inner", "beta"]);
    assert_eq!(
        rows(&picker, cx),
        panel,
        "the rows are what the panel shows"
    );
    assert_eq!(status(&picker, cx), None);

    // Each row says what it is and where it is.
    picker.read_with(cx, |picker, _| {
        let delegate = &picker.delegate;
        assert_eq!(delegate.chain(&delegate.items[0]), "");
        assert_eq!(delegate.chain(&delegate.items[2]), "outer");
    });
    let icon = format!(
        "symbol-picker-icon:inner:{}",
        symbol_icon(OutlineKind::Function).path()
    );
    assert!(cx.debug_bounds(icon.leak()).is_some(), "the kind's icon");

    // A subsequence, ignoring case, not a prefix.
    cx.simulate_input("NR");
    draw(cx);
    assert_eq!(rows(&picker, cx), ["inner"]);
    let highlights = picker.read_with(cx, |picker, _| picker.delegate.matches[0].1.clone());
    assert_eq!(
        highlights,
        [1..2, 4..5],
        "the chars matched are highlighted"
    );

    press("backspace", cx);
    assert_eq!(rows(&picker, cx), ["inner"], "only inner has an n");
    press("backspace", cx);
    assert_eq!(rows(&picker, cx), panel);

    cx.simulate_input("zzz");
    draw(cx);
    assert_eq!(rows(&picker, cx), [] as [&str; 0]);
    assert_eq!(status(&picker, cx).as_deref(), Some("No matching symbols"));
}

#[gpui::test]
fn no_outline_says_no_symbols(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, editor, cx) = opened(&dir, "notes.txt", "just words\n", cx);
    let picker = show(&workbench, cx);
    assert_eq!(rows(&picker, cx), [] as [&str; 0]);
    assert_eq!(status(&picker, cx).as_deref(), Some("No symbols"));
    // Enter with nothing to pick stays, and escape leaves.
    press("enter", cx);
    assert!(self::picker(&workbench, cx).is_some());
    press("escape", cx);
    assert!(self::picker(&workbench, cx).is_none());
    assert!(editor_focused(&editor, cx));
}

#[gpui::test]
fn no_editor_says_no_symbols(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let picker = show(&workbench, cx);
    assert_eq!(status(&picker, cx).as_deref(), Some("No symbols"));
}

#[gpui::test]
fn the_key_toggles_it_and_outline_keeps_its_own(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, editor, cx) = opened(&dir, "sym.rs", &source(), cx);
    show(&workbench, cx);
    press("cmd-shift-o", cx);
    assert!(picker(&workbench, cx).is_none(), "the key hides it again");
    assert!(editor_focused(&editor, cx));

    press("cmd-alt-o", cx);
    assert!(picker(&workbench, cx).is_none());
    let outline = outline_of(&workbench, cx);
    assert!(
        cx.update(|window, cx| outline.focus_handle(cx).is_focused(window)),
        "the Outline panel's new key"
    );
}

#[gpui::test]
fn served_outline_is_listed(cx: &mut TestAppContext) {
    let text = source();
    let dir = tempfile::tempdir().unwrap();
    let (workbench, editor, cx) = opened(&dir, "sym.rs", &text, cx);
    let served = at(&text, "beta");
    serve(
        &workbench,
        &editor,
        vec![served_item("from_server", served.clone(), 204)],
        cx,
    );

    let shown = show(&workbench, cx);
    assert_eq!(rows(&shown, cx), ["from_server"]);
    press("enter", cx);
    assert_eq!(selection(&editor, cx), served, "the server's name range");

    // The server's gone: tree-sitter's again.
    let outline = outline_of(&workbench, cx);
    let id = editor.entity_id();
    outline.update(cx, |outline, cx| outline.set_served(id, None, cx));
    draw(cx);
    let shown = show(&workbench, cx);
    assert_eq!(rows(&shown, cx), ["alpha", "outer", "inner", "beta"]);
}

#[gpui::test]
fn pick_selects_name_range_and_escape_restores_caret(cx: &mut TestAppContext) {
    let text = source();
    let dir = tempfile::tempdir().unwrap();
    let (workbench, editor, cx) = opened(&dir, "sym.rs", &text, cx);
    // The caret is mid-file, so that scrolling to any symbol shows.
    let middle = text.match_indices('\n').nth(49).unwrap().0 + 1;
    editor.update(cx, |editor, cx| {
        editor.select_and_reveal(middle..middle, cx)
    });
    draw(cx);
    let caret = selection(&editor, cx);
    let top = scroll_top(&editor, cx);
    assert!(top > 0., "scrolled to the caret");
    let beta_row = text[..at(&text, "beta").start].matches('\n').count() as u32;
    let outer_row = text[..at(&text, "outer").start].matches('\n').count() as u32;

    // Opening moves nothing.
    show(&workbench, cx);
    assert_eq!(scroll_top(&editor, cx), top, "opening doesn't scroll");

    // Each move shows its symbol; the caret stays.
    press("down", cx);
    assert!(row_shown(&editor, outer_row, cx), "outer is previewed");
    assert!(!row_shown(&editor, 50, cx));
    press("down down", cx);
    assert!(row_shown(&editor, beta_row, cx), "beta is previewed");
    assert_eq!(selection(&editor, cx), caret, "a preview leaves the caret");
    press("up up up", cx);
    assert!(row_shown(&editor, 0, cx), "back on the first row");
    press("down", cx);
    assert!(row_shown(&editor, outer_row, cx));

    // Escape puts it all back.
    press("escape", cx);
    assert!(picker(&workbench, cx).is_none());
    assert_eq!(scroll_top(&editor, cx), top, "the scroll is back");
    assert_eq!(selection(&editor, cx), caret, "the caret is where it was");
    assert!(row_shown(&editor, 50, cx));
    assert!(editor_focused(&editor, cx));

    // Filtering previews the first match.
    show(&workbench, cx);
    cx.simulate_input("bet");
    draw(cx);
    assert!(row_shown(&editor, beta_row, cx), "the first match is shown");

    // Enter selects the name and focuses the editor; back returns.
    press("enter", cx);
    assert!(picker(&workbench, cx).is_none());
    assert_eq!(selection(&editor, cx), at(&text, "beta"));
    assert!(editor_focused(&editor, cx));
    assert!(row_shown(&editor, beta_row, cx), "the name stays in view");
    press("ctrl--", cx);
    assert_eq!(selection(&editor, cx), caret, "a pick is a jump");
}

#[gpui::test]
fn stale_outline_refuses_the_pick(cx: &mut TestAppContext) {
    let text = source();
    let dir = tempfile::tempdir().unwrap();
    let (workbench, editor, cx) = opened(&dir, "sym.rs", &text, cx);
    let caret = selection(&editor, cx);

    // The outline changes after the list is made.
    let shown = show(&workbench, cx);
    press("down", cx);
    let new_name = at(&text, "outer");
    serve(
        &workbench,
        &editor,
        vec![served_item("newer", new_name.clone(), 101)],
        cx,
    );
    assert_eq!(
        rows(&shown, cx),
        ["alpha", "outer", "inner", "beta"],
        "a list stands until a pick finds it stale"
    );

    press("enter", cx);
    assert!(picker(&workbench, cx).is_some(), "the pick is refused");
    assert_eq!(selection(&editor, cx), caret, "and lands nowhere");
    assert_eq!(rows(&shown, cx), ["newer"], "the list is made again");

    press("enter", cx);
    assert!(picker(&workbench, cx).is_none());
    assert_eq!(selection(&editor, cx), new_name, "from the new list");

    // The text moved past the outline: refused too, until it catches up.
    show(&workbench, cx);
    editor.update(cx, |editor, cx| {
        editor.model_mut().set_cursor(0);
        editor.model_mut().insert("// moved\n");
        cx.notify();
    });
    press("enter", cx);
    assert!(picker(&workbench, cx).is_some(), "behind the text: refused");
    assert_eq!(selection(&editor, cx), 9..9, "the edit's caret, no pick");
}

#[gpui::test]
fn a_list_made_before_the_outline_lands_fills_when_it_does(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sym.rs");
    fs::write(&path, source()).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    let outline = outline_of(&workbench, cx);
    outline.update(cx, |outline, _| outline.hold_jobs());
    open(&workbench, &path, cx);

    let shown = show(&workbench, cx);
    assert_eq!(rows(&shown, cx), [] as [&str; 0]);
    assert_eq!(status(&shown, cx).as_deref(), Some("Parsing…"));

    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown");
    outline.update(cx, |outline, _| outline.release_jobs());
    draw(cx);
    assert_eq!(rows(&shown, cx), ["alpha", "outer", "inner", "beta"]);
    assert_eq!(status(&shown, cx), None);
    assert_eq!(scroll_top(&editor, cx), 0., "landing doesn't scroll");
}

#[gpui::test]
fn a_wrapped_row_above_a_symbol_doesnt_hide_it(cx: &mut TestAppContext) {
    let filler = "// filler\n".repeat(100);
    let long = format!("// {}\n", "wrapped ".repeat(4000));
    let text = format!("fn alpha() {{}}\n{filler}{long}fn beta() {{}}\n");
    let beta_row = text[..at(&text, "beta").start].matches('\n').count() as u32;
    let dir = tempfile::tempdir().unwrap();
    let (workbench, editor, cx) = opened(&dir, "sym.rs", &text, cx);
    editor.update(cx, |editor, _| editor.model_mut().set_soft_wrap(true));
    draw(cx);

    show(&workbench, cx);
    press("down", cx);
    assert!(row_shown(&editor, beta_row, cx), "beta is in view");
}

#[gpui::test]
fn a_preview_opens_the_fold_hiding_its_symbol(cx: &mut TestAppContext) {
    let text = source();
    let row = |name: &str| text[..at(&text, name).start].matches('\n').count() as u32;
    let (outer, inner) = (row("outer"), row("inner"));
    let dir = tempfile::tempdir().unwrap();
    let (workbench, editor, cx) = opened(&dir, "sym.rs", &text, cx);
    editor.update(cx, |editor, _| {
        editor.model_mut().fold_rows(outer..=outer + 2)
    });
    draw(cx);
    let caret = selection(&editor, cx);
    let top = scroll_top(&editor, cx);
    let folds = |editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| editor.model().display().folds().to_vec())
    };
    let folded = folds(&editor, cx);
    assert_eq!(folded.len(), 1);

    show(&workbench, cx);
    press("down", cx);
    assert_eq!(
        folds(&editor, cx),
        folded,
        "its header shows without opening"
    );
    press("down", cx);
    assert!(folds(&editor, cx).is_empty(), "inner's fold opens");
    assert!(row_shown(&editor, inner, cx), "inner is in view");
    assert_eq!(selection(&editor, cx), caret, "a preview leaves the caret");

    // Moving on closes it again, as does leaving.
    press("down", cx);
    assert_eq!(folds(&editor, cx), folded);
    press("up", cx);
    assert!(folds(&editor, cx).is_empty());
    press("escape", cx);
    assert_eq!(folds(&editor, cx), folded, "escape closes it");
    assert_eq!(scroll_top(&editor, cx), top);
    assert_eq!(selection(&editor, cx), caret);

    // A pick leaves it open: the name is selected in it.
    show(&workbench, cx);
    press("down down enter", cx);
    assert!(folds(&editor, cx).is_empty());
    assert_eq!(selection(&editor, cx), at(&text, "inner"));
    assert!(row_shown(&editor, inner, cx));
}

#[gpui::test]
fn a_list_with_no_row_to_pick_follows_the_outline(cx: &mut TestAppContext) {
    let text = source();
    let name = at(&text, "beta");
    let dir = tempfile::tempdir().unwrap();

    // An outline with no symbols, then the server's.
    let (workbench, editor, cx) = opened(&dir, "empty.rs", "// nothing\n", cx);
    let shown = show(&workbench, cx);
    assert_eq!(status(&shown, cx).as_deref(), Some("No symbols"));
    serve(&workbench, &editor, vec![served_item("late", 3..7, 0)], cx);
    assert_eq!(rows(&shown, cx), ["late"], "it fills in");
    assert_eq!(status(&shown, cx), None);
    press("enter", cx);
    assert!(picker(&workbench, cx).is_none(), "and it can be picked");
    assert_eq!(selection(&editor, cx), 3..7);

    // A query matching nothing, then an outline that matches.
    let path = dir.path().join("sym.rs");
    fs::write(&path, &text).unwrap();
    open(&workbench, &path, cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown");
    let shown = show(&workbench, cx);
    cx.simulate_input("newer");
    draw(cx);
    assert_eq!(rows(&shown, cx), [] as [&str; 0]);
    serve(
        &workbench,
        &editor,
        vec![served_item("newer", name.clone(), 204)],
        cx,
    );
    assert_eq!(rows(&shown, cx), ["newer"], "the outline landing fills it");
    press("escape", cx);

    // The outline already changed under rows that stood: a query only the
    // new one matches finds it.
    serve(
        &workbench,
        &editor,
        vec![served_item("first", name.clone(), 204)],
        cx,
    );
    let shown = show(&workbench, cx);
    serve(
        &workbench,
        &editor,
        vec![served_item("second", name.clone(), 204)],
        cx,
    );
    assert_eq!(rows(&shown, cx), ["first"], "rows stand until a pick");
    cx.simulate_input("sec");
    draw(cx);
    assert_eq!(rows(&shown, cx), ["second"]);
    press("enter", cx);
    assert!(picker(&workbench, cx).is_none(), "picked from the new list");
    assert_eq!(selection(&editor, cx), name);
}

#[gpui::test]
fn closing_a_previews_fold_leaves_the_selections(cx: &mut TestAppContext) {
    let text = source();
    let row = |name: &str| text[..at(&text, name).start].matches('\n').count() as u32;
    let outer = row("outer");
    let dir = tempfile::tempdir().unwrap();
    let (workbench, editor, cx) = opened(&dir, "sym.rs", &text, cx);
    editor.update(cx, |editor, _| {
        editor.model_mut().fold_rows(outer..=outer + 2)
    });
    // The last `filler`, then the next occurrence wrapping to the first: the
    // primary selection isn't the last.
    let last = text.rfind("filler").unwrap();
    editor.update(cx, |editor, cx| {
        editor.select_and_reveal(last..last + "filler".len(), cx)
    });
    draw(cx);
    press("cmd-d", cx);
    let state = |editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| {
            let model = editor.model();
            (model.selections(), model.primary_selection())
        })
    };
    let before = state(&editor, cx);
    assert_eq!(before.0.len(), 2, "two selections");
    assert_eq!(before.1.range, at(&text, "filler"), "the first is primary");

    // Moving away from inner, whose fold opened, closes it.
    show(&workbench, cx);
    press("down down", cx);
    let open = |editor: &Entity<EditorView>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| editor.model().display().folds().is_empty())
    };
    assert!(open(&editor, cx));
    press("down", cx);
    assert!(!open(&editor, cx));
    assert_eq!(state(&editor, cx), before, "after moving away");

    // So does escape.
    press("up", cx);
    assert!(open(&editor, cx));
    press("escape", cx);
    assert!(!open(&editor, cx));
    assert_eq!(state(&editor, cx), before, "after escape");
}

#[gpui::test]
fn a_caret_moved_into_a_previews_fold_keeps_it_open(cx: &mut TestAppContext) {
    let text = source();
    let row = |name: &str| text[..at(&text, name).start].matches('\n').count() as u32;
    let outer = row("outer");
    let dir = tempfile::tempdir().unwrap();
    let (workbench, editor, cx) = opened(&dir, "sym.rs", &text, cx);
    editor.update(cx, |editor, _| {
        editor.model_mut().fold_rows(outer..=outer + 2)
    });
    draw(cx);
    show(&workbench, cx);
    press("down down", cx);
    // As a go-to-definition's answer landing would.
    let inner = at(&text, "inner");
    editor.update(cx, |editor, cx| editor.select_and_reveal(inner.clone(), cx));
    draw(cx);
    press("escape", cx);
    let folds = editor.read_with(cx, |editor, _| editor.model().display().folds().len());
    assert_eq!(folds, 0, "the caret isn't folded away");
    assert_eq!(selection(&editor, cx), inner);
}
