//! The Settings tab's theme dropdowns: `theme.dark`, `theme.light` and
//! `theme.name` list the bundled themes and an installed extension's, as
//! Change Color Theme does, and a pick writes what that would.
//!
//! `set_setting` puts the user's file where the environment says
//! (`TENMOKU_CONFIG_DIR`), and a project can't set the theme, so the
//! tests that write run again in a process of their own with the variable
//! set. The tab reads the user's file from the test-only override, set to
//! the same directory.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_config::{
    ChoiceSource,
    themes::{BundledTheme, bundled_themes},
};
use gpui::{Entity, Focusable as _, Modifiers, TestAppContext, VisualTestContext};
use kiln::{
    Config,
    core::{ApiVersion, extension::MANIFEST_FILE, theme::Appearance},
    platform::config::{BatchSender, batch_channel},
};

use super::{
    Scope, SettingsEditor, choices,
    controls::{self, DEFAULT_CHOICE, NOT_INSTALLED},
};
use crate::{Launch, Workbench, open::ConfigDirOverride};

/// The variable naming the [`Dirs`] base that a test run again in a
/// process of its own works in.
const OWN_PROCESS_DIRS: &str = "FE_SETTINGS_CHOICES_TEST_DIRS";

/// Acme's themes, as they're installed first: a stem, title and
/// appearance each.
const ACME: &[(&str, &str, &str)] = &[
    ("solarized-dark", "Solarized Dark", "dark"),
    ("solarized-light", "Solarized Light", "light"),
];

/// Installs the extension `id` with `themes` under `base`'s extensions
/// directory, replacing what it had.
fn extension(base: &Path, id: &str, themes: &[(&str, &str, &str)]) {
    let dir = base.join("extensions").join(id);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("themes")).unwrap();
    let files: Vec<String> = (themes.iter())
        .map(|(stem, _, _)| format!("\"themes/{stem}.toml\""))
        .collect();
    let manifest = format!(
        "id = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\nlicense = \"MIT\"\n\n\
         [engine]\ntenmoku = \"1.0\"\n\n[contributes]\nthemes = [{}]\n",
        files.join(", ")
    );
    fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
    for (stem, title, appearance) in themes {
        let theme = format!("name = \"{title}\"\nappearance = \"{appearance}\"\n");
        fs::write(dir.join(format!("themes/{stem}.toml")), theme).unwrap();
    }
}

/// A project folder, a config directory and the Acme extension under one
/// temporary base.
pub(super) struct Dirs {
    _temp: Option<tempfile::TempDir>,
    pub(super) base: PathBuf,
    root: PathBuf,
    config: PathBuf,
}

impl Dirs {
    pub(super) fn new() -> Self {
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
        fs::create_dir_all(root.join(".tenmoku")).unwrap();
        fs::create_dir_all(&config).unwrap();
        extension(base, "acme.solarized", ACME);
        Self {
            _temp: None,
            base: base.to_owned(),
            root,
            config,
        }
    }

    pub(super) fn user(&self) -> PathBuf {
        self.config.join("settings.toml")
    }

    fn workspace(&self) -> PathBuf {
        fe_config::project_settings_path(&self.root)
    }
}

/// Runs the test `name` of this module again, in a process of its own
/// whose `TENMOKU_CONFIG_DIR` is a temporary config directory, and checks
/// that it passed: `None` here, once it has. There, it's `Some` with the
/// directories it runs in.
fn in_own_process(name: &str) -> Option<Dirs> {
    in_own_process_of(module_path!(), name)
}

