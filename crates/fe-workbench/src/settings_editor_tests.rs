//! The Settings tab (`workbench.open_settings_ui`, cmd-comma) in a headless
//! window: one tab, found again in any pane, never in a saved layout; its
//! search, groups, scopes and modified markers; its keys; and its list
//! building only the rows in view. The user's config directory is a
//! temporary one, through the test-only override.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_command::{ContextFrame, ContextStack, Invocation};
use fe_config::{Settings, SettingsModel};
use fe_gpui::{SettingsEvent, commands::RunCommand};
use gpui::{Entity, Focusable as _, TestAppContext, VisualTestContext};
use kiln::platform::config::{self, BatchSender, ConfigService};
use kiln_shell::{PaneItem as _, layout::LayoutStore, layout::WindowLayout};

use crate::{
    Launch, Workbench,
    open::ConfigDirOverride,
    settings_editor::{NOTHING_FOUND, Row, Scope, SettingsEditor},
};

/// A workbench on `root`, or on no folder, whose user settings are
/// `config`'s `settings.toml`, focused.
fn workbench<'a>(
    cx: &'a mut TestAppContext,
    root: Option<&Path>,
    config: &Path,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let launch = Launch {
        root: root.map(Path::to_owned),
        file: None,
    };
    let (workbench, cx, _) = crate::tests::open(cx, launch);
    cx.update(|window, cx| {
        cx.set_global(ConfigDirOverride(Some(config.to_owned())));
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

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(
        Invocation::of(id)
            .unwrap()
            .context(ContextStack::new().with(ContextFrame::new("workbench"))),
    );
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

/// Every Settings tab, in every pane, with its pane's index.
fn settings_tabs(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(usize, Entity<SettingsEditor>)> {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.panes(cx).iter().enumerate())
            .flat_map(|(ix, pane)| {
                (pane.read(cx).tabs().iter())
                    .filter_map(|tab| tab.item().downcast::<SettingsEditor>())
                    .map(move |editor| (ix, editor))
                    .collect::<Vec<_>>()
            })
            .collect()
    })
}

/// Opens the Settings tab and returns it, its files read.
fn open_settings(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Entity<SettingsEditor> {
    run("workbench.open_settings_ui", cx);
    draw(cx);
    let tabs = settings_tabs(workbench, cx);
    let [(_, editor)] = <[_; 1]>::try_from(tabs).expect("one Settings tab");
    assert!(editor.read_with(cx, |editor, _| editor.model().is_some()));
    editor
}

fn search_focused(editor: &Entity<SettingsEditor>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| {
        let search = editor.read(cx).search_input().focus_handle(cx);
        search.is_focused(window)
    })
}

fn search_text(editor: &Entity<SettingsEditor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, cx| {
        editor.search_input().read(cx).text().to_owned()
    })
}

/// Types `text` into the focused search field.
fn search(text: &str, cx: &mut VisualTestContext) {
    cx.simulate_input(text);
    draw(cx);
}

/// The rows as text: `# Group` for a heading, the dotted key for a setting.
fn row_texts(editor: &Entity<SettingsEditor>, cx: &mut VisualTestContext) -> Vec<String> {
    editor.read_with(cx, |editor, _| {
        let model = editor.model().unwrap();
        (editor.rows().iter())
            .map(|row| match row {
                Row::Group(title) => format!("# {title}"),
                Row::Setting(ix) => model.entries()[*ix].key_path(),
            })
            .collect()
    })
}

fn drawn(selector: &str, cx: &mut VisualTestContext) -> bool {
    cx.debug_bounds(selector.to_owned().leak()).is_some()
}

/// `key`'s value as the tab shows it, and whether it's marked modified.
fn shown(editor: &Entity<SettingsEditor>, key: &str, cx: &mut VisualTestContext) -> (String, bool) {
    editor.read_with(cx, |editor, _| {
        let model = editor.model().unwrap();
        let entry = (model.entries().iter())
            .find(|entry| entry.key_path() == key)
            .unwrap();
        editor.shown_value(entry).unwrap()
    })
}

fn set_scope(editor: &Entity<SettingsEditor>, scope: Scope, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| editor.set_scope(scope, cx));
    draw(cx);
}

/// A project folder and a config directory, each in a temporary directory.
struct Dirs {
    _temp: tempfile::TempDir,
    root: std::path::PathBuf,
    config: std::path::PathBuf,
}

