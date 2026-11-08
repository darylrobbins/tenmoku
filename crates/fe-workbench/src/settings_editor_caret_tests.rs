//! The Settings tab's inputs and search field take the caret keys: Home,
//! End and the word motions move the caret, and Backspace and Delete edit
//! at it; Up and Down still work the list. Left's regression tests are
//! `tests::settings_caret`.

use std::fs;

use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use super::{Row, SettingsEditor};
use crate::{Launch, Workbench, open::ConfigDirOverride};

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
fn home_end_and_word_motions(cx: &mut TestAppContext) {
    let (_temp, editor, cx) = settings(cx);
    // Each platform's keymap, with its word motions' modifier.
    for (keymap, word) in [
        (fe_keymap::DEFAULT_MACOS, "alt"),
        (fe_keymap::DEFAULT_LINUX, "ctrl"),
    ] {
        cx.update(|_, cx| {
            fe_gpui::set_user_keymap(cx, fe_keymap::load_keymap(keymap, None).0);
            let search = editor.read(cx).search_input().clone();
            search.update(cx, |search, cx| search.clear(cx));
        });
        type_text("foo bar", cx);
        press("home", cx);
        type_text("X", cx);
        press("end", cx);
        type_text("Y", cx);
        assert_eq!(search_text(&editor, cx), "Xfoo barY", "{word}");

        // Backspace and Delete edit at the caret, before `barY`.
        press(&format!("{word}-left backspace delete"), cx);
        assert_eq!(search_text(&editor, cx), "XfooarY", "{word}");
        press(&format!("{word}-left {word}-right"), cx);
        type_text("Z", cx);
        assert_eq!(search_text(&editor, cx), "XfooarYZ", "{word}");
    }
}

#[gpui::test]
fn arrows_still_move_the_row_selection(cx: &mut TestAppContext) {
    let (_temp, editor, cx) = settings(cx);
    let key = "editor.tab_size";
    click_input(&editor, key, cx);
    let selected = |cx: &mut VisualTestContext| editor.read_with(cx, |editor, _| editor.selected());
    let input_focused = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| editor.read(cx).editing(window, cx).is_some())
    };
    let before = selected(cx);
    assert!(input_focused(cx));

    // Up and Down leave the input and the selection alone.
    press("down up", cx);
    assert!(input_focused(cx), "still in the input");
    assert_eq!(selected(cx), before);
    press("right left", cx);
    type_text("2", cx);
    assert_eq!(field(&editor, key, cx), "24");

    // Out of it, in the list, they move the selection again: Escape
    // leaves the input, and again clears the search.
    press("escape", cx);
    assert!(!input_focused(cx));
    press("escape down", cx);
    let down = selected(cx);
    assert!(down.is_some());
    press("down", cx);
    assert_ne!(selected(cx), down);
    press("up", cx);
    assert_eq!(selected(cx), down);
}
