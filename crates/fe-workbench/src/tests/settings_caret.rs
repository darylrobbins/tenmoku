//! Left in the Settings tab's inputs and search field moves the caret, so
//! what's typed next goes in before the last character (T1230). Written
//! against what the tab had before the fix, as a regression test: it drives
//! keys and reads the inputs' text.

use std::fs;

use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use crate::{
    Launch, Workbench,
    open::ConfigDirOverride,
    settings_editor::{Row, SettingsEditor},
};

/// The Settings tab in a workbench on a temporary folder, whose user
/// settings live in a temporary config directory, its search focused.
fn settings(
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    Entity<SettingsEditor>,
    &mut VisualTestContext,
) {
    let temp = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(temp.path()).unwrap();
    let (root, config) = (base.join("project"), base.join("config"));
    fs::create_dir_all(root.join(".tenmoku")).unwrap();
    fs::create_dir_all(&config).unwrap();
    let launch = Launch {
        root: Some(root),
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| {
        cx.set_global(ConfigDirOverride(Some(config)));
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(cx);
    workbench.update_in(cx, |workbench: &mut Workbench, window, cx| {
        workbench.open_settings_ui(window, cx)
    });
    draw(cx);
    let editor = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let tab = &pane.tabs()[pane.active().unwrap()];
        tab.item().downcast::<SettingsEditor>().unwrap()
    });
    (temp, editor, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

fn type_text(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(text);
    draw(cx);
}

fn search_text(editor: &Entity<SettingsEditor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, cx| {
        editor.search_input().read(cx).text().to_owned()
    })
}

/// The settings' keys the list shows.
fn listed(editor: &Entity<SettingsEditor>, cx: &mut VisualTestContext) -> Vec<String> {
    editor.read_with(cx, |editor, _| {
        let model = editor.model().unwrap();
        (editor.rows().iter())
            .filter_map(|row| match row {
                Row::Setting(ix) => Some(model.entries()[*ix].key_path()),
                Row::Group(_) => None,
            })
            .collect()
    })
}

/// Searches for `key` and clicks into its row's input.
fn click_input(editor: &Entity<SettingsEditor>, key: &str, cx: &mut VisualTestContext) {
    type_text(key, cx);
    assert!(listed(editor, cx).contains(&key.to_owned()), "{key} shows");
    let input = cx.debug_bounds(format!("setting-input-{key}").leak());
    let at = input.expect("its input is drawn").center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

fn field(editor: &Entity<SettingsEditor>, key: &str, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, cx| editor.field(key, cx).unwrap().0)
}

#[gpui::test]
fn left_arrow_moves_the_caret_in_a_setting_input(cx: &mut TestAppContext) {
    let (_temp, editor, cx) = settings(cx);
    let key = "editor.tab_size";
    click_input(&editor, key, cx);
    assert_eq!(field(&editor, key, cx), "4");
    press("left", cx);
    type_text("1", cx);
    assert_eq!(field(&editor, key, cx), "14");
}

#[gpui::test]
fn left_arrow_moves_the_caret_in_the_search_field(cx: &mut TestAppContext) {
    let (_temp, editor, cx) = settings(cx);
    type_text("abc", cx);
    press("left left", cx);
    type_text("X", cx);
    assert_eq!(search_text(&editor, cx), "aXbc");

    // The list filters on the text as it is with the caret's edits.
    press("cmd-backspace", cx);
    type_text("tab_sze", cx);
    assert!(!listed(&editor, cx).contains(&"editor.tab_size".to_owned()));
    press("left left", cx);
    type_text("i", cx);
    assert_eq!(search_text(&editor, cx), "tab_size");
    assert!(listed(&editor, cx).contains(&"editor.tab_size".to_owned()));
}