impl Dirs {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(temp.path()).unwrap();
        let (root, config) = (base.join("project"), base.join("config"));
        fs::create_dir_all(root.join(".fastedit")).unwrap();
        fs::create_dir_all(&config).unwrap();
        Self {
            _temp: temp,
            root,
            config,
        }
    }

    fn write_user(&self, text: &str) {
        fs::write(self.config.join("settings.toml"), text).unwrap();
    }

    fn write_workspace(&self, text: &str) {
        fs::write(fe_config::project_settings_path(&self.root), text).unwrap();
    }
}

#[gpui::test]
fn settings_command_opens_one_tab(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let editor = open_settings(&workbench, cx);
    let (title, path, serialized) = editor.read_with(cx, |editor, cx| {
        (
            editor.tab_title(cx),
            editor.path(cx).is_none(),
            editor.serialize(cx),
        )
    });
    assert_eq!(title, "Settings");
    assert!(path, "the tab is no file");
    assert_eq!(serialized, None);
    let active = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let tab = &pane.tabs()[pane.active().unwrap()];
        (tab.path.clone(), tab.item().downcast::<SettingsEditor>())
    });
    assert_eq!(active, (None, Some(editor.clone())));
    assert!(search_focused(&editor, cx), "the search field has focus");
    assert!(drawn("settings-search", cx));

    // cmd-comma runs it too: still the one tab.
    cx.simulate_keystrokes("cmd-,");
    draw(cx);
    assert_eq!(settings_tabs(&workbench, cx).len(), 1);

    // It closes like any tab.
    run("workbench.close_tab", cx);
    assert!(settings_tabs(&workbench, cx).is_empty());
}

#[gpui::test]
fn second_open_focuses_it(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    fs::write(dirs.root.join("a.txt"), "alpha\n").unwrap();
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let editor = open_settings(&workbench, cx);
    // Another pane, with a file active in it and focused.
    run("pane.split_right", cx);
    let file = dirs.root.join("a.txt");
    workbench.update_in(cx, |workbench, window, cx| {
        workbench
            .pane()
            .clone()
            .update(cx, |pane, cx| pane.open(file, window, cx))
    });
    draw(cx);
    assert!(!search_focused(&editor, cx));
    let right = workbench.read_with(cx, |workbench, _| workbench.pane().clone());

    run("workbench.open_settings_ui", cx);
    let tabs = settings_tabs(&workbench, cx);
    assert_eq!(tabs.len(), 1, "no second tab");
    assert_eq!(
        tabs[0],
        (0, editor.clone()),
        "the tab stays in the left pane"
    );
    let (active_pane, shown) = workbench.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        let tab = &pane.tabs()[pane.active().unwrap()];
        (
            workbench.pane().clone(),
            tab.item().downcast::<SettingsEditor>(),
        )
    });
    assert_ne!(active_pane, right, "its pane is the active one");
    assert_eq!(shown, Some(editor.clone()));
    assert!(search_focused(&editor, cx));
}

