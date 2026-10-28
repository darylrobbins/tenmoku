//! fastedit's command catalog, [`crate::command_catalog`], built from
//! kiln's recorded commands and the registries left: it knows every
//! command the default keymaps bind and the default menus name, on macOS
//! and on Linux, with no app; and the command palette, which lists the
//! typed commands itself and the rest from [`crate::command_list`], lists
//! what it always has.

use std::path::Path;

use fe_buffer::Buffer;
use fe_command::{CommandId, CommandMeta, Value};
use fe_gpui::{EditorSettings, EditorTheme};
use fe_keymap::{DEFAULT_KEYMAP_PATH, ItemKind, KeymapLayer};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln_shell::{
    Picker,
    command_palette::{COMMAND_PALETTE, CommandPalette},
};

use crate::{Launch, Workbench};

/// The commands `keymap`, a default keymap as resolved for one platform,
/// binds, with their args.
fn bound(keymap: &str) -> Vec<(CommandId, Value)> {
    let (layer, diagnostics) = KeymapLayer::parse(Path::new(DEFAULT_KEYMAP_PATH), keymap, None);
    assert_eq!(diagnostics, []);
    (layer
        .expect("the default keymap parses")
        .entries
        .into_iter())
    .filter_map(|entry| Some((entry.command?, entry.args)))
    .collect()
}

/// The commands the default menus, as on one platform, name: their items'
/// and their alternatives'.
fn named(bar: Option<fe_keymap::MenuBar>) -> Vec<(CommandId, Value)> {
    let bar = bar.expect("the default menus parse");
    let mut named = Vec::new();
    for item in bar.menus.iter().flat_map(|menu| &menu.items) {
        if let ItemKind::Command(command) = &item.kind {
            named.push((command.command.clone(), command.args.clone()));
            for alternative in &command.alternatives {
                named.push((alternative.command.clone(), alternative.args.clone()));
            }
        }
    }
    named
}

/// The commands of `commands` that `crate::command_catalog()` doesn't
/// take, with why.
fn unchecked(commands: &[(CommandId, Value)]) -> Vec<String> {
    let catalog = crate::command_catalog();
    (commands.iter())
        .filter_map(|(id, args)| {
            let checked = catalog.check(id, args);
            checked
                .err()
                .map(|error| format!("{id} {args:?}: {error:?}"))
        })
        .collect()
}

#[test]
fn the_default_keymaps_commands_check_with_no_app() {
    for (os, keymap) in [
        ("macOS", fe_keymap::DEFAULT_MACOS),
        ("Linux", fe_keymap::DEFAULT_LINUX),
    ] {
        let commands = bound(keymap);
        assert!(commands.len() > 100, "{os}: {} bindings", commands.len());
        assert_eq!(unchecked(&commands), Vec::<String>::new(), "{os}");
    }
}

#[test]
fn the_default_menus_commands_check_with_no_app() {
    let (macos, macos_diagnostics) = fe_keymap::default_menus_macos(None);
    let (linux, linux_diagnostics) = fe_keymap::default_menus_linux(None);
    assert_eq!(macos_diagnostics, []);
    assert_eq!(linux_diagnostics, []);
    for (os, commands) in [("macOS", named(macos)), ("Linux", named(linux))] {
        assert!(commands.len() > 50, "{os}: {} items", commands.len());
        assert_eq!(unchecked(&commands), Vec::<String>::new(), "{os}");
    }
}

/// Checked against the catalog, the default keymaps and menus load whole,
/// on both platforms: none of their entries is dropped as unknown.
#[test]
fn the_default_keymaps_and_menus_load_whole_against_the_catalog() {
    let catalog = crate::command_catalog();
    for keymap in [fe_keymap::DEFAULT_MACOS, fe_keymap::DEFAULT_LINUX] {
        let path = Path::new(DEFAULT_KEYMAP_PATH);
        let (layer, diagnostics) = KeymapLayer::parse(path, keymap, Some(&*catalog));
        assert_eq!(diagnostics, []);
        let unchecked = KeymapLayer::parse(path, keymap, None).0.unwrap();
        assert_eq!(layer.unwrap().entries.len(), unchecked.entries.len());
    }
    let (_, diagnostics) = fe_keymap::default_menus_macos(Some(&*catalog));
    assert_eq!(diagnostics, []);
    let (_, diagnostics) = fe_keymap::default_menus_linux(Some(&*catalog));
    assert_eq!(diagnostics, []);
}

