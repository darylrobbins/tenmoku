//! The commands that moved from `fe-command` registries to typed commands
//! (`#[kiln::command]` methods in `workbench_commands.rs`) keep the ID,
//! title and context they had: each is a registered typed command with
//! them, and no registry in [`crate::command_catalog`] still claims it.
//! Each family that moves appends its commands to [`MOVED`].

use fe_buffer::Buffer;
use fe_command::{CommandId, ContextFrame, ContextPredicate, ContextStack, Invocation, Value};
use fe_gpui::{EditorSettings, EditorTheme, commands::RunCommand};
use fe_keymap::CatalogError;
use gpui::{Focusable as _, TestAppContext, VisualTestContext};
use kiln::platform::command_actions::typed_command;

use crate::{Launch, Workbench};

/// Each moved command's ID, title and key context, as its registry had them.
const MOVED: &[(&str, &str, &str)] = &[
    ("workbench.close_tab", "Close Tab", "workbench"),
    ("workbench.next_tab", "Next Tab", "workbench"),
    ("workbench.previous_tab", "Previous Tab", "workbench"),
    ("pane.split_right", "Split Right", "workbench"),
    ("pane.split_down", "Split Down", "workbench"),
    ("pane.focus_left", "Focus Left Pane", "workbench"),
    ("pane.focus_right", "Focus Right Pane", "workbench"),
    ("pane.focus_up", "Focus Pane Above", "workbench"),
    ("pane.focus_down", "Focus Pane Below", "workbench"),
    ("pane.move_tab_left", "Move Tab Left", "workbench"),
    ("pane.move_tab_right", "Move Tab Right", "workbench"),
    ("pane.move_tab_up", "Move Tab Up", "workbench"),
    ("pane.move_tab_down", "Move Tab Down", "workbench"),
    ("pane.close", "Close Pane", "workbench"),
    ("pane.grow_width", "Grow Pane Width", "workbench"),
    ("pane.shrink_width", "Shrink Pane Width", "workbench"),
    ("pane.grow_height", "Grow Pane Height", "workbench"),
    ("pane.shrink_height", "Shrink Pane Height", "workbench"),
    ("pane.equalize", "Equalize Panes", "workbench"),
    ("pane.focus_next", "Focus Next Pane", "workbench"),
    ("pane.focus_previous", "Focus Previous Pane", "workbench"),
    ("dock.grow", "Grow Dock", "workbench"),
    ("dock.shrink", "Shrink Dock", "workbench"),
    (
        "terminal.move_to_bottom",
        "Move Terminal to Bottom",
        "workbench",
    ),
    (
        "terminal.move_to_left",
        "Move Terminal to Left",
        "workbench",
    ),
    (
        "terminal.move_to_right",
        "Move Terminal to Right",
        "workbench",
    ),
    (
        "dock.move_sidebar_left",
        "Move Sidebar to Left",
        "workbench",
    ),
    (
        "dock.move_sidebar_right",
        "Move Sidebar to Right",
        "workbench",
    ),
    ("view.toggle_status_bar", "Toggle Status Bar", "workbench"),
    ("quick_open.toggle", "Go to File", "workbench"),
    ("go_to_line.toggle", "Go to Line/Column…", "workbench"),
    ("toast.dismiss_all", "Dismiss Notifications", "workbench"),
    (
        "workbench.show_config_problems",
        "Show Config Problems",
        "workbench",
    ),
    ("view.toggle_breadcrumbs", "Toggle Breadcrumbs", "workbench"),
    ("language.select", "Change Language Mode…", "workbench"),
    ("nav.goto_definition", "Go to Definition", "workbench"),
    (
        "nav.workspace_symbols",
        "Go to Symbol in Workspace…",
        "workbench",
    ),
    ("nav.back", "Go Back", "workbench"),
    ("nav.forward", "Go Forward", "workbench"),
    ("workbench.open_recent", "Open Recent…", "workbench"),
    (
        "workbench.clear_recent",
        "Clear Recently Opened",
        "workbench",
    ),
    (
        "recent.open_in_new_window",
        "Open Recent in New Window",
        "picker",
    ),
    ("lsp.restart", "Restart Language Server", "workbench"),
    ("lsp.show_log", "Language Server Log", "workbench"),
    ("lsp.rename", "Rename Symbol…", "editor"),
    ("lsp.code_actions", "Code Actions…", "editor"),
    ("lsp.format_document", "Format Document", "editor"),
    ("lsp.format_selection", "Format Selection", "editor"),
    ("workbench.open_folder", "Open Folder…", "workbench"),
    ("workbench.open_file", "Open File…", "workbench"),
    ("workbench.open_settings", "Settings…", "workbench"),
    ("window.new", "New Window", "workbench"),
    (
        "workbench.open_folder_in_new_window",
        "Open Folder in New Window…",
        "workbench",
    ),
    ("workbench.new_file", "New File", "workbench"),
    ("workbench.save_as", "Save As…", "workbench"),
    ("workbench.trust_folder", "Trust This Folder", "workbench"),
    (
        "workbench.untrust_folder",
        "Untrust This Folder",
        "workbench",
    ),
    ("find.show", "Find", "editor"),
    ("find.show_replace", "Replace…", "editor || find_bar"),
    ("find.next", "Find Next", "editor || find_bar"),
    ("find.previous", "Find Previous", "editor || find_bar"),
    ("find.close", "Close Find", "find_bar"),
    ("find.toggle_case", "Match Case", "find_bar"),
    ("find.toggle_whole_word", "Match Whole Word", "find_bar"),
    ("find.toggle_regex", "Use Regular Expression", "find_bar"),
    ("find.toggle_replace", "Toggle Replace", "find_bar"),
    ("find.replace_next", "Replace", "find_bar"),
    ("find.replace_all", "Replace All", "find_bar"),
    ("terminal.toggle", "Toggle Terminal", "workbench"),
    ("terminal.new", "New Terminal", "workbench"),
    ("terminal.close", "Close Terminal", "workbench"),
    ("terminal.next", "Next Terminal", "workbench"),
    ("terminal.previous", "Previous Terminal", "workbench"),
    ("terminal.find", "Terminal: Find", "workbench"),
    (
        "terminal.previous_prompt",
        "Terminal: Previous Prompt",
        "terminal",
    ),
    ("terminal.next_prompt", "Terminal: Next Prompt", "terminal"),
    ("terminal.vi_mode", "Terminal: Vi Copy Mode", "terminal"),
];