#[gpui::test]
fn tab_is_not_persisted(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    fs::write(dirs.root.join("a.txt"), "alpha\n").unwrap();
    let store = LayoutStore::new(dirs.config.join("layouts"), &dirs.root);
    let mut app = cx.clone();
    let (first, cx) = workbench(&mut app, Some(&dirs.root), &dirs.config);
    let file = dirs.root.join("a.txt");
    first.update_in(cx, |workbench, window, cx| {
        workbench
            .pane()
            .clone()
            .update(cx, |pane, cx| pane.open(file, window, cx))
    });
    draw(cx);
    open_settings(&first, cx);
    let claim = store.claim_new().unwrap();
    first.update_in(cx, |workbench, window, cx| {
        workbench.set_layout(WindowLayout::new(store.clone(), claim), window, cx)
    });
    draw(cx);
    cx.update(|window, _| window.remove_window());
    drop(first);
    cx.run_until_parked();
    let entries = store.entries();
    let saved = entries[0].to_string();
    assert!(saved.contains("a.txt"), "{saved}");
    assert!(!saved.contains("settings_editor"), "{saved}");

    // Restored: the file, and no Settings tab.
    let mut app = app.clone();
    let (second, cx) = workbench(&mut app, Some(&dirs.root), &dirs.config);
    let (claim, entry) = store.restorable().pop().expect("the entry restores");
    second.update_in(cx, |workbench, window, cx| {
        workbench.restore_layout(&entry, window, cx);
        workbench.set_layout(WindowLayout::new(store.clone(), claim), window, cx);
    });
    draw(cx);
    let paths = second.read_with(cx, |workbench, cx| {
        let pane = workbench.pane().read(cx);
        pane.tabs()
            .iter()
            .map(|tab| tab.path.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(paths, [Some(dirs.root.join("a.txt"))]);
    assert!(settings_tabs(&second, cx).is_empty());
}

#[gpui::test]
fn lists_every_group(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let editor = open_settings(&workbench, cx);
    let model = SettingsModel::new(None, None);
    let rows = row_texts(&editor, cx);
    // Every setting once, and a heading for every group with settings,
    // before them.
    for entry in model.entries() {
        let key = entry.key_path();
        let at = rows.iter().position(|row| *row == key);
        let at = at.unwrap_or_else(|| panic!("{key} is listed"));
        assert_eq!(rows.iter().filter(|row| **row == key).count(), 1, "{key}");
        if !entry.group.is_empty() {
            let heading = format!("# {}", entry.group);
            let group = rows[..at].iter().rposition(|row| row.starts_with("# "));
            assert_eq!(group.map(|ix| &rows[ix]), Some(&heading), "{key}");
        }
    }
    for group in model.groups().iter().filter(|group| group.count > 0) {
        let heading = format!("# {}", group.title);
        assert_eq!(rows.iter().filter(|row| **row == heading).count(), 1);
    }
    assert!(rows.contains(&"# Editor".to_owned()));
    assert!(rows.contains(&"# Editor > Minimap".to_owned()));
    assert_eq!(
        rows.len(),
        model.entries().len() + model.groups().iter().filter(|g| g.count > 0).count()
    );
    // The first rows are drawn: a heading, and a setting's row under it.
    let first_group = rows.iter().find(|row| row.starts_with("# ")).unwrap();
    assert!(drawn(&format!("settings-group-{}", &first_group[2..]), cx));
    let first_setting = rows.iter().find(|row| !row.starts_with("# ")).unwrap();
    assert!(drawn(&format!("settings-row-{first_setting}"), cx));
}

#[gpui::test]
fn search_filters_and_hides_empty_groups(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let editor = open_settings(&workbench, cx);
    search("minimap", cx);
    assert_eq!(search_text(&editor, cx), "minimap");
    let rows = row_texts(&editor, cx);
    let model = SettingsModel::new(None, None);
    let found: Vec<String> = (model.search("minimap").iter())
        .map(|entry| entry.key_path())
        .collect();
    let listed: Vec<&String> = rows.iter().filter(|row| !row.starts_with("# ")).collect();
    assert_eq!(listed, found.iter().collect::<Vec<_>>());
    assert!(rows.contains(&"# Editor > Minimap".to_owned()), "{rows:?}");
    assert!(!rows.iter().any(|row| row == "# Terminal"), "{rows:?}");
    // Every heading has a setting under it.
    for (ix, row) in rows.iter().enumerate() {
        if row.starts_with("# ") {
            let next = rows.get(ix + 1);
            assert!(next.is_some_and(|next| !next.starts_with("# ")), "{rows:?}");
        }
    }
    assert!(drawn("settings-group-Editor > Minimap", cx));
    assert!(drawn("settings-row-editor.minimap.enabled", cx));
    assert!(!drawn("settings-group-Terminal", cx));
    assert!(!drawn("settings-empty", cx));
}

#[gpui::test]
fn empty_result_message(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let editor = open_settings(&workbench, cx);
    search("zzqqxyzzy", cx);
    assert!(row_texts(&editor, cx).is_empty());
    assert!(drawn("settings-empty", cx));
    assert_eq!(NOTHING_FOUND, "No settings found");
    assert!(!drawn("settings-group-Editor", cx));
}

#[gpui::test]
fn scope_bar_hidden_without_folder(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, None, &dirs.config);
    let editor = open_settings(&workbench, cx);
    assert!(drawn("settings-scope-user", cx));
    assert!(!drawn("settings-scope-workspace", cx));
    // Asked for Workspace anyway, it shows the user's values.
    set_scope(&editor, Scope::Workspace, cx);
    assert_eq!(
        editor.read_with(cx, |editor, _| editor.scope()),
        Scope::User
    );

    // A folder opens: the Workspace tab shows.
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_root(dirs.root.clone(), window, cx)
    });
    draw(cx);
    assert!(drawn("settings-scope-user", cx));
    assert!(drawn("settings-scope-workspace", cx));
}

