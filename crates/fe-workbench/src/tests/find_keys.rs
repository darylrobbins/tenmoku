//! The default keymap's in-file find keys: the table, and the keys driving
//! the pane's find bar in a window.

use std::{fs, ops::Range};

use fe_command::{ContextFrame, ContextStack};
use fe_gpui::{EditorSettings, EditorTheme, EditorView, set_vim_enabled};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::{
    Picker,
    command_palette::{COMMAND_PALETTE, CommandPalette},
};

use crate::{Launch, Pane, Workbench};

const TABLE: [(&str, &str, &str); 15] = [
    ("cmd-f", "find.show", "editor"),
    ("cmd-g", "find.next", "editor || find_bar"),
    ("cmd-shift-g", "find.previous", "editor || find_bar"),
    ("enter", "find.next", "find_bar && text_input"),
    ("shift-enter", "find.previous", "find_bar && text_input"),
    ("escape", "find.close", "find_bar"),
    ("alt-cmd-c", "find.toggle_case", "find_bar"),
    ("alt-cmd-w", "find.toggle_whole_word", "find_bar"),
    ("alt-cmd-r", "find.toggle_regex", "find_bar"),
    ("alt-cmd-p", "find.toggle_preserve_case", "find_bar"),
    ("alt-cmd-l", "find.toggle_in_selection", "find_bar"),
    ("alt-cmd-f", "find.show_replace", "workbench"),
    ("cmd-!", "find.replace_next", "find_bar"),
    ("alt-cmd-enter", "find.replace_all", "find_bar"),
    (
        "enter",
        "find.replace_next",
        "find_bar && replace_input && text_input",
    ),
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

/// Replace's key opens it in the active pane from a panel too, where the
/// pane's own handler isn't on focus's path, and from the pane itself with
/// no editor focused; the palette lists it from a panel.
#[gpui::test]
fn replace_opens_from_a_panel(cx: &mut TestAppContext) {
    for vim in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, "foo one\nfoo two\n").unwrap();
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

        // The project tree, then Problems, then the pane's own focus.
        for panel in ["cmd-shift-e", "cmd-shift-m", "pane"] {
            let name = format!("vim {vim}, {panel}");
            if panel == "pane" {
                cx.update(|window, cx| window.focus(&pane.focus_handle(cx), cx));
                draw(cx);
            } else {
                keys(panel, cx);
            }
            assert!(!editor_focused(&editor, cx), "{name}: the panel has focus");
            keys("alt-cmd-f", cx);
            assert!(open(&pane, cx), "{name}");
            let (replacing, focused) = cx.update(|window, cx| {
                let bar = pane.read(cx).find_bar().unwrap();
                let input = bar.replace_input().focus_handle(cx);
                (bar.is_replacing(), input.is_focused(window))
            });
            assert!(replacing, "{name}");
            assert!(focused, "{name}: the replace input has focus");
            keys("escape", cx);
            assert!(!open(&pane, cx), "{name}");
        }

        // The palette, opened from the tree, lists Replace with its key.
        keys("cmd-shift-e", cx);
        keys("cmd-shift-p", cx);
        cx.simulate_input("Replace");
        draw(cx);
        let picker = palette(&workbench, cx).expect("the palette shows");
        let titles: Vec<_> = picker.read_with(cx, |picker, cx| {
            let palette = &picker.delegate;
            (palette.matches().iter().enumerate())
                .map(|(ix, entry)| {
                    let key = palette.key_label(ix, cx).map(|key| key.to_string());
                    (entry.title.to_string(), key)
                })
                .collect()
        });
        assert!(
            titles
                .iter()
                .any(|(title, key)| title == "Replace…" && key.is_some()),
            "vim {vim}: {titles:?}"
        );
        keys("escape", cx);
    }
}

/// Replace, by its key and as the menu runs it, opens in the active pane
/// from the terminal's find input too: the row has no replace of its own,
/// so it lets the command through to the workbench's fallback.
#[gpui::test]
#[cfg_attr(windows, ignore = "runs a /bin/sh script in the terminal")]
fn replace_opens_from_the_terminal_find_input(cx: &mut TestAppContext) {
    use fe_buffer::Buffer;
    use fe_command::Invocation;
    use fe_gpui::commands::RunCommand;

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
    let stack = |frames: &[&str]| -> ContextStack {
        frames.iter().map(|name| ContextFrame::new(*name)).collect()
    };
    let run = RunCommand(
        Invocation::of("terminal.toggle")
            .unwrap()
            .context(stack(&["workbench"])),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
    let view = panel.read_with(cx, |panel, _| panel.view().unwrap().clone());
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    draw(cx);
    keys("cmd-f", cx);
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());

    for menu in [false, true] {
        let input = panel.read_with(cx, |panel, _| panel.find().unwrap().input().clone());
        cx.update(|window, cx| window.focus(&input.focus_handle(cx), cx));
        draw(cx);
        assert!(!open(&pane, cx), "menu {menu}");
        if menu {
            let run = RunCommand(
                Invocation::of("find.show_replace")
                    .unwrap()
                    .context(stack(&["workbench", "find_bar", "text_input"])),
            );
            cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
            draw(cx);
        } else {
            keys("alt-cmd-f", cx);
        }
        assert!(open(&pane, cx), "menu {menu}");
        assert!(
            pane.read_with(cx, |pane, _| pane.find_bar().unwrap().is_replacing()),
            "menu {menu}"
        );
        keys("escape", cx);
        assert!(!open(&pane, cx), "menu {menu}");
    }
}