/// [`in_own_process`] for the test `name` of `module`, a `module_path!()`.
pub(super) fn in_own_process_of(module: &str, name: &str) -> Option<Dirs> {
    if let Some(base) = std::env::var_os(OWN_PROCESS_DIRS) {
        return Some(Dirs::at(Path::new(&base)));
    }
    let dirs = Dirs::new();
    let module = module.split_once("::").unwrap().1;
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([&format!("{module}::{name}"), "--exact", "--test-threads=1"])
        .env("TENMOKU_CONFIG_DIR", &dirs.config)
        .env(OWN_PROCESS_DIRS, &dirs.base)
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

/// A workbench on `dirs`' root, with the Acme extension installed and the
/// user's settings `dirs`' config directory's, focused; and the sender
/// standing in for the config watcher.
pub(super) fn workbench<'a>(
    cx: &'a mut TestAppContext,
    dirs: &Dirs,
) -> (Entity<Workbench>, &'a mut VisualTestContext, BatchSender) {
    let api = ApiVersion { major: 1, minor: 0 };
    let (sender, batches) = batch_channel();
    let kiln = Config::new("dev.fastedit.test", "fastedit")
        .extensions(
            dirs.base.join("extensions"),
            &fe_config::paths::app_id().short,
            api,
        )
        .watch_with(batches);
    cx.update(|cx| assert_eq!(kiln::init(cx, kiln), []));
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
    (workbench, cx, sender)
}

/// Draws, delivers the next frame's callbacks, as the platform would, and
/// draws what they changed.
pub(super) fn draw(cx: &mut VisualTestContext) {
    for _ in 0..2 {
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.simulate_next_frame(cx);
        });
    }
    cx.run_until_parked();
}

/// Opens the Settings tab on `scope` and searches for `key`, so its row
/// shows at the top.
pub(super) fn open_settings(
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
    search_for(&editor, key, cx);
    editor
}

/// Searches for `key` instead, from the search field.
pub(super) fn search_for(editor: &Entity<SettingsEditor>, key: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let search = editor.read(cx).search_input().focus_handle(cx);
        window.focus(&search, cx);
    });
    cx.simulate_keystrokes("cmd-backspace");
    cx.simulate_input(key);
    draw(cx);
    assert!(drawn(&format!("settings-row-{key}"), cx), "{key} shows");
}

pub(super) fn bounds(selector: &str, cx: &mut VisualTestContext) -> gpui::Bounds<gpui::Pixels> {
    let found = cx.debug_bounds(selector.to_owned().leak());
    found.unwrap_or_else(|| panic!("{selector} is drawn"))
}

pub(super) fn drawn(selector: &str, cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(selector.to_owned().leak()).is_some()
}

pub(super) fn click(selector: &str, cx: &mut VisualTestContext) {
    let at = bounds(selector, cx).center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

pub(super) fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    draw(cx);
}

pub(super) fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

pub(super) fn option(key: &str, value: &str) -> String {
    format!("setting-option-{key}-{value}")
}

/// What `key`'s dropdown lists now, `None` for Default (built-in).
pub(super) fn listed(
    editor: &Entity<SettingsEditor>,
    key: &str,
    cx: &mut VisualTestContext,
) -> Vec<Option<String>> {
    editor.read_with(cx, |editor, cx| {
        let entry = editor.entry(key).unwrap();
        let options = controls::options(entry, cx).unwrap();
        (options.into_iter())
            .map(|option| option.map(|variant| variant.value))
            .collect()
    })
}

/// The open dropdown's setting, the options it shows, `None` for Default
/// (built-in), and the one highlighted.
pub(super) fn open_list(
    editor: &Entity<SettingsEditor>,
    cx: &mut VisualTestContext,
) -> Option<(String, Vec<Option<String>>, usize)> {
    editor.read_with(cx, |editor, _| {
        let (key, options, highlighted) = editor.open_select()?;
        let options = (options.iter())
            .map(|option| option.as_ref().map(|variant| variant.value.clone()))
            .collect();
        Some((key.to_owned(), options, highlighted))
    })
}

/// Whether `selector` is drawn wholly inside `key`'s open list, scrolled
/// into view.
fn in_view(key: &str, selector: &str, cx: &mut VisualTestContext) -> bool {
    let menu = bounds(&format!("setting-menu-{key}"), cx);
    let option = bounds(selector, cx);
    option.top() >= menu.top() && option.bottom() <= menu.bottom()
}