#[gpui::test]
fn modified_marker_follows_scope(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    dirs.write_user("[editor]\ntab_size = 2\n");
    dirs.write_workspace("[editor.minimap]\nenabled = false\n");
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let editor = open_settings(&workbench, cx);
    search("editor", cx);
    let (tab_size, minimap) = ("editor.tab_size", "editor.minimap.enabled");
    // Both rows show.
    search(" tab size", cx);
    assert!(drawn(&format!("settings-row-{tab_size}"), cx));
    assert!(drawn(&format!("settings-modified-{tab_size}"), cx));
    assert_eq!(shown(&editor, tab_size, cx), ("2".to_owned(), true));

    // The row at its default has no marker.
    let input = editor.read_with(cx, |editor, _| editor.search_input().clone());
    input.update(cx, |input, cx| input.clear(cx));
    search("minimap enabled", cx);
    assert!(drawn(&format!("settings-row-{minimap}"), cx));
    assert!(!drawn(&format!("settings-modified-{minimap}"), cx));
    assert_eq!(shown(&editor, minimap, cx), ("true".to_owned(), false));

    // Under Workspace, the project's file's.
    set_scope(&editor, Scope::Workspace, cx);
    assert!(drawn(&format!("settings-modified-{minimap}"), cx));
    assert_eq!(shown(&editor, minimap, cx), ("false".to_owned(), true));
    // The user's tab size shows, inherited, unmarked.
    assert_eq!(shown(&editor, tab_size, cx), ("2".to_owned(), false));
    input.update(cx, |input, cx| input.clear(cx));
    search("tab size", cx);
    assert!(drawn(&format!("settings-row-{tab_size}"), cx));
    assert!(!drawn(&format!("settings-modified-{tab_size}"), cx));
}

#[gpui::test]
fn switching_scope_rereads(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    dirs.write_user("[editor]\ntab_size = 2\n");
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let editor = open_settings(&workbench, cx);
    let key = "editor.tab_size";
    assert_eq!(shown(&editor, key, cx), ("2".to_owned(), true));

    // The files change with no reload: the scope switch reads them.
    dirs.write_workspace("[editor]\ntab_size = 8\n");
    dirs.write_user("[editor]\ntab_size = 3\n");
    set_scope(&editor, Scope::Workspace, cx);
    assert_eq!(shown(&editor, key, cx), ("8".to_owned(), true));
    set_scope(&editor, Scope::User, cx);
    assert_eq!(shown(&editor, key, cx), ("3".to_owned(), true));
}

#[gpui::test]
fn workspace_restricted_rows_are_dimmed(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let editor = open_settings(&workbench, cx);
    search("shell", cx);
    let rows = row_texts(&editor, cx);
    let restricted = "terminal.shell";
    assert!(rows.iter().any(|row| row == restricted), "{rows:?}");
    let allowed = editor.read_with(cx, |editor, _| {
        let model = editor.model().unwrap();
        (model.entries().iter())
            .find(|entry| entry.key_path() == restricted)
            .is_some_and(|entry| model.workspace_allowed(entry))
    });
    assert!(!allowed);
    let note = format!("settings-restricted-{restricted}");
    assert!(drawn(&format!("settings-row-{restricted}"), cx));
    assert!(!drawn(&note, cx), "not under User");

    set_scope(&editor, Scope::Workspace, cx);
    assert!(drawn(&note, cx));
    // A row a workspace may set has no note.
    let other = (rows.iter())
        .filter(|row| !row.starts_with("# ") && *row != restricted)
        .find(|row| {
            editor.read_with(cx, |editor, _| {
                let model = editor.model().unwrap();
                (model.entries().iter())
                    .find(|entry| entry.key_path() == **row)
                    .is_some_and(|entry| model.workspace_allowed(entry))
            })
        });
    if let Some(other) = other {
        assert!(drawn(&format!("settings-row-{other}"), cx));
        assert!(!drawn(&format!("settings-restricted-{other}"), cx));
    }
}

