//! The Keyboard Shortcuts tab (`workbench.open_keyboard_shortcuts`,
//! secondary-k secondary-s) and Open Keyboard Shortcuts (JSON) in a
//! headless window: one tab, never in a saved layout; its lines, search,
//! conflict and hidden markers; its following the keymap in use; and its
//! menu's and Delete's writes to the user's keymap. The user's config
//! directory is a temporary one, through the test-only override, and the
//! keymap in use is this platform's defaults with that directory's
//! `keymap.toml` over them, loaded as the app loads it; a test reloads it
//! as the app's watcher would once a write lands.

use std::{cell::OnceCell, fs, path::Path};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_gpui::commands::RunCommand;
use fe_keymap::{
    DEFAULT, DEFAULT_KEYMAP_PATH, KEYMAP_HEADER, KeySequence, KeymapLayer, KeymapStore,
    ShortcutSource,
};
use gpui::{
    Entity, Focusable as _, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext,
    VisualTestContext,
};
use kiln::core::Os;
use kiln_platform::config::{self, BatchSender, ConfigService};
use kiln_shell::PaneItem as _;

use crate::{
    Launch, Workbench,
    open::ConfigDirOverride,
    shortcuts_editor::{
        MenuItem, NOTHING_FOUND, RESET_REFUSED_HINT, ShortcutsEditor, conflict_note,
    },
    workbench::toast::Level,
};

const OPEN: &str = "workbench.open_keyboard_shortcuts";
const OPEN_JSON: &str = "workbench.open_keyboard_shortcuts_json";
const SETTINGS_UI: &str = "workbench.open_settings_ui";

/// A config directory in a temporary directory, and what tells the
/// config service its keymap changed once a workbench starts it.
struct Dirs {
    _temp: tempfile::TempDir,
    config: std::path::PathBuf,
    watcher: OnceCell<BatchSender>,
}

impl Dirs {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let config = fs::canonicalize(temp.path()).unwrap().join("config");
        fs::create_dir_all(&config).unwrap();
        Self {
            _temp: temp,
            config,
            watcher: OnceCell::new(),
        }
    }

    fn keymap(&self) -> std::path::PathBuf {
        self.config.join(fe_keymap::KEYMAP_FILE)
    }

    fn write_keymap(&self, text: &str) {
        fs::write(self.keymap(), text).unwrap();
    }

    fn read_keymap(&self) -> String {
        fs::read_to_string(self.keymap()).unwrap()
    }
}

/// `written`, keys as a keymap file writes them, as this platform reads
/// them: `secondary-k` is `cmd-k` on macOS, `ctrl-k` elsewhere.
fn keys(written: &str) -> String {
    KeySequence::parse_for(written, Os::current())
        .unwrap()
        .to_string()
}

/// kiln's own keys, laid under the app's keymap when it asks for them.
const KILN_KEYMAP: &str = include_str!("../../../kiln/crates/kiln/src/setup/keymap.toml");

/// An extension's keymap, laid over fastedit's by [`Shipped::extension`].
const EXTENSION_KEYMAP_PATH: &str = "extensions/demo/keymap.toml";

/// The default layers a test's keymap store starts with.
#[derive(Clone, Copy)]
struct Shipped {
    /// An extension's keymap over fastedit's: [`EXTENSION_KEYMAP_PATH`].
    extension: Option<&'static str>,
    /// The extension's keymap as kept on the other platforms, if not
    /// `extension`: its last good text there, when the one here has an
    /// error there.
    extension_elsewhere: Option<&'static str>,
    /// Whether every platform's default layers are kept with the store,
    /// as kiln's setup keeps them.
    platforms: bool,
}

impl Shipped {
    /// As the app ships them: kiln's keys and fastedit's, every
    /// platform's kept.
    const APP: Self = Self {
        extension: None,
        extension_elsewhere: None,
        platforms: true,
    };

    /// The default layers as read on `os`: kiln's keys, fastedit's over
    /// them, then the extension's.
    fn layers(self, os: Os) -> Vec<KeymapLayer> {
        let catalog = crate::command_catalog();
        let files = [
            (Path::new("kiln/keymap.toml"), KILN_KEYMAP),
            (Path::new(DEFAULT_KEYMAP_PATH), DEFAULT),
        ];
        let extension = match os == Os::current() {
            true => self.extension,
            false => self.extension_elsewhere.or(self.extension),
        };
        let extension = extension.map(|text| (Path::new(EXTENSION_KEYMAP_PATH), text));
        (files.into_iter().chain(extension))
            .map(|(path, text)| {
                let (layer, diagnostics) = KeymapLayer::parse_for(path, text, Some(&*catalog), os);
                assert_eq!(diagnostics, [], "{path:?} on {os:?}");
                layer.unwrap()
            })
            .collect()
    }
}

/// Starts the config service on a keymap store as the app's: kiln's keys
/// and this platform's defaults over them, then `dirs`' `keymap.toml`.
fn start_config(dirs: &Dirs, shipped: Shipped, cx: &mut VisualTestContext) {
    let (sender, batches) = config::batch_channel();
    assert!(dirs.watcher.set(sender).is_ok());
    let keymap = dirs.keymap();
    cx.update(|_, cx| {
        let catalog = crate::command_catalog();
        let os = Os::current();
        let layers = shipped.layers(os);
        let store = KeymapStore::with_defaults(layers, Vec::new(), Some(keymap), Some(catalog));
        let service = ConfigService::new().keymap(store);
        if shipped.platforms {
            let platforms = (Os::ALL.into_iter())
                .map(|on| (on, shipped.layers(on)))
                .collect();
            let store = service.keymap_store().unwrap();
            store.set_platform_defaults(os, platforms);
        }
        service.start_with(batches, cx).detach();
    });
    draw(cx);
}

/// Tells the config service `dirs`' `keymap.toml` changed, as its watcher
/// would once a write lands: it reloads the keymap and puts it in use.
fn reload(dirs: &Dirs, cx: &mut VisualTestContext) {
    dirs.watcher.get().unwrap().send(vec![dirs.keymap()]);
    draw(cx);
    let diagnostics = cx.update(|_, cx| config::diagnostics(cx).to_vec());
    assert_eq!(diagnostics, [], "the user keymap loads");
}

/// A workbench on no folder whose user keymap is `dirs`', focused.
fn workbench<'a>(
    cx: &'a mut TestAppContext,
    dirs: &Dirs,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    workbench_with(cx, dirs, Shipped::APP)
}

/// [`workbench`], its keymap store's default layers `shipped`.
fn workbench_with<'a>(
    cx: &'a mut TestAppContext,
    dirs: &Dirs,
    shipped: Shipped,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let launch = Launch {
        root: None,
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| {
        cx.set_global(ConfigDirOverride(Some(dirs.config.clone())));
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    start_config(dirs, shipped, cx);
    (workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// Every Keyboard Shortcuts tab, in every pane.
fn shortcut_tabs(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<Entity<ShortcutsEditor>> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.panes(cx).iter())
            .flat_map(|pane| {
                (pane.read(cx).tabs().iter())
                    .filter_map(|tab| tab.item().downcast::<ShortcutsEditor>())
                    .collect::<Vec<_>>()
            })
            .collect()
    })
}

fn open_shortcuts(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Entity<ShortcutsEditor> {
    run(OPEN, cx);
    let [editor] = <[_; 1]>::try_from(shortcut_tabs(workbench, cx)).expect("one tab");
    editor
}

fn search_focused(editor: &Entity<ShortcutsEditor>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| {
        let search = editor.read(cx).search_input().focus_handle(cx);
        search.is_focused(window)
    })
}

