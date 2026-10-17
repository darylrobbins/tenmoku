//! The commands that moved from `fe-command` registries to typed commands
//! (`#[kiln::command]` methods in `workbench_commands.rs`) keep the ID,
//! title and context they had: each is a registered typed command with
//! them, and no registry in [`crate::command_catalog`] still claims it.
//! Each family that moves appends its commands to [`MOVED`].

use fe_command::{CommandId, ContextPredicate, Value};
use fe_keymap::CatalogError;
use gpui::TestAppContext;
use kiln::platform::command_actions::typed_command;

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