/// What `key`'s dropdown shows, and its hint.
pub(super) fn label(
    editor: &Entity<SettingsEditor>,
    key: &str,
    cx: &mut VisualTestContext,
) -> (String, Option<&'static str>) {
    editor.read_with(cx, |editor, cx| {
        let entry = editor.entry(key).unwrap();
        let options = controls::options(entry, cx).unwrap();
        let (value, _) = editor.value(entry, cx).unwrap();
        controls::select_label(entry, &options, &value)
    })
}

/// The bundled themes of `appearance`, or all of them.
fn bundled(appearance: Option<Appearance>) -> impl Iterator<Item = &'static BundledTheme> {
    (bundled_themes().iter()).filter(move |theme| appearance.is_none_or(|a| theme.appearance == a))
}

#[gpui::test]
fn theme_dropdown_lists_installed_and_bundled(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx, _watcher) = workbench(cx, &dirs);

    // theme.name: Default (built-in), then every bundled theme by title,
    // in the picker's order.
    let key = "theme.name";
    let editor = open_settings(&workbench, Scope::User, key, cx);
    let titles = bundled(None).map(|theme| Some(theme.title.to_owned()));
    let expected: Vec<Option<String>> = [None].into_iter().chain(titles).collect();
    assert_eq!(listed(&editor, key, cx), expected);
    assert!(!drawn(&option(key, "Dark Modern"), cx), "closed");
    click(&format!("setting-select-{key}"), cx);
    let mut above = bounds(&format!("setting-option-default-{key}"), cx).top();
    for theme in bundled(None) {
        let top = bounds(&option(key, theme.title), cx).top();
        assert!(top > above, "{} is listed in order", theme.title);
        above = top;
    }
    press("escape", cx);

    // A slot: the bundled themes of its appearance, then the installed
    // ones, by ID.
    for (key, appearance, installed) in [
        (
            "theme.dark",
            Appearance::Dark,
            "acme.solarized/solarized-dark",
        ),
        (
            "theme.light",
            Appearance::Light,
            "acme.solarized/solarized-light",
        ),
    ] {
        search_for(&editor, key, cx);
        let ids = bundled(Some(appearance))
            .map(|theme| kiln::extensions::builtin_theme_id(theme.id))
            .chain([installed.to_owned()]);
        let expected: Vec<Option<String>> = [None].into_iter().chain(ids.map(Some)).collect();
        assert_eq!(listed(&editor, key, cx), expected, "{key}");
        click(&format!("setting-select-{key}"), cx);
        for value in expected.iter().flatten() {
            assert!(drawn(&option(key, value), cx), "{value} is listed");
        }
        let other = bundled(None).find(|theme| theme.appearance != appearance);
        let other = kiln::extensions::builtin_theme_id(other.unwrap().id);
        assert!(!drawn(&option(key, &other), cx), "{other} isn't");
        press("escape", cx);
    }

    // Each described by its title, or its appearance for a title.
    let described = cx.update(|_, cx| {
        let dark = Some(Appearance::Dark);
        let slot = choices::variants(ChoiceSource::Themes { appearance: dark }, cx);
        let name = choices::variants(ChoiceSource::Themes { appearance: None }, cx);
        (
            slot.last().cloned().unwrap(),
            name.first().cloned().unwrap(),
        )
    });
    assert_eq!(described.0.description, "Solarized Dark");
    assert_eq!(described.1.description, "dark");
}

