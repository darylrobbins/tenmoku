//! A remap's `<Cmd>fe:…<CR>`: a command that fails as it runs ends the map,
//! and the host's catalog tells which commands exist at all.

use fe_buffer::{Buffer, Origin, Point};
use fe_editor::EditorModel;
use fe_vim::{Remap, RemapMode};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};

use super::*;
use crate::{EditorTheme, EditorView, init};

fn vim_editor(
    cx: &mut TestAppContext,
    remaps: Vec<Remap>,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new("one\ntwo\nthree"), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.update(|_, cx| set_vim_enabled(cx, true));
    let issues = cx.update(|_, cx| set_vim_remaps(cx, &remaps, ","));
    assert_eq!(issues, []);
    cx.run_until_parked();
    (view, cx)
}

fn noremap(from: &str, to: &str) -> Remap {
    Remap {
        modes: vec![RemapMode::Normal],
        from: from.to_string(),
        to: to.to_string(),
        recursive: false,
    }
}

fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Point {
    view.read_with(cx, |view, _| {
        let model = view.model();
        model
            .text()
            .offset_to_point(model.primary_selection().head())
    })
}

/// `editor.undo` fails when a later edit of another origin changed what the
/// user's edit wrote; the map's `j` after it doesn't run.
#[gpui::test]
fn a_failing_editor_command_stops_the_map(cx: &mut TestAppContext) {
    let remaps = vec![noremap("<leader>u", "<Cmd>fe:editor.undo<CR>j")];
    let (view, cx) = vim_editor(cx, remaps);
    view.update(cx, |view, _| {
        view.model_mut().insert("Z");
        view.model_mut()
            .buffer_mut()
            .edit([(0..1, "Q")], Origin::Lsp);
    });
    cx.simulate_keystrokes(", u");
    cx.run_until_parked();
    assert_eq!(caret(&view, cx).row, 0);
    assert!(view.read_with(cx, |view, _| view.command_failed));
}

/// A workbench panel's command is known once the host hands over its
/// catalog, and isn't before.
#[gpui::test]
fn workbench_panel_commands_are_known(cx: &mut TestAppContext) {
    struct Panel;
    impl CommandCatalog for Panel {
        fn check(&self, command: &CommandId, _: &Value) -> Result<(), CatalogError> {
            if command.as_str() == "project_panel.focus" {
                Ok(())
            } else {
                Err(CatalogError::UnknownCommand)
            }
        }
    }
    let remaps = vec![noremap("<leader>p", "<Cmd>fe:project_panel.focus<CR>j")];
    let (view, cx) = vim_editor(cx, remaps);
    let message = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            view.vim_state().unwrap().message().map(str::to_owned)
        })
    };
    cx.simulate_keystrokes(", p");
    cx.run_until_parked();
    assert_eq!(
        message(&view, cx).as_deref(),
        Some("E492: Not an editor command: fe:project_panel.focus")
    );
    set_host_catalog(Box::new(Panel));
    cx.simulate_keystrokes(", p");
    cx.run_until_parked();
    assert_ne!(
        message(&view, cx).as_deref(),
        Some("E492: Not an editor command: fe:project_panel.focus")
    );
    set_host_catalog_cleared();
}

fn set_host_catalog_cleared() {
    HOST_CATALOG.with_borrow_mut(|host| *host = None);
}
