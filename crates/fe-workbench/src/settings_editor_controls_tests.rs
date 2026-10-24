//! The Settings tab's controls in a headless window: a switch, a dropdown,
//! number and text inputs and a list editor writing the selected scope's
//! file, typed into and clicked as a person would; Reset to Default; a
//! write that fails; and Edit in settings.toml.
//!
//! `set_setting` puts the user's file where the environment says
//! (`FASTEDIT_CONFIG_DIR`), so these tests write the workspace's file,
//! all but [`user_scope_writes_user_file`], which runs again in a process
//! of its own with the variable set. The tab reads the user's file from
//! the test-only override, set to the same directory.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_config::Settings;
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln::platform::config::{self, BatchSender, ConfigService};

use crate::{
    Launch, Workbench,
    open::ConfigDirOverride,
    settings_editor::{Scope, SettingsEditor},
};

/// The variable naming the [`Dirs`] base that a test run again in a
/// process of its own works in.
const OWN_PROCESS_DIRS: &str = "FE_SETTINGS_CONTROLS_TEST_DIRS";

/// A project folder and a config directory under one temporary base.
struct Dirs {
    _temp: Option<tempfile::TempDir>,
    root: PathBuf,
    config: PathBuf,
}

impl Dirs {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(temp.path()).unwrap();
        Self {
            _temp: Some(temp),
            ..Self::at(&base)
        }
    }

    /// The directories under `base`, made if they aren't there.
    fn at(base: &Path) -> Self {
        let (root, config) = (base.join("project"), base.join("config"));
        fs::create_dir_all(root.join(".fastedit")).unwrap();
        fs::create_dir_all(&config).unwrap();
        Self {
            _temp: None,
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

/// Runs the test `name` of this module again, in a process of its own
/// whose `FASTEDIT_CONFIG_DIR` is a temporary config directory, and checks
/// that it passed: `None` here, once it has. There, it's `Some` with the
/// directories it runs in.
fn in_own_process(name: &str) -> Option<Dirs> {
    if let Some(base) = std::env::var_os(OWN_PROCESS_DIRS) {
        return Some(Dirs::at(Path::new(&base)));
    }
    let dirs = Dirs::new();
    let base = dirs.config.parent().unwrap().to_owned();
    let module = module_path!().split_once("::").unwrap().1;
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([&format!("{module}::{name}"), "--exact", "--test-threads=1"])
        .env("FASTEDIT_CONFIG_DIR", &dirs.config)
        .env(OWN_PROCESS_DIRS, &base)
        .output()
        .unwrap();
    let (stdout, stderr) = (
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert!(stdout.contains("1 passed"), "it ran: {stdout}");
    None
}

/// A workbench on `dirs`' root, whose user settings are `dirs`' config
/// directory's `settings.toml`, focused.
fn workbench<'a>(
    cx: &'a mut TestAppContext,
    dirs: &Dirs,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
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
    (workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Opens the Settings tab on `scope` and searches for `key`, so its row
/// shows at the top.
fn open_settings(
    workbench: &Entity<Workbench>,
    scope: Scope,
    key: &str,
    cx: &mut VisualTestContext,
) -> Entity<SettingsEditor> {
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
    cx.simulate_input(key);
    draw(cx);
    assert!(drawn(&format!("settings-row-{key}"), cx), "{key} shows");
    editor
}

fn bounds(selector: &str, cx: &mut VisualTestContext) -> gpui::Bounds<gpui::Pixels> {
    let found = cx.debug_bounds(selector.to_owned().leak());
    found.unwrap_or_else(|| panic!("{selector} is drawn"))
}

fn drawn(selector: &str, cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(selector.to_owned().leak()).is_some()
}

fn click(selector: &str, cx: &mut VisualTestContext) {
    let at = bounds(selector, cx).center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

/// Clicks into `selector`'s input, clears it and types `text` there.
fn type_into(selector: &str, text: &str, cx: &mut VisualTestContext) {
    click(selector, cx);
    cx.simulate_keystrokes("cmd-backspace");
    cx.simulate_input(text);
    draw(cx);
}

/// Searches for `text` instead, from the search field.
fn search_for(editor: &Entity<SettingsEditor>, text: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let search = editor.read(cx).search_input().focus_handle(cx);
        window.focus(&search, cx);
    });
    cx.simulate_keystrokes("cmd-backspace");
    cx.simulate_input(text);
    draw(cx);
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

/// `key`'s value as its row shows it, and whether it's marked modified.
fn shown(editor: &Entity<SettingsEditor>, key: &str, cx: &mut VisualTestContext) -> (String, bool) {
    editor.read_with(cx, |editor, cx| {
        let model = editor.model().unwrap();
        let entry = (model.entries().iter())
            .find(|entry| entry.key_path() == key)
            .unwrap();
        editor.row_value(entry, cx).unwrap()
    })
}

/// What `key`'s input holds, and why it wasn't written, if it wasn't.
fn field(
    editor: &Entity<SettingsEditor>,
    key: &str,
    cx: &mut VisualTestContext,
) -> (String, Option<String>) {
    editor.read_with(cx, |editor, cx| editor.field(key, cx).unwrap())
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

/// What the window's shown toasts say, top first.
fn toasts(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| entry.toast.message.clone())
            .collect()
    })
}

#[gpui::test]
fn bool_switch_writes(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.soft_wrap";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    assert_eq!(shown(&editor, key, cx), ("false".to_owned(), false));

    click(&format!("setting-switch-{key}"), cx);
    assert_eq!(read(&dirs.workspace()), "[editor]\nsoft_wrap = true\n");
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));

    // The click selected the row and focused the list: Space toggles it.
    let listing = cx.update(|window, cx| editor.read(cx).list_focus().is_focused(window));
    assert!(listing);
    press("space", cx);
    assert_eq!(read(&dirs.workspace()), "[editor]\nsoft_wrap = false\n");
    assert_eq!(shown(&editor, key, cx), ("false".to_owned(), true));
}

