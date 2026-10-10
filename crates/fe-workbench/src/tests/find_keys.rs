//! The default keymap's in-file find keys: the table, and the keys driving
//! the pane's find bar in a window.

use std::{fs, ops::Range};

use fe_command::{ContextFrame, ContextStack};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, set_vim_enabled};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use crate::{Launch, Pane, Workbench};

const TABLE: [(&str, &str, &str); 9] = [
    ("cmd-f", "find.show", "editor"),
    ("cmd-g", "find.next", "editor || find_bar"),
    ("cmd-shift-g", "find.previous", "editor || find_bar"),
    ("enter", "find.next", "find_bar && text_input"),
    ("shift-enter", "find.previous", "find_bar && text_input"),
    ("escape", "find.close", "find_bar"),
    ("alt-cmd-c", "find.toggle_case", "find_bar"),
    ("alt-cmd-w", "find.toggle_whole_word", "find_bar"),
    ("alt-cmd-r", "find.toggle_regex", "find_bar"),
];

#[test]
fn default_bindings() {
    let store = fe_keymap::KeymapStore::load(
        fe_keymap::DEFAULT_MACOS,
        None,
        Some(crate::command_catalog()),
    );
    assert_eq!(store.diagnostics(), []);
    let keymap = store.keymap();
    let found: Vec<(String, &str, String)> = keymap
        .bindings()
        .iter()
        .filter(|binding| binding.command.as_str().starts_with("find."))
        .map(|binding| {
            let context = binding.context.as_ref().map(ToString::to_string);
            (
                binding.keys.to_string(),
                binding.command.as_str(),
                context.unwrap_or_default(),
            )
        })
        .collect();
    let expected: Vec<(String, &str, String)> = TABLE
        .iter()
        .map(|&(keys, command, context)| {
            let keys = fe_keymap::KeySequence::parse(keys).unwrap().to_string();
            (keys, command, context.to_owned())
        })
        .collect();
    assert_eq!(found, expected);

    let stack = |frames: &[&str]| -> ContextStack {
        frames.iter().map(|name| ContextFrame::new(*name)).collect()
    };
    let issues = keymap.report_with(&[
        stack(&["workbench", "editor"]),
        stack(&["workbench", "find_bar", "text_input"]),
    ]);
    assert_eq!(issues, []);
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn selection(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> Range<usize> {
    editor.read_with(cx, |editor, _| editor.model().primary_selection().range)
}

fn keys(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

fn open(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> bool {
    pane.read_with(cx, |pane, _| {
        pane.find_bar().is_some_and(|bar| bar.is_open())
    })
}

fn match_case(pane: &Entity<Pane>, cx: &mut VisualTestContext) -> bool {
    pane.read_with(cx, |pane, cx| pane.find_bar().unwrap().query(cx).match_case)
}

fn editor_focused(editor: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

#[gpui::test]
fn keys_drive_the_bar(cx: &mut TestAppContext) {
    for vim in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, "foo one\nfoo two\nfoo three\n").unwrap();
        cx.update(|cx| {
            fe_gpui::init(cx);
            crate::init(cx);
            set_vim_enabled(cx, vim);
        });
        let launch = Launch {
            root: Some(dir.path().to_owned()),
            file: None,
        };
        let (workbench, cx) = cx.add_window_view(move |window, cx| {
            let theme = EditorTheme::default_dark(cx);
            Workbench::new(launch, theme, EditorSettings::default(), window, cx)
        });
        cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
        draw(cx);
        let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(path, window, cx)));
        draw(cx);
        let editor = pane
            .read_with(cx, |pane, _| pane.active_editor().cloned())
            .unwrap();
        assert!(editor_focused(&editor, cx), "vim {vim}");

        keys("cmd-f", cx);
        assert!(open(&pane, cx), "vim {vim}");
        keys("f o o", cx);
        assert_eq!(selection(&editor, cx).start, 0, "vim {vim}");
        let starts = |cx: &mut VisualTestContext, keys_: &str| {
            keys(keys_, cx);
            selection(&editor, cx).start
        };
        assert_eq!(starts(cx, "enter"), 8, "vim {vim}");
        assert_eq!(starts(cx, "enter"), 16, "vim {vim}");
        assert_eq!(starts(cx, "shift-enter"), 8, "vim {vim}");
        assert_eq!(starts(cx, "cmd-g"), 16, "vim {vim}");
        assert_eq!(starts(cx, "cmd-shift-g"), 8, "vim {vim}");

        assert!(!match_case(&pane, cx));
        keys("alt-cmd-c", cx);
        assert!(match_case(&pane, cx), "vim {vim}");

        keys("escape", cx);
        assert!(!open(&pane, cx), "vim {vim}");
        assert!(editor_focused(&editor, cx), "vim {vim}");
        // From the editor, with the bar closed, cmd-g still steps.
        assert_eq!(starts(cx, "cmd-g"), 16, "vim {vim}");
    }
}
