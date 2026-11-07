//! Search Keymaps: the keymap's bindings with their commands' titles, the
//! active editor's vim maps with their leader spelled out, and Enter running
//! a row's command, or typing a map's keys, in the editor focus was in.

use std::{fs, path::Path};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::{EditorView, commands::RunCommand, set_vim_enabled, set_vim_remaps};
use fe_vim::{Remap, RemapMode};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::VIM_NORMAL;
use crate::{PickerDelegate as _, Workbench, tests::open_root, workbench::Palette};

const TEXT: &str = "one\ntwo\nthree\n";

/// A workbench showing `notes.txt` holding [`TEXT`], focused.
fn opened(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<Workbench>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.txt");
    fs::write(&path, TEXT).unwrap();
    let (workbench, cx) = open_root(cx, dir.path());
    cx.update(|window, _| window.activate_window());
    open(&workbench, &path, cx);
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a file is shown");
    (dir, workbench, editor, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Opens `path` in the active pane, focused.
fn open(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
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

/// The picker's rows: keys, title and context.
fn rows(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(String, String, String)> {
    workbench.read_with(cx, |workbench, cx| {
        let picker = (workbench.keymaps_picker(cx)).expect("the keymaps picker shows");
        picker.read(cx).delegate.labels()
    })
}

fn row(keys: &str, title: &str, context: &str) -> (String, String, String) {
    (keys.to_owned(), title.to_owned(), context.to_owned())
}

/// Types `query` into the picker's input.
fn type_query(workbench: &Entity<Workbench>, query: &str, cx: &mut VisualTestContext) {
    let picker = (workbench.read_with(cx, |workbench, cx| workbench.keymaps_picker(cx)))
        .expect("the keymaps picker shows");
    let input = picker.read_with(cx, |picker, _| picker.input().clone());
    // Setting the text doesn't say it changed.
    input.update(cx, |input, cx| input.set_text(query, cx));
    picker.update(cx, |picker, cx| {
        picker.delegate.update_matches(query.to_owned(), cx)
    });
    draw(cx);
}

/// Confirms the row `pick` finds among the rows.
fn confirm_row(
    workbench: &Entity<Workbench>,
    pick: impl Fn(&(String, String, String)) -> bool,
    cx: &mut VisualTestContext,
) {
    let ix = (rows(workbench, cx).iter().position(pick)).expect("the row is listed");
    let picker = (workbench.read_with(cx, |workbench, cx| workbench.keymaps_picker(cx)))
        .expect("the keymaps picker shows");
    cx.update(|window, cx| picker.update(cx, |picker, cx| picker.confirm(ix, window, cx)));
    draw(cx);
}

/// `command`'s first binding's keys in the keymap in use, as a row shows
/// them.
fn keys_of(command: &str, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| {
        let keymap = fe_gpui::current_keymap(cx);
        let binding = (keymap.bindings().iter())
            .find(|binding| binding.command.as_str() == command)
            .expect("the default keymap binds it");
        fe_keymap::shortcut_label(&binding.keys)
    })
}

fn palette(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Palette> {
    workbench.read_with(cx, |workbench, _| workbench.palette)
}

#[gpui::test]
fn lists_default_bindings_with_titles(cx: &mut TestAppContext) {
    let (_dir, workbench, _, cx) = opened(cx);
    run("keymaps.search", cx);
    assert_eq!(palette(&workbench, cx), Some(Palette::Keymaps));
    let listed = rows(&workbench, cx);
    let select_all = keys_of("editor.select_all", cx);
    let go_to_file = keys_of("quick_open.toggle", cx);
    assert!(
        listed.contains(&row(&select_all, "Select All", "editor")),
        "{listed:?}"
    );
    assert!(
        listed.contains(&row(&go_to_file, "Go to File", "workbench")),
        "{listed:?}"
    );
    // By title.
    let titles: Vec<String> = listed
        .iter()
        .map(|(_, title, _)| title.to_lowercase())
        .collect();
    assert!(titles.is_sorted(), "{titles:?}");
    // No vim maps with vim off.
    assert!(!listed.iter().any(|(.., context)| context == VIM_NORMAL));

    // Typing matches the title, and the keys.
    type_query(&workbench, "go to file", cx);
    assert!(rows(&workbench, cx).contains(&row(&go_to_file, "Go to File", "workbench")));
    type_query(&workbench, &go_to_file, cx);
    assert!(rows(&workbench, cx).contains(&row(&go_to_file, "Go to File", "workbench")));
}

#[gpui::test]
fn includes_user_vim_remaps(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    let remap = |from: &str, to: &str| Remap {
        modes: vec![RemapMode::Normal],
        from: from.to_owned(),
        to: to.to_owned(),
        recursive: false,
    };
    let remaps = [
        remap("<leader>fn", "<Cmd>fe:workbench.new_file<CR>"),
        remap("<leader>xx", "dd"),
        remap("Q", "gg"),
    ];
    let issues = cx.update(|_, cx| set_vim_remaps(cx, &remaps, "<Space>"));
    assert_eq!(issues, []);
    draw(cx);

    run("keymaps.search", cx);
    let maps: Vec<_> = (rows(&workbench, cx).into_iter())
        .filter(|(.., context)| context == VIM_NORMAL)
        .collect();
    // The leader spelled out; a command's map by its title, any other by
    // its keys.
    assert_eq!(
        maps,
        [
            row("<Space>fn", "New File", VIM_NORMAL),
            row("<Space>xx", "dd", VIM_NORMAL),
            row("Q", "gg", VIM_NORMAL),
        ]
    );

    // Enter on a map of keys types them in the editor: `dd` takes the
    // first row.
    type_query(&workbench, "spacexx", cx);
    confirm_row(&workbench, |(keys, ..)| keys == "<Space>xx", cx);
    assert_eq!(palette(&workbench, cx), None);
    let text = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| editor.model().buffer().text().to_string())
    };
    assert_eq!(text(cx), "two\nthree\n");

    // From insert mode, it's back in normal mode first: nothing is typed
    // as text.
    cx.simulate_keystrokes("i");
    draw(cx);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.vim_mode()),
        Some("insert")
    );
    run("keymaps.search", cx);
    confirm_row(&workbench, |(keys, ..)| keys == "<Space>xx", cx);
    assert_eq!(text(cx), "three\n");
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.vim_mode()),
        Some("normal")
    );
}

#[gpui::test]
fn enter_runs_the_command_in_the_focused_editor(cx: &mut TestAppContext) {
    let (_dir, workbench, editor, cx) = opened(cx);
    run("keymaps.search", cx);
    type_query(&workbench, "select all", cx);
    confirm_row(
        &workbench,
        |(_, title, context)| title == "Select All" && context == "editor",
        cx,
    );
    assert_eq!(palette(&workbench, cx), None);
    let selection = editor.read_with(cx, |editor, _| editor.model().primary_selection().range);
    assert_eq!(selection, 0..TEXT.len(), "the editor's text is selected");
    let focused = cx.update(|window, cx| editor.focus_handle(cx).is_focused(window));
    assert!(focused, "focus is back in the editor");
}