#[gpui::test]
fn enum_pick_writes(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.auto_save";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    let option = |value: &str| format!("setting-option-{key}-{value}");
    assert!(!drawn(&option("off"), cx), "closed");

    // A click opens the list of the schema's variants; a click picks one.
    click(&format!("setting-select-{key}"), cx);
    for value in ["off", "after_delay", "on_focus_change", "on_window_change"] {
        assert!(drawn(&option(value), cx), "{value} is listed");
    }
    click(&option("on_focus_change"), cx);
    assert!(!drawn(&option("off"), cx), "closed once picked");
    assert_eq!(
        read(&dirs.workspace()),
        "[editor]\nauto_save = \"on_focus_change\"\n"
    );
    assert_eq!(
        shown(&editor, key, cx),
        ("on_focus_change".to_owned(), true)
    );

    // Enter on the selected row opens it at the value; Down and Enter
    // pick the next.
    press("enter", cx);
    assert!(drawn(&option("off"), cx), "open");
    press("down enter", cx);
    assert!(!drawn(&option("off"), cx), "closed once picked");
    assert_eq!(
        read(&dirs.workspace()),
        "[editor]\nauto_save = \"on_window_change\"\n"
    );
    assert_eq!(
        shown(&editor, key, cx),
        ("on_window_change".to_owned(), true)
    );

    // Its button closes it, as a click elsewhere does.
    let select = format!("setting-select-{key}");
    click(&select, cx);
    assert!(drawn(&option("off"), cx), "open");
    click(&select, cx);
    assert!(!drawn(&option("off"), cx), "closed by its button");
    click(&select, cx);
    click(&format!("settings-description-{key}"), cx);
    assert!(!drawn(&option("off"), cx), "closed by a click elsewhere");
    assert_eq!(
        shown(&editor, key, cx),
        ("on_window_change".to_owned(), true)
    );

    // Escape closes it, picking nothing.
    press("enter", cx);
    press("up escape", cx);
    assert!(!drawn(&option("off"), cx));
    assert_eq!(
        shown(&editor, key, cx),
        ("on_window_change".to_owned(), true)
    );
}

#[gpui::test]
fn number_commits_on_enter(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.tab_size";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    let input = format!("setting-input-{key}");
    assert_eq!(field(&editor, key, cx), ("4".to_owned(), None));

    type_into(&input, "2", cx);
    assert_eq!(read(&dirs.workspace()), "", "nothing yet");
    press("enter", cx);
    assert_eq!(read(&dirs.workspace()), "[editor]\ntab_size = 2\n");
    assert_eq!(shown(&editor, key, cx), ("2".to_owned(), true));
    assert_eq!(field(&editor, key, cx), ("2".to_owned(), None));

    // Leaving the input commits it too.
    type_into(&input, "8", cx);
    click(&format!("settings-description-{key}"), cx);
    assert_eq!(read(&dirs.workspace()), "[editor]\ntab_size = 8\n");
    assert_eq!(shown(&editor, key, cx), ("8".to_owned(), true));

    // A float's input writes a float.
    let font = "editor.font_size";
    search_for(&editor, font, cx);
    type_into(&format!("setting-input-{font}"), "15", cx);
    press("enter", cx);
    assert_eq!(
        read(&dirs.workspace()),
        "[editor]\ntab_size = 8\nfont_size = 15.0\n"
    );
    assert_eq!(field(&editor, font, cx), ("15.0".to_owned(), None));
}

#[gpui::test]
fn number_out_of_range_shows_message_and_does_not_write(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.tab_size";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    let (input, message) = (
        format!("setting-input-{key}"),
        format!("setting-message-{key}"),
    );
    assert!(!drawn(&message, cx));

    type_into(&input, "99", cx);
    press("enter", cx);
    // In the words of the config problems for a file setting it.
    let expected = "99 is out of range: must be from 1 to 16";
    assert_eq!(
        field(&editor, key, cx),
        ("99".to_owned(), Some(expected.to_owned()))
    );
    let input_bounds = bounds(&input, cx);
    assert!(
        bounds(&message, cx).top() >= input_bounds.bottom(),
        "under it"
    );
    assert!(!dirs.workspace().exists(), "nothing written");
    assert_eq!(shown(&editor, key, cx), ("4".to_owned(), false));

    // Leaving it writes nothing either, and the typed text stays.
    click(&format!("settings-description-{key}"), cx);
    assert!(!dirs.workspace().exists());
    assert_eq!(
        field(&editor, key, cx),
        ("99".to_owned(), Some(expected.to_owned()))
    );

    // Not a number.
    type_into(&input, "two", cx);
    press("enter", cx);
    let wrong = field(&editor, key, cx).1;
    assert_eq!(wrong.as_deref(), Some("must be a whole number"));
    assert!(!dirs.workspace().exists());

    // A value in range clears the message.
    type_into(&input, "16", cx);
    press("enter", cx);
    assert_eq!(field(&editor, key, cx), ("16".to_owned(), None));
    assert!(!drawn(&message, cx));
    assert_eq!(read(&dirs.workspace()), "[editor]\ntab_size = 16\n");
}