/// Types `text` into the focused search field.
fn search(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(text);
    draw(cx);
}

/// Sets the search to `text`, as typing it into an empty field does.
fn set_search(editor: &Entity<ShortcutsEditor>, text: &str, cx: &mut VisualTestContext) {
    let input = editor.read_with(cx, |editor, _| editor.search_input().clone());
    cx.update(|window, cx| {
        input.update(cx, |input, cx| input.clear(cx));
        window.focus(&input.focus_handle(cx), cx);
    });
    search(text, cx);
}

/// A line as the tab shows it: command, keys, when and source.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Shown {
    command: String,
    keys: Option<String>,
    when: Option<String>,
    source: Option<ShortcutSource>,
}

fn lines(editor: &Entity<ShortcutsEditor>, cx: &mut VisualTestContext) -> Vec<Shown> {
    editor.read_with(cx, |editor, _| {
        (0..editor.lines().len())
            .map(|ix| {
                let (row, binding) = editor.line(ix).unwrap();
                Shown {
                    command: row.command.clone(),
                    keys: binding.map(|binding| binding.keys.to_string()),
                    when: binding.and_then(|binding| binding.when.clone()),
                    source: binding.map(|binding| editor.source(binding)),
                }
            })
            .collect()
    })
}

/// The lines of `command`.
fn lines_of(
    editor: &Entity<ShortcutsEditor>,
    command: &str,
    cx: &mut VisualTestContext,
) -> Vec<Shown> {
    (lines(editor, cx).into_iter())
        .filter(|line| line.command == command)
        .collect()
}

/// The index of the first line of `command` bound to `keys`, or of its
/// line without a binding with `keys` `None`.
fn line_ix(
    editor: &Entity<ShortcutsEditor>,
    command: &str,
    keys: Option<&str>,
    cx: &mut VisualTestContext,
) -> usize {
    (lines(editor, cx).iter())
        .position(|line| line.command == command && line.keys.as_deref() == keys)
        .unwrap_or_else(|| panic!("no line of {command} with {keys:?}"))
}

fn drawn(selector: &str, cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(selector.to_owned().leak()).is_some()
}

/// Right-clicks `selector`'s element, opening its line's menu.
fn right_click(selector: &str, cx: &mut VisualTestContext) {
    let at = cx
        .debug_bounds(selector.to_owned().leak())
        .unwrap_or_else(|| panic!("{selector} isn't drawn"))
        .center();
    cx.simulate_event(MouseDownEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Right,
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        position: at,
        modifiers: Modifiers::none(),
        button: MouseButton::Right,
        click_count: 1,
    });
    draw(cx);
}

/// Clicks the open menu's `item`.
fn click_menu_item(item: MenuItem, cx: &mut VisualTestContext) {
    let selector = format!("menu-item:{}", item.title());
    let at = cx
        .debug_bounds(selector.clone().leak())
        .unwrap_or_else(|| panic!("{selector} isn't drawn"))
        .center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
}

fn menu_items(
    editor: &Entity<ShortcutsEditor>,
    cx: &mut VisualTestContext,
) -> Option<Vec<MenuItem>> {
    editor.read_with(cx, |editor, _| editor.menu_items().map(<[_]>::to_vec))
}

/// Each toast shown: its level, message and detail.
fn toasts(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(Level, String, Option<String>)> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| {
                let toast = &entry.toast;
                (toast.level, toast.message.clone(), toast.detail.clone())
            })
            .collect()
    })
}

#[gpui::test]
fn command_opens_one_tab(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    let (title, no_path, serialized) = editor.read_with(cx, |editor, cx| {
        (
            editor.tab_title(cx),
            editor.path(cx).is_none(),
            editor.serialize(cx),
        )
    });
    assert_eq!(title, "Keyboard Shortcuts");
    assert!(no_path, "the tab is no file");
    assert_eq!(serialized, None, "it isn't kept in saved layouts");
    let active = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let tab = &pane.tabs()[pane.active().unwrap()];
        tab.item().downcast::<ShortcutsEditor>()
    });
    assert_eq!(active, Some(editor.clone()));
    assert!(search_focused(&editor, cx), "the search field has focus");
    assert!(drawn("shortcuts-search", cx));
    assert!(drawn("shortcuts-header", cx));

    // Away to another tab and back with the command: the same tab, shown
    // and focused again.
    run(SETTINGS_UI, cx);
    assert!(!search_focused(&editor, cx));
    run(OPEN, cx);
    assert_eq!(shortcut_tabs(&workbench, cx), std::slice::from_ref(&editor));
    assert!(search_focused(&editor, cx));

    // Its chord runs it too, from the tab's own search: still one tab.
    let chord = keys("secondary-k secondary-s");
    cx.simulate_keystrokes(&chord);
    draw(cx);
    assert_eq!(shortcut_tabs(&workbench, cx), std::slice::from_ref(&editor));
    run(SETTINGS_UI, cx);
    cx.simulate_keystrokes(&chord);
    draw(cx);
    assert_eq!(shortcut_tabs(&workbench, cx), std::slice::from_ref(&editor));
    assert!(search_focused(&editor, cx));

    // It closes like any tab.
    run("workbench.close_tab", cx);
    assert!(shortcut_tabs(&workbench, cx).is_empty());
}

#[gpui::test]
fn json_command_opens_the_file(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    assert!(!dirs.keymap().exists());
    run(OPEN_JSON, cx);
    assert_eq!(dirs.read_keymap(), KEYMAP_HEADER, "created from the header");
    let active = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |workbench, cx| {
            let pane = workbench.pane().read(cx);
            pane.tabs()[pane.active().unwrap()].path.clone()
        })
    };
    assert_eq!(active(cx), Some(dirs.keymap()));

    // A file already there is opened as it is.
    run("workbench.close_tab", cx);
    let text = "# mine\n[[bindings]]\nkeys = \"secondary-alt-9\"\ncommand = \"workbench.open_keyboard_shortcuts\"\n";
    dirs.write_keymap(text);
    run(OPEN_JSON, cx);
    assert_eq!(dirs.read_keymap(), text);
    assert_eq!(active(cx), Some(dirs.keymap()));
}

#[gpui::test]
fn lists_commands_with_bindings(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    let all = lines(&editor, cx);
    assert!(all.len() > 100, "every command: {}", all.len());

    // Bound: a line per binding, with its keys, when and source.
    assert_eq!(
        lines_of(&editor, OPEN, cx),
        [Shown {
            command: OPEN.into(),
            keys: Some(keys("secondary-k secondary-s")),
            when: Some("workbench".into()),
            source: Some(ShortcutSource::Default),
        }]
    );
    // Unbound: a line of its own.
    assert_eq!(
        lines_of(&editor, OPEN_JSON, cx),
        [Shown {
            command: OPEN_JSON.into(),
            keys: None,
            when: None,
            source: None,
        }]
    );
    // Titled as the palette titles them, in title order.
    let titles: Vec<String> = editor.read_with(cx, |editor, _| {
        (0..editor.lines().len())
            .map(|ix| editor.line(ix).unwrap().0.title.clone())
            .collect()
    });
    let ix = line_ix(&editor, OPEN, Some(&keys("secondary-k secondary-s")), cx);
    assert_eq!(titles[ix], "Open Keyboard Shortcuts");
    let mut sorted = titles.clone();
    sorted.sort_by_key(|title| title.to_lowercase());
    assert_eq!(titles, sorted);

    // The first lines are drawn, a row each, with their selectors; only
    // those in view are built.
    let first = all[0].command.clone();
    assert!(drawn(&format!("shortcuts-row-{first}"), cx));
    let built = editor.update(cx, |editor, _| editor.take_built());
    assert!(!built.is_empty());
    assert!(built.iter().all(|range| range.len() < 100), "{built:?}");

    // A search for it draws its row.
    search("open keyboard shortcuts", cx);
    assert!(drawn(&format!("shortcuts-row-{OPEN}"), cx));
    assert!(drawn(&format!("shortcuts-row-{OPEN_JSON}"), cx));
}

