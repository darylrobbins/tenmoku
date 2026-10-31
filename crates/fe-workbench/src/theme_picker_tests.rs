//! Change Color Theme (`theme.select`): the bundled and installed themes,
//! the one showing marked; a pick written to the user's `settings.toml`,
//! the rest of it kept, and applied by the reload that brings, as the host
//! applies `[theme]` ([`select`] stands in for fastedit's `theme.rs`).

use std::{fs, path::Path};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::{
    Settings, SettingsStore, ThemeModeSetting, ThemeSettings,
    theme::{EXTENSIONS, EditorPalette},
    themes::{bundled_themes, find},
};
use fe_gpui::commands::RunCommand;
use gpui::{App, Entity, Focusable as _, TestAppContext, VisualTestContext, WindowAppearance};
use kiln::{
    Config, GlobalSettings,
    core::{
        ApiVersion,
        extension::MANIFEST_FILE,
        theme::{Appearance, ThemeData},
    },
    extensions::{builtin_theme_id, install_builtin_themes, select_themes},
    platform::config::{BatchSender, batch_channel},
    prelude::AppExt as _,
    ui::{ActiveTheme as _, ThemeMode},
};
use tempfile::TempDir;

use super::ThemePicker;
use crate::{Workbench, open::ConfigDirOverride, picker::Picker, tests::open_root};

const MANIFEST: &str = "\
id = \"acme.night\"
name = \"Night\"
version = \"1.0.0\"
license = \"MIT\"

[engine]
fastedit = \"1.0\"