#[gpui::test]
fn text_commit(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.font_family";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    let input = format!("setting-input-{key}");
    assert_eq!(field(&editor, key, cx), (String::new(), None), "unset");

    type_into(&input, "Iosevka Term", cx);
    press("enter", cx);
    assert_eq!(
        read(&dirs.workspace()),
        "[editor]\nfont_family = \"Iosevka Term\"\n"
    );
    assert_eq!(shown(&editor, key, cx), ("Iosevka Term".to_owned(), true));

    // Leaving it commits; a quote is escaped.
    type_into(&input, "Fira \"Code\"", cx);
    click(&format!("settings-description-{key}"), cx);
    assert_eq!(
        read(&dirs.workspace()),
        "[editor]\nfont_family = \"Fira \\\"Code\\\"\"\n"
    );
    assert_eq!(shown(&editor, key, cx), ("Fira \"Code\"".to_owned(), true));

    // Emptied, a setting unset by default is unset again.
    type_into(&input, "", cx);
    press("enter", cx);
    assert_eq!(read(&dirs.workspace()), "");
    assert_eq!(shown(&editor, key, cx), ("Not set".to_owned(), false));
}

#[gpui::test]
fn list_add_remove_writes(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.code_actions_on_save";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    let texts =
        |cx: &mut VisualTestContext| editor.read_with(cx, |editor, cx| editor.list_texts(key, cx));
    assert!(texts(cx).is_empty());
    let add = format!("setting-add-{key}");

    // Add Item gives an empty input with focus, written once committed.
    click(&add, cx);
    assert_eq!(texts(cx), [""]);
    assert!(!dirs.workspace().exists());
    cx.simulate_input("source.organizeImports");
    press("enter", cx);
    assert_eq!(
        read(&dirs.workspace()),
        "[editor]\ncode_actions_on_save = [\"source.organizeImports\"]\n"
    );

    click(&add, cx);
    cx.simulate_input("source.fixAll");
    press("enter", cx);
    assert_eq!(
        read(&dirs.workspace()),
        "[editor]\ncode_actions_on_save = [\"source.organizeImports\", \"source.fixAll\"]\n"
    );
    assert_eq!(texts(cx), ["source.organizeImports", "source.fixAll"]);
    assert!(drawn(&format!("setting-item-{key}-1"), cx));

    // Editing an item in place writes the whole list.
    click(&format!("setting-item-{key}-0"), cx);
    press(
        "backspace backspace backspace backspace backspace backspace backspace",
        cx,
    );
    press("enter", cx);
    assert_eq!(
        read(&dirs.workspace()),
        "[editor]\ncode_actions_on_save = [\"source.organize\", \"source.fixAll\"]\n"
    );

    // Removing one writes the rest.
    click(&format!("setting-remove-{key}-0"), cx);
    assert_eq!(
        read(&dirs.workspace()),
        "[editor]\ncode_actions_on_save = [\"source.fixAll\"]\n"
    );
    assert_eq!(texts(cx), ["source.fixAll"]);
    assert!(!drawn(&format!("setting-item-{key}-1"), cx));
    let (value, modified) = shown(&editor, key, cx);
    assert_eq!((value.as_str(), modified), ("[\"source.fixAll\"]", true));
}

#[gpui::test]
fn reset_removes_the_key(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    fs::write(
        dirs.workspace(),
        "# Mine\n[editor]\ntab_size = 2 # narrow\nsoft_wrap = true\n",
    )
    .unwrap();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.tab_size";
    open_settings(&workbench, Scope::Workspace, key, cx);

    click(&format!("setting-reset-{key}"), cx);
    assert_eq!(
        read(&dirs.workspace()),
        "# Mine\n[editor]\nsoft_wrap = true\n"
    );
}

#[gpui::test]
fn reset_updates_the_row(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    fs::write(dirs.workspace(), "[editor]\ntab_size = 2\n").unwrap();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.tab_size";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    let (reset, marker) = (
        format!("setting-reset-{key}"),
        format!("settings-modified-{key}"),
    );
    assert_eq!(shown(&editor, key, cx), ("2".to_owned(), true));
    assert!(drawn(&reset, cx) && drawn(&marker, cx));
    // Its button is on the row.
    let row = bounds(&format!("settings-row-{key}"), cx);
    assert!(row.contains(&bounds(&reset, cx).center()));

    click(&reset, cx);
    assert_eq!(shown(&editor, key, cx), ("4".to_owned(), false));
    assert_eq!(field(&editor, key, cx), ("4".to_owned(), None));
    assert!(!drawn(&marker, cx), "no marker");
    assert!(!drawn(&reset, cx), "a row at its default has no reset");

    // The user's file's value is what a workspace row inherits; it isn't
    // the workspace's to reset.
    fs::write(dirs.user(), "[editor]\ntab_size = 3\n").unwrap();
    editor.update(cx, |editor, cx| editor.reload(cx));
    draw(cx);
    assert_eq!(shown(&editor, key, cx), ("3".to_owned(), false));
    assert!(!drawn(&reset, cx));
}

#[gpui::test]
fn setting_the_default_keeps_it_modified(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.tab_size";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    assert_eq!(shown(&editor, key, cx), ("4".to_owned(), false));

    // Enter on the default as typed still writes it.
    click(&format!("setting-input-{key}"), cx);
    press("enter", cx);
    assert_eq!(read(&dirs.workspace()), "[editor]\ntab_size = 4\n");
    assert_eq!(shown(&editor, key, cx), ("4".to_owned(), true));
    assert!(drawn(&format!("settings-modified-{key}"), cx));
    assert!(drawn(&format!("setting-reset-{key}"), cx));

    // So does picking the default variant.
    let auto_save = "editor.auto_save";
    search_for(&editor, auto_save, cx);
    click(&format!("setting-select-{auto_save}"), cx);
    click(&format!("setting-option-{auto_save}-off"), cx);
    assert_eq!(
        read(&dirs.workspace()),
        "[editor]\ntab_size = 4\nauto_save = \"off\"\n"
    );
    assert_eq!(shown(&editor, auto_save, cx), ("off".to_owned(), true));
}