#[gpui::test]
fn search_by_keystroke_filters_rows(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    let chord = keys("secondary-k secondary-s");
    let first = keys("secondary-k");

    // Keys as the labels print them, with `+`: bindings that start with
    // them, and none else.
    let modifier = match Os::current() {
        Os::MacOs => "cmd",
        _ => "ctrl",
    };
    search(&format!("{modifier}+k"), cx);
    let found = lines(&editor, cx);
    assert!(found.len() > 2, "the secondary-k chords: {found:?}");
    for line in &found {
        let keys = line.keys.as_deref().unwrap_or_default();
        assert!(
            keys == first || keys.starts_with(&format!("{first} ")),
            "{line:?}"
        );
    }
    assert!(found.iter().any(|line| line.command == OPEN));

    // Quoted, exactly those keys.
    set_search(&editor, &format!("\"{chord}\""), cx);
    assert_eq!(
        lines(&editor, cx),
        [Shown {
            command: OPEN.into(),
            keys: Some(chord.clone()),
            when: Some("workbench".into()),
            source: Some(ShortcutSource::Default),
        }]
    );
    assert!(drawn(&format!("shortcuts-row-{OPEN}"), cx));

    // Nothing found says so; Escape clears the search.
    set_search(&editor, "no such command anywhere", cx);
    assert_eq!(lines(&editor, cx), []);
    assert!(drawn("shortcuts-empty", cx));
    let _ = NOTHING_FOUND;
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert!(lines(&editor, cx).len() > 100);
}

#[gpui::test]
fn reloads_when_keymap_changes(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    search("keyboard shortcuts json", cx);
    assert_eq!(lines_of(&editor, OPEN_JSON, cx)[0].keys, None);

    // The user binds it, and the keymap reloads: the tab follows, its
    // search kept.
    dirs.write_keymap(&format!(
        "[[bindings]]\nkeys = \"secondary-alt-9\"\ncommand = \"{OPEN_JSON}\"\ncontext = \"workbench\"\n"
    ));
    reload(&dirs, cx);
    assert_eq!(
        lines(&editor, cx),
        [Shown {
            command: OPEN_JSON.into(),
            keys: Some(keys("secondary-alt-9")),
            when: Some("workbench".into()),
            source: Some(ShortcutSource::User),
        }]
    );

    // `@source:` keeps the bindings from the user's file, or from the
    // keymaps the app ships, fastedit's not being the first.
    set_search(&editor, "@source:user", cx);
    let user: Vec<String> = (lines(&editor, cx).into_iter())
        .map(|line| line.command)
        .collect();
    assert_eq!(user, [OPEN_JSON]);
    set_search(&editor, "@source:default open keyboard", cx);
    let defaults = lines(&editor, cx);
    assert!(!defaults.is_empty());
    assert!(
        (defaults.iter()).all(|line| line.source == Some(ShortcutSource::Default)),
        "{defaults:?}"
    );
    assert!(defaults.iter().any(|line| line.command == OPEN));

    // A tab in the background follows once shown again.
    run(SETTINGS_UI, cx);
    dirs.write_keymap("");
    reload(&dirs, cx);
    run(OPEN, cx);
    set_search(&editor, "keyboard shortcuts json", cx);
    assert_eq!(lines_of(&editor, OPEN_JSON, cx)[0].keys, None);
}

#[gpui::test]
fn conflict_marker_names_the_other_command(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    // The same keys in the workbench and in an editor: in an editor both
    // apply, and the editor's wins.
    dirs.write_keymap(&format!(
        "[[bindings]]\nkeys = \"secondary-alt-9\"\ncommand = \"{SETTINGS_UI}\"\ncontext = \"workbench\"\n\n\
         [[bindings]]\nkeys = \"secondary-alt-9\"\ncommand = \"{OPEN_JSON}\"\ncontext = \"editor\"\n"
    ));
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    let chord = keys("secondary-alt-9");
    set_search(&editor, &format!("\"{chord}\""), cx);
    let notes: Vec<(String, Option<String>)> = editor.read_with(cx, |editor, _| {
        (0..editor.lines().len())
            .map(|ix| {
                let (row, binding) = editor.line(ix).unwrap();
                (row.command.clone(), conflict_note(binding.unwrap()))
            })
            .collect()
    });
    assert_eq!(notes.len(), 2, "{notes:?}");
    for (command, note) in &notes {
        let other = if command == SETTINGS_UI {
            OPEN_JSON
        } else {
            SETTINGS_UI
        };
        assert_eq!(note.as_deref(), Some(&*format!("Also bound to {other}")));
        assert!(
            drawn(&format!("shortcuts-conflict-{command}-0"), cx),
            "{command}"
        );
    }

    // A binding with no clash has no marker.
    set_search(
        &editor,
        &format!("\"{}\"", keys("secondary-k secondary-s")),
        cx,
    );
    let note = editor.read_with(cx, |editor, _| {
        conflict_note(editor.line(0).unwrap().1.unwrap())
    });
    assert_eq!(note, None);
    assert!(!drawn(&format!("shortcuts-conflict-{OPEN}-0"), cx));
}

#[gpui::test]
fn hidden_binding_is_dimmed(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    // The default's keys and context, to another command: it replaces it.
    dirs.write_keymap(&format!(
        "# Settings on the shortcuts chord.\n[[bindings]]\nkeys = \"secondary-k secondary-s\"\n\
         command = \"{SETTINGS_UI}\"\ncontext = \"workbench\"\n"
    ));
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    let chord = keys("secondary-k secondary-s");
    set_search(&editor, &format!("\"{chord}\""), cx);
    let shown = lines(&editor, cx);
    assert_eq!(shown.len(), 2, "{shown:?}");
    let ix = line_ix(&editor, OPEN, Some(&chord), cx);
    let (hidden, conflict) = editor.read_with(cx, |editor, _| {
        let binding = editor.line(ix).unwrap().1.unwrap();
        (editor.hidden_note(binding), conflict_note(binding))
    });
    let location = format!("{}:2:1", dirs.keymap().display());
    assert_eq!(hidden, Some(format!("Hidden by {location}")));
    assert_eq!(conflict, None, "it never runs, so it clashes with nothing");
    assert!(drawn(&format!("shortcuts-hidden-{OPEN}-0"), cx));

    // The winner isn't hidden; it's marked as taking the other's keys.
    let winner = line_ix(&editor, SETTINGS_UI, Some(&chord), cx);
    let (hidden, conflict) = editor.read_with(cx, |editor, _| {
        let binding = editor.line(winner).unwrap().1.unwrap();
        (editor.hidden_note(binding), conflict_note(binding))
    });
    assert_eq!(hidden, None);
    assert_eq!(conflict, Some(format!("Also bound to {OPEN}")));
    assert!(!drawn(&format!("shortcuts-hidden-{SETTINGS_UI}-0"), cx));

    // A hidden binding can't be removed on its own: its menu leaves it out.
    right_click(&format!("shortcuts-row-{OPEN}"), cx);
    let items = menu_items(&editor, cx).expect("the menu is open");
    assert!(!items.contains(&MenuItem::RemoveKeybinding), "{items:?}");
}

