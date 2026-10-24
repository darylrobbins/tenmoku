//! Which key contexts an editor's frames attach under when the Keyboard
//! Shortcuts tab marks clashes: those of the views that hold editors.

use gpui::TestAppContext;

use super::clashing;

#[gpui::test]
fn diffview_negative_only_editor_condition_is_marked(cx: &mut TestAppContext) {
    // `DiffView && !vim.enabled` only rules an editor key out, but a diff
    // view holds editors, so it holds with the editor's binding.
    let marked = clashing(
        &[(
            "secondary-alt-7",
            "DiffView && !vim.enabled",
            "editor && !vim.enabled",
        )],
        cx,
    );
    assert_eq!(marked, [true]);
}

#[gpui::test]
fn non_editor_view_stays_unmarked(cx: &mut TestAppContext) {
    // The project panel holds no editor.
    let marked = clashing(
        &[(
            "secondary-alt-8",
            "project_panel && !vim.enabled",
            "editor && !vim.enabled",
        )],
        cx,
    );
    assert_eq!(marked, [false]);
}

#[gpui::test]
fn git_panel_negative_only_editor_condition_is_marked(cx: &mut TestAppContext) {
    // The Changes panel's commit message is an editor.
    let marked = clashing(
        &[(
            "secondary-alt-9",
            "git_panel && !vim.enabled",
            "editor && !vim.enabled",
        )],
        cx,
    );
    assert_eq!(marked, [true]);
}