#[gpui::test]
fn user_scope_writes_user_file(cx: &mut TestAppContext) {
    let Some(dirs) = in_own_process("user_scope_writes_user_file") else {
        return;
    };
    assert_eq!(fe_config::user_settings_path(), Some(dirs.user()));
    fs::write(dirs.user(), "# Mine\n[editor]\ntab_size = 2\n").unwrap();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.soft_wrap";
    let editor = open_settings(&workbench, Scope::User, key, cx);

    click(&format!("setting-switch-{key}"), cx);
    assert_eq!(
        read(&dirs.user()),
        "# Mine\n[editor]\ntab_size = 2\nsoft_wrap = true\n"
    );
    assert!(!dirs.workspace().exists(), "the workspace's isn't written");
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));

    // Reset goes to the user's file too.
    click(&format!("setting-reset-{key}"), cx);
    assert_eq!(read(&dirs.user()), "# Mine\n[editor]\ntab_size = 2\n");
    assert_eq!(shown(&editor, key, cx), ("false".to_owned(), false));
}

/// Kiln's config service on `dirs`' files, as the host starts it, with no
/// watcher: the sender stands in for it.
fn start_service(cx: &mut VisualTestContext, dirs: &Dirs) -> BatchSender {
    let store = fe_config::SettingsStore::load(Some(dirs.user()), Some(dirs.workspace()));
    let (sender, batches) = config::batch_channel();
    cx.update(|_, cx| {
        ConfigService::new()
            .settings(store.into_inner())
            .start_with(batches, cx)
            .detach();
    });
    sender
}

#[gpui::test]
fn workspace_scope_writes_project_file(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let user = "[editor]\nsoft_wrap = true\n";
    fs::write(dirs.user(), user).unwrap();
    let (workbench, cx) = workbench(cx, &dirs);
    let sender = start_service(cx, &dirs);
    let key = "editor.soft_wrap";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    // The user's value is what the workspace inherits.
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), false));

    click(&format!("setting-switch-{key}"), cx);
    assert_eq!(read(&dirs.workspace()), "[editor]\nsoft_wrap = false\n");
    assert_eq!(read(&dirs.user()), user, "the user's isn't written");
    assert_eq!(shown(&editor, key, cx), ("false".to_owned(), true));
    let settings = cx.update(|_, cx| config::settings::<Settings>(cx));
    assert!(settings.editor.soft_wrap, "not reloaded yet");

    // The hot reload that follows changes nothing the tab shows.
    let tab_size = "editor.tab_size";
    search_for(&editor, tab_size, cx);
    type_into(&format!("setting-input-{tab_size}"), "2", cx);
    press("enter", cx);
    assert_eq!(field(&editor, tab_size, cx), ("2".to_owned(), None));
    let written = read(&dirs.workspace());
    sender.send(vec![dirs.workspace()]);
    draw(cx);
    let settings = cx.update(|_, cx| config::settings::<Settings>(cx));
    assert!(!settings.editor.soft_wrap, "reloaded");
    assert_eq!(settings.editor.tab_size, 2);
    assert_eq!(shown(&editor, tab_size, cx), ("2".to_owned(), true));
    assert_eq!(field(&editor, tab_size, cx), ("2".to_owned(), None));
    assert_eq!(read(&dirs.workspace()), written);
    search_for(&editor, key, cx);

    // A row a project can't set takes no change.
    search_for(&editor, "vim.shell", cx);
    assert!(drawn("settings-restricted-vim.shell", cx));
    type_into("setting-input-vim.shell", "zsh", cx);
    press("enter", cx);
    assert_eq!(read(&dirs.workspace()), written);
}

#[gpui::test]
fn write_error_toasts_and_keeps_value(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    // A file that isn't TOML.
    let broken = "[editor\nsoft_wrap = true\n";
    fs::write(dirs.workspace(), broken).unwrap();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.soft_wrap";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    click(&format!("setting-switch-{key}"), cx);
    assert_eq!(read(&dirs.workspace()), broken);
    assert_eq!(toasts(&workbench, cx), [format!("Can't change {key}")]);
    assert_eq!(shown(&editor, key, cx), ("false".to_owned(), false));

    // A folder that can't be written to: the input shows the old value.
    fs::remove_file(dirs.workspace()).unwrap();
    let folder = dirs.workspace().parent().unwrap().to_owned();
    let mut permissions = fs::metadata(&folder).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&folder, permissions.clone()).unwrap();
    let tab_size = "editor.tab_size";
    search_for(&editor, tab_size, cx);
    type_into(&format!("setting-input-{tab_size}"), "2", cx);
    press("enter", cx);
    #[expect(
        clippy::permissions_set_readonly_false,
        reason = "the test's own folder"
    )]
    permissions.set_readonly(false);
    fs::set_permissions(&folder, permissions).unwrap();
    assert!(!dirs.workspace().exists());
    let shown_toasts = toasts(&workbench, cx);
    assert_eq!(
        shown_toasts.last(),
        Some(&format!("Can't change {tab_size}"))
    );
    assert_eq!(shown(&editor, tab_size, cx), ("4".to_owned(), false));
    assert_eq!(field(&editor, tab_size, cx), ("4".to_owned(), None));
}