#[gpui::test]
fn remove_default_writes_unbind(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    let chord = keys("secondary-k secondary-s");
    search("open keyboard shortcuts", cx);
    right_click(&format!("shortcuts-row-{OPEN}"), cx);
    assert_eq!(
        menu_items(&editor, cx),
        Some(vec![
            MenuItem::CopyCommandId,
            MenuItem::AddKeybinding,
            MenuItem::RemoveKeybinding,
            MenuItem::ShowSameKeybindings,
        ]),
        "a default with no user entries can't be reset"
    );
    assert!(drawn("shortcuts-menu", cx));
    click_menu_item(MenuItem::RemoveKeybinding, cx);
    assert_eq!(menu_items(&editor, cx), None, "the menu closes");

    // The file is made, with an entry unbinding the default.
    let text = dirs.read_keymap();
    assert!(text.starts_with(KEYMAP_HEADER), "{text}");
    let path = dirs.keymap();
    let (layer, diagnostics) = KeymapLayer::parse_for(&path, &text, None, Os::current());
    assert_eq!(diagnostics, []);
    let entries = layer.unwrap().entries;
    assert_eq!(entries.len(), 1, "{text}");
    assert_eq!(entries[0].keys.to_string(), chord);
    assert_eq!(entries[0].command, None);
    assert_eq!(
        entries[0].context.as_ref().map(ToString::to_string),
        Some("workbench".into())
    );

    // Reloaded, the command has no binding, and can be reset.
    reload(&dirs, cx);
    let open = lines_of(&editor, OPEN, cx);
    assert_eq!(open[0].keys, None, "{open:?}");
    right_click(&format!("shortcuts-row-{OPEN}"), cx);
    assert_eq!(
        menu_items(&editor, cx),
        Some(vec![
            MenuItem::CopyCommandId,
            MenuItem::AddKeybinding,
            MenuItem::ResetKeybinding
        ])
    );
}

#[gpui::test]
fn remove_user_entry(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let text = format!(
        "# Mine.\n\n# The JSON on a key.\n[[bindings]]\nkeys = \"secondary-alt-9\"\n\
         command = \"{OPEN_JSON}\"\ncontext = \"workbench\"\n"
    );
    dirs.write_keymap(&text);
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    search("keyboard shortcuts json", cx);
    let ix = line_ix(&editor, OPEN_JSON, Some(&keys("secondary-alt-9")), cx);

    // Selected, Delete removes it, from the list.
    let at = cx
        .debug_bounds(format!("shortcuts-row-{OPEN_JSON}").leak())
        .unwrap()
        .center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.selected()),
        Some(ix)
    );
    let list_focused = cx.update(|window, cx| editor.read(cx).list_focus().is_focused(window));
    assert!(list_focused);
    cx.simulate_keystrokes("delete");
    draw(cx);

    // The entry goes, with the comment over it; the rest is kept.
    assert_eq!(dirs.read_keymap().trim_end(), "# Mine.");
    reload(&dirs, cx);
    assert_eq!(lines_of(&editor, OPEN_JSON, cx)[0].keys, None);
}

#[gpui::test]
fn reset_keybinding(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    // The default unbound and the command on a key of the user's.
    let chord = keys("secondary-k secondary-s");
    dirs.write_keymap(&format!(
        "# Mine.\n\n[[bindings]]\nkeys = \"{chord}\"\ncommand = \"\"\ncontext = \"workbench\"\n\n\
         [[bindings]]\nkeys = \"secondary-alt-9\"\ncommand = \"{OPEN}\"\ncontext = \"workbench\"\n"
    ));
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    search("open keyboard shortcuts", cx);
    let user = keys("secondary-alt-9");
    assert_eq!(
        (lines_of(&editor, OPEN, cx).iter())
            .map(|line| line.keys.clone())
            .collect::<Vec<_>>(),
        [Some(user)]
    );
    right_click(&format!("shortcuts-row-{OPEN}"), cx);
    assert_eq!(
        menu_items(&editor, cx),
        Some(vec![
            MenuItem::CopyCommandId,
            MenuItem::AddKeybinding,
            MenuItem::RemoveKeybinding,
            MenuItem::ResetKeybinding,
            MenuItem::ShowSameKeybindings,
        ])
    );
    click_menu_item(MenuItem::ResetKeybinding, cx);

    // Both entries go; reloaded, the default is back.
    assert_eq!(dirs.read_keymap().trim_end(), "# Mine.");
    reload(&dirs, cx);
    assert_eq!(
        lines_of(&editor, OPEN, cx),
        [Shown {
            command: OPEN.into(),
            keys: Some(chord),
            when: Some("workbench".into()),
            source: Some(ShortcutSource::Default),
        }]
    );
}

#[gpui::test]
fn copy_command_id(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    search("keyboard shortcuts json", cx);
    right_click(&format!("shortcuts-row-{OPEN_JSON}"), cx);
    assert_eq!(
        menu_items(&editor, cx),
        Some(vec![MenuItem::CopyCommandId, MenuItem::AddKeybinding]),
        "an unbound command's line has nothing to remove or show"
    );
    click_menu_item(MenuItem::CopyCommandId, cx);
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some(OPEN_JSON));
    assert_eq!(menu_items(&editor, cx), None);

    // Escape closes a menu, and clicking elsewhere does too.
    right_click(&format!("shortcuts-row-{OPEN_JSON}"), cx);
    assert!(menu_items(&editor, cx).is_some());
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(menu_items(&editor, cx), None);
    right_click(&format!("shortcuts-row-{OPEN_JSON}"), cx);
    let at = cx.debug_bounds("shortcuts-search").unwrap().center();
    cx.simulate_click(at, Modifiers::none());
    draw(cx);
    assert_eq!(menu_items(&editor, cx), None);
}

#[gpui::test]
fn show_same_keybindings_searches_the_keys(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    search("open keyboard shortcuts", cx);
    right_click(&format!("shortcuts-row-{OPEN}"), cx);
    click_menu_item(MenuItem::ShowSameKeybindings, cx);
    let chord = keys("secondary-k secondary-s");
    let text = editor.read_with(cx, |editor, cx| {
        editor.search_input().read(cx).text().to_owned()
    });
    assert_eq!(text, format!("\"{chord}\""));
    assert!(search_focused(&editor, cx));
    let found = lines(&editor, cx);
    assert!(!found.is_empty());
    assert!(
        found
            .iter()
            .all(|line| line.keys.as_deref() == Some(&*chord)),
        "{found:?}"
    );
}

#[gpui::test]
fn a_failed_write_shows_a_toast(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    // The keymap's path is a directory: it can't be read or written.
    fs::create_dir_all(dirs.keymap()).unwrap();
    let editor = open_shortcuts(&workbench, cx);
    search("open keyboard shortcuts", cx);
    right_click(&format!("shortcuts-row-{OPEN}"), cx);
    click_menu_item(MenuItem::RemoveKeybinding, cx);
    let shown = toasts(&workbench, cx);
    assert_eq!(shown.len(), 1, "{shown:?}");
    assert_eq!(shown[0].0, Level::Error);
    assert_eq!(shown[0].1, "Can't change the keybinding");
    assert!(shown[0].2.is_some());
    assert_eq!(
        lines_of(&editor, OPEN, cx)[0].keys,
        Some(keys("secondary-k secondary-s"))
    );
}