#[gpui::test]
fn picking_writes_the_id(cx: &mut TestAppContext) {
    let Some(dirs) = in_own_process("picking_writes_the_id") else {
        return;
    };
    fs::write(dirs.user(), "# Mine\n[theme]\n# Dark.\nmode = \"dark\"\n").unwrap();
    let (workbench, cx, _watcher) = workbench(cx, &dirs);

    // An installed theme, by a click: its ID.
    let key = "theme.dark";
    let editor = open_settings(&workbench, Scope::User, key, cx);
    click(&format!("setting-select-{key}"), cx);
    click(&option(key, "acme.solarized/solarized-dark"), cx);
    assert!(!drawn(&option(key, "acme.solarized/solarized-dark"), cx));
    assert_eq!(
        read(&dirs.user()),
        "# Mine\n[theme]\n# Dark.\nmode = \"dark\"\ndark = \"acme.solarized/solarized-dark\"\n"
    );
    assert!(!dirs.workspace().exists(), "the workspace's isn't written");
    let shown = ("acme.solarized/solarized-dark".to_owned(), None);
    assert_eq!(label(&editor, key, cx), shown);

    // A bundled theme for theme.name, by Enter: its title.
    let key = "theme.name";
    search_for(&editor, key, cx);
    click(&format!("settings-description-{key}"), cx);
    press("enter", cx);
    assert!(drawn(&option(key, "Dark Modern"), cx), "open");
    // Default (built-in) is highlighted; the first bundled one is next.
    press("down enter", cx);
    let first = bundled_themes()[0].title;
    assert_eq!(
        read(&dirs.user()),
        format!(
            "# Mine\n[theme]\n# Dark.\nmode = \"dark\"\n\
             dark = \"acme.solarized/solarized-dark\"\nname = \"{first}\"\n"
        )
    );
    assert_eq!(label(&editor, key, cx), (first.to_owned(), None));
}

#[gpui::test]
fn unknown_value_still_shows(cx: &mut TestAppContext) {
    let Some(dirs) = in_own_process("unknown_value_still_shows") else {
        return;
    };
    let user = "[theme]\n# Gone.\ndark = \"acme.gone/old\"\n";
    fs::write(dirs.user(), user).unwrap();
    let (workbench, cx, _watcher) = workbench(cx, &dirs);

    // The value, as the file has it, with a hint, and left alone.
    let key = "theme.dark";
    let editor = open_settings(&workbench, Scope::User, key, cx);
    let gone = ("acme.gone/old".to_owned(), Some(NOT_INSTALLED));
    assert_eq!(label(&editor, key, cx), gone);
    assert!(drawn(&format!("setting-hint-{key}"), cx));
    click(&format!("setting-select-{key}"), cx);
    assert!(drawn(&option(key, "acme.solarized/solarized-dark"), cx));
    press("escape", cx);
    assert!(!drawn(&option(key, "acme.solarized/solarized-dark"), cx));
    assert_eq!(read(&dirs.user()), user);

    // A slot not set: Default (built-in), first.
    let light = "theme.light";
    search_for(&editor, light, cx);
    assert_eq!(label(&editor, light, cx), (DEFAULT_CHOICE.to_owned(), None));
    assert!(!drawn(&format!("setting-hint-{light}"), cx));
    search_for(&editor, key, cx);

    // Default (built-in) resets the key, the comments kept.
    click(&format!("setting-select-{key}"), cx);
    let default = format!("setting-option-default-{key}");
    let first = bounds(&option(key, "builtin/vscode-dark-modern"), cx).top();
    assert!(bounds(&default, cx).top() < first, "Default is first");
    click(&default, cx);
    assert_eq!(read(&dirs.user()), "[theme]\n# Gone.\n");
    assert_eq!(label(&editor, key, cx), (DEFAULT_CHOICE.to_owned(), None));
    assert!(!drawn(&format!("setting-hint-{key}"), cx));
}