#[gpui::test]
fn file_comments_survive(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let text = "# My settings\n\n[editor] # the editor\n# narrow tabs\ntab_size = 2 # two\n\
        soft_wrap = false\n\n# keep this\n[terminal]\nscrollback = 500\n";
    fs::write(dirs.workspace(), text).unwrap();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.tab_size";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);

    type_into(&format!("setting-input-{key}"), "3", cx);
    press("enter", cx);
    let set = "# My settings\n\n[editor] # the editor\n# narrow tabs\ntab_size = 3 # two\n\
        soft_wrap = false\n\n# keep this\n[terminal]\nscrollback = 500\n";
    assert_eq!(read(&dirs.workspace()), set);

    // A new key goes in its table, the rest in place.
    let wrap = "editor.soft_wrap";
    search_for(&editor, "editor.minimap.enabled", cx);
    click("setting-switch-editor.minimap.enabled", cx);
    let minimap = format!("{set}\n[editor.minimap]\nenabled = false\n");
    assert_eq!(read(&dirs.workspace()), minimap);

    // Reset keeps the comments around the key.
    search_for(&editor, wrap, cx);
    click(&format!("setting-reset-{wrap}"), cx);
    let reset = "# My settings\n\n[editor] # the editor\n# narrow tabs\ntab_size = 3 # two\n\n\
        # keep this\n[terminal]\nscrollback = 500\n\n[editor.minimap]\nenabled = false\n";
    assert_eq!(read(&dirs.workspace()), reset);
    assert_eq!(shown(&editor, wrap, cx), ("false".to_owned(), false));
}

/// The active editor's file, and its caret's row.
fn caret(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> (PathBuf, u32) {
    workbench.read_with(cx, |workbench, cx| {
        let editor = workbench.active_editor(cx).expect("a file is shown");
        let model = editor.read(cx).model();
        let path = model.path().expect("a file").to_owned();
        let offset = model.primary_selection().range.start;
        (path, model.text().offset_to_point(offset).row)
    })
}

#[gpui::test]
fn other_rows_edit_in_settings_toml(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    fs::write(
        dirs.user(),
        "# Mine\n[editor]\ntab_size = 2\n\n[[vim.remap]]\nfrom = \"jj\"\nto = \"<esc>\"\n",
    )
    .unwrap();
    fs::write(
        dirs.workspace(),
        "[editor]\ntab_size = 2\n\n[lsp.servers.rust]\nenabled = false\n",
    )
    .unwrap();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "vim.remap";
    let editor = open_settings(&workbench, Scope::User, key, cx);
    let link = format!("setting-edit-{key}");
    assert!(!drawn(&format!("setting-input-{key}"), cx));

    click(&link, cx);
    assert_eq!(caret(&workbench, cx), (dirs.user(), 4), "at [[vim.remap]]");

    // Under Workspace, the project's file; a table in the key's.
    let servers = "lsp.servers";
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let tab = pane.read_with(cx, |pane, _| {
        (pane.tabs().iter()).position(|tab| tab.item().downcast::<SettingsEditor>().is_some())
    });
    pane.update_in(cx, |pane, window, cx| {
        pane.activate(tab.unwrap(), window, cx)
    });
    editor.update(cx, |editor, cx| editor.set_scope(Scope::Workspace, cx));
    draw(cx);
    search_for(&editor, servers, cx);
    click(&format!("setting-edit-{servers}"), cx);
    assert_eq!(
        caret(&workbench, cx),
        (dirs.workspace(), 3),
        "at [lsp.servers.rust]"
    );
}

#[test]
fn table_rows() {
    use super::settings_editor::table_row_for_tests as row;
    let text = "# Mine\n[editor]\ntab_size = 2\n[ vim . \"remap\" ]\n[[lsp.servers.rust]]\n";
    assert_eq!(row(text, "vim.remap"), Some(3));
    assert_eq!(row(text, "lsp.servers"), Some(4));
    assert_eq!(row(text, "editor.tab_size"), Some(2));
    assert_eq!(row(text, "terminal.shell"), None);
    assert_eq!(row("vim.remap = []\n", "vim.remap"), Some(0));
    // Quoted keys, and a header-like line inside a string.
    let quoted =
        "[editor]\nx = \"\"\"\n[[vim.remap]]\n\"\"\"\n\n[['vim'.'remap']]\nfrom = \"jj\"\n";
    assert_eq!(row(quoted, "vim.remap"), Some(5));
    assert_eq!(row("[editor\n", "editor"), None, "not TOML");
}

/// Holds `editor`'s writes until the sender sends or is dropped.
fn hold_writes(
    editor: &Entity<SettingsEditor>,
    cx: &mut VisualTestContext,
) -> async_channel::Sender<()> {
    editor.update(cx, |editor, _| editor.hold_writes())
}

#[gpui::test]
fn a_change_being_written_stays_shown_and_editable(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.tab_size";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    let input = format!("setting-input-{key}");
    let held = hold_writes(&editor, cx);

    type_into(&input, "2", cx);
    press("enter", cx);
    assert!(!dirs.workspace().exists(), "held");
    // Rendered while the write waits, the row shows the change.
    draw(cx);
    assert_eq!(shown(&editor, key, cx), ("2".to_owned(), true));
    assert_eq!(field(&editor, key, cx), ("2".to_owned(), None));

    // Typing goes on over it.
    cx.simulate_keystrokes("cmd-backspace");
    cx.simulate_input("3");
    draw(cx);
    assert_eq!(field(&editor, key, cx), ("3".to_owned(), None));

    drop(held);
    draw(cx);
    assert_eq!(read(&dirs.workspace()), "[editor]\ntab_size = 2\n");
    assert_eq!(
        field(&editor, key, cx),
        ("3".to_owned(), None),
        "still typed"
    );
    press("enter", cx);
    assert_eq!(read(&dirs.workspace()), "[editor]\ntab_size = 3\n");
    assert_eq!(shown(&editor, key, cx), ("3".to_owned(), true));
}

#[gpui::test]
fn toggles_before_a_write_lands_each_count(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.soft_wrap";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    let switch = format!("setting-switch-{key}");
    let held = hold_writes(&editor, cx);

    click(&switch, cx);
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));
    click(&switch, cx);
    assert_eq!(shown(&editor, key, cx), ("false".to_owned(), true));
    press("space", cx);
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));
    assert!(!dirs.workspace().exists(), "held");

    drop(held);
    draw(cx);
    assert_eq!(read(&dirs.workspace()), "[editor]\nsoft_wrap = true\n");
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));
}