#[gpui::test]
fn external_edit_changing_an_effective_value_refreshes_the_tab(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let editor = open_settings(&workbench, cx);
    let key = "editor.tab_size";
    search("tab size", cx);
    assert_eq!(shown(&editor, key, cx), ("4".to_owned(), false));
    assert!(!drawn(&format!("settings-modified-{key}"), cx));

    // The watcher sees the edit; the workbench reloads its settings.
    dirs.write_user("[editor]\ntab_size = 2\n");
    let mut settings = Settings::default();
    settings.editor.tab_size = 2;
    let event = SettingsEvent {
        settings: Arc::new(settings),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    draw(cx);
    assert_eq!(shown(&editor, key, cx), ("2".to_owned(), true));
    assert!(drawn(&format!("settings-modified-{key}"), cx));
}

#[gpui::test]
fn keys_move_the_selection_and_reach_the_search(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let editor = open_settings(&workbench, cx);
    let settings_at = |editor: &Entity<SettingsEditor>, cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| {
            (editor.rows().iter().enumerate())
                .filter(|(_, row)| matches!(row, Row::Setting(_)))
                .map(|(ix, _)| ix)
                .collect::<Vec<_>>()
        })
    };
    let settings = settings_at(&editor, cx);
    let selected = |cx: &mut VisualTestContext| editor.read_with(cx, |editor, _| editor.selected());
    let list_focused = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| editor.read(cx).list_focus().is_focused(window))
    };

    // Down from the search selects the first setting, in the list.
    cx.simulate_keystrokes("down");
    draw(cx);
    assert_eq!(selected(cx), Some(settings[0]));
    assert!(list_focused(cx));
    cx.simulate_keystrokes("down down up");
    draw(cx);
    assert_eq!(selected(cx), Some(settings[1]));

    // Far down: the row scrolls into view.
    let far = 40.min(settings.len() - 1);
    for _ in 1..far {
        cx.simulate_keystrokes("down");
    }
    draw(cx);
    draw(cx);
    assert_eq!(selected(cx), Some(settings[far]));
    let key = editor.read_with(cx, |editor, _| match &editor.rows()[settings[far]] {
        Row::Setting(ix) => editor.model().unwrap().entries()[*ix].key_path(),
        Row::Group(_) => unreachable!(),
    });
    let row = cx.debug_bounds(format!("settings-row-{key}").leak());
    let row = row.expect("the selected row is drawn");
    let window = cx.update(|window, _| window.viewport_size());
    assert!(row.bottom() <= window.height, "{row:?} in {window:?}");

    // `/` goes to the search, which types it as text.
    cx.simulate_keystrokes("/");
    draw(cx);
    assert!(search_focused(&editor, cx));
    assert_eq!(search_text(&editor, cx), "");
    search("tab", cx);
    assert_eq!(search_text(&editor, cx), "tab");

    // Escape clears it.
    cx.simulate_keystrokes("escape");
    draw(cx);
    assert_eq!(search_text(&editor, cx), "");
    assert_eq!(settings_at(&editor, cx), settings);

    // secondary-f too goes to the search.
    cx.simulate_keystrokes("down");
    draw(cx);
    assert!(list_focused(cx));
    let find = if cfg!(target_os = "macos") {
        "cmd-f"
    } else {
        "ctrl-f"
    };
    cx.simulate_keystrokes(find);
    draw(cx);
    assert!(search_focused(&editor, cx));
}

#[gpui::test]
fn list_builds_only_the_rows_in_view(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let editor = open_settings(&workbench, cx);
    editor.update(cx, |editor, _| editor.take_built());
    cx.update(|window, _| window.refresh());
    draw(cx);
    let rows = editor.read_with(cx, |editor, _| editor.rows().len());
    let built = editor.update(cx, |editor, _| editor.take_built());
    let most = built.iter().map(|range| range.len()).max().unwrap_or(0);
    let window = cx.update(|window, _| window.viewport_size());
    // A row is 84px: what fits in the window, and one more either side.
    let fits = (f32::from(window.height) / 84.).ceil() as usize + 2;
    assert!(most > 1, "the list drew rows: {built:?}");
    assert!(most <= fits, "{most} rows built of {rows}; {fits} fit");
    assert!(rows > fits, "the schema's rows are more than fit: {rows}");
}

#[gpui::test]
fn search_edits_through_the_text_input_keys(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let editor = open_settings(&workbench, cx);
    search("minimapx", cx);
    assert!(row_texts(&editor, cx).is_empty());
    cx.simulate_keystrokes("backspace");
    draw(cx);
    assert_eq!(search_text(&editor, cx), "minimap");
    assert!(row_texts(&editor, cx).contains(&"editor.minimap.enabled".to_owned()));
    search(" side", cx);
    cx.simulate_keystrokes("alt-backspace");
    draw(cx);
    assert_eq!(search_text(&editor, cx), "minimap ");
    cx.simulate_keystrokes("cmd-backspace");
    draw(cx);
    assert_eq!(search_text(&editor, cx), "");
    cx.write_to_clipboard(gpui::ClipboardItem::new_string("tab size".to_owned()));
    cx.simulate_keystrokes("cmd-v");
    draw(cx);
    assert_eq!(search_text(&editor, cx), "tab size");
    assert!(row_texts(&editor, cx).contains(&"editor.tab_size".to_owned()));
}