/// The registries left name one typed command of the workbench's,
/// `project_panel.reveal_active`, for the project panel to pass on; the
/// palette's list leaves it to the typed ones, so no family that moved to
/// `#[kiln::command]` is listed from a registry. Each registry's other
/// commands are in the list once, and the editor's once in each context.
#[test]
fn the_palette_list_holds_no_typed_command() {
    let typed = kiln::recorded_catalog("fe_workbench");
    let is_typed = |id: &CommandId| typed.check(id, &Value::Null).is_ok();
    let registries: Vec<CommandMeta> = (crate::search_panel::registry().commands())
        .chain(crate::commit_box_registry().commands())
        .chain(crate::project_panel::registry().commands())
        .chain(crate::status_bar::registry().commands())
        .cloned()
        .collect();
    let named: Vec<&str> = (registries.iter())
        .filter(|meta| is_typed(&meta.id))
        .map(|meta| meta.id.as_str())
        .collect();
    assert_eq!(named, ["project_panel.reveal_active"]);
    let list = crate::command_list();
    let listed: Vec<&str> = (list.iter())
        .filter(|meta| is_typed(&meta.id))
        .map(|meta| meta.id.as_str())
        .collect();
    assert_eq!(listed, Vec::<&str>::new());
    for meta in registries.iter().filter(|meta| !is_typed(&meta.id)) {
        let times = list.iter().filter(|listed| listed.id == meta.id).count();
        assert_eq!(times, 1, "{}", meta.id);
    }
    for (ix, meta) in list.iter().enumerate() {
        let again = (list[ix + 1..].iter())
            .any(|later| later.id == meta.id && later.context == meta.context);
        assert!(!again, "{} is listed twice", meta.id);
    }
}