#[gpui::test]
fn reset_drops_what_is_typed(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    fs::write(
        dirs.workspace(),
        "[editor]\ntab_size = 2\ncode_actions_on_save = [\"a\"]\n",
    )
    .unwrap();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.tab_size";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    let held = hold_writes(&editor, cx);

    // Typed, not committed, then Reset: leaving the input writes nothing.
    type_into(&format!("setting-input-{key}"), "7", cx);
    click(&format!("setting-reset-{key}"), cx);
    assert_eq!(field(&editor, key, cx), ("4".to_owned(), None));
    assert_eq!(shown(&editor, key, cx), ("4".to_owned(), false));

    // A list's new item, likewise.
    let list = "editor.code_actions_on_save";
    search_for(&editor, list, cx);
    click(&format!("setting-add-{list}"), cx);
    cx.simulate_input("b");
    draw(cx);
    click(&format!("setting-reset-{list}"), cx);
    let texts = editor.read_with(cx, |editor, cx| editor.list_texts(list, cx));
    assert!(texts.is_empty(), "{texts:?}");

    drop(held);
    draw(cx);
    assert_eq!(read(&dirs.workspace()), "");
    assert_eq!(shown(&editor, list, cx), ("[]".to_owned(), false));
    search_for(&editor, key, cx);
    assert_eq!(shown(&editor, key, cx), ("4".to_owned(), false));
}

#[gpui::test]
fn closing_the_tab_finishes_its_writes(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.soft_wrap";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    let held = hold_writes(&editor, cx);
    click(&format!("setting-switch-{key}"), cx);
    search_for(&editor, "editor.tab_size", cx);
    type_into("setting-input-editor.tab_size", "2", cx);
    press("enter", cx);
    assert!(!dirs.workspace().exists(), "held");

    close_settings(&workbench, editor, cx);
    // A tab opened meanwhile shows the writes once they land.
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    assert_eq!(shown(&editor, key, cx), ("false".to_owned(), false));

    drop(held);
    draw(cx);
    assert_eq!(
        read(&dirs.workspace()),
        "[editor]\nsoft_wrap = true\ntab_size = 2\n"
    );
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));
}

#[gpui::test]
fn a_failed_write_leaves_later_typing_and_other_scopes(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let broken = "[editor\n";
    fs::write(dirs.workspace(), broken).unwrap();
    fs::write(dirs.user(), "[editor]\ntab_size = 3\n").unwrap();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.tab_size";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    let input = format!("setting-input-{key}");

    // Typing after the commit, before the write fails, stays.
    let held = hold_writes(&editor, cx);
    type_into(&input, "2", cx);
    press("enter", cx);
    cx.simulate_keystrokes("cmd-backspace");
    cx.simulate_input("5");
    drop(held);
    draw(cx);
    assert_eq!(toasts(&workbench, cx), [format!("Can't change {key}")]);
    assert_eq!(read(&dirs.workspace()), broken);
    assert_eq!(field(&editor, key, cx), ("5".to_owned(), None));

    // Failing once the User scope shows, it leaves that scope's value.
    let held = hold_writes(&editor, cx);
    press("enter", cx);
    editor.update(cx, |editor, cx| editor.set_scope(Scope::User, cx));
    draw(cx);
    assert_eq!(field(&editor, key, cx), ("3".to_owned(), None));
    drop(held);
    draw(cx);
    assert_eq!(read(&dirs.workspace()), broken);
    assert_eq!(field(&editor, key, cx), ("3".to_owned(), None));
    assert_eq!(shown(&editor, key, cx), ("3".to_owned(), true));
}

#[gpui::test]
fn an_older_read_never_replaces_a_newer_one(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.tab_size";
    let editor = open_settings(&workbench, Scope::User, key, cx);
    let model = |tab_size: u32| {
        let text = format!("[editor]\ntab_size = {tab_size}\n");
        fe_config::SettingsModel::new(Some(&text), None)
    };
    editor.update(cx, |editor, cx| {
        let (older, newer) = (editor.next_read(), editor.next_read());
        editor.show_read(newer, model(7), cx);
        // A write's read finishing after a watcher's read that started
        // later.
        editor.show_read(older, model(2), cx);
    });
    draw(cx);
    assert_eq!(shown(&editor, key, cx), ("7".to_owned(), true));
}

#[gpui::test]
fn a_long_list_scrolls_within_its_row(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let items: Vec<String> = (0..10).map(|n| format!("\"source.action{n}\"")).collect();
    let text = format!("[editor]\ncode_actions_on_save = [{}]\n", items.join(", "));
    fs::write(dirs.workspace(), text).unwrap();
    let (workbench, cx) = workbench(cx, &dirs);
    cx.simulate_resize(gpui::size(gpui::px(700.), gpui::px(700.)));
    let key = "editor.code_actions_on_save";
    open_settings(&workbench, Scope::Workspace, key, cx);
    let within = |inner: gpui::Bounds<gpui::Pixels>, outer: gpui::Bounds<gpui::Pixels>| {
        inner.left() >= outer.left() && inner.right() <= outer.right()
    };
    let row = bounds(&format!("settings-row-{key}"), cx);
    let scroller = bounds(&format!("setting-items-{key}"), cx);
    assert!(within(scroller, row), "{scroller:?} in {row:?}");
    let add = bounds(&format!("setting-add-{key}"), cx);
    assert!(within(add, row), "Add Item shows: {add:?} in {row:?}");
    let last = format!("setting-item-{key}-9");
    let first = bounds(&last, cx);
    assert!(!within(first, scroller), "out of view at first: {first:?}");

    cx.simulate_event(gpui::ScrollWheelEvent {
        position: scroller.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(-100_000.), gpui::px(0.))),
        ..Default::default()
    });
    draw(cx);
    let last = bounds(&last, cx);
    assert!(
        within(last, scroller),
        "scrolled to: {last:?} in {scroller:?}"
    );
}