#[gpui::test]
fn moved_commands_keep_their_ids_titles_and_contexts(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    cx.read(|cx| {
        for &(id, title, context) in MOVED {
            let command = typed_command(&CommandId::parse(id).unwrap(), cx);
            let command = command.unwrap_or_else(|| panic!("{id} is a typed command"));
            assert_eq!(command.title, title, "{id}");
            assert_eq!(
                command.context,
                Some(ContextPredicate::parse(context).unwrap()),
                "{id}"
            );
        }
    });
}

#[test]
fn no_registry_still_claims_a_moved_command() {
    let registries = crate::registry_catalogs();
    for &(id, _, _) in MOVED {
        let id = CommandId::parse(id).unwrap();
        for registry in &registries {
            assert_eq!(
                registry.check(&id, &Value::Null),
                Err(CatalogError::UnknownCommand),
                "{id}"
            );
        }
        assert_eq!(crate::command_catalog().check(&id, &Value::Null), Ok(()));
    }
}

/// The LSP commands in the editor's context run only where it holds, as
/// their registry's dispatch checked: a menu pick or `RunCommand` with the
/// project panel or the terminal focused does nothing. With no language
/// server, rename and code actions say so in a prompt, and formatting in
/// a toast.
#[gpui::test]
fn editor_lsp_commands_run_only_in_the_editors_context(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "alpha\n").unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: None,
        file: Some((path, Some(Buffer::new("alpha\n").into()))),
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.run_until_parked();
    assert!(!cx.has_pending_prompt());

    let toasts = |cx: &mut VisualTestContext| {
        let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
        layer.read_with(cx, |layer, _| {
            (layer.queue().shown().iter())
                .map(|entry| (entry.toast.message.clone(), entry.count))
                .collect::<Vec<_>>()
        })
    };
    let run = |id: &str, frames: &[&'static str], cx: &mut VisualTestContext| {
        let stack = (frames.iter()).fold(ContextStack::new(), |stack, frame| {
            stack.with(ContextFrame::new(*frame))
        });
        let run = RunCommand(Invocation::of(id).unwrap().context(stack));
        cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
        cx.run_until_parked();
    };
    let outside: [&[&'static str]; 3] = [
        &["workbench"],
        &["workbench", "project_panel"],
        &["workbench", "terminal"],
    ];
    let editor = &["workbench", fe_editor::commands::CONTEXT];

    for id in ["lsp.rename", "lsp.code_actions"] {
        for frames in outside {
            run(id, frames, cx);
            assert!(!cx.has_pending_prompt(), "{id} in {frames:?}");
        }
        run(id, editor, cx);
        let (question, _) = (cx.pending_prompt()).unwrap_or_else(|| panic!("{id} ran"));
        assert_eq!(question, "No language server for this file", "{id}");
        cx.simulate_prompt_answer("OK");
    }

    let no_formatter = "No formatter for Plain Text".to_owned();
    let mut ran = 0;
    for id in ["lsp.format_document", "lsp.format_selection"] {
        for frames in outside {
            run(id, frames, cx);
            let shown = toasts(cx);
            assert!(
                shown.iter().all(|(_, count)| *count == ran),
                "{id} in {frames:?}: {shown:?}"
            );
        }
        run(id, editor, cx);
        ran += 1;
        assert_eq!(toasts(cx), vec![(no_formatter.clone(), ran)], "{id}");
    }
}