/// Whether `inner` lies within `outer`.
fn within(inner: gpui::Bounds<gpui::Pixels>, outer: gpui::Bounds<gpui::Pixels>) -> bool {
    inner.top() >= outer.top()
        && inner.bottom() <= outer.bottom()
        && inner.left() >= outer.left()
        && inner.right() <= outer.right()
}

fn bounds(selector: &str, cx: &mut VisualTestContext) -> gpui::Bounds<gpui::Pixels> {
    let found = cx.debug_bounds(selector.to_owned().leak());
    found.unwrap_or_else(|| panic!("{selector} is drawn"))
}

/// A setting whose description wraps to two lines keeps its title, both
/// lines and its value inside its row, each at its full height.
#[gpui::test]
fn rows_fit_a_wrapped_description_and_the_value(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let editor = open_settings(&workbench, cx);
    let key = "editor.bracket_pair_colors";
    let description = editor.read_with(cx, |editor, _| {
        let model = editor.model().unwrap();
        let entry = model.entries().iter().find(|e| e.key_path() == key);
        entry.unwrap().description.clone()
    });
    assert!(description.len() > 80, "{description}");
    // Narrow enough that the description wraps.
    cx.simulate_resize(gpui::size(gpui::px(560.), gpui::px(700.)));
    search("bracket pair colors", cx);
    let row = bounds(&format!("settings-row-{key}"), cx);
    let text = bounds(&format!("settings-description-{key}"), cx);
    let value = bounds(&format!("settings-value-{key}"), cx);
    assert!(within(text, row), "{text:?} in {row:?}");
    assert!(within(value, row), "{value:?} in {row:?}");
    assert_eq!(text.size.height, gpui::px(32.), "two lines");
    assert_eq!(value.size.height, gpui::px(18.));
    assert!(value.top() >= text.bottom());
}

/// In a list with room for barely one row, the selected setting is always
/// in view, the first one too, going down or up.
#[gpui::test]
fn the_selected_row_shows_in_a_short_list(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let editor = open_settings(&workbench, cx);
    let list = bounds("settings-list", cx);
    let tall = list.size.height;
    // Shrinks the window till the list has room for 1.5 rows.
    let window = cx.update(|window, _| window.viewport_size());
    let short = window.height - tall + gpui::px(144.);
    cx.simulate_resize(gpui::size(window.width, short));
    draw(cx);
    let list = bounds("settings-list", cx);
    assert!(list.size.height < gpui::px(168.), "{list:?}");
    let selected_key = |cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| {
            match &editor.rows()[editor.selected().unwrap()] {
                Row::Setting(ix) => editor.model().unwrap().entries()[*ix].key_path(),
                Row::Group(_) => unreachable!(),
            }
        })
    };
    let assert_shown = |cx: &mut VisualTestContext| {
        draw(cx);
        let key = selected_key(cx);
        let row = bounds(&format!("settings-row-{key}"), cx);
        assert!(within(row, list), "{key}: {row:?} in {list:?}");
    };
    cx.simulate_keystrokes("down");
    assert_shown(cx);
    for _ in 0..5 {
        cx.simulate_keystrokes("down");
        assert_shown(cx);
    }
    for _ in 0..5 {
        cx.simulate_keystrokes("up");
        assert_shown(cx);
    }
    let first = editor.read_with(cx, |editor, _| {
        editor
            .rows()
            .iter()
            .position(|row| matches!(row, Row::Setting(_)))
    });
    assert_eq!(editor.read_with(cx, |editor, _| editor.selected()), first);
}