#[gpui::test]
fn conflicts_across_contexts(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    // The same keys in the workbench and in the terminal: with the
    // terminal focused both apply, and the terminal's wins. In the editor
    // and in the project panel they never both apply: no clash.
    dirs.write_keymap(&format!(
        "[[bindings]]\nkeys = \"secondary-alt-9\"\ncommand = \"{SETTINGS_UI}\"\ncontext = \"workbench\"\n\n\
         [[bindings]]\nkeys = \"secondary-alt-9\"\ncommand = \"{OPEN_JSON}\"\ncontext = \"terminal\"\n\n\
         [[bindings]]\nkeys = \"secondary-alt-8\"\ncommand = \"{SETTINGS_UI}\"\ncontext = \"editor\"\n\n\
         [[bindings]]\nkeys = \"secondary-alt-8\"\ncommand = \"{OPEN_JSON}\"\ncontext = \"project_panel\"\n\n\
         [[bindings]]\nkeys = \"secondary-alt-7\"\ncommand = \"{SETTINGS_UI}\"\ncontext = \"editor && vim.mode == normal\"\n\n\
         [[bindings]]\nkeys = \"secondary-alt-7\"\ncommand = \"{OPEN_JSON}\"\ncontext = \"editor && !vim.enabled\"\n"
    ));
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    let notes = |keys_written: &str, cx: &mut VisualTestContext| {
        set_search(&editor, &format!("\"{}\"", keys(keys_written)), cx);
        editor.read_with(cx, |editor, _| {
            (0..editor.lines().len())
                .map(|ix| {
                    let (row, binding) = editor.line(ix).unwrap();
                    (row.command.clone(), conflict_note(binding.unwrap()))
                })
                .collect::<Vec<_>>()
        })
    };
    let mut terminal = notes("secondary-alt-9", cx);
    terminal.sort();
    assert_eq!(
        terminal,
        [
            (
                OPEN_JSON.to_owned(),
                Some(format!("Also bound to {SETTINGS_UI}"))
            ),
            (
                SETTINGS_UI.to_owned(),
                Some(format!("Also bound to {OPEN_JSON}"))
            ),
        ]
    );
    for (command, _) in &terminal {
        assert!(
            drawn(&format!("shortcuts-conflict-{command}-0"), cx),
            "{command}"
        );
    }
    let panels = notes("secondary-alt-8", cx);
    assert_eq!(panels.len(), 2);
    assert!(panels.iter().all(|(_, note)| note.is_none()), "{panels:?}");
    let vim = notes("secondary-alt-7", cx);
    assert_eq!(vim.len(), 2);
    assert!(vim.iter().all(|(_, note)| note.is_none()), "{vim:?}");
}

#[gpui::test]
fn reset_is_offered_for_entries_not_in_effect(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let other = match Os::current() {
        Os::Windows => "macos",
        _ => "windows",
    };
    // The JSON command bound on another platform only; Open Settings (UI)
    // bound, then unbound by a later entry.
    let text = format!(
        "[[bindings]]\nkeys = \"secondary-alt-9\"\ncommand = \"{OPEN_JSON}\"\nplatform = \"{other}\"\n\n\
         [[bindings]]\nkeys = \"secondary-alt-8\"\ncommand = \"{SETTINGS_UI}\"\n\n\
         [[bindings]]\nkeys = \"secondary-alt-8\"\ncommand = \"\"\n"
    );
    dirs.write_keymap(&text);
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    search("keyboard shortcuts json", cx);
    assert_eq!(
        lines_of(&editor, OPEN_JSON, cx)[0].keys,
        None,
        "not bound here"
    );
    right_click(&format!("shortcuts-row-{OPEN_JSON}"), cx);
    assert_eq!(
        menu_items(&editor, cx),
        Some(vec![
            MenuItem::CopyCommandId,
            MenuItem::AddKeybinding,
            MenuItem::ResetKeybinding
        ])
    );
    click_menu_item(MenuItem::ResetKeybinding, cx);
    let after = dirs.read_keymap();
    assert!(!after.contains(OPEN_JSON), "{after}");
    assert!(after.contains(SETTINGS_UI), "{after}");
    reload(&dirs, cx);

    set_search(&editor, "open settings ui", cx);
    let settings = lines_of(&editor, SETTINGS_UI, cx);
    assert!(
        !settings
            .iter()
            .any(|line| line.source == Some(ShortcutSource::User)),
        "{settings:?}"
    );
    right_click(&format!("shortcuts-row-{SETTINGS_UI}"), cx);
    let items = menu_items(&editor, cx).unwrap();
    assert!(items.contains(&MenuItem::ResetKeybinding), "{items:?}");
    click_menu_item(MenuItem::ResetKeybinding, cx);
    let after = dirs.read_keymap();
    assert!(!after.contains(SETTINGS_UI), "{after}");

    // With nothing of its own left, a command has no reset.
    reload(&dirs, cx);
    right_click(&format!("shortcuts-row-{SETTINGS_UI}"), cx);
    let items = menu_items(&editor, cx).unwrap();
    assert!(!items.contains(&MenuItem::ResetKeybinding), "{items:?}");
}

#[gpui::test]
fn remove_and_reset_a_kiln_default(cx: &mut TestAppContext) {
    const PALETTE: &str = "command_palette.toggle";
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    let chord = keys("secondary-shift-p");
    set_search(&editor, &format!("\"{chord}\""), cx);
    // kiln's, anywhere, then fastedit's, in the workbench.
    let palette = |cx: &mut VisualTestContext| {
        (lines_of(&editor, PALETTE, cx).into_iter())
            .map(|line| line.when)
            .collect::<Vec<_>>()
    };
    assert_eq!(palette(cx), [None, Some("workbench".to_owned())]);
    right_click(&format!("shortcuts-row-{PALETTE}"), cx);
    click_menu_item(MenuItem::RemoveKeybinding, cx);
    assert_eq!(toasts(&workbench, cx), [], "removed");

    // An entry unbinding kiln's, with no context, as kiln's has none.
    let path = dirs.keymap();
    let text = dirs.read_keymap();
    let (layer, diagnostics) = KeymapLayer::parse_for(&path, &text, None, Os::current());
    assert_eq!(diagnostics, []);
    let entries = layer.unwrap().entries;
    assert_eq!(entries.len(), 1, "{text}");
    assert_eq!(entries[0].keys.to_string(), chord);
    assert_eq!(
        (entries[0].command.as_ref(), entries[0].context.as_ref()),
        (None, None)
    );
    reload(&dirs, cx);
    assert_eq!(palette(cx), [Some("workbench".to_owned())]);

    // Reset deletes it, and kiln's is back.
    right_click(&format!("shortcuts-row-{PALETTE}"), cx);
    let items = menu_items(&editor, cx).unwrap();
    assert!(items.contains(&MenuItem::ResetKeybinding), "{items:?}");
    click_menu_item(MenuItem::ResetKeybinding, cx);
    assert!(
        !dirs.read_keymap().contains("[[bindings]]"),
        "{}",
        dirs.read_keymap()
    );
    reload(&dirs, cx);
    assert_eq!(palette(cx), [None, Some("workbench".to_owned())]);
}

/// Each line found for `keys` exactly: its command and conflict note.
fn notes_for(
    editor: &Entity<ShortcutsEditor>,
    keys: &str,
    cx: &mut VisualTestContext,
) -> Vec<(String, Option<String>)> {
    set_search(editor, &format!("\"{keys}\""), cx);
    editor.read_with(cx, |editor, _| {
        (0..editor.lines().len())
            .map(|ix| {
                let (row, binding) = editor.line(ix).unwrap();
                (row.command.clone(), conflict_note(binding.unwrap()))
            })
            .collect()
    })
}