/// The find commands run only where their context holds, as their
/// registry's dispatch checked: Close Find with the editor focused, or
/// Find with the find bar, does nothing.
#[gpui::test]
fn find_commands_run_only_where_their_context_holds(cx: &mut TestAppContext) {
    use crate::tests::find_bar::{draw, open, opened, pane};

    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo bar foo\n")]);
    let pane = pane(&workbench, cx);
    let run = |id: &str, frames: &[&'static str], cx: &mut VisualTestContext| {
        let stack = (frames.iter()).fold(ContextStack::new(), |stack, frame| {
            stack.with(ContextFrame::new(*frame))
        });
        let run = RunCommand(Invocation::of(id).unwrap().context(stack));
        cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
        draw(cx);
    };
    let editor = &["workbench", fe_editor::commands::CONTEXT];
    let find_bar = &["workbench", "find_bar", "text_input"];

    run("find.show", find_bar, cx);
    assert!(!open(&pane, cx), "find.show in the find bar");
    run("find.show", editor, cx);
    assert!(open(&pane, cx), "find.show in the editor");
    run("find.close", editor, cx);
    assert!(open(&pane, cx), "find.close in the editor");
    run("find.close", find_bar, cx);
    assert!(!open(&pane, cx), "find.close in the find bar");
}

/// The pane's root attaches its commands without putting the pane in the
/// find bar's key context: with the bar open and the editor focused, the
/// editor's keys aren't the bar's, so `escape` collapses the editor's
/// selections and leaves the bar open.
#[gpui::test]
fn the_find_bar_keys_stay_in_the_find_bar(cx: &mut TestAppContext) {
    use crate::tests::find_bar::{draw, editor, focus, open, opened, pane, run};

    let (_dir, workbench, cx) = opened(cx, &[("a.txt", "foo bar foo\n")]);
    let pane = pane(&workbench, cx);
    let editor = editor(&pane, cx);
    run("find.show", cx);
    assert!(open(&pane, cx));
    focus(&editor, cx);
    let in_find_bar = cx.update(|window, _| {
        (window.context_stack().iter()).any(|context| context.contains("find_bar"))
    });
    assert!(!in_find_bar);
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(open(&pane, cx));
}

/// The terminal's vi mode command runs only where the terminal's context
/// holds, as its registry's dispatch checked: a menu pick or `RunCommand`
/// with the editor or the project panel focused doesn't show the terminal.
#[gpui::test]
fn terminal_commands_run_only_in_the_terminals_context(cx: &mut TestAppContext) {
    // Shown, the terminal starts a session, whose pty's threads wake its
    // event task.
    cx.executor().allow_parking();
    let dir = tempfile::tempdir().unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let terminal = workbench.read_with(cx, |workbench, _| workbench.terminal().clone());
    terminal.update(cx, |terminal, _| {
        terminal.set_shell("/bin/sh", &["-c", "read x"])
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    cx.run_until_parked();

    let run = |id: &str, frames: &[&'static str], cx: &mut VisualTestContext| {
        let stack = (frames.iter()).fold(ContextStack::new(), |stack, frame| {
            stack.with(ContextFrame::new(*frame))
        });
        let run = RunCommand(Invocation::of(id).unwrap().context(stack));
        cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
        cx.run_until_parked();
    };
    let shown = |cx: &mut VisualTestContext| {
        let dock = workbench.read_with(cx, |workbench, _| workbench.terminal_dock().clone());
        dock.read_with(cx, |dock, _| dock.is_visible())
    };
    let outside: [&[&'static str]; 3] = [
        &["workbench"],
        &["workbench", fe_editor::commands::CONTEXT],
        &["workbench", "project_panel"],
    ];
    for frames in outside {
        run("terminal.vi_mode", frames, cx);
        assert!(!shown(cx), "terminal.vi_mode in {frames:?}");
    }
    run("terminal.vi_mode", &["workbench", "terminal"], cx);
    assert!(shown(cx), "terminal.vi_mode in the terminal");
}