#[gpui::test]
fn empty_items_are_kept(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    fs::write(
        dirs.workspace(),
        "[editor]\ncode_actions_on_save = [\"a\", \"\", \"b\"]\n",
    )
    .unwrap();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.code_actions_on_save";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    let texts =
        |cx: &mut VisualTestContext| editor.read_with(cx, |editor, cx| editor.list_texts(key, cx));
    assert_eq!(texts(cx), ["a", "", "b"]);

    click(&format!("setting-item-{key}-2"), cx);
    cx.simulate_input("c");
    press("enter", cx);
    assert_eq!(
        read(&dirs.workspace()),
        "[editor]\ncode_actions_on_save = [\"a\", \"\", \"bc\"]\n"
    );

    // A new item left empty isn't written, the others are.
    click(&format!("setting-add-{key}"), cx);
    click(&format!("setting-remove-{key}-0"), cx);
    assert_eq!(
        read(&dirs.workspace()),
        "[editor]\ncode_actions_on_save = [\"\", \"bc\"]\n"
    );
    assert_eq!(texts(cx), ["", "bc", ""]);
}

#[gpui::test]
fn a_read_only_file_is_left_alone(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let text = "[editor]\ntab_size = 2\n";
    fs::write(dirs.workspace(), text).unwrap();
    let mut permissions = fs::metadata(dirs.workspace()).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(dirs.workspace(), permissions.clone()).unwrap();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.soft_wrap";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);

    click(&format!("setting-switch-{key}"), cx);
    assert_eq!(toasts(&workbench, cx), [format!("Can't change {key}")]);
    assert_eq!(shown(&editor, key, cx), ("false".to_owned(), false));
    let tab_size = "editor.tab_size";
    search_for(&editor, tab_size, cx);
    click(&format!("setting-reset-{tab_size}"), cx);
    assert_eq!(shown(&editor, tab_size, cx), ("2".to_owned(), true));
    assert_eq!(read(&dirs.workspace()), text);
    #[expect(clippy::permissions_set_readonly_false, reason = "the test's own file")]
    permissions.set_readonly(false);
    fs::set_permissions(dirs.workspace(), permissions).unwrap();
}

#[gpui::test]
fn inputs_that_are_off_take_no_edits(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    fs::write(
        dirs.user(),
        "[vim]\nshell = \"zsh\"\n[terminal]\nshell_args = [\"-l\"]\n",
    )
    .unwrap();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "vim.shell";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("bash".to_owned()));

    click(&format!("setting-input-{key}"), cx);
    press("backspace alt-backspace cmd-backspace cmd-v", cx);
    cx.simulate_input("x");
    draw(cx);
    assert_eq!(field(&editor, key, cx), ("zsh".to_owned(), None));

    let list = "terminal.shell_args";
    search_for(&editor, list, cx);
    click(&format!("setting-item-{list}-0"), cx);
    press("backspace alt-backspace cmd-backspace cmd-v", cx);
    let texts = editor.read_with(cx, |editor, cx| editor.list_texts(list, cx));
    assert_eq!(texts, ["-l"]);
    assert!(!dirs.workspace().exists());
}

/// Closes the active tab, the Settings tab, and checks it's gone.
fn close_settings(
    workbench: &Entity<Workbench>,
    editor: Entity<SettingsEditor>,
    cx: &mut VisualTestContext,
) {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    pane.update_in(cx, |pane, window, cx| pane.close_active(window, cx));
    let weak = editor.downgrade();
    drop(editor);
    draw(cx);
    assert!(weak.upgrade().is_none(), "the tab is gone");
}

#[gpui::test]
fn changes_to_a_key_in_both_scopes_stay_apart(cx: &mut TestAppContext) {
    let Some(dirs) = in_own_process("changes_to_a_key_in_both_scopes_stay_apart") else {
        return;
    };
    fs::write(dirs.user(), "# Mine\n").unwrap();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.soft_wrap";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    let switch = format!("setting-switch-{key}");
    let held = hold_writes(&editor, cx);
    let scope = |scope: Scope, cx: &mut VisualTestContext| {
        editor.update(cx, |editor, cx| editor.set_scope(scope, cx));
        draw(cx);
    };

    click(&switch, cx);
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));
    scope(Scope::User, cx);
    assert_eq!(shown(&editor, key, cx), ("false".to_owned(), false));
    click(&switch, cx);
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));
    // Back in Workspace, its own change still shows, and a toggle counts
    // from it.
    scope(Scope::Workspace, cx);
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));
    click(&switch, cx);
    assert_eq!(shown(&editor, key, cx), ("false".to_owned(), true));
    assert!(!dirs.workspace().exists(), "held");

    drop(held);
    draw(cx);
    assert_eq!(read(&dirs.workspace()), "[editor]\nsoft_wrap = false\n");
    let user = read(&dirs.user());
    assert!(user.starts_with("# Mine\n"), "{user}");
    assert!(user.contains("[editor]\nsoft_wrap = true\n"), "{user}");
    assert_eq!(shown(&editor, key, cx), ("false".to_owned(), true));
    scope(Scope::User, cx);
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));
}

#[gpui::test]
fn a_reopened_tab_writes_after_the_closed_ones_writes(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.tab_size";
    let input = format!("setting-input-{key}");
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    let held = hold_writes(&editor, cx);
    type_into(&input, "2", cx);
    press("enter", cx);
    close_settings(&workbench, editor, cx);

    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    type_into(&input, "3", cx);
    press("enter", cx);
    assert!(!dirs.workspace().exists(), "behind the old tab's write");
    assert_eq!(shown(&editor, key, cx), ("3".to_owned(), true));

    drop(held);
    draw(cx);
    assert_eq!(read(&dirs.workspace()), "[editor]\ntab_size = 3\n");
    assert_eq!(shown(&editor, key, cx), ("3".to_owned(), true));
}

