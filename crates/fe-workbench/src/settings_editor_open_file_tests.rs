//! The Settings tab's Open settings.toml button.

use std::{fs, path::PathBuf};

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext};

use crate::{
    Launch, Workbench,
    open::ConfigDirOverride,
    settings_editor::{Scope, SettingsEditor},
};

struct Dirs {
    _temp: tempfile::TempDir,
    root: PathBuf,
    config: PathBuf,
}

impl Dirs {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(temp.path()).unwrap();
        let (root, config) = (base.join("project"), base.join("config"));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&config).unwrap();
        Self {
            _temp: temp,
            root,
            config,
        }
    }

    fn user(&self) -> PathBuf {
        self.config.join("settings.toml")
    }

    fn workspace(&self) -> PathBuf {
        fe_config::project_settings_path(&self.root)
    }
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn drawn(selector: &str, cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(selector.to_owned().leak()).is_some()
}

/// A workbench on `root` with the Settings tab open on `scope`.
fn open_settings<'a>(
    cx: &'a mut TestAppContext,
    dirs: &Dirs,
    root: bool,
    scope: Scope,
) -> (
    Entity<Workbench>,
    Entity<SettingsEditor>,
    &'a mut VisualTestContext,
) {
    let launch = Launch {
        root: root.then(|| dirs.root.clone()),
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| {
        cx.set_global(ConfigDirOverride(Some(dirs.config.clone())));
        window.activate_window();
    });
    draw(cx);
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.open_settings_ui(window, cx)
    });
    draw(cx);
    let editor = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let tab = &pane.tabs()[pane.active().unwrap()];
        tab.item().downcast::<SettingsEditor>().unwrap()
    });
    editor.update(cx, |editor, cx| editor.set_scope(scope, cx));
    draw(cx);
    (workbench, editor, cx)
}

fn click_open(cx: &mut VisualTestContext) {
    let at = cx
        .debug_bounds("setting-open-file")
        .expect("the button is drawn")
        .center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

/// The active editor's file, if a file is active.
fn active_file(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    workbench.read_with(cx, |workbench, cx| {
        let editor = workbench.active_editor(cx)?;
        editor.read(cx).model().path().map(ToOwned::to_owned)
    })
}

fn settings_tab_open(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.pane().read(cx).tabs().iter())
            .any(|tab| tab.item().downcast::<SettingsEditor>().is_some())
    })
}

#[gpui::test]
fn button_opens_user_file(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, _, cx) = open_settings(cx, &dirs, true, Scope::User);
    assert!(!dirs.user().exists());
    click_open(cx);
    assert_eq!(active_file(&workbench, cx), Some(dirs.user()));
    assert!(dirs.user().exists(), "created from the template");
    assert!(settings_tab_open(&workbench, cx));
}

#[gpui::test]
fn button_follows_scope(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    fs::create_dir_all(dirs.workspace().parent().unwrap()).unwrap();
    fs::write(dirs.workspace(), "[editor]\ntab_size = 2\n").unwrap();
    let (workbench, _, cx) = open_settings(cx, &dirs, true, Scope::Workspace);
    click_open(cx);
    assert_eq!(active_file(&workbench, cx), Some(dirs.workspace()));
    assert!(settings_tab_open(&workbench, cx));
}

#[gpui::test]
fn no_folder_has_no_workspace_scope(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_, editor, cx) = open_settings(cx, &dirs, false, Scope::Workspace);
    assert!(!drawn("segment:This project", cx));
    let file = editor.read_with(cx, |editor, cx| editor.open_file_target(cx));
    assert_eq!(file, Some(dirs.user()));
}

/// The Open button opens the selected scope's file, as its tooltip shows:
/// the switch beside it names the scope.
#[gpui::test]
fn button_shows_the_scope_file(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (_, editor, cx) = open_settings(cx, &dirs, true, Scope::User);
    let target = |editor: &Entity<SettingsEditor>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, cx| editor.open_file_target(cx))
    };
    assert_eq!(target(&editor, cx), Some(dirs.user()), "the tooltip's path");
    editor.update(cx, |editor, cx| editor.set_scope(Scope::Workspace, cx));
    draw(cx);
    assert_eq!(target(&editor, cx), Some(dirs.workspace()));
}

#[gpui::test]
fn workspace_file_missing_is_created_or_toasts(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, _, cx) = open_settings(cx, &dirs, true, Scope::Workspace);
    assert!(!dirs.workspace().exists());
    click_open(cx);
    assert!(dirs.workspace().exists(), "created from the template");
    assert_eq!(active_file(&workbench, cx), Some(dirs.workspace()));
}
