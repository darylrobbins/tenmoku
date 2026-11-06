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
    (
        "workbench.close_other_tabs",
        "Close Other Tabs",
        "workbench",
    ),
    ("workbench.close_saved", "Close Saved Editors", "workbench"),
    (
        "workbench.close_group_editors",
        "Close Editors in Group",
        "workbench",
    ),
    (
        "workbench.close_all_editors",
        "Close All Editors",
        "workbench",
    ),
    ("workbench.next_tab", "Next Tab", "workbench"),
    ("workbench.previous_tab", "Previous Tab", "workbench"),
    (
        "workbench.open_editor_at_index1",
        "Open Editor at Index 1",
        "workbench",
    ),
    (
        "workbench.open_editor_at_index2",
        "Open Editor at Index 2",
        "workbench",
    ),
    (
        "workbench.open_editor_at_index3",
        "Open Editor at Index 3",
        "workbench",
    ),
    (
        "workbench.open_editor_at_index4",
        "Open Editor at Index 4",
        "workbench",
    ),
    (
        "workbench.open_editor_at_index5",
        "Open Editor at Index 5",
        "workbench",
    ),
    (
        "workbench.open_editor_at_index6",
        "Open Editor at Index 6",
        "workbench",
    ),
    (
        "workbench.open_editor_at_index7",
        "Open Editor at Index 7",
        "workbench",
    ),
    (
        "workbench.open_editor_at_index8",
        "Open Editor at Index 8",
        "workbench",
    ),
    (
        "workbench.open_editor_at_index9",
        "Open Editor at Index 9",
        "workbench",
    ),
    (
        "workbench.last_editor_in_group",
        "Last Editor in Group",
        "workbench",
    ),
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
    ("pane.focus_group1", "Focus First Editor Group", "workbench"),
    (
        "pane.focus_group2",
        "Focus Second Editor Group",
        "workbench",
    ),
    ("pane.focus_group3", "Focus Third Editor Group", "workbench"),
    (
        "pane.focus_group4",
        "Focus Fourth Editor Group",
        "workbench",
    ),
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
    ("dock.show_project", "Show Project", "workbench"),
    ("dock.toggle", "Toggle Dock", "workbench"),
    ("search.show", "Find in Files", "workbench"),
    ("search.show_replace", "Replace in Files…", "workbench"),
    ("search.replace_all", "Replace All in Files", "workbench"),
    ("git.show_changes", "Show Changes", "workbench"),
    ("git.refresh", "Refresh Changes", "workbench"),
    ("outline.show", "Show Outline", "workbench"),
    ("nav.find_references", "Find All References", "workbench"),
    (
        "references_panel.select_prev",
        "Select Previous Reference",
        "ReferencesPanel",
    ),
    (
        "references_panel.select_next",
        "Select Next Reference",
        "ReferencesPanel",
    ),
    ("references_panel.open", "Open Reference", "ReferencesPanel"),
    ("problems.show", "Show Problems", "workbench"),
    ("ai.show_changes", "Show AI Changes", "workbench"),
    (
        "chat.review_latest_changes",
        "Review Latest Chat Changes",
        "workbench",
    ),
    (
        "ai.changes_accept_all",
        "Accept All AI Changes",
        "workbench",
    ),
    (
        "ai.changes_reject_all",
        "Reject All AI Changes",
        "workbench",
    ),
    ("ai.changes_revert", "Revert AI Turn", "workbench"),
    (
        "ai.changes_next_file",
        "Select Next AI Changed File",
        "ai_changes",
    ),
    (
        "ai.changes_previous_file",
        "Select Previous AI Changed File",
        "ai_changes",
    ),
    (
        "problems.select_next",
        "Select Next Problem",
        "problems_panel",
    ),
    (
        "problems.select_previous",
        "Select Previous Problem",
        "problems_panel",
    ),
    ("problems.open", "Open Problem", "problems_panel"),
    ("diagnostics.next", "Go to Next Problem", "workbench"),
    (
        "diagnostics.previous",
        "Go to Previous Problem",
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
    ("theme.select", "Change Color Theme…", "workbench"),
    (
        "workbench.show_extensions",
        "Extensions: Show Installed",
        "workbench",
    ),
    (
        "workbench.revoke_plugin_grants",
        "Extensions: Revoke Plugin Grants",
        "workbench",
    ),
    (
        "workbench.uninstall_plugin",
        "Extensions: Uninstall Plugin",
        "workbench",
    ),
    (
        "workbench.install_extension",
        "Extensions: Install from File or Folder…",
        "workbench",
    ),
    (
        "workbench.link_extension_folder",
        "Extensions: Install Folder for Development…",
        "workbench",
    ),
    (
        "workbench.show_plugin_log",
        "Extensions: Show Plugin Log",
        "workbench",
    ),
    ("nav.goto_definition", "Go to Definition", "workbench"),
    ("nav.goto_declaration", "Go to Declaration", "workbench"),
    (
        "nav.goto_type_definition",
        "Go to Type Definition",
        "workbench",
    ),
    (
        "nav.goto_implementation",
        "Go to Implementations",
        "workbench",
    ),
    ("nav.document_symbols", "Go to Symbol in File…", "workbench"),
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
    ("ai.inline_edit", "Inline Edit with AI", "editor"),
    ("inline_edit.submit", "Send Inline Edit", "inline_edit"),
    ("inline_edit.cancel", "Cancel Inline Edit", "inline_edit"),
    ("lsp.code_actions", "Code Actions…", "editor"),
    ("lsp.run_lens", "Run Code Lens", "editor"),
    ("lsp.organize_imports", "Organize Imports", "editor"),
    ("lsp.fix_all", "Fix All", "editor"),
    ("lsp.refactor", "Refactor…", "editor"),
    ("lsp.source_action", "Source Action…", "editor"),
    ("lsp.format_document", "Format Document", "editor"),
    ("lsp.format_selection", "Format Selection", "editor"),
    ("workbench.open_folder", "Open Folder…", "workbench"),
    ("workbench.open_file", "Open File…", "workbench"),
    (
        "workbench.open_settings",
        "Open Settings (JSON)",
        "workbench",
    ),
    (
        "workbench.open_settings_ui",
        "Open Settings (UI)",
        "workbench",
    ),
    (
        "workbench.open_keyboard_shortcuts",
        "Open Keyboard Shortcuts",
        "workbench",
    ),
    (
        "workbench.open_keyboard_shortcuts_json",
        "Open Keyboard Shortcuts (JSON)",
        "workbench",
    ),
    ("window.new", "New Window", "workbench"),
    (
        "workbench.open_folder_in_new_window",
        "Open Folder in New Window…",
        "workbench",
    ),
    ("workbench.new_file", "New File", "workbench"),
    ("workbench.save_as", "Save As…", "workbench"),
    ("workbench.trust_folder", "Trust This Folder", "workbench"),
    ("spell.suggest", "Spelling: Suggest Corrections…", "editor"),
    (
        "spell.add_word",
        "Spelling: Add Word to Dictionary",
        "editor",
    ),
    ("spell.ignore_word", "Spelling: Ignore Word", "editor"),
    ("spell.toggle", "Spelling: Toggle Spell Check", "workbench"),
    ("spell.next", "Spelling: Next Misspelling", "editor"),
    ("spell.previous", "Spelling: Previous Misspelling", "editor"),
    ("tasks.run", "Tasks: Run Task…", "workbench"),
    ("tasks.rerun_last", "Tasks: Rerun Last Task", "workbench"),
    ("tasks.run_build", "Tasks: Run Build Task", "workbench"),
    (
        "workbench.untrust_folder",
        "Untrust This Folder",
        "workbench",
    ),
    ("find.show", "Find", "editor"),
    ("find.show_replace", "Replace…", "workbench"),
    ("find.next", "Find Next", "editor || find_bar"),
    ("find.previous", "Find Previous", "editor || find_bar"),
    ("find.close", "Close Find", "find_bar"),
    ("find.toggle_case", "Match Case", "find_bar"),
    ("find.toggle_whole_word", "Match Whole Word", "find_bar"),
    ("find.toggle_regex", "Use Regular Expression", "find_bar"),
    (
        "find.toggle_preserve_case",
        "Toggle Preserve Case",
        "find_bar",
    ),
    ("find.toggle_in_selection", "Find in Selection", "find_bar"),
    ("find.toggle_replace", "Toggle Replace", "find_bar"),
    ("find.replace_next", "Replace", "find_bar"),
    ("find.replace_all", "Replace All", "find_bar"),
    ("terminal.toggle", "Toggle Terminal", "workbench"),
    ("terminal.new", "New Terminal", "workbench"),
    ("terminal.close", "Kill Terminal", "workbench"),
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
    ("git.diff_index", "Git: Compare with Index", "workbench"),
    ("git.diff_head", "Git: Compare with HEAD", "workbench"),
    (
        "diff.compare_file",
        "Compare Active File With…",
        "workbench",
    ),
    (
        "diff.compare_saved",
        "Compare Active File with Saved",
        "editor",
    ),
    ("git.next_change", "Go to Next Change", "workbench"),
    ("git.previous_change", "Go to Previous Change", "workbench"),
    ("git.next_conflict", "Git: Next Conflict", "workbench"),
    (
        "git.previous_conflict",
        "Git: Previous Conflict",
        "workbench",
    ),
    (
        "git.accept_current",
        "Git: Accept Current Change",
        "workbench",
    ),
    (
        "git.accept_incoming",
        "Git: Accept Incoming Change",
        "workbench",
    ),
    ("git.accept_both", "Git: Accept Both Changes", "workbench"),
    (
        "git.compare_conflict",
        "Git: Compare Conflict Changes",
        "workbench",
    ),
    (
        "git.accept_all_current",
        "Git: Accept All Current",
        "workbench",
    ),
    (
        "git.accept_all_incoming",
        "Git: Accept All Incoming",
        "workbench",
    ),
    ("git.accept_all_both", "Git: Accept All Both", "workbench"),
    ("git.stage_file", "Git: Stage File", "workbench"),
    ("git.unstage_file", "Git: Unstage File", "workbench"),
    ("git.discard_file", "Git: Discard File Changes", "workbench"),
    (
        "git.reset_buffer",
        "Git: Revert Editor to Index",
        "workbench",
    ),
    ("git.stage_hunk", "Git: Stage Change", "workbench"),
    ("git.unstage_hunk", "Git: Undo Stage Change", "workbench"),
    ("git.revert_hunk", "Git: Revert Change", "workbench"),
    ("git.commit", "Git: Commit Staged", "workbench"),
    (
        "git.commit_amend",
        "Git: Commit Staged (Amend)",
        "workbench",
    ),
    (
        "git.toggle_line_blame",
        "Git: Toggle Line Blame",
        "workbench",
    ),
    ("git.blame_line", "Git: Show Line Blame", "workbench"),
    (
        "git.open_line_commit",
        "Git: Open Line's Commit Change",
        "workbench",
    ),
    ("git.switch_branch", "Git: Switch Branch…", "workbench"),
    ("git.create_branch", "Git: Create Branch…", "workbench"),
    ("git.file_history", "Git: File History", "workbench"),
    ("git.log", "Git: Log", "workbench"),
    ("git.log_all", "Git: Log (All Branches)", "workbench"),
    ("git.stash", "Git: Stash", "workbench"),
    (
        "git.stash_include_untracked",
        "Git: Stash (Include Untracked)",
        "workbench",
    ),
    ("git.stash_pop", "Git: Pop Stash…", "workbench"),
    ("git.stash_apply", "Git: Apply Stash…", "workbench"),
    ("app.check_for_updates", "Check for Updates…", "workbench"),
    ("app.set_up", "Set Up Tenmoku…", "workbench"),
    ("git.fetch", "Git: Fetch", "workbench"),
    ("git.pull", "Git: Pull", "workbench"),
    ("git.push", "Git: Push", "workbench"),
    (
        "git.push_force_with_lease",
        "Git: Push (Force with Lease)",
        "workbench",
    ),
    (
        "git.toggle_blame_annotations",
        "Git: Toggle Blame Annotations",
        "workbench",
    ),
    (
        "workbench.reopen_closed_editor",
        "Reopen Closed Editor",
        "workbench",
    ),
    ("workbench.toggle_zen_mode", "Toggle Zen Mode", "workbench"),
    (
        "workbench.copy_active_file_path",
        "Copy Path of Active File",
        "workbench",
    ),
    (
        "workbench.copy_active_file_relative_path",
        "Copy Relative Path of Active File",
        "workbench",
    ),
    (
        "workbench.reveal_active_file_in_os",
        "Reveal Active File in File Manager",
        "workbench",
    ),
    ("workbench.save_all", "Save All", "workbench"),
    (
        "workbench.save_without_formatting",
        "Save without Formatting",
        "workbench",
    ),
    ("editor.peek_definition", "Peek Definition", "editor"),
    ("editor.peek_references", "Peek References", "editor"),
    ("peek.close", "Close Peek", "peek || peek_shown"),
    ("peek.open", "Open Peeked Definition", "peek"),
    (
        "workbench.change_encoding",
        "Change File Encoding",
        "workbench",
    ),
    (
        "workbench.change_line_ending",
        "Change End of Line Sequence…",
        "workbench",
    ),
    (
        "workbench.reopen_with_encoding",
        "Reopen Editor with Encoding…",
        "workbench",
    ),
    (
        "workbench.save_with_encoding",
        "Save with Encoding…",
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
                Some(ContextPredicate::parse(context).unwrap()),
                "{id}"
            );
        }
    });
}