#[gpui::test]
fn a_write_failing_once_the_tab_closed_still_toasts(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let broken = "[editor\n";
    fs::write(dirs.workspace(), broken).unwrap();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.soft_wrap";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    let held = hold_writes(&editor, cx);
    click(&format!("setting-switch-{key}"), cx);
    close_settings(&workbench, editor, cx);
    assert!(toasts(&workbench, cx).is_empty());

    drop(held);
    draw(cx);
    assert_eq!(toasts(&workbench, cx), [format!("Can't change {key}")]);
    assert_eq!(read(&dirs.workspace()), broken);
}

#[gpui::test]
fn switching_scope_drops_what_is_typed(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.tab_size";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    type_into(&format!("setting-input-{key}"), "7", cx);
    // The scope changes with the input still focused: its blur, after,
    // writes nothing, to either file (a write to the user's would fail the
    // test's check of where it goes).
    editor.update(cx, |editor, cx| editor.set_scope(Scope::User, cx));
    draw(cx);
    click(&format!("settings-description-{key}"), cx);
    assert!(!dirs.workspace().exists());
    assert!(!dirs.user().exists());
    assert_eq!(field(&editor, key, cx), ("4".to_owned(), None));
}

#[gpui::test]
fn changing_root_drops_what_is_typed(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let other = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.tab_size";
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    type_into(&format!("setting-input-{key}"), "7", cx);
    // The root changes with the input still focused: its blur, after,
    // writes nothing to the new root.
    workbench.update(cx, |_, cx| cx.emit(crate::RootChanged(other.root.clone())));
    draw(cx);
    click(&format!("settings-description-{key}"), cx);
    assert!(!other.workspace().exists());
    assert!(!dirs.workspace().exists());
    assert_eq!(field(&editor, key, cx), ("4".to_owned(), None));
}

#[gpui::test]
fn changes_to_a_key_in_two_roots_stay_apart(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let other = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.soft_wrap";
    let switch = format!("setting-switch-{key}");
    let editor = open_settings(&workbench, Scope::Workspace, key, cx);
    let held = hold_writes(&editor, cx);
    let root = |dirs: &Dirs, cx: &mut VisualTestContext| {
        workbench.update(cx, |_, cx| cx.emit(crate::RootChanged(dirs.root.clone())));
        draw(cx);
    };

    // Each root's file has its own change waiting.
    click(&switch, cx);
    root(&other, cx);
    assert_eq!(shown(&editor, key, cx), ("false".to_owned(), false));
    click(&switch, cx);
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));
    root(&dirs, cx);
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));
    // A third toggle counts from the first root's own change.
    click(&switch, cx);
    assert_eq!(shown(&editor, key, cx), ("false".to_owned(), true));
    root(&other, cx);
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));

    // The first write landing leaves the other root's change waiting.
    held.try_send(()).unwrap();
    draw(cx);
    assert_eq!(read(&dirs.workspace()), "[editor]\nsoft_wrap = true\n");
    assert!(!other.workspace().exists(), "held");
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));

    drop(held);
    draw(cx);
    assert_eq!(read(&dirs.workspace()), "[editor]\nsoft_wrap = false\n");
    assert_eq!(read(&other.workspace()), "[editor]\nsoft_wrap = true\n");
}

#[gpui::test]
fn changes_to_a_key_in_two_user_files_stay_apart(cx: &mut TestAppContext) {
    let Some(dirs) = in_own_process("changes_to_a_key_in_two_user_files_stay_apart") else {
        return;
    };
    let other = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let key = "editor.soft_wrap";
    let switch = format!("setting-switch-{key}");
    let editor = open_settings(&workbench, Scope::User, key, cx);
    let held = hold_writes(&editor, cx);
    // The config directory the tab reads and writes, as a test's own
    // process has it set.
    let config = |dirs: &Dirs, cx: &mut VisualTestContext| {
        // SAFETY: the test runs alone in its own process, on one thread.
        unsafe { std::env::set_var("FASTEDIT_CONFIG_DIR", &dirs.config) };
        cx.update(|_, cx| cx.set_global(ConfigDirOverride(Some(dirs.config.clone()))));
        editor.update(cx, |editor, cx| editor.reload(cx));
        draw(cx);
    };

    click(&switch, cx);
    config(&other, cx);
    assert_eq!(shown(&editor, key, cx), ("false".to_owned(), false));
    click(&switch, cx);
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));
    config(&dirs, cx);
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));
    click(&switch, cx);
    assert_eq!(shown(&editor, key, cx), ("false".to_owned(), true));
    config(&other, cx);
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));

    // A write goes to the config directory as it is when it runs: have
    // each land in the one it was made for, in the order made.
    config(&dirs, cx);
    held.try_send(()).unwrap();
    draw(cx);
    assert!(read(&dirs.user()).contains("soft_wrap = true"));
    assert!(!other.user().exists(), "held");
    config(&other, cx);
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));
    held.try_send(()).unwrap();
    draw(cx);
    assert!(read(&other.user()).contains("soft_wrap = true"));
    assert_eq!(shown(&editor, key, cx), ("true".to_owned(), true));
    // The first root's last change is still waiting.
    config(&dirs, cx);
    assert_eq!(shown(&editor, key, cx), ("false".to_owned(), true));
    drop(held);
    draw(cx);
    assert!(read(&dirs.user()).contains("soft_wrap = false"));
    assert!(read(&other.user()).contains("soft_wrap = true"));
}