#[gpui::test]
fn editor_states_clash_with_the_editor(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    // Independent conditions on one editor: completions shown in vim's
    // insert mode.
    dirs.write_keymap(&format!(
        "[[bindings]]\nkeys = \"secondary-alt-6\"\ncommand = \"{SETTINGS_UI}\"\ncontext = \"editor && completion\"\n\n\
         [[bindings]]\nkeys = \"secondary-alt-6\"\ncommand = \"{OPEN_JSON}\"\ncontext = \"editor && vim.mode == insert\"\n"
    ));
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);

    // The shipped Down: the completion menu's while it shows, else the
    // editor's.
    let down = notes_for(&editor, "down", cx);
    let note = |command: &str, notes: &[(String, Option<String>)]| {
        (notes.iter())
            .find(|(ours, _)| ours == command)
            .unwrap_or_else(|| panic!("no {command}: {notes:?}"))
            .1
            .clone()
            .unwrap_or_default()
    };
    assert!(
        note("completion.select_next", &down).contains("editor.move_down"),
        "{down:?}"
    );
    assert!(
        note("editor.move_down", &down).contains("completion.select_next"),
        "{down:?}"
    );

    // The shipped accept-word key of ghost text, the editor's word motion.
    set_search(&editor, "ghost_text.accept_word", cx);
    let accept = lines_of(&editor, "ghost_text.accept_word", cx);
    let accept = accept[0].keys.clone().unwrap();
    let word = notes_for(&editor, &accept, cx);
    // Cmd-right on macOS is the editor's line end; ctrl-right elsewhere is
    // its word motion.
    let motion = if cfg!(target_os = "macos") {
        "editor.move_line_end"
    } else {
        "editor.move_word_right"
    };
    assert!(
        note("ghost_text.accept_word", &word).contains(motion),
        "{word:?}"
    );

    let mut both = notes_for(&editor, &keys("secondary-alt-6"), cx);
    both.sort();
    assert_eq!(
        both,
        [
            (
                OPEN_JSON.to_owned(),
                Some(format!("Also bound to {SETTINGS_UI}"))
            ),
            (
                SETTINGS_UI.to_owned(),
                Some(format!("Also bound to {OPEN_JSON}"))
            ),
        ]
    );
}

#[gpui::test]
fn vim_states_need_vim_on(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    // Visual mode and a pending command are vim's, never with vim off.
    dirs.write_keymap(&format!(
        "[[bindings]]\nkeys = \"secondary-alt-5\"\ncommand = \"{SETTINGS_UI}\"\ncontext = \"editor && vim.visual\"\n\n\
         [[bindings]]\nkeys = \"secondary-alt-5\"\ncommand = \"{OPEN_JSON}\"\ncontext = \"editor && !vim.enabled\"\n\n\
         [[bindings]]\nkeys = \"secondary-alt-4\"\ncommand = \"{SETTINGS_UI}\"\ncontext = \"editor && vim.pending\"\n\n\
         [[bindings]]\nkeys = \"secondary-alt-4\"\ncommand = \"{OPEN_JSON}\"\ncontext = \"editor && !vim.enabled\"\n"
    ));
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    for written in ["secondary-alt-5", "secondary-alt-4"] {
        let notes = notes_for(&editor, &keys(written), cx);
        assert_eq!(notes.len(), 2, "{written}: {notes:?}");
        assert!(
            notes.iter().all(|(_, note)| note.is_none()),
            "{written}: {notes:?}"
        );
    }
}

#[gpui::test]
fn reset_follows_the_file_while_open(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let other = match Os::current() {
        Os::Windows => "macos",
        _ => "windows",
    };
    dirs.write_keymap("");
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    search("keyboard shortcuts json", cx);
    let row = format!("shortcuts-row-{OPEN_JSON}");
    right_click(&row, cx);
    assert_eq!(
        menu_items(&editor, cx),
        Some(vec![MenuItem::CopyCommandId, MenuItem::AddKeybinding])
    );

    // An entry for another platform, and one a later entry unbinds: the
    // keymap in use is the same, but each is the command's to reset. The
    // open menu offers it once the file is read.
    let entries = [
        format!(
            "[[bindings]]\nkeys = \"secondary-alt-9\"\ncommand = \"{OPEN_JSON}\"\nplatform = \"{other}\"\n"
        ),
        format!(
            "[[bindings]]\nkeys = \"secondary-alt-8\"\ncommand = \"{OPEN_JSON}\"\n\n\
             [[bindings]]\nkeys = \"secondary-alt-8\"\ncommand = \"\"\n"
        ),
    ];
    for entry in entries {
        let keymap = cx.update(|_, cx| fe_gpui::current_keymap(cx));
        dirs.write_keymap(&entry);
        reload(&dirs, cx);
        let same = cx.update(|_, cx| std::sync::Arc::ptr_eq(&keymap, &fe_gpui::current_keymap(cx)));
        assert!(same, "the keymap in use doesn't change: {entry}");
        assert_eq!(
            menu_items(&editor, cx),
            Some(vec![
                MenuItem::CopyCommandId,
                MenuItem::AddKeybinding,
                MenuItem::ResetKeybinding
            ]),
            "{entry}"
        );
        // Deleted again: no reset.
        dirs.write_keymap("");
        reload(&dirs, cx);
        assert_eq!(
            menu_items(&editor, cx),
            Some(vec![MenuItem::CopyCommandId, MenuItem::AddKeybinding]),
            "{entry}"
        );
    }
}

#[gpui::test]
fn reset_restores_kiln_and_extension_defaults_unbound_on_another_platform(cx: &mut TestAppContext) {
    const PALETTE: &str = "command_palette.toggle";
    // The extension's keys for Open Settings differ by platform.
    const EXTENSION: &str = "[[bindings]]\n\
        keys = { macos = \"cmd-alt-7\", default = \"ctrl-alt-7\" }\n\
        command = \"workbench.open_settings_ui\"\n";
    let dirs = Dirs::new();
    // kiln's palette keys and the extension's keys unbound on another
    // platform, as written there; this platform's extension keys unbound
    // there too, which aren't its keys there; and fastedit's Open
    // Keyboard Shortcuts bound there.
    let (other, palette_keys, settings_keys, here_keys, open_keys) = match Os::current() {
        Os::MacOs => (
            "linux",
            "ctrl-shift-p",
            "ctrl-alt-7",
            "cmd-alt-7",
            "ctrl-alt-9",
        ),
        _ => (
            "macos",
            "cmd-shift-p",
            "cmd-alt-7",
            "ctrl-alt-7",
            "cmd-alt-9",
        ),
    };
    let unbind = |keys: &str| {
        format!("[[bindings]]\nkeys = \"{keys}\"\ncommand = \"\"\nplatform = \"{other}\"\n\n")
    };
    let open = format!(
        "[[bindings]]\nkeys = \"{open_keys}\"\ncommand = \"{OPEN}\"\nplatform = \"{other}\"\n"
    );
    let text = [
        unbind(palette_keys),
        unbind(settings_keys),
        unbind(here_keys),
        open.clone(),
    ]
    .concat();
    dirs.write_keymap(&text);
    let shipped = Shipped {
        extension: Some(EXTENSION),
        extension_elsewhere: None,
        platforms: true,
    };
    let (workbench, cx) = workbench_with(cx, &dirs, shipped);
    let editor = open_shortcuts(&workbench, cx);
    let reset = |search: &str, command: &str, cx: &mut VisualTestContext| {
        set_search(&editor, search, cx);
        right_click(&format!("shortcuts-row-{command}"), cx);
        let items = menu_items(&editor, cx).unwrap();
        assert!(
            items.contains(&MenuItem::ResetKeybinding),
            "{command}: {items:?}"
        );
        let disabled = editor.read_with(cx, |editor, _| {
            editor.disabled_menu_items().map(<[_]>::to_vec)
        });
        assert_eq!(disabled, Some(vec![]), "{command}");
        click_menu_item(MenuItem::ResetKeybinding, cx);
        assert_eq!(toasts(&workbench, cx), [], "{command}");
        reload(&dirs, cx);
        dirs.read_keymap()
    };

    // kiln's: only its unbind goes.
    let after = reset("show all commands", PALETTE, cx);
    assert_eq!(
        after,
        [unbind(settings_keys), unbind(here_keys), open.clone()].concat()
    );

    // The extension's: the unbind of its keys there goes; the one of keys
    // that are its keys only here stays, as it unbinds nothing there.
    let after = reset("open settings", SETTINGS_UI, cx);
    assert_eq!(after, [unbind(here_keys), open].concat());

    // fastedit's own resets as before.
    let after = reset("open keyboard shortcuts", OPEN, cx);
    assert_eq!(after.trim_end(), unbind(here_keys).trim_end());
}

