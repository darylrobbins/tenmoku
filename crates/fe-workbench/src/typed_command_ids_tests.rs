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
                Some(ContextPredicate::identifier(context)),
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
