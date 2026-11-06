//! The find bar's, the terminal's find, the search panel's, the Keyboard
//! Shortcuts filter's and inline edit's inputs take the caret keys (Left,
//! Right, Home, End, the word motions, Delete), as the Settings tab's do
//! (T1240): real keys through the default keymap, then the input's text
//! after typing at the caret.

use std::fs;

use fe_buffer::Buffer;
use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_ui::TextInput;
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{
    Launch, Workbench, inline_edit, open::ConfigDirOverride, shortcuts_editor::ShortcutsEditor,
};

/// The keys are the macOS keymap's: `fe_gpui::init` installs
/// `DEFAULT_MACOS` on every platform, and the tests press its keys.
const WORD_LEFT: &str = "alt-left";
const FIND: &str = "cmd-f";
const SEARCH: &str = "cmd-shift-f";
const SEARCH_DETAILS: &str = "cmd-shift-j";

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

fn type_text(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(text);
    draw(cx);
}

fn text(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> String {
    input.read_with(cx, |input, _| input.text().to_owned())
}

fn focused(input: &Entity<TextInput>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| input.focus_handle(cx).is_focused(window))
}

/// Into `input`, focused and empty: abc, Left, X gives abXc; Home, Y and
/// End, Z put them at either end; Right steps over a character; word-left
/// steps back over a word; Delete takes the character after the caret.
fn caret_keys(input: &Entity<TextInput>, cx: &mut VisualTestContext) {
    assert!(focused(input, cx), "the input has focus");
    assert_eq!(text(input, cx), "");
    type_text("abc", cx);
    press("left", cx);
    type_text("X", cx);
    assert_eq!(text(input, cx), "abXc");
    press("home", cx);
    type_text("Y", cx);
    assert_eq!(text(input, cx), "YabXc");
    press("end", cx);
    type_text("Z", cx);
    assert_eq!(text(input, cx), "YabXcZ");
    press("home right", cx);
    type_text("R", cx);
    assert_eq!(text(input, cx), "YRabXcZ");
    press("end", cx);
    type_text(" de", cx);
    press(WORD_LEFT, cx);
    type_text("W", cx);
    assert_eq!(text(input, cx), "YRabXcZ Wde");
    press("delete", cx);
    assert_eq!(text(input, cx), "YRabXcZ We");
    // The edits the inputs already took still go.
    press("backspace", cx);
    assert_eq!(text(input, cx), "YRabXcZ e");
    assert!(focused(input, cx), "the input keeps focus");
}

/// A workbench on a temporary folder holding `a.txt`, open and focused.
fn editing(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let path = root.join("a.txt");
    fs::write(&path, "one two\n").unwrap();
    let (workbench, cx) = super::open_root(cx, &root);
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
    draw(cx);
    let editor = pane.read_with(cx, |pane, _| pane.active_editor().cloned().unwrap());
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    draw(cx);
    (dir, workbench, cx)
}

/// Runs `id` as a key would in the frames `contexts`, at the focus.
fn run(id: &str, contexts: &[&str], cx: &mut VisualTestContext) {
    let stack = (contexts.iter()).fold(ContextStack::new(), |stack, context| {
        stack.with(ContextFrame::new(*context))
    });
    let run = RunCommand(Invocation::of(id).unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

#[gpui::test]
fn find_bar_takes_caret_keys(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = editing(cx);
    press(FIND, cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let input = pane.read_with(cx, |pane, _| pane.find_bar().unwrap().input().clone());
    caret_keys(&input, cx);
}

#[gpui::test]
fn terminal_find_takes_caret_keys(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    fs::write(&path, "one\n").unwrap();
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("one\n").into()))),
    };
    let (workbench, cx, _) = super::open(cx, launch);
    let panel = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    panel.update(cx, |panel, _| {
        panel.set_shell("/bin/sh", &["-c", "read line"])
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    run("terminal.toggle", &["workbench"], cx);
    let view = panel.read_with(cx, |panel, _| panel.view().unwrap().clone());
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    draw(cx);
    press(FIND, cx);
    let input = panel.read_with(cx, |panel, _| panel.find().unwrap().input().clone());
    caret_keys(&input, cx);
}

#[gpui::test]
fn search_panel_takes_caret_keys(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "one\n").unwrap();
    let (workbench, cx) = super::open_root(cx, dir.path());
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    press(SEARCH, cx);
    let search = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).search().clone()
    });
    let query = search.read_with(cx, |search, _| search.input().clone());
    caret_keys(&query, cx);
    let pattern = search.read_with(cx, |search, cx| search.query(cx).pattern);
    assert_eq!(pattern, "YRabXcZ e");

    // The include and exclude filters, shown.
    let (include, exclude) = search.read_with(cx, |search, _| {
        (
            search.include_input().clone(),
            search.exclude_input().clone(),
        )
    });
    if !search.read_with(cx, |search, _| search.details_shown()) {
        press(SEARCH_DETAILS, cx);
    }
    assert!(search.read_with(cx, |search, _| search.details_shown()));
    for filter in [include, exclude] {
        cx.update(|window, cx| window.focus(&filter.focus_handle(cx), cx));
        draw(cx);
        caret_keys(&filter, cx);
    }
}

#[gpui::test]
fn shortcuts_editor_filter_takes_caret_keys(cx: &mut TestAppContext) {
    let temp = tempfile::tempdir().unwrap();
    let config = dunce::canonicalize(temp.path()).unwrap().join("config");
    fs::create_dir_all(&config).unwrap();
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, cx, _) = super::open(cx, launch);
    cx.update(|window, cx| {
        cx.set_global(ConfigDirOverride(Some(config)));
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(cx);
    run("workbench.open_keyboard_shortcuts", &["workbench"], cx);
    let editor = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let tab = &pane.tabs()[pane.active().unwrap()];
        tab.item().downcast::<ShortcutsEditor>().unwrap()
    });
    let input = editor.read_with(cx, |editor, _| editor.search_input().clone());
    caret_keys(&input, cx);
}

#[gpui::test]
fn inline_edit_takes_caret_keys(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = editing(cx);
    let editor = workbench.read_with(cx, |workbench, cx| {
        workbench.pane().read(cx).active_editor().cloned().unwrap()
    });
    run("ai.inline_edit", &["workbench", "editor"], cx);
    let open = cx.update(|_, cx| inline_edit::open_for(editor.entity_id(), cx));
    let input = open
        .expect("the input is open")
        .read_with(cx, |open, _| open.input().clone());
    caret_keys(&input, cx);
    // The editor under the input took none of the keys.
    let text = editor.read_with(cx, |editor, _| editor.model().text().to_string());
    assert_eq!(text, "one two\n");
}
