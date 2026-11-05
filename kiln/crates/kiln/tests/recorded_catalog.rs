// SPDX-License-Identifier: Apache-2.0

//! `kiln::recorded_catalog`: a crate's `#[kiln::command]`s, fns and view
//! methods, as `inventory` recorded them, checked with no app: their IDs
//! known with no args, args rejected, anything else unknown.

#![allow(
    dead_code,
    reason = "the view is never made: only its commands' records are read"
)]

use gpui::FocusHandle;
use kiln::{
    Cx,
    command::{CommandId, Value},
    core::CatalogError,
    shell::Workspace,
};
use serde_json::json;

/// New Note
#[kiln::command(key = "secondary-n", menu = "File")]
fn new_note(_: &mut Workspace, _: &mut Cx<Workspace>) {}

/// Archive Note
#[kiln::command(id = "notes.archive")]
fn archive(_: &mut Workspace, _: &mut Cx<Workspace>) {}

struct NoteView {
    focus: FocusHandle,
}

#[kiln::command(context = "note")]
impl NoteView {
    /// Rename Note
    #[command(id = "notes.rename", key = "f2")]
    fn rename(&mut self, _: &mut Cx<Self>) {}
}

fn id(id: &str) -> CommandId {
    CommandId::parse(id).unwrap()
}

#[test]
fn a_crate_catalog_knows_its_recorded_commands_with_no_app() {
    let catalog = kiln::recorded_catalog(env!("CARGO_CRATE_NAME"));
    for command in ["recorded_catalog.new_note", "notes.archive", "notes.rename"] {
        assert_eq!(
            catalog.check(&id(command), &Value::Null),
            Ok(()),
            "{command}"
        );
        assert!(
            matches!(
                catalog.check(&id(command), &json!({ "index": 1 })),
                Err(CatalogError::BadArgs(_))
            ),
            "{command} takes no args"
        );
    }
    for command in ["notes.unknown", "app.quit", "picker.confirm"] {
        assert_eq!(
            catalog.check(&id(command), &Value::Null),
            Err(CatalogError::UnknownCommand),
            "{command}"
        );
    }
}

#[test]
fn another_crate_catalog_knows_none_of_them() {
    let catalog = kiln::recorded_catalog("notes_lib");
    assert_eq!(
        catalog.check(&id("notes.rename"), &Value::Null),
        Err(CatalogError::UnknownCommand)
    );
}