#[gpui::test]
fn reset_restores_an_extension_default_kept_on_another_platform_only(cx: &mut TestAppContext) {
    // The extension's file binds Open Settings to secondary-alt-7 now. The
    // other platforms keep an earlier one, which bound f9, as its last good
    // file there: the new one has an error there (as `secondary-ctrl-k`
    // holds ctrl twice off macOS).
    const NOW: &str = "[[bindings]]\nkeys = \"secondary-alt-7\"\n\
        command = \"workbench.open_settings_ui\"\n";
    reset_an_extension_default_kept_elsewhere(Some(NOW), cx);
}

#[gpui::test]
fn reset_restores_an_extension_default_in_a_layer_on_another_platform_only(
    cx: &mut TestAppContext,
) {
    // The extension's file has an error here, and no last good version:
    // there's no layer of it here, only on the other platforms.
    reset_an_extension_default_kept_elsewhere(None, cx);
}

/// Resets Open Settings, whose extension keys on the other platforms are
/// f9, with `here` the extension's keymap on this one, and the user's
/// unbinds of f9 and f8 there: f9's goes, f8's, not its key anywhere,
/// stays.
fn reset_an_extension_default_kept_elsewhere(here: Option<&'static str>, cx: &mut TestAppContext) {
    const LAST_GOOD: &str = "[[bindings]]\nkeys = \"f9\"\n\
        command = \"workbench.open_settings_ui\"\n";
    let other = match Os::current() {
        Os::MacOs => "linux",
        _ => "macos",
    };
    let dirs = Dirs::new();
    // f9 unbound on the other platform, where it's the extension's key.
    let text = format!(
        "[[bindings]]\nkeys = \"f9\"\ncommand = \"\"\nplatform = \"{other}\"\n\n\
         [[bindings]]\nkeys = \"f8\"\ncommand = \"\"\nplatform = \"{other}\"\n"
    );
    dirs.write_keymap(&text);
    let shipped = Shipped {
        extension: here,
        extension_elsewhere: Some(LAST_GOOD),
        platforms: true,
    };
    let (workbench, cx) = workbench_with(cx, &dirs, shipped);
    let editor = open_shortcuts(&workbench, cx);
    set_search(&editor, "open settings", cx);
    right_click(&format!("shortcuts-row-{SETTINGS_UI}"), cx);
    let items = menu_items(&editor, cx).unwrap();
    assert!(items.contains(&MenuItem::ResetKeybinding), "{items:?}");
    let disabled = editor.read_with(cx, |editor, _| {
        editor.disabled_menu_items().map(<[_]>::to_vec)
    });
    assert_eq!(disabled, Some(vec![]));
    click_menu_item(MenuItem::ResetKeybinding, cx);
    assert_eq!(toasts(&workbench, cx), []);
    let after = dirs.read_keymap();
    assert_eq!(
        after.trim_start(),
        format!("[[bindings]]\nkeys = \"f8\"\ncommand = \"\"\nplatform = \"{other}\"\n")
    );
}

#[gpui::test]
fn reset_is_disabled_for_another_platforms_entry_on_a_kiln_default_without_its_platforms(
    cx: &mut TestAppContext,
) {
    const PALETTE: &str = "command_palette.toggle";
    let dirs = Dirs::new();
    // kiln's palette keys unbound on another platform, as written there;
    // and fastedit's Open Keyboard Shortcuts bound there too.
    let (other, palette_keys, open_keys) = match Os::current() {
        Os::MacOs => ("linux", "ctrl-shift-p", "ctrl-alt-9"),
        _ => ("macos", "cmd-shift-p", "cmd-alt-9"),
    };
    let text = format!(
        "[[bindings]]\nkeys = \"{palette_keys}\"\ncommand = \"\"\nplatform = \"{other}\"\n\n\
         [[bindings]]\nkeys = \"{open_keys}\"\ncommand = \"{OPEN}\"\nplatform = \"{other}\"\n"
    );
    dirs.write_keymap(&text);
    // A store kept with this platform's default layers alone, not
    // kiln's setup.
    let shipped = Shipped {
        extension: None,
        extension_elsewhere: None,
        platforms: false,
    };
    let (workbench, cx) = workbench_with(cx, &dirs, shipped);
    let editor = open_shortcuts(&workbench, cx);
    let disabled = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| {
            editor.disabled_menu_items().map(<[_]>::to_vec)
        })
    };

    // The tab can't tell what the file does to kiln's default there: Reset
    // shows, disabled, with its hint, and does nothing.
    set_search(&editor, "show all commands", cx);
    right_click(&format!("shortcuts-row-{PALETTE}"), cx);
    let items = menu_items(&editor, cx).unwrap();
    assert!(items.contains(&MenuItem::ResetKeybinding), "{items:?}");
    assert_eq!(disabled(cx), Some(vec![MenuItem::ResetKeybinding]));
    let item = cx
        .debug_bounds("menu-item:Reset Keybinding")
        .expect("drawn");
    assert!(drawn("shortcuts-menu-hint", cx), "{RESET_REFUSED_HINT}");
    cx.simulate_click(item.center(), Modifiers::none());
    draw(cx);
    assert_eq!(dirs.read_keymap(), text, "nothing written");
    assert_eq!(toasts(&workbench, cx), []);
    assert!(
        menu_items(&editor, cx).is_some(),
        "a disabled item leaves the menu open"
    );
    cx.simulate_keystrokes("escape");
    draw(cx);

    // fastedit's own default resets as before, its file passed as shipped.
    set_search(&editor, "open keyboard shortcuts", cx);
    right_click(&format!("shortcuts-row-{OPEN}"), cx);
    assert_eq!(disabled(cx), Some(vec![]));
    click_menu_item(MenuItem::ResetKeybinding, cx);
    let after = dirs.read_keymap();
    assert!(!after.contains(OPEN), "{after}");
    assert!(after.contains(palette_keys), "{after}");
}