/// Activating the tab, or focus coming back into it, reads the files
/// again: an edit the settings reload doesn't report, as setting a key to
/// its default, shows (the Known limits' recovery).
#[gpui::test]
fn activating_the_tab_rereads(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    fs::write(dirs.root.join("a.txt"), "alpha\n").unwrap();
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let editor = open_settings(&workbench, cx);
    let key = "editor.tab_size";
    assert_eq!(shown(&editor, key, cx), ("4".to_owned(), false));
    let file = dirs.root.join("a.txt");
    workbench.update_in(cx, |workbench, window, cx| {
        workbench
            .pane()
            .clone()
            .update(cx, |pane, cx| pane.open(file, window, cx))
    });
    draw(cx);
    assert!(!search_focused(&editor, cx));

    // The default, set explicitly: the effective settings don't change.
    // Its tab, activated as a click on it does, shows it.
    dirs.write_user("[editor]\ntab_size = 4\n");
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let ix = pane.read_with(cx, |pane, _| {
        (pane.tabs().iter())
            .position(|tab| tab.item().downcast_ref::<SettingsEditor>().is_some())
            .unwrap()
    });
    pane.update_in(cx, |pane, window, cx| pane.activate(ix, window, cx));
    draw(cx);
    assert!(search_focused(&editor, cx));
    assert_eq!(shown(&editor, key, cx), ("4".to_owned(), true));

    // Re-rooted at a folder no watcher watches: an edit to its settings
    // shows once focus comes back.
    let other = dirs.root.parent().unwrap().join("other");
    fs::create_dir_all(other.join(".fastedit")).unwrap();
    workbench.update_in(cx, |workbench, window, cx| {
        workbench.set_root(other.clone(), window, cx)
    });
    draw(cx);
    set_scope(&editor, Scope::Workspace, cx);
    assert_eq!(shown(&editor, key, cx), ("4".to_owned(), false));
    // The tree takes focus.
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    cx.update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    draw(cx);
    assert!(!search_focused(&editor, cx));
    let project = fe_config::project_settings_path(&other);
    fs::write(project, "[editor]\ntab_size = 6\n").unwrap();
    run("workbench.open_settings_ui", cx);
    assert_eq!(shown(&editor, key, cx), ("6".to_owned(), true));
}

/// Up to a group's first setting after the wheel scrolled that group out
/// of view shows the setting, and its heading above it.
#[gpui::test]
fn up_after_scrolling_away_shows_the_setting(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let editor = open_settings(&workbench, cx);
    let list = bounds("settings-list", cx);
    let fits = (f32::from(list.size.height) / 96.) as usize;
    // A heading below the first screen with two settings under it.
    let (rows, heading) = editor.read_with(cx, |editor, _| {
        let rows = editor.rows().to_vec();
        let heading = (fits + 1..rows.len() - 2).find(|&ix| {
            matches!(rows[ix], Row::Group(_))
                && matches!(rows[ix + 1], Row::Setting(_))
                && matches!(rows[ix + 2], Row::Setting(_))
        });
        (rows, heading.expect("a group below the first screen"))
    });
    let downs = (rows[..=heading + 2].iter())
        .filter(|row| matches!(row, Row::Setting(_)))
        .count();
    for _ in 0..downs {
        cx.simulate_keystrokes("down");
    }
    draw(cx);
    assert_eq!(editor.read_with(cx, |e, _| e.selected()), Some(heading + 2));
    // The wheel scrolls back to the top: the group is below the view.
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: list.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(gpui::px(0.), gpui::px(100_000.))),
        ..Default::default()
    });
    draw(cx);
    let key = |ix: usize, cx: &mut VisualTestContext| {
        editor.read_with(cx, |editor, _| match &editor.rows()[ix] {
            Row::Setting(entry) => editor.model().unwrap().entries()[*entry].key_path(),
            Row::Group(title) => title.clone(),
        })
    };
    let second = key(heading + 2, cx);
    assert!(
        !drawn(&format!("settings-row-{second}"), cx),
        "scrolled away"
    );

    cx.simulate_keystrokes("up");
    draw(cx);
    assert_eq!(editor.read_with(cx, |e, _| e.selected()), Some(heading + 1));
    let first = key(heading + 1, cx);
    let row = bounds(&format!("settings-row-{first}"), cx);
    assert!(within(row, list), "{row:?} in {list:?}");
    let title = key(heading, cx);
    let group = bounds(&format!("settings-group-{title}"), cx);
    assert!(within(group, list), "{group:?} in {list:?}");
}

/// Kiln's config service on the user's settings file in `config` and
/// `root`'s, as the host starts it, with no watcher: the sender stands in
/// for it.
fn start_service(cx: &mut TestAppContext, config: &Path, root: &Path) -> BatchSender {
    let user = config.join(fe_config::paths::SETTINGS_FILE);
    let project = fe_config::project_settings_path(root);
    let store = fe_config::SettingsStore::load(Some(user), Some(project));
    let (sender, batches) = config::batch_channel();
    cx.update(|cx| {
        cx.set_global(ConfigDirOverride(Some(config.to_owned())));
        ConfigService::new()
            .settings(store.into_inner())
            .start_with(batches, cx)
            .detach();
    });
    sender
}