#[gpui::test]
fn workspace_scope_is_off(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx, _watcher) = workbench(cx, &dirs);
    let editor = open_settings(&workbench, Scope::Workspace, "theme.dark", cx);
    for key in ["theme.dark", "theme.light", "theme.name"] {
        search_for(&editor, key, cx);
        assert!(drawn(&format!("settings-restricted-{key}"), cx), "{key}");
        // Neither a click nor Enter opens it.
        click(&format!("setting-select-{key}"), cx);
        assert!(
            !drawn(&format!("setting-option-default-{key}"), cx),
            "{key}"
        );
        click(&format!("settings-description-{key}"), cx);
        press("enter", cx);
        assert!(
            !drawn(&format!("setting-option-default-{key}"), cx),
            "{key}"
        );
    }
    assert_eq!(read(&dirs.workspace()), "");
}

#[gpui::test]
fn theme_name_aliases_are_known(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    fs::write(dirs.user(), "[theme]\nname = \"vscode-dark-modern\"\n").unwrap();
    let (workbench, cx, _watcher) = workbench(cx, &dirs);
    let key = "theme.name";
    let editor = open_settings(&workbench, Scope::User, key, cx);

    // The bundled theme's ID: shown as the file has it, no hint, and its
    // option highlighted on opening.
    assert_eq!(
        label(&editor, key, cx),
        ("vscode-dark-modern".to_owned(), None)
    );
    assert!(!drawn(&format!("setting-hint-{key}"), cx));
    click(&format!("setting-select-{key}"), cx);
    let (_, options, highlighted) = open_list(&editor, cx).unwrap();
    assert_eq!(options[highlighted].as_deref(), Some("Dark Modern"));
    press("escape", cx);
    assert_eq!(
        read(&dirs.user()),
        "[theme]\nname = \"vscode-dark-modern\"\n"
    );

    // A short title, or another case and spaces, as themes::find takes.
    editor.read_with(cx, |editor, cx| {
        let entry = editor.entry(key).unwrap();
        let options = controls::options(entry, cx).unwrap();
        for alias in ["Ember", "  dark MODERN ", "catppuccin-mocha"] {
            let value = serde_json::Value::from(alias);
            let shown = controls::select_label(entry, &options, &value);
            assert_eq!(shown, (alias.to_owned(), None), "{alias:?}");
        }
        let gone = serde_json::Value::from("Solarized Dark");
        let shown = controls::select_label(entry, &options, &gone);
        assert_eq!(
            shown.1,
            Some(NOT_INSTALLED),
            "an installed one isn't a name"
        );
    });
}

#[gpui::test]
fn an_open_list_keeps_its_options(cx: &mut TestAppContext) {
    let Some(dirs) = in_own_process("an_open_list_keeps_its_options") else {
        return;
    };
    fs::write(dirs.user(), "[theme]\n").unwrap();
    let (workbench, cx, watcher) = workbench(cx, &dirs);
    let key = "theme.dark";
    let editor = open_settings(&workbench, Scope::User, key, cx);
    let rescan = |themes: &[(&str, &str, &str)], cx: &mut VisualTestContext| {
        extension(&dirs.base, "acme.solarized", themes);
        watcher.send(vec![dirs.base.join("extensions")]);
        draw(cx);
    };
    let solarized = "acme.solarized/solarized-dark";
    let aardvark = "acme.solarized/aardvark";

    // Down to Solarized Dark, then a theme sorting before it is installed.
    click(&format!("settings-description-{key}"), cx);
    press("enter", cx);
    let (_, before, _) = open_list(&editor, cx).unwrap();
    let at = before.iter().position(|o| o.as_deref() == Some(solarized));
    for _ in 0..at.unwrap() {
        press("down", cx);
    }
    let added = [("aardvark", "Acme Aardvark", "dark"), ACME[0], ACME[1]];
    rescan(&added, cx);
    let now = listed(&editor, key, cx);
    assert!(
        now.contains(&Some(aardvark.to_owned())),
        "rescanned: {now:?}"
    );
    let (_, shown, highlighted) = open_list(&editor, cx).unwrap();
    assert_eq!(shown, before, "the open list is as it opened");
    assert_eq!(shown[highlighted].as_deref(), Some(solarized));
    assert!(!drawn(&option(key, aardvark), cx));
    press("enter", cx);
    assert_eq!(
        read(&dirs.user()),
        format!("[theme]\ndark = \"{solarized}\"\n")
    );

    // Reopened, it lists the new one; the one highlighted is removed and
    // the one before it is still what Up picks.
    press("enter", cx);
    assert!(drawn(&option(key, aardvark), cx), "reopened afresh");
    let (_, before, highlighted) = open_list(&editor, cx).unwrap();
    assert_eq!(before[highlighted].as_deref(), Some(solarized));
    assert_eq!(before[highlighted - 1].as_deref(), Some(aardvark));
    rescan(&[("aardvark", "Acme Aardvark", "dark"), ACME[1]], cx);
    assert!(!listed(&editor, key, cx).contains(&Some(solarized.to_owned())));
    assert_eq!(open_list(&editor, cx).unwrap().1, before);
    press("up enter", cx);
    assert_eq!(
        read(&dirs.user()),
        format!("[theme]\ndark = \"{aardvark}\"\n")
    );
}