/// `project_panel.reveal_active` is a typed command with its title and
/// context too, which the project panel's registry still lists, naming no
/// op, and the panel passes on.
#[gpui::test]
fn reveal_active_is_a_typed_command(cx: &mut TestAppContext) {
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    cx.read(|cx| {
        let id = CommandId::parse("project_panel.reveal_active").unwrap();
        let command = typed_command(&id, cx).expect("a typed command");
        assert_eq!(command.title, "Reveal Active File in Project");
        assert_eq!(
            command.context,
            Some(ContextPredicate::identifier("workbench"))
        );
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

/// Dispatches `id` as a `RunCommand` from the focused element, invoked in
/// `frames`' context, and lets it land.
fn run_in(id: &str, frames: &[&'static str], cx: &mut VisualTestContext) {
    let stack = (frames.iter()).fold(ContextStack::new(), |stack, frame| {
        stack.with(ContextFrame::new(*frame))
    });
    let run = RunCommand(Invocation::of(id).unwrap().context(stack));
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// A workbench rooted at a project of `a.rs`, showing it, focused, its
/// caret in the first `helper`.
fn helper_project(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    gpui::Entity<Workbench>,
    &mut VisualTestContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(dir.path()).unwrap();
    let text = "fn helper() {}\n\nfn main() {\n    helper();\n    helper();\n}\n";
    std::fs::write(root.join("a.rs"), text).unwrap();
    let (workbench, cx) = crate::tests::open_root(cx, &root);
    cx.update(|window, cx| {
        let pane = workbench.read(cx).pane().clone();
        pane.update(cx, |pane, cx| pane.open(root.join("a.rs"), window, cx));
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    let editor = workbench
        .read_with(cx, |workbench, cx| workbench.active_editor(cx).cloned())
        .expect("a.rs is shown");
    editor.update(cx, |editor, cx| editor.select_and_reveal(4..4, cx));
    cx.run_until_parked();
    (dir, workbench, cx)
}

/// The workbench's moved commands in its context, the dock's, run only
/// where it holds, as their registry's dispatch checked.
#[gpui::test]
fn the_dock_commands_run_only_in_the_workbenchs_context(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = helper_project(cx);
    let dock = workbench.read_with(cx, |workbench, _| workbench.dock().clone());
    let visible = |cx: &mut VisualTestContext| dock.read_with(cx, |dock, _| dock.is_visible());
    let shown = visible(cx);
    run_in("dock.toggle", &[fe_editor::commands::CONTEXT], cx);
    assert_eq!(visible(cx), shown, "not in the workbench's context");
    run_in(
        "dock.toggle",
        &["workbench", fe_editor::commands::CONTEXT],
        cx,
    );
    assert_eq!(visible(cx), !shown);
}

/// The replace commands the search panel does itself while focused run
/// only where the workbench's context holds, as the dock's registry
/// checked.
#[gpui::test]
fn the_search_panels_replace_commands_check_their_context(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = helper_project(cx);
    run_in("search.show", &["workbench"], cx);
    let search = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).search().clone()
    });
    let replace_focused = |cx: &mut VisualTestContext| {
        let replace = search.read_with(cx, |search, _| search.replace_input().clone());
        cx.update(|window, cx| replace.focus_handle(cx).is_focused(window))
    };
    assert!(!replace_focused(cx));
    let panel = ["search_panel", fe_ui::TextInput::KEY_CONTEXT];
    run_in("search.show_replace", &panel, cx);
    assert!(!replace_focused(cx), "not in the workbench's context");
    run_in(
        "search.show_replace",
        &["workbench", panel[0], panel[1]],
        cx,
    );
    assert!(replace_focused(cx));
}

/// The References panel's commands run only where its context holds,
/// whether the panel, focused, does them or the workbench does.
#[gpui::test]
fn the_references_commands_run_only_in_the_panels_context(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = helper_project(cx);
    let editor = ["workbench", fe_editor::commands::CONTEXT];
    run_in("nav.find_references", &editor[1..], cx);
    let panel = workbench.read_with(cx, |workbench, cx| {
        workbench.dock().read(cx).references().clone()
    });
    let selected = |cx: &mut VisualTestContext| panel.read_with(cx, |panel, _| panel.selected());
    assert_eq!(selected(cx), None, "not in the workbench's context");
    run_in("nav.find_references", &editor, cx);
    let first = selected(cx);
    assert!(first.is_some(), "the references are listed");

    // The panel, focused, does them.
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    run_in("references_panel.select_next", &["workbench"], cx);
    assert_eq!(selected(cx), first, "not in the panel's context");
    let in_panel = ["workbench", "ReferencesPanel"];
    run_in("references_panel.select_next", &in_panel, cx);
    let second = selected(cx);
    assert!(second > first, "{second:?}");
    run_in("references_panel.select_prev", &["workbench"], cx);
    assert_eq!(selected(cx), second);
    run_in("references_panel.select_prev", &in_panel, cx);
    assert_eq!(selected(cx), first);

    // The workbench does them for an invocation in the panel's context
    // from elsewhere, and nothing for one that isn't.
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    run_in("references_panel.select_next", &editor, cx);
    assert_eq!(selected(cx), first, "not in the panel's context");
    run_in("references_panel.select_next", &in_panel, cx);
    assert_eq!(selected(cx), second);
}

/// The editor's own registry (not the typed commands in [`MOVED`]) lists the
/// line sort commands with these IDs and titles.
const EDITOR_LINE_COMMANDS: &[(&str, &str)] = &[
    ("editor.sort_lines_ascending", "Sort Lines Ascending"),
    ("editor.sort_lines_descending", "Sort Lines Descending"),
    ("editor.delete_duplicate_lines", "Delete Duplicate Lines"),
    ("editor.join_lines", "Join Lines"),
    ("editor.delete_all_left", "Delete All Left"),
    ("editor.delete_all_right", "Delete All Right"),
    ("editor.delete_word_right", "Delete Word Right"),
    ("editor.transpose_letters", "Transpose Letters"),
    ("editor.toggle_block_comment", "Toggle Block Comment"),
    ("editor.insert_line_below", "Insert Line Below"),
    ("editor.insert_line_above", "Insert Line Above"),
    ("editor.copy_rows_up", "Copy Rows Up"),
    (
        "editor.trim_trailing_whitespace",
        "Trim Trailing Whitespace",
    ),
];

#[test]
fn the_editor_registry_lists_the_line_sort_commands() {
    let registry = fe_editor::commands::registry();
    for &(id, title) in EDITOR_LINE_COMMANDS {
        let meta = registry.commands().find(|meta| meta.id.as_str() == id);
        let meta = meta.unwrap_or_else(|| panic!("{id} is registered"));
        assert_eq!(meta.title, title, "{id}");
    }
}

/// The editor's own registry lists the case transform commands with these IDs
/// and titles.
#[test]
fn the_editor_registry_lists_the_case_transform_commands() {
    let registry = fe_editor::commands::registry();
    for (id, title) in [
        ("editor.transform_uppercase", "Transform to Uppercase"),
        ("editor.transform_lowercase", "Transform to Lowercase"),
        ("editor.transform_titlecase", "Transform to Title Case"),
    ] {
        let meta = registry.commands().find(|meta| meta.id.as_str() == id);
        let meta = meta.unwrap_or_else(|| panic!("{id} is registered"));
        assert_eq!(meta.title, title, "{id}");
    }
}

/// The end of line commands are registered with these IDs and titles.
#[test]
fn the_end_of_line_commands_have_their_ids_and_titles() {
    let registry = fe_editor::commands::registry();
    for (id, title) in [
        (
            "editor.set_end_of_line_lf",
            "Change End of Line Sequence to LF",
        ),
        (
            "editor.set_end_of_line_crlf",
            "Change End of Line Sequence to CRLF",
        ),
    ] {
        let meta = registry.commands().find(|meta| meta.id.as_str() == id);
        assert_eq!(
            meta.map(|meta| meta.title.to_string()),
            Some(title.to_string()),
            "{id}"
        );
    }
}

/// The indentation commands are registered with these IDs and titles.
#[test]
fn the_indentation_commands_have_their_ids_and_titles() {
    let registry = fe_editor::commands::registry();
    for (id, title) in [
        (
            "editor.convert_indentation_to_spaces",
            "Convert Indentation to Spaces",
        ),
        (
            "editor.convert_indentation_to_tabs",
            "Convert Indentation to Tabs",
        ),
        (
            "editor.detect_indentation",
            "Detect Indentation from Content",
        ),
    ] {
        let meta = registry.commands().find(|meta| meta.id.as_str() == id);
        assert_eq!(
            meta.map(|meta| meta.title.to_string()),
            Some(title.to_string()),
            "{id}"
        );
    }
}

/// The fold level and recursive fold commands are registered with these IDs
/// and titles.
#[test]
fn the_fold_level_commands_have_their_ids_and_titles() {
    let registry = fe_editor::commands::registry();
    let levels = (1..=7).map(|level| {
        (
            format!("editor.fold_level_{level}"),
            format!("Fold Level {level}"),
        )
    });
    let recursive = [
        ("editor.fold_recursively", "Fold Recursively"),
        ("editor.unfold_recursively", "Unfold Recursively"),
    ]
    .map(|(id, title)| (id.to_string(), title.to_string()));
    for (id, title) in levels.chain(recursive) {
        let meta = registry.commands().find(|meta| meta.id.as_str() == id);
        assert_eq!(meta.map(|meta| meta.title.to_string()), Some(title), "{id}");
    }
}

/// Go to Last Edit Location is registered with this ID and title.
#[test]
fn go_to_last_edit_location_has_its_id_and_title() {
    let registry = fe_editor::commands::registry();
    let id = "editor.go_to_last_edit_location";
    let meta = registry.commands().find(|meta| meta.id.as_str() == id);
    assert_eq!(
        meta.map(|meta| meta.title.to_string()).as_deref(),
        Some("Go to Last Edit Location")
    );
}

/// Move Last Selection to Next Find Match is registered with this ID and
/// title.
#[test]
fn move_last_selection_to_next_find_match_has_its_id_and_title() {
    let registry = fe_editor::commands::registry();
    let id = "editor.move_last_selection_to_next_find_match";
    let meta = registry.commands().find(|meta| meta.id.as_str() == id);
    assert_eq!(
        meta.map(|meta| meta.title.to_string()).as_deref(),
        Some("Move Last Selection to Next Find Match")
    );
}
