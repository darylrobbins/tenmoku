//! Show Buffers: the open editors most recent first, marked modified and
//! current, the previous one selected, and Enter activating a row's tab.

use std::fs;
use std::path::Path;

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use gpui::{Entity, TestAppContext, VisualTestContext};

use crate::{Pane, PickerDelegate as _, Workbench, tests::open_root, workbench::Palette};

/// A workbench rooted at a temp dir holding `a.rs`, `b.rs` and `src/c.rs`,
/// with `names` opened in order in its pane.
fn opened<'a>(
    names: &[&str],
    cx: &'a mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    &'a mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    for name in ["a.rs", "b.rs", "src/c.rs"] {
        fs::write(dir.path().join(name), format!("// {name}\n")).unwrap();
    }
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, _| window.activate_window());
    for name in names {
        open(&workbench, &dir.path().join(name), cx);
    }
    (dir, workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn pane(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<Pane> {
    workbench.read_with(cx, |workbench, _| workbench.pane().clone())
}

/// Opens `path` in the active pane, focused.
fn open(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = pane(workbench, cx);
    let path = path.to_owned();
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    cx.update(|window, cx| {
        let focus = pane.read(cx).focus_target(cx);
        window.focus(&focus, cx);
    });
    draw(cx);
}

/// Runs command `id` where focus is, in the workbench's context.
fn run(id: &str, cx: &mut VisualTestContext) {
    let context = ContextStack::new().with(ContextFrame::new("workbench"));
    let run = RunCommand(Invocation::of(id).unwrap().context(context));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn show_buffers(cx: &mut VisualTestContext) {
    run("buffers.show", cx);
}

/// The picker's rows (name, folder, modified, current) and its selection.
fn rows(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Vec<(String, String, bool, bool)>, usize) {
    workbench.read_with(cx, |workbench, cx| {
        let picker = workbench
            .buffer_picker(cx)
            .expect("the buffer picker shows");
        let picker = picker.read(cx);
        (picker.delegate.labels(), picker.selected())
    })
}

fn names(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    (rows(workbench, cx).0.into_iter())
        .map(|(name, ..)| name)
        .collect()
}

/// The active pane's tab count and its shown tab's title.
fn shown(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (usize, String) {
    let pane = pane(workbench, cx);
    pane.read_with(cx, |pane, cx| {
        let title = pane.active_item().unwrap().tab_title(cx).to_string();
        (pane.tabs().len(), title)
    })
}

fn palette(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Palette> {
    workbench.read_with(cx, |workbench, _| workbench.palette)
}

/// Types `query` into the picker's input.
fn type_query(workbench: &Entity<Workbench>, query: &str, cx: &mut VisualTestContext) {
    let picker = (workbench.read_with(cx, |workbench, cx| workbench.buffer_picker(cx)))
        .expect("the buffer picker shows");
    let input = picker.read_with(cx, |picker, _| picker.input().clone());
    // Setting the text doesn't say it changed.
    input.update(cx, |input, cx| input.set_text(query, cx));
    picker.update(cx, |picker, cx| {
        picker.delegate.update_matches(query.to_owned(), cx)
    });
    draw(cx);
}

/// Confirms row `ix`.
fn confirm(workbench: &Entity<Workbench>, ix: usize, cx: &mut VisualTestContext) {
    let picker = (workbench.read_with(cx, |workbench, cx| workbench.buffer_picker(cx)))
        .expect("the buffer picker shows");
    cx.update(|window, cx| picker.update(cx, |picker, cx| picker.confirm(ix, window, cx)));
    draw(cx);
}

#[gpui::test]
fn lists_open_editors_most_recent_first(cx: &mut TestAppContext) {
    let (dir, workbench, cx) = opened(&["a.rs", "src/c.rs", "b.rs"], cx);
    show_buffers(cx);
    assert_eq!(palette(&workbench, cx), Some(Palette::Buffers));
    assert_eq!(names(&workbench, cx), ["b.rs", "c.rs", "a.rs"]);
    let folders: Vec<String> = (rows(&workbench, cx).0.into_iter())
        .map(|(_, folder, ..)| folder)
        .collect();
    assert_eq!(folders, ["", "src", ""], "folders from the root");

    // Showing `a.rs` again puts it first, and `b.rs` after it.
    cx.update(|window, cx| {
        (workbench.clone()).update(cx, |workbench, cx| workbench.dismiss_palette(window, cx))
    });
    open(&workbench, &dir.path().join("a.rs"), cx);
    show_buffers(cx);
    assert_eq!(names(&workbench, cx), ["a.rs", "b.rs", "c.rs"]);

    // Typing filters by the path from the root.
    type_query(&workbench, "srcc", cx);
    assert_eq!(names(&workbench, cx), ["c.rs"]);
}

#[gpui::test]
fn marks_modified_and_current(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(&["a.rs", "src/c.rs", "b.rs"], cx);
    // `c.rs` gets an unsaved change.
    let pane = pane(&workbench, cx);
    let editor = pane.read_with(cx, |pane, _| {
        let c = (pane.tabs().iter())
            .find(|tab| {
                tab.path
                    .as_ref()
                    .is_some_and(|path| path.ends_with("src/c.rs"))
            })
            .unwrap();
        c.editor().unwrap().clone()
    });
    editor.update(cx, |editor, _| editor.model_mut().insert("edit "));
    draw(cx);

    show_buffers(cx);
    let (listed, _) = rows(&workbench, cx);
    let flags: Vec<(&str, bool, bool)> = (listed.iter())
        .map(|(name, _, modified, current)| (name.as_str(), *modified, *current))
        .collect();
    assert_eq!(
        flags,
        [
            ("b.rs", false, true),
            ("c.rs", true, false),
            ("a.rs", false, false)
        ]
    );
}

#[gpui::test]
fn first_row_selected_is_the_previous_buffer(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(&["a.rs", "b.rs"], cx);
    show_buffers(cx);
    let (listed, selected) = rows(&workbench, cx);
    assert_eq!(listed[0].0, "b.rs");
    assert!(listed[0].3, "the current buffer is listed first");
    assert_eq!(selected, 1, "the previous buffer is selected");

    // So Enter goes back to it, as alt-tab does.
    let picker = workbench.read_with(cx, |workbench, cx| workbench.buffer_picker(cx).unwrap());
    let selected = picker.read_with(cx, |picker, _| picker.selected());
    confirm(&workbench, selected, cx);
    assert_eq!(shown(&workbench, cx).1, "a.rs");

    // A query selects its first match, even the current buffer.
    show_buffers(cx);
    type_query(&workbench, "a", cx);
    assert_eq!(
        rows(&workbench, cx),
        (vec![("a.rs".into(), "".into(), false, true)], 0)
    );
}

#[gpui::test]
fn first_row_selected_is_the_only_buffer(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(&["a.rs"], cx);
    show_buffers(cx);
    assert_eq!(rows(&workbench, cx).1, 0);
}

#[gpui::test]
fn enter_on_a_row_activates_that_editor(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(&["a.rs", "src/c.rs", "b.rs"], cx);
    assert_eq!(shown(&workbench, cx), (3, "b.rs".into()));
    show_buffers(cx);
    assert_eq!(names(&workbench, cx)[2], "a.rs");
    confirm(&workbench, 2, cx);
    // Its tab is shown, no other opened, and the picker is gone.
    assert_eq!(shown(&workbench, cx), (3, "a.rs".into()));
    assert_eq!(palette(&workbench, cx), None);
    // Focus is in the editor.
    let pane = pane(&workbench, cx);
    let focused =
        cx.update(|window, cx| pane.read(cx).focus_target(cx).contains_focused(window, cx));
    assert!(focused);
}

#[gpui::test]
fn an_untitled_buffer_in_another_pane_activates_that_pane(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(&["a.rs"], cx);
    let first = pane(&workbench, cx);
    run("pane.split_right", cx);
    run("workbench.new_file", cx);
    let second = pane(&workbench, cx);
    assert_ne!(first.entity_id(), second.entity_id());
    let untitled = shown(&workbench, cx);
    assert_eq!(untitled.0, 2, "a.rs and the untitled buffer");
    cx.update(|window, cx| {
        (workbench.clone()).update(cx, |workbench, cx| {
            workbench.activate(first.clone(), window, cx)
        })
    });
    draw(cx);

    show_buffers(cx);
    // The first pane's a.rs, then the second pane's untitled buffer: its
    // a.rs is the same file.
    assert_eq!(
        names(&workbench, cx),
        ["a.rs".to_owned(), untitled.1.clone()]
    );
    confirm(&workbench, 1, cx);

    // The second pane is the active one, showing it, and commands go there.
    assert_eq!(pane(&workbench, cx).entity_id(), second.entity_id());
    assert_eq!(shown(&workbench, cx), untitled);
    let editor = second.read_with(cx, |pane, _| pane.active_editor().unwrap().entity_id());
    let active = workbench.read_with(cx, |workbench, cx| {
        workbench.active_editor(cx).map(|editor| editor.entity_id())
    });
    assert_eq!(active, Some(editor));
    run("workbench.close_tab", cx);
    assert_eq!(second.read_with(cx, |pane, _| pane.tabs().len()), 1);
    assert_eq!(first.read_with(cx, |pane, _| pane.tabs().len()), 1);
}
