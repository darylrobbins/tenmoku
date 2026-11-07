//! `editor.todo_comments`.

use fe_config::Settings;

#[test]
fn todo_comments_defaults_to_true() {
    assert!(Settings::default().editor.todo_comments);
}