[contributes]
themes = [\"themes/midnight.toml\", \"themes/noon.toml\"]
";

const MIDNIGHT: &str = "\
name = \"Acme Midnight\"
appearance = \"dark\"

[surface]
background = \"#000010\"

[editor]
background = \"#000020\"
";

const NOON: &str = "\
name = \"Acme Noon\"
appearance = \"light\"

[surface]
background = \"#fffff0\"
";

/// The user's settings: other keys, and comments, a write must keep.
const USER: &str = "\
# My settings.
[editor]
# Wider.
tab_size = 8

[theme]
# The one I like.
name = \"Tenmoku Ember\"
";

/// The app, its settings file and the sender that reloads it.
struct Host {
    _dir: TempDir,
    user: std::path::PathBuf,
    sender: BatchSender,
}

/// Chooses the themes `theme` names, and its mode, as fastedit's host
/// does.
fn select(theme: &ThemeSettings, cx: &mut App) {
    cx.set_theme_mode(match theme.mode {
        ThemeModeSetting::Dark => ThemeMode::Dark,
        ThemeModeSetting::Light => ThemeMode::Light,
        ThemeModeSetting::System => ThemeMode::System,
    });
    let (mut dark, mut light) = (theme.dark.clone(), theme.light.clone());
    if let Some(found) = theme.name.as_deref().and_then(find) {
        let slot = match found.appearance {
            Appearance::Dark => &mut dark,
            Appearance::Light => &mut light,
        };
        slot.get_or_insert_with(|| builtin_theme_id(found.id));
    }
    select_themes(cx, dark.as_deref(), light.as_deref());
}

/// kiln's config service over `user`'s text, with the Acme extension
/// installed and the bundled themes, chosen as the settings say, now and
/// on each reload.
fn host(user: &str, cx: &mut TestAppContext) -> Host {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config");
    fs::create_dir_all(&config).unwrap();
    let path = config.join(fe_config::paths::SETTINGS_FILE);
    fs::write(&path, user).unwrap();
    let acme = dir.path().join("extensions/acme");
    fs::create_dir_all(acme.join("themes")).unwrap();
    fs::write(acme.join(MANIFEST_FILE), MANIFEST).unwrap();
    fs::write(acme.join("themes/midnight.toml"), MIDNIGHT).unwrap();
    fs::write(acme.join("themes/noon.toml"), NOON).unwrap();

    let store = SettingsStore::load(Some(path.clone()), None);
    let (sender, batches) = batch_channel();
    let api = ApiVersion { major: 1, minor: 0 };
    let kiln = Config::new("dev.fastedit.test", "fastedit")
        .settings_store(store.into_inner())
        .theme_mode(ThemeMode::Dark)
        .theme_extensions(EXTENSIONS)
        .extensions(dir.path().join("extensions"), "fastedit", api)
        .watch_with(batches);
    cx.update(|cx| {
        assert_eq!(kiln::init(cx, kiln), []);
        let files: Vec<_> = (bundled_themes().iter())
            .map(|theme| (theme.id, theme.file()))
            .collect();
        let files: Vec<_> = files.iter().map(|(id, file)| (*id, file)).collect();
        install_builtin_themes(cx, &files);
        let theme = cx
            .global::<GlobalSettings<Settings>>()
            .settings()
            .theme
            .clone();
        select(&theme, cx);
        cx.observe_global::<GlobalSettings<Settings>>(|cx| {
            let theme = cx
                .global::<GlobalSettings<Settings>>()
                .settings()
                .theme
                .clone();
            select(&theme, cx);
        })
        .detach();
    });
    cx.run_until_parked();
    Host {
        _dir: dir,
        user: path,
        sender,
    }
}

/// A workbench whose config directory is `host`'s, focused.
fn workbench<'a>(
    host: &Host,
    cx: &'a mut TestAppContext,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let root = host._dir.path().join("project");
    fs::create_dir_all(&root).unwrap();
    let (workbench, cx) = open_root(cx, &root);
    let config = host.user.parent().map(Path::to_owned);
    cx.update(|window, cx| {
        cx.set_global(ConfigDirOverride(config));
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(cx);
    (workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn dispatch(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn picker(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Entity<Picker<ThemePicker>> {
    workbench.read_with(cx, |workbench, _| workbench.theme_picker().clone())
}

fn open(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |workbench, _| workbench.theme_picker_open())
}

type Labels = Vec<(String, Appearance, bool)>;

fn labels(picker: &Entity<Picker<ThemePicker>>, cx: &mut VisualTestContext) -> Labels {
    picker.read_with(cx, |picker, _| {
        (picker.delegate.labels().into_iter())
            .map(|(title, appearance, current)| (title.to_string(), appearance, current))
            .collect()
    })
}

/// Picks the row titled `title` with the keys: typed, then enter.
fn pick(title: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(title);
    draw(cx);
    cx.simulate_keystrokes("enter");
    draw(cx);
}

/// Has kiln's service reload the user's settings, as its watcher would.
fn reload(host: &Host, cx: &mut VisualTestContext) {
    host.sender.send(vec![host.user.clone()]);
    draw(cx);
}

/// The editor palette of the active theme, as the host makes it.
fn active_palette(cx: &mut VisualTestContext) -> EditorPalette {
    cx.update(|_, cx| EditorPalette::for_theme(cx.theme().data()).0)
}

/// The editor palette the bundled theme `title` gives.
fn palette_of(title: &str) -> EditorPalette {
    let data = ThemeData::from_file(&find(title).expect("bundled").file());
    EditorPalette::for_theme(&data).0
}

fn read(host: &Host) -> String {
    fs::read_to_string(&host.user).unwrap()
}

/// The `[theme]` settings the user's file holds.
fn theme_settings(host: &Host) -> ThemeSettings {
    let store = SettingsStore::load(Some(host.user.clone()), None);
    store.settings().theme.clone()
}

#[gpui::test]
fn choosing_a_bundled_theme_applies_and_keeps_it(cx: &mut TestAppContext) {
    let host = host(USER, cx);
    let (workbench, cx) = workbench(&host, cx);
    let ember = palette_of("Tenmoku Ember");
    assert_eq!(active_palette(cx), ember);
    let picker = picker(&workbench, cx);

    dispatch("theme.select", cx);
    assert!(open(&workbench, cx));
    assert!(cx.update(|window, cx| picker.focus_handle(cx).is_focused(window)));
    let row = |title: &str, appearance, current| (title.to_owned(), appearance, current);
    assert_eq!(
        labels(&picker, cx),
        [
            row("Dark Modern", Appearance::Dark, false),
            row("Dark 2026", Appearance::Dark, false),
            row("Light 2026", Appearance::Light, false),
            row("Catppuccin Latte", Appearance::Light, false),
            row("Catppuccin Frappe", Appearance::Dark, false),
            row("Catppuccin Macchiato", Appearance::Dark, false),
            row("Catppuccin Mocha", Appearance::Dark, false),
            row("Tenmoku Dark", Appearance::Dark, false),
            row("Tenmoku Light", Appearance::Light, false),
            row("Tenmoku Ember", Appearance::Dark, true),
            row("Acme Midnight", Appearance::Dark, false),
            row("Acme Noon", Appearance::Light, false),
        ]
    );

    pick("Dark Modern", cx);
    assert!(!open(&workbench, cx));
    assert!(cx.update(|window, cx| workbench.focus_handle(cx).is_focused(window)));
    assert_eq!(
        read(&host),
        USER.replace("\"Tenmoku Ember\"", "\"Dark Modern\"")
    );
    reload(&host, cx);
    let dark_modern = palette_of("Dark Modern");
    assert_ne!(dark_modern, ember);
    assert_eq!(active_palette(cx), dark_modern);
    assert_eq!(
        cx.update(|_, cx| cx.theme().name.clone()),
        "VS Code Dark Modern"
    );

    // Marked current when it opens again.
    dispatch("theme.select", cx);
    let current: Vec<_> = (labels(&picker, cx).into_iter())
        .filter(|(_, _, current)| *current)
        .map(|(title, _, _)| title)
        .collect();
    assert_eq!(current, ["Dark Modern"]);
    cx.simulate_keystrokes("escape");
    draw(cx);

    // A light one turns the mode light too, and the dark slot keeps Dark
    // Modern.
    dispatch("theme.select", cx);
    pick("Tenmoku Light", cx);
    let theme = theme_settings(&host);
    assert_eq!(theme.name.as_deref(), Some("Tenmoku Light"));
    assert_eq!(theme.mode, ThemeModeSetting::Light);
    assert_eq!(theme.dark.as_deref(), Some("builtin/vscode-dark-modern"));
    let text = read(&host);
    assert!(text.starts_with("# My settings.\n[editor]\n# Wider.\ntab_size = 8\n"));
    reload(&host, cx);
    assert_eq!(active_palette(cx), palette_of("Tenmoku Light"));
    cx.update(|_, cx| cx.set_theme_mode(ThemeMode::Dark));
    assert_eq!(active_palette(cx), dark_modern);
}

#[gpui::test]
fn choosing_an_installed_theme_sets_its_slot(cx: &mut TestAppContext) {
    let host = host(USER, cx);
    let (workbench, cx) = workbench(&host, cx);
    let picker = picker(&workbench, cx);

    // The filter narrows the rows by title.
    dispatch("theme.select", cx);
    cx.simulate_input("acme");
    draw(cx);
    let titles: Vec<_> = (labels(&picker, cx).into_iter())
        .map(|(title, _, _)| title)
        .collect();
    assert_eq!(titles, ["Acme Midnight", "Acme Noon"]);
    cx.simulate_input(" mid");
    draw(cx);
    let titles: Vec<_> = (labels(&picker, cx).into_iter())
        .map(|(title, _, _)| title)
        .collect();
    assert_eq!(titles, ["Acme Midnight"]);

    // Escape closes it, and changes nothing.
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(!open(&workbench, cx));
    assert!(cx.update(|window, cx| workbench.focus_handle(cx).is_focused(window)));
    assert_eq!(read(&host), USER);
    reload(&host, cx);
    assert_eq!(active_palette(cx), palette_of("Tenmoku Ember"));

    // An installed dark theme is theme.dark, by ID; the dark name that
    // would lose to it goes.
    dispatch("theme.select", cx);
    pick("Acme Midnight", cx);
    assert!(!open(&workbench, cx));
    assert_eq!(
        read(&host),
        "# My settings.\n[editor]\n# Wider.\ntab_size = 8\n\n[theme]\n\
         # The one I like.\ndark = \"acme.night/midnight\"\n"
    );
    reload(&host, cx);
    let background = cx.update(|_, cx| {
        let data = cx.theme().data();
        data.tokens.get("surface.background").unwrap().to_string()
    });
    assert_eq!(background, "#000010");
    assert_eq!(cx.update(|_, cx| cx.theme().name.clone()), "Acme Midnight");
    dispatch("theme.select", cx);
    let current: Vec<_> = (labels(&picker, cx).into_iter())
        .filter(|(_, _, current)| *current)
        .map(|(title, _, _)| title)
        .collect();
    assert_eq!(current, ["Acme Midnight"]);
}

/// The rows marked current.
fn current(picker: &Entity<Picker<ThemePicker>>, cx: &mut VisualTestContext) -> Vec<String> {
    (labels(picker, cx).into_iter())
        .filter(|(_, _, current)| *current)
        .map(|(title, _, _)| title)
        .collect()
}

/// Rewrites the user's settings, reloaded.
fn rewrite(host: &Host, text: &str, cx: &mut VisualTestContext) {
    fs::write(&host.user, text).unwrap();
    reload(host, cx);
}

#[gpui::test]
fn system_mode_turns_to_show_the_pick(cx: &mut TestAppContext) {
    const SYSTEM: &str = "[theme]\nmode = \"system\"\n";
    let host = host(SYSTEM, cx);
    let (workbench, cx) = workbench(&host, cx);
    let picker = picker(&workbench, cx);
    // The test platform's appearance is light.
    assert_eq!(cx.update(|_, cx| cx.theme().appearance), Appearance::Light);
    dispatch("theme.select", cx);
    assert_eq!(current(&picker, cx), ["Tenmoku Light"]);
    cx.simulate_keystrokes("escape");
    draw(cx);

    // A bundled theme of the appearance showing leaves the mode the
    // system's.
    dispatch("theme.select", cx);
    pick("Tenmoku Light", cx);
    let theme = theme_settings(&host);
    assert_eq!(theme.name.as_deref(), Some("Tenmoku Light"));
    assert_eq!(theme.mode, ThemeModeSetting::System);

    // One of the other turns the mode to it, the light slot kept.
    reload(&host, cx);
    dispatch("theme.select", cx);
    pick("Dark Modern", cx);
    let theme = theme_settings(&host);
    assert_eq!(theme.name.as_deref(), Some("Dark Modern"));
    assert_eq!(theme.mode, ThemeModeSetting::Dark);
    assert_eq!(theme.light.as_deref(), Some("builtin/tenmoku-light"));
    reload(&host, cx);
    assert_eq!(active_palette(cx), palette_of("Dark Modern"));

    // With the system dark: an installed dark theme leaves the mode be.
    rewrite(&host, SYSTEM, cx);
    cx.update(|_, cx| kiln::ui::system_appearance_changed(cx, WindowAppearance::Dark));
    assert_eq!(cx.update(|_, cx| cx.theme().appearance), Appearance::Dark);
    dispatch("theme.select", cx);
    assert_eq!(current(&picker, cx), ["Tenmoku Dark"]);
    pick("Acme Midnight", cx);
    let theme = theme_settings(&host);
    assert_eq!(theme.dark.as_deref(), Some("acme.night/midnight"));
    assert_eq!(theme.mode, ThemeModeSetting::System);

    // And an installed light one turns it light.
    rewrite(&host, SYSTEM, cx);
    cx.update(|_, cx| kiln::ui::system_appearance_changed(cx, WindowAppearance::Dark));
    dispatch("theme.select", cx);
    pick("Acme Noon", cx);
    let theme = theme_settings(&host);
    assert_eq!(theme.light.as_deref(), Some("acme.night/noon"));
    assert_eq!(theme.mode, ThemeModeSetting::Light);
    reload(&host, cx);
    assert_eq!(cx.update(|_, cx| cx.theme().name.clone()), "Acme Noon");
}

#[gpui::test]
fn the_current_mark_follows_kilns_fallback(cx: &mut TestAppContext) {
    let host = host("", cx);
    let (workbench, cx) = workbench(&host, cx);
    let picker = picker(&workbench, cx);
    let marked = |text: &str, cx: &mut VisualTestContext| {
        rewrite(&host, text, cx);
        dispatch("theme.select", cx);
        let marked = current(&picker, cx);
        cx.simulate_keystrokes("escape");
        draw(cx);
        marked
    };

    // Nothing chosen: fastedit's own.
    assert_eq!(marked("", cx), ["Tenmoku Dark"]);
    // A light theme in the dark slot, which kiln refuses for it.
    assert_eq!(
        marked("[theme]\ndark = \"acme.night/noon\"\n", cx),
        ["Tenmoku Dark"]
    );
    assert_eq!(active_palette(cx), *EditorPalette::fastedit_dark());
    // A theme not installed.
    assert_eq!(
        marked("[theme]\ndark = \"acme.gone/midnight\"\n", cx),
        ["Tenmoku Dark"]
    );
    // A name nothing bundled has, then one by its short name.
    assert_eq!(marked("[theme]\nname = \"Nope\"\n", cx), ["Tenmoku Dark"]);
    assert_eq!(marked("[theme]\nname = \"ember\"\n", cx), ["Tenmoku Ember"]);
    // A light name leaves the dark slot fastedit's own.
    assert_eq!(
        marked("[theme]\nname = \"Tenmoku Light\"\n", cx),
        ["Tenmoku Dark"]
    );
    // A slot's own setting wins over the name.
    assert_eq!(
        marked(
            "[theme]\nname = \"Dark Modern\"\ndark = \"acme.night/midnight\"\n",
            cx
        ),
        ["Acme Midnight"]
    );
}