fn opened(
    cx: &mut TestAppContext,
) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let launch = Launch {
        root: Some(dir.path().to_owned()),
        file: Some((path, Some(Buffer::new("alpha\n").into()))),
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (dir, workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// The command IDs the palette lists, opened where focus is now, then
/// closed.
fn listed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.simulate_keystrokes("cmd-shift-p");
    draw(cx);
    let picker: Entity<Picker<CommandPalette>> = workbench.read_with(cx, |workbench, cx| {
        let layer = workbench.shell().read(cx).modals().read(cx);
        assert_eq!(layer.modal(), Some(COMMAND_PALETTE));
        layer.modal_view().unwrap().clone().downcast().unwrap()
    });
    let mut ids: Vec<String> = picker.read_with(cx, |picker, _| {
        (picker.delegate.matches().iter())
            .map(|entry| entry.id.to_string())
            .collect()
    });
    cx.simulate_keystrokes("escape");
    draw(cx);
    ids.sort();
    ids
}

/// The palette, in a test window, still lists every command it listed at
/// the base, with the editor focused and with the project panel focused:
/// the typed commands kiln lists, and the registries' and the editor's,
/// from [`crate::command_list`].
#[gpui::test]
fn the_palette_lists_what_it_listed(cx: &mut TestAppContext) {
    let (_dir, workbench, cx) = opened(cx);
    let in_editor = listed(&workbench, cx);
    cx.simulate_keystrokes("cmd-shift-e");
    draw(cx);
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    assert!(cx.update(|window, cx| panel.focus_handle(cx).is_focused(window)));
    let in_panel = listed(&workbench, cx);
    for (focused, listed, expected) in [
        (
            "the editor",
            &in_editor,
            [LISTED_ANYWHERE, LISTED_IN_THE_EDITOR],
        ),
        (
            "the project panel",
            &in_panel,
            [LISTED_ANYWHERE, LISTED_IN_THE_PROJECT_PANEL],
        ),
    ] {
        let missing: Vec<&str> = (expected.concat().into_iter())
            .filter(|id| !listed.iter().any(|listed| listed == id))
            .collect();
        assert_eq!(missing, Vec::<&str>::new(), "with {focused} focused");
    }
}

/// The IDs the palette listed at the base both with the editor focused and
/// with the project panel focused.
const LISTED_ANYWHERE: &[&str] = &[
    "ai.changes_accept_all",
    "ai.changes_reject_all",
    "ai.changes_revert",
    "ai.show_changes",
    "app.hide",
    "app.hide_others",
    "app.quit",
    "app.show_all",
    "chat.review_latest_changes",
    "diagnostics.next",
    "diagnostics.previous",
    "diff.compare_file",
    "dock.grow",
    "dock.move_sidebar_left",
    "dock.move_sidebar_right",
    "dock.show_project",
    "dock.shrink",
    "dock.toggle",
    "find.show_replace",
    "git.accept_all_both",
    "git.accept_all_current",
    "git.accept_all_incoming",
    "git.accept_both",
    "git.accept_current",
    "git.accept_incoming",
    "git.blame_line",
    "git.commit",
    "git.commit_amend",
    "git.compare_conflict",
    "git.create_branch",
    "git.diff_head",
    "git.diff_index",
    "git.discard_file",
    "git.file_history",
    "git.log",
    "git.log_all",
    "git.next_change",
    "git.next_conflict",
    "git.open_line_commit",
    "git.previous_change",
    "git.previous_conflict",
    "git.refresh",
    "git.reset_buffer",
    "git.revert_hunk",
    "git.show_changes",
    "git.stage_file",
    "git.stage_hunk",
    "git.stash",
    "git.stash_apply",
    "git.stash_include_untracked",
    "git.stash_pop",
    "git.switch_branch",
    "git.toggle_blame_annotations",
    "git.toggle_line_blame",
    "git.unstage_file",
    "git.unstage_hunk",
    "go_to_line.toggle",
    "language.select",
    "lsp.restart",
    "lsp.show_log",
    "menu_bar.open",
    "nav.back",
    "nav.find_references",
    "nav.forward",
    "nav.goto_declaration",
    "nav.goto_definition",
    "nav.goto_implementation",
    "nav.goto_type_definition",
    "nav.workspace_symbols",
    "outline.show",
    "pane.close",
    "pane.equalize",
    "pane.focus_down",
    "pane.focus_left",
    "pane.focus_next",
    "pane.focus_previous",
    "pane.focus_right",
    "pane.focus_up",
    "pane.grow_height",
    "pane.grow_width",
    "pane.move_tab_down",
    "pane.move_tab_left",
    "pane.move_tab_right",
    "pane.move_tab_up",
    "pane.shrink_height",
    "pane.shrink_width",
    "pane.split_down",
    "pane.split_right",
    "problems.show",
    "project_panel.reveal_active",
    "quick_open.toggle",
    "search.replace_all",
    "search.show",
    "search.show_replace",
    "terminal.close",
    "terminal.find",
    "terminal.move_to_bottom",
    "terminal.move_to_left",
    "terminal.move_to_right",
    "terminal.new",
    "terminal.next",
    "terminal.previous",
    "terminal.rename",
    "terminal.toggle",
    "theme.select",
    "toast.dismiss_all",
    "view.toggle_breadcrumbs",
    "view.toggle_status_bar",
    "vim.key",
    "vim.toggle",
    "window.close",
    "window.minimize",
    "window.new",
    "window.toggle_fullscreen",
    "window.zoom",
    "workbench.change_encoding",
    "workbench.clear_recent",
    "workbench.close_all_editors",
    "workbench.close_group_editors",
    "workbench.close_other_tabs",
    "workbench.close_saved",
    "workbench.close_tab",
    "workbench.keep_editor",
    "workbench.last_editor_in_group",
    "workbench.new_file",
    "workbench.next_tab",
    "workbench.open_editor_at_index1",
    "workbench.open_editor_at_index2",
    "workbench.open_editor_at_index3",
    "workbench.open_editor_at_index4",
    "workbench.open_editor_at_index5",
    "workbench.open_editor_at_index6",
    "workbench.open_editor_at_index7",
    "workbench.open_editor_at_index8",
    "workbench.open_editor_at_index9",
    "workbench.open_file",
    "workbench.open_folder",
    "workbench.open_folder_in_new_window",
    "workbench.open_keyboard_shortcuts",
    "workbench.open_keyboard_shortcuts_json",
    "workbench.open_recent",
    "workbench.open_settings",
    "workbench.open_settings_ui",
    "workbench.previous_tab",
    "workbench.reopen_closed_editor",
    "workbench.reopen_with_encoding",
    "workbench.save_as",
    "workbench.save_with_encoding",
    "workbench.show_config_problems",
    "workbench.show_extensions",
    "workbench.toggle_zen_mode",
    "workbench.trust_folder",
    "workbench.untrust_folder",
];

/// The IDs it listed at the base only with the editor focused.
const LISTED_IN_THE_EDITOR: &[&str] = &[
    "editor.add_caret_at_next_occurrence",
    "editor.add_carets_at_all_occurrences",
    "editor.add_cursor_above",
    "editor.add_cursor_below",
    "editor.backspace",
    "editor.column_select_down",
    "editor.column_select_left",
    "editor.column_select_right",
    "editor.column_select_up",
    "editor.convert_indentation_to_spaces",
    "editor.convert_indentation_to_tabs",
    "editor.copy",
    "editor.cut",
    "editor.delete",
    "editor.cursor_redo",
    "editor.cursor_undo",
    "editor.delete_duplicate_lines",
    "editor.delete_rows",
    "editor.delete_word_left",
    "editor.delete_word_right",
    "editor.delete_all_left",
    "editor.delete_all_right",
    "editor.transpose_letters",
    "editor.detect_indentation",
    "editor.duplicate_rows",
    "editor.expand_selection",
    "editor.fold",
    "editor.fold_all",
    "editor.go_to_bracket",
    "editor.fold_level_1",
    "editor.fold_level_2",
    "editor.fold_level_3",
    "editor.fold_level_4",
    "editor.fold_level_5",
    "editor.fold_level_6",
    "editor.fold_level_7",
    "editor.fold_recursively",
    "editor.go_to_last_edit_location",
    "editor.indent",
    "editor.move_document_end",
    "editor.move_document_start",
    "editor.move_last_selection_to_next_find_match",
    "editor.move_down",
    "editor.move_left",
    "editor.move_line_end",
    "editor.move_line_start",
    "editor.move_right",
    "editor.join_lines",
    "editor.trim_trailing_whitespace",
    "editor.move_rows_down",
    "editor.move_rows_up",
    "editor.move_up",
    "editor.move_word_left",
    "editor.move_word_right",
    "editor.newline",
    "editor.open_link",
    "editor.outdent",
    "editor.page_down",
    "editor.page_up",
    "editor.paste",
    "editor.peek_definition",
    "editor.peek_references",
    "editor.redo",
    "editor.save",
    "editor.select_all",
    "editor.select_all_occurrences",
    "editor.select_document_end",
    "editor.select_document_start",
    "editor.select_down",
    "editor.select_left",
    "editor.select_line_end",
    "editor.select_line_start",
    "editor.select_next_occurrence",
    "editor.select_page_down",
    "editor.select_page_up",
    "editor.select_right",
    "editor.select_up",
    "editor.select_word_left",
    "editor.select_word_right",
    "editor.set_end_of_line_crlf",
    "editor.set_end_of_line_lf",
    "editor.show_hover",
    "editor.show_signature_help",
    "editor.shrink_selection",
    "editor.single_selection",
    "editor.sort_lines_ascending",
    "editor.sort_lines_descending",
    "editor.split_selection_into_lines",
    "editor.tab",
    "editor.transform_lowercase",
    "editor.transform_titlecase",
    "editor.transform_uppercase",
    "editor.toggle_comment",
    "editor.toggle_soft_wrap",
    "editor.trigger_completion",
    "editor.undo",
    "editor.unfold",
    "editor.unfold_all",
    "editor.unfold_recursively",
    "find.next",
    "find.previous",
    "find.show",
    "lsp.code_actions",
    "lsp.format_document",
    "lsp.format_selection",
    "lsp.rename",
    "markdown.show_preview",
    "markdown.show_preview_to_side",
    "markdown.toggle_preview",
    "vim.normal_resume",
];

/// The IDs it listed at the base only with the project panel focused.
const LISTED_IN_THE_PROJECT_PANEL: &[&str] = &[
    "project_panel.collapse_or_parent",
    "project_panel.copy_path",
    "project_panel.copy_relative_path",
    "project_panel.delete",
    "project_panel.duplicate",
    "project_panel.expand_or_first_child",
    "project_panel.new_file",
    "project_panel.new_folder",
    "project_panel.open",
    "project_panel.rename",
    "project_panel.select_first",
    "project_panel.select_last",
    "project_panel.select_next",
    "project_panel.select_prev",
];

/// The case commands are registered with these IDs and titles.
#[test]
fn the_case_transform_commands_have_their_ids_and_titles() {
    let registry = fe_editor::commands::registry();
    for (id, title) in [
        ("editor.transform_uppercase", "Transform to Uppercase"),
        ("editor.transform_lowercase", "Transform to Lowercase"),
        ("editor.transform_titlecase", "Transform to Title Case"),
    ] {
        let meta = registry.commands().find(|meta| meta.id.as_str() == id);
        assert_eq!(
            meta.map(|meta| meta.title.to_string()),
            Some(title.to_string()),
            "{id}"
        );
    }
}

/// The sort and dedupe line commands are registered with these IDs and titles.
#[test]
fn the_line_sort_commands_have_their_ids_and_titles() {
    let registry = fe_editor::commands::registry();
    for (id, title) in [
        ("editor.sort_lines_ascending", "Sort Lines Ascending"),
        ("editor.sort_lines_descending", "Sort Lines Descending"),
        ("editor.delete_duplicate_lines", "Delete Duplicate Lines"),
        ("editor.join_lines", "Join Lines"),
        ("editor.delete_all_left", "Delete All Left"),
        ("editor.delete_all_right", "Delete All Right"),
        ("editor.delete_word_right", "Delete Word Right"),
        ("editor.transpose_letters", "Transpose Letters"),
        (
            "editor.trim_trailing_whitespace",
            "Trim Trailing Whitespace",
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

/// The image viewer's zoom commands, bound in its key context, are in the
/// catalog and the palette's list, which shows them where the viewer has
/// the focus.
#[test]
fn the_image_viewers_zoom_commands_are_cataloged_and_listed() {
    let ids = [
        "image.zoom_in",
        "image.zoom_out",
        "image.zoom_fit",
        "image.zoom_actual",
    ];
    let catalog = crate::command_catalog();
    let list = crate::command_list();
    for id in ids {
        let command = CommandId::parse(id).unwrap();
        assert_eq!(catalog.check(&command, &Value::Null), Ok(()), "{id}");
        let listed: Vec<_> = list.iter().filter(|meta| meta.id == command).collect();
        assert_eq!(listed.len(), 1, "{id}");
        assert!(listed[0].context.is_some(), "{id} is listed in a context");
    }
}

/// Open as Text and Open as Image, which swap an SVG's viewer tab and
/// editor tab, are typed commands in the catalog.
#[test]
fn the_svg_view_as_text_commands_are_cataloged() {
    let catalog = crate::command_catalog();
    for id in ["image.open_as_text", "image.open_as_image"] {
        let command = CommandId::parse(id).unwrap();
        assert_eq!(catalog.check(&command, &Value::Null), Ok(()), "{id}");
    }
}

/// Preserve Case's commands, the find bar's typed one and the search
/// panel's, are in the catalog with their titles and contexts.
#[test]
fn the_preserve_case_commands_are_in_the_catalog() {
    let catalog = crate::command_catalog();
    for id in ["find.toggle_preserve_case", "search.toggle_preserve_case"] {
        let command = CommandId::parse(id).unwrap();
        assert_eq!(catalog.check(&command, &Value::Null), Ok(()), "{id}");
    }
    let registry = crate::search_panel::registry();
    let meta = (registry.commands()).find(|meta| meta.id.as_str() == "search.toggle_preserve_case");
    let meta = meta.expect("search.toggle_preserve_case");
    assert_eq!(meta.title.to_string(), "Toggle Preserve Case");
    assert_eq!(
        meta.context.as_ref().map(ToString::to_string).as_deref(),
        Some("search_panel")
    );
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
