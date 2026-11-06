//! Blur commits an input edited since its last commit or sync, even when
//! its text is back to the value it showed; with no edit it writes nothing.

use std::fs;

use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};

use crate::{
    Launch, Workbench,
    open::ConfigDirOverride,
    settings_editor::{Scope, SettingsEditor},
};

struct Dirs {
    _temp: tempfile::TempDir,
    root: std::path::PathBuf,
    config: std::path::PathBuf,
}

impl Dirs {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(temp.path()).unwrap();
        let (root, config) = (base.join("project"), base.join("config"));
        fs::create_dir_all(root.join(".tenmoku")).unwrap();
        fs::create_dir_all(&config).unwrap();
        Self {
            _temp: temp,
            root,
            config,
        }
    }

    fn workspace(&self) -> std::path::PathBuf {
        fe_config::project_settings_path(&self.root)
    }

    fn read_workspace(&self) -> String {
        fs::read_to_string(self.workspace()).unwrap_or_default()
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn click(selector: &str, cx: &mut VisualTestContext) {
    let at = cx
        .debug_bounds(selector.to_owned().leak())
        .unwrap_or_else(|| panic!("{selector} is drawn"))
        .center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

/// A workbench on `dirs` with the Settings tab on the workspace scope,
/// searched for `key`.
fn open<'a>(
    cx: &'a mut TestAppContext,
    dirs: &Dirs,
    key: &str,
) -> (Entity<SettingsEditor>, &'a mut VisualTestContext) {
    let launch = Launch {
        root: Some(dirs.root.clone()),
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| {
        cx.set_global(ConfigDirOverride(Some(dirs.config.clone())));
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
    editor.update(cx, |editor, cx| editor.set_scope(Scope::Workspace, cx));
    draw(cx);
    cx.simulate_input(key);
    draw(cx);
    (editor, cx)
}

fn blur(key: &str, cx: &mut VisualTestContext) {
    click(&format!("settings-description-{key}"), cx);
}

#[gpui::test]
fn blur_after_retyping_the_default_writes(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let key = "editor.tab_size";
    let (_editor, cx) = open(cx, &dirs, key);

    // Cleared and retyped to the default it showed, then blurred.
    click(&format!("setting-input-{key}"), cx);
    cx.simulate_keystrokes("cmd-backspace");
    cx.simulate_input("4");
    draw(cx);
    assert_eq!(dirs.read_workspace(), "");
    blur(key, cx);
    assert_eq!(dirs.read_workspace(), "[editor]\ntab_size = 4\n");
}

#[gpui::test]
fn blur_after_retyping_an_inherited_list_writes(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let key = "editor.code_actions_on_save";
    fs::write(
        dirs.config.join("settings.toml"),
        "[editor]\ncode_actions_on_save = [\"source.fixAll\"]\n",
    )
    .unwrap();
    let (editor, cx) = open(cx, &dirs, key);
    editor.update(cx, |editor, cx| editor.reload(cx));
    draw(cx);
    assert_eq!(
        editor.read_with(cx, |e, cx| e.list_texts(key, cx)),
        ["source.fixAll"]
    );

    let item = format!("setting-item-{key}-0");
    click(&item, cx);
    cx.simulate_keystrokes("cmd-backspace");
    cx.simulate_input("source.fixAll");
    draw(cx);
    assert_eq!(dirs.read_workspace(), "");
    blur(key, cx);
    assert_eq!(
        dirs.read_workspace(),
        "[editor]\ncode_actions_on_save = [\"source.fixAll\"]\n"
    );
}

#[gpui::test]
fn blur_without_edit_writes_nothing(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let key = "editor.tab_size";
    let (_editor, cx) = open(cx, &dirs, key);
    click(&format!("setting-input-{key}"), cx);
    blur(key, cx);
    assert_eq!(dirs.read_workspace(), "");
}
