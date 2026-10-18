//! Gitignored entries in the project tree: listed greyed by default, hidden
//! by `tree.show_ignored = false`, and `.git` never listed.

use std::{fs, path::Path, sync::Arc};

use fe_config::{Settings, SettingsEvent, SettingsStore};
use fe_gpui::{EditorSettings, EditorTheme};
use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln_platform::config::{self, ConfigService};

use crate::{Launch, Workbench};

/// A folder with `.git`, `.idea` and `target` (the last two ignored) and `a.rs`.
fn folder() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    for ignored in [".idea", "target"] {
        fs::create_dir_all(root.join(ignored)).unwrap();
        fs::write(root.join(ignored).join("x"), "x\n").unwrap();
    }
    fs::write(root.join("a.rs"), "a\n").unwrap();
    fs::write(root.join(".gitignore"), ".idea\ntarget\n").unwrap();
    dir
}

/// The tree's rows, as `(name, ignored)`, in a workbench rooted at `root`
/// whose user settings file holds `settings`.
fn rows(root: &Path, settings: &str, cx: &mut TestAppContext) -> Vec<(String, bool)> {
    let (workbench, cx) = open(root, settings, cx);
    listed(&workbench, cx)
}

fn open<'a>(
    root: &Path,
    settings: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<Workbench>, &'a mut VisualTestContext) {
    let config = tempfile::tempdir().unwrap();
    let user = config.path().join("settings.toml");
    fs::write(&user, settings).unwrap();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
    });
    let store = SettingsStore::load(Some(user), None);
    let (_sender, batches) = config::batch_channel();
    let _service = cx.update(|cx| {
        ConfigService::new()
            .settings(store.into_inner())
            .start_with(batches, cx)
    });
    let launch = Launch {
        root: Some(root.to_owned()),
        file: None,
    };
    let (workbench, cx) = cx.add_window_view(move |window, cx| {
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    draw(cx);
    (workbench, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn listed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
    workbench.read_with(cx, |workbench, cx| {
        let panel = workbench.panel().read(cx);
        let tree = panel.tree().expect("open");
        (tree.rows(0..tree.row_count()))
            .map(|row| (row.name.to_string_lossy().into_owned(), row.ignored))
            .collect()
    })
}

#[gpui::test]
fn ignored_entries_are_listed_greyed_by_default(cx: &mut TestAppContext) {
    let dir = folder();
    let rows = rows(dir.path(), "", cx);
    assert!(rows.contains(&(".idea".to_owned(), true)), "{rows:?}");
    assert!(rows.contains(&("target".to_owned(), true)), "{rows:?}");
    assert!(rows.contains(&("a.rs".to_owned(), false)), "{rows:?}");
    assert!(rows.iter().all(|(name, _)| name != ".git"), "{rows:?}");
}

#[gpui::test]
fn turning_the_setting_off_hides_them(cx: &mut TestAppContext) {
    let dir = folder();
    let rows = rows(dir.path(), "[tree]\nshow_ignored = false\n", cx);
    let names: Vec<&str> = rows.iter().map(|(name, _)| name.as_str()).collect();
    assert!(names.contains(&"a.rs"), "{names:?}");
    for hidden in [".idea", "target", ".git"] {
        assert!(!names.contains(&hidden), "{hidden} in {names:?}");
    }
}

#[gpui::test]
fn a_windows_own_settings_decide_what_its_tree_lists(cx: &mut TestAppContext) {
    let dir = folder();
    let (workbench, cx) = open(dir.path(), "", cx);
    assert!(
        listed(&workbench, cx)
            .iter()
            .any(|(name, _)| name == "target")
    );

    // The window's project file turns it off, as the host's reload gives it.
    let mut off = Settings::default();
    off.tree.show_ignored = false;
    let reload = |settings: Settings, previous: Settings, cx: &mut VisualTestContext| {
        let event = SettingsEvent {
            settings: Arc::new(settings),
            previous: Arc::new(previous),
            diagnostics: Vec::new(),
        };
        workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
        draw(cx);
    };
    reload(off.clone(), Settings::default(), cx);
    let names: Vec<String> = listed(&workbench, cx)
        .into_iter()
        .map(|row| row.0)
        .collect();
    assert_eq!(names, [".gitignore", "a.rs"]);

    reload(Settings::default(), off, cx);
    assert!(
        listed(&workbench, cx)
            .iter()
            .any(|(name, _)| name == "target")
    );
}

#[gpui::test]
fn reopening_the_tree_forgets_the_old_trees_nodes(cx: &mut TestAppContext) {
    let dir = folder();
    let (workbench, cx) = open(dir.path(), "", cx);
    let panel = workbench.read_with(cx, |workbench, _| workbench.panel().clone());
    // Select and expand `target`, an ignored directory.
    panel.update(cx, |panel, cx| {
        let tree = panel.tree().expect("open");
        let id = (tree.rows(0..tree.row_count()))
            .find(|row| row.name == "target")
            .expect("a row")
            .id;
        panel.click(id, cx);
    });
    draw(cx);
    assert!(
        panel
            .read_with(cx, |panel, _| panel.selected_row())
            .is_some()
    );

    let mut off = Settings::default();
    off.tree.show_ignored = false;
    let event = SettingsEvent {
        settings: Arc::new(off),
        previous: Arc::new(Settings::default()),
        diagnostics: Vec::new(),
    };
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    draw(cx);
    assert_eq!(panel.read_with(cx, |panel, _| panel.selected_row()), None);
    let names: Vec<String> = listed(&workbench, cx)
        .into_iter()
        .map(|row| row.0)
        .collect();
    assert_eq!(names, [".gitignore", "a.rs"]);
}