#[gpui::test]
fn alternative_branches_clash(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    // Each holds in the Keyboard Shortcuts tab, through its second branch.
    dirs.write_keymap(&format!(
        "[[bindings]]\nkeys = \"secondary-alt-3\"\ncommand = \"{SETTINGS_UI}\"\ncontext = \"settings_editor || shortcuts_editor\"\n\n\
         [[bindings]]\nkeys = \"secondary-alt-3\"\ncommand = \"{OPEN_JSON}\"\ncontext = \"vim_line || shortcuts_editor\"\n"
    ));
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    let mut notes = notes_for(&editor, &keys("secondary-alt-3"), cx);
    notes.sort();
    assert_eq!(
        notes,
        [
            (
                OPEN_JSON.to_owned(),
                Some(format!("Also bound to {SETTINGS_UI}"))
            ),
            (
                SETTINGS_UI.to_owned(),
                Some(format!("Also bound to {OPEN_JSON}"))
            ),
        ]
    );
}

/// A keymap binding the Settings UI and the JSON shortcuts to each
/// `(keys, one context, the other)`, and a Keyboard Shortcuts tab on it:
/// for each pair, whether both its lines are marked as clashing.
fn clashing(pairs: &[(&str, &str, &str)], cx: &mut TestAppContext) -> Vec<bool> {
    let dirs = Dirs::new();
    let entry = |keys: &str, command: &str, context: &str| {
        format!(
            "[[bindings]]\nkeys = \"{keys}\"\ncommand = \"{command}\"\ncontext = \"{context}\"\n\n"
        )
    };
    dirs.write_keymap(
        &(pairs.iter())
            .map(|(keys, a, b)| entry(keys, SETTINGS_UI, a) + &entry(keys, OPEN_JSON, b))
            .collect::<String>(),
    );
    let (workbench, cx) = workbench(cx, &dirs);
    let editor = open_shortcuts(&workbench, cx);
    (pairs.iter())
        .map(|(written, ..)| {
            let notes = notes_for(&editor, &keys(written), cx);
            assert_eq!(notes.len(), 2, "{written}: {notes:?}");
            let marked = notes.iter().filter(|(_, note)| note.is_some()).count();
            assert_ne!(marked, 1, "{written}: {notes:?}");
            marked == 2
        })
        .collect()
}

#[gpui::test]
fn visual_frames_clash(cx: &mut TestAppContext) {
    let marked = clashing(
        &[
            // Only a visual mode has both.
            (
                "secondary-alt-9",
                "editor && vim.visual",
                "editor && vim.mode == visual_block",
            ),
            (
                "secondary-alt-8",
                "editor && vim.visual",
                "editor && vim.pending_kind == text_object",
            ),
            // Never in one: `vim.visual` goes with its modes, and `r` in a
            // visual mode is a pending replace, not the operator.
            (
                "secondary-alt-7",
                "editor && vim.visual",
                "editor && vim.mode != visual && vim.mode != visual_line && vim.mode != visual_block",
            ),
            (
                "secondary-alt-6",
                "editor && vim.visual",
                "editor && vim.pending_kind == operator",
            ),
            (
                "secondary-alt-5",
                "editor && vim.visual",
                "editor && completion",
            ),
        ],
        cx,
    );
    assert_eq!(marked, [true, true, false, false, false]);
}

#[gpui::test]
fn pending_operator_frames_clash(cx: &mut TestAppContext) {
    let marked = clashing(
        &[
            // `di`, `dj` held for a map, and `<C-o>d`.
            (
                "secondary-alt-9",
                "editor && vim.mode == operator_pending",
                "editor && vim.pending_kind == text_object",
            ),
            (
                "secondary-alt-8",
                "editor && vim.mode == operator_pending",
                "editor && vim.pending_kind == remap",
            ),
            (
                "secondary-alt-7",
                "editor && vim.mode == operator_pending",
                "editor && vim.insert_pending",
            ),
            // An operator always waits on something, never on a register
            // (`d\"` cancels) or a mark to set.
            (
                "secondary-alt-6",
                "editor && vim.mode == operator_pending",
                "editor && !vim.pending",
            ),
            (
                "secondary-alt-5",
                "editor && vim.mode == operator_pending",
                "editor && (vim.pending_kind == register || vim.pending_kind == set_mark)",
            ),
            (
                "secondary-alt-4",
                "editor && vim.pending_kind == operator",
                "editor && vim.mode == normal",
            ),
        ],
        cx,
    );
    assert_eq!(marked, [true, true, true, false, false, false]);
}

#[gpui::test]
fn cmdline_frames_clash(cx: &mut TestAppContext) {
    let marked = clashing(
        &[
            // Only on the command line, and there while recording or after
            // `<C-o>`.
            (
                "secondary-alt-9",
                "editor && vim.cmdline_kind == ex",
                "editor && vim.mode == cmdline",
            ),
            (
                "secondary-alt-8",
                "editor && vim.cmdline_kind == search_backward",
                "editor && vim.recording == q && vim.insert_pending",
            ),
            // The kind goes with the mode, the line takes its keys with
            // nothing pending, and no menu or suggestion shows there.
            (
                "secondary-alt-7",
                "editor && vim.cmdline_kind == search_forward",
                "editor && vim.mode != cmdline",
            ),
            (
                "secondary-alt-6",
                "editor && vim.mode == cmdline",
                "editor && vim.pending",
            ),
            (
                "secondary-alt-5",
                "editor && vim.mode == cmdline",
                "editor && ghost_text",
            ),
        ],
        cx,
    );
    assert_eq!(marked, [true, true, false, false, false]);
}

#[gpui::test]
fn editor_flags_only_where_they_show(cx: &mut TestAppContext) {
    let marked = clashing(
        &[
            // The menu and a suggestion show with vim off and where vim's
            // keys type; a proposal in every mode.
            (
                "secondary-alt-9",
                "editor && completion",
                "editor && vim.mode == replace",
            ),
            (
                "secondary-alt-8",
                "editor && ghost_text",
                "editor && !vim.enabled",
            ),
            (
                "secondary-alt-7",
                "editor && proposal",
                "editor && vim.pending_kind == find",
            ),
            (
                "secondary-alt-6",
                "editor && completion",
                "editor && vim.mode == normal",
            ),
            (
                "secondary-alt-5",
                "editor && ghost_text",
                "editor && vim.pending",
            ),
        ],
        cx,
    );
    assert_eq!(marked, [true, true, true, false, false]);
}

#[gpui::test]
fn ruling_out_editor_keys_holds_outside_an_editor(cx: &mut TestAppContext) {
    let marked = clashing(
        &[
            // The panel and the terminal hold no editor, whatever its keys.
            ("secondary-alt-9", "project_panel && !vim.enabled", "editor"),
            (
                "secondary-alt-8",
                "terminal && vim.mode != normal",
                "editor",
            ),
            ("secondary-alt-7", "terminal && !vim.visual", "terminal"),
            // With no other view named, an editor's frame may be focused.
            ("secondary-alt-6", "workbench && !vim.enabled", "editor"),
            (
                "secondary-alt-5",
                "vim.mode != normal",
                "editor && vim.mode == insert",
            ),
        ],
        cx,
    );
    assert_eq!(marked, [false, false, true, true, true]);
}

#[gpui::test]
fn an_editor_key_named_as_a_flag_never_holds(cx: &mut TestAppContext) {
    let marked = clashing(
        &[
            // `vim.recording` and `vim.mode` are keys of the editor's,
            // never flags, though no context here names their values.
            (
                "secondary-alt-9",
                "editor && vim.recording",
                "editor && !vim.enabled",
            ),
            ("secondary-alt-8", "vim.mode", "editor"),
        ],
        cx,
    );
    assert_eq!(marked, [false, false]);
}

#[path = "shortcuts_recorder_tests.rs"]
mod recorder_tests;

#[path = "shortcuts_editor_reset_race_tests.rs"]
mod reset_race_tests;

#[path = "shortcuts_editor_frames_tests.rs"]
mod frames_tests;