/// A window of a workbench on `root`, drawn, beside any others.
fn window_on(cx: &mut TestAppContext, root: &Path) -> (Entity<Workbench>, VisualTestContext) {
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        let theme = fe_gpui::EditorTheme::default_dark(cx);
        Workbench::new(
            launch,
            theme,
            fe_gpui::EditorSettings::default(),
            window,
            cx,
        )
    });
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let workbench = window.root(cx).unwrap();
    vcx.update(|window, cx| {
        window.activate_window();
        window.focus(&workbench.focus_handle(cx), cx);
    });
    draw(&mut vcx);
    (workbench, vcx)
}

/// A folder `name` beside the fixture's root, with a `.fastedit` folder.
fn another_root(dirs: &Dirs, name: &str) -> PathBuf {
    let root = dirs.root.with_file_name(name);
    fs::create_dir_all(root.join(".fastedit")).unwrap();
    root
}

#[gpui::test]
fn external_edit_with_unchanged_effective_values_refreshes_the_tab(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    let (workbench, cx) = workbench(cx, Some(&dirs.root), &dirs.config);
    let sender = start_service(cx, &dirs.config, &dirs.root);
    let editor = open_settings(&workbench, cx);
    let key = "editor.tab_size";
    search("tab size", cx);
    assert_eq!(shown(&editor, key, cx), ("4".to_owned(), false));
    let settings = cx.update(|_, cx| config::settings::<Settings>(cx));

    // Set to its default: the file changed, the settings didn't.
    dirs.write_user("[editor]\ntab_size = 4\n");
    sender.send(vec![dirs.config.join("settings.toml")]);
    draw(cx);
    let now = cx.update(|_, cx| config::settings::<Settings>(cx));
    assert!(Arc::ptr_eq(&settings, &now), "no settings reload");
    assert_eq!(shown(&editor, key, cx), ("4".to_owned(), true));
    assert!(drawn(&format!("settings-modified-{key}"), cx));

    // A comment alone, in the workspace's file.
    set_scope(&editor, Scope::Workspace, cx);
    assert_eq!(shown(&editor, key, cx), ("4".to_owned(), false));
    dirs.write_workspace("# tabs\n[editor]\ntab_size = 4\n");
    sender.send(vec![fe_config::project_settings_path(&dirs.root)]);
    draw(cx);
    assert_eq!(shown(&editor, key, cx), ("4".to_owned(), true));

    // A batch naming neither file reads nothing again.
    dirs.write_user("");
    sender.send(vec![dirs.config.join("keymap.toml")]);
    draw(cx);
    assert_eq!(shown(&editor, key, cx), ("4".to_owned(), true));
}

#[gpui::test]
fn workspace_edit_under_a_second_windows_root_refreshes_the_tab(cx: &mut TestAppContext) {
    let dirs = Dirs::new();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    // The service loads the launch root's settings, not the second's.
    let sender = start_service(cx, &dirs.config, &dirs.root);
    let (other, third) = (another_root(&dirs, "other"), another_root(&dirs, "third"));
    let project = |root: &Path| fe_config::project_settings_path(root);
    let (_first, _first_cx) = window_on(cx, &dirs.root);
    let (second, mut second_cx) = window_on(cx, &other);
    let watched = |cx: &mut TestAppContext| cx.update(|cx| config::watched_files(cx));
    assert_eq!(watched(cx), [project(&dirs.root), project(&other)]);

    let cx_ = &mut second_cx;
    let editor = open_settings(&second, cx_);
    set_scope(&editor, Scope::Workspace, cx_);
    let key = "editor.tab_size";
    search("tab size", cx_);
    assert_eq!(shown(&editor, key, cx_), ("4".to_owned(), false));

    // What the watcher sends for the second root's file.
    fs::write(project(&other), "[editor]\ntab_size = 2\n").unwrap();
    sender.send(vec![project(&other)]);
    draw(cx_);
    assert_eq!(shown(&editor, key, cx_), ("2".to_owned(), true));

    // Re-rooted, the window's new root is watched in place of the old.
    second.update_in(cx_, |workbench, window, cx| {
        workbench.set_root(third.clone(), window, cx)
    });
    draw(cx_);
    assert_eq!(watched(cx), [project(&dirs.root), project(&third)]);
    assert_eq!(shown(&editor, key, cx_), ("4".to_owned(), false));
    fs::write(project(&third), "[editor]\ntab_size = 8\n").unwrap();
    sender.send(vec![project(&third)]);
    draw(cx_);
    assert_eq!(shown(&editor, key, cx_), ("8".to_owned(), true));

    // Closed, the window's root isn't watched.
    drop(editor);
    drop(second);
    cx_.update(|window, _| window.remove_window());
    cx.run_until_parked();
    assert_eq!(watched(cx), [project(&dirs.root)]);
}