#[gpui::test]
fn a_long_list_scrolls(cx: &mut TestAppContext) {
    let Some(dirs) = in_own_process("a_long_list_scrolls") else {
        return;
    };
    fs::write(dirs.user(), "[theme]\n").unwrap();
    let many: Vec<(String, String)> = (0..30)
        .map(|n| (format!("many-{n:02}"), format!("Many {n:02}")))
        .collect();
    let themes: Vec<(&str, &str, &str)> = (many.iter())
        .map(|(stem, title)| (stem.as_str(), title.as_str(), "dark"))
        .collect();
    extension(&dirs.base, "acme.many", &themes);
    let (workbench, cx, _watcher) = workbench(cx, &dirs);
    cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(480.)));
    draw(cx);
    let key = "theme.dark";
    let editor = open_settings(&workbench, Scope::User, key, cx);
    // Solarized Dark sorts after the Many themes: it's the last.
    let last = option(key, "acme.solarized/solarized-dark");

    // The list is no taller than the window; the last option is below.
    click(&format!("settings-description-{key}"), cx);
    press("enter", cx);
    let (_, options, _) = open_list(&editor, cx).unwrap();
    assert!(options.len() > 30);
    let window = cx.update(|window, _| window.viewport_size());
    let menu = bounds(&format!("setting-menu-{key}"), cx);
    assert!(menu.size.height <= window.height, "{menu:?} in {window:?}");
    assert!(!in_view(key, &last, cx), "the last is scrolled out of view");

    // Down to it scrolls it into view, and Enter picks it.
    for _ in 1..options.len() {
        press("down", cx);
    }
    assert!(in_view(key, &last, cx), "Down scrolled to it");
    press("enter", cx);
    let picked = "[theme]\ndark = \"acme.solarized/solarized-dark\"\n";
    assert_eq!(read(&dirs.user()), picked);

    // Reopened at it; Up to the top scrolls back, and Default resets.
    press("enter", cx);
    assert!(in_view(key, &last, cx), "opened with its value in view");
    for _ in 1..options.len() {
        press("up", cx);
    }
    let default = format!("setting-option-default-{key}");
    assert!(in_view(key, &default, cx) && !in_view(key, &last, cx));
    press("enter", cx);
    assert_eq!(read(&dirs.user()), "", "reset, its emptied table dropped");

    // The wheel scrolls it too, and a click picks the last.
    click(&format!("setting-select-{key}"), cx);
    assert!(!in_view(key, &last, cx));
    let menu = bounds(&format!("setting-menu-{key}"), cx);
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: menu.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(-100_000.))),
        ..Default::default()
    });
    draw(cx);
    assert!(in_view(key, &last, cx), "the wheel scrolled to it");
    click(&last, cx);
    // The emptied file gets the template, the key set in it.
    let written = read(&dirs.user());
    assert!(
        written.contains("\ndark = \"acme.solarized/solarized-dark\"\n"),
        "{written}"
    );
}
