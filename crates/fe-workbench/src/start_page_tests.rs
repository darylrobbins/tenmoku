//! The Start page: its model from a recent list, the dashboard's keys with
//! vim on, when the empty pane shows it, the setup wizard over it, and
//! `workbench.start_page` off.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use fe_command::{Invocation, Value};
use fe_config::{Settings, recent::RecentList};
use fe_gpui::{EditorSettings, EditorTheme, SettingsEvent, commands::RunCommand, set_vim_enabled};
use fe_keymap::CommandCatalog as _;
use gpui::{Entity, Focusable as _, Keystroke, TestAppContext, VisualTestContext};

use crate::{
    Launch, Workbench,
    start_page::StartPageView,
    start_page_model::{
        MAX_RECENT, OPEN_RECENT_FILE, OPEN_RECENT_FOLDER, Options, SectionKind, StartPage,
        invocation_path,
    },
    workbench::recent_picker::set_recent_file,
};

/// `count` folders and `count` files under `dir`, made on disk.
fn entries(dir: &Path, count: usize) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let folders: Vec<PathBuf> = (0..count)
        .map(|ix| dir.join(format!("folder{ix}")))
        .collect();
    let files: Vec<PathBuf> = (0..count)
        .map(|ix| dir.join(format!("file{ix}.txt")))
        .collect();
    for folder in &folders {
        fs::create_dir(folder).unwrap();
    }
    for file in &files {
        fs::write(file, "text\n").unwrap();
    }
    (folders, files)
}

fn commands(page: &StartPage, kind: SectionKind) -> Vec<String> {
    let section = page.section(kind).expect("the section is listed");
    (section.rows.iter())
        .map(|row| row.invocation.command.as_str().to_owned())
        .collect()
}

fn paths(page: &StartPage, kind: SectionKind) -> Vec<PathBuf> {
    let section = page.section(kind).expect("the section is listed");
    (section.rows.iter())
        .map(|row| invocation_path(&row.invocation).expect("a recent row has a path"))
        .collect()
}

fn keys(page: &StartPage) -> Vec<(String, char)> {
    (page.rows())
        .filter_map(|row| Some((row.label.clone(), row.key?)))
        .collect()
}

#[test]
fn model_lists_start_actions_and_existing_recents() {
    let dir = tempfile::tempdir().unwrap();
    let (folders, files) = entries(dir.path(), 7);
    fs::remove_dir(&folders[1]).unwrap();
    let recent = RecentList {
        folders: folders.clone(),
        files: files.clone(),
    };
    let home = dir.path();
    let page = StartPage::from(
        &recent,
        Options {
            vim: false,
            clone: false,
            home: Some(home),
        },
    );

    let kinds: Vec<SectionKind> = page.sections.iter().map(|section| section.kind).collect();
    assert_eq!(
        kinds,
        [
            SectionKind::Start,
            SectionKind::RecentFolders,
            SectionKind::RecentFiles,
            SectionKind::Settings
        ]
    );
    assert_eq!(
        commands(&page, SectionKind::Start),
        [
            "workbench.new_file",
            "workbench.open_file",
            "workbench.open_folder"
        ]
    );
    // The five most recent that still exist, the deleted one left out.
    let existing: Vec<PathBuf> = [0, 2, 3, 4, 5].map(|ix| folders[ix].clone()).into();
    assert_eq!(paths(&page, SectionKind::RecentFolders), existing);
    assert_eq!(
        commands(&page, SectionKind::RecentFolders),
        [OPEN_RECENT_FOLDER; MAX_RECENT]
    );
    assert_eq!(paths(&page, SectionKind::RecentFiles), files[..MAX_RECENT]);
    assert_eq!(
        commands(&page, SectionKind::RecentFiles),
        [OPEN_RECENT_FILE; MAX_RECENT]
    );
    assert_eq!(
        commands(&page, SectionKind::Settings),
        [
            "workbench.open_settings_ui",
            "workbench.open_keyboard_shortcuts"
        ]
    );
    // A recent row says its name, then where it is from `~`.
    let first = &page.section(SectionKind::RecentFiles).unwrap().rows[0];
    assert_eq!(first.label, "file0.txt");
    assert_eq!(first.detail.as_deref(), Some("~"));
    let folder = &page.section(SectionKind::RecentFolders).unwrap().rows[1];
    assert_eq!(folder.label, "folder2");
    assert!(keys(&page).is_empty(), "no letter keys with vim off");

    // Clone Repository only where a clone command is registered.
    let clone = StartPage::from(
        &recent,
        Options {
            vim: false,
            clone: true,
            home: None,
        },
    );
    assert_eq!(
        commands(&clone, SectionKind::Start)
            .last()
            .map(String::as_str),
        Some("git.clone")
    );

    // Nothing recent: no recent sections.
    let empty = StartPage::from(&RecentList::default(), Options::default());
    let kinds: Vec<SectionKind> = empty.sections.iter().map(|section| section.kind).collect();
    assert_eq!(kinds, [SectionKind::Start, SectionKind::Settings]);
}

/// Every command a row runs is one the app knows, with vim on (which lists
/// the most): the workbench's, the app's, and the page's own.
#[test]
fn every_row_runs_a_known_command() {
    let dir = tempfile::tempdir().unwrap();
    let (folders, files) = entries(dir.path(), 1);
    let recent = RecentList { folders, files };
    let page = StartPage::from(
        &recent,
        Options {
            vim: true,
            clone: false,
            home: None,
        },
    );
    let catalog = crate::command_catalog();
    for row in page.rows() {
        let command = &row.invocation.command;
        // A typed command's catalog takes no args: the page's own read
        // theirs from the invocation.
        let known = command.as_str() == "app.quit" || catalog.check(command, &Value::Null).is_ok();
        assert!(known, "{} runs unknown {command}", row.label);
    }
}

/// A rootless window with no tab, active and drawn, vim on or off, keeping
/// the recent list in `recent`.
fn window(
    cx: &mut TestAppContext,
    vim: bool,
    recent: &Path,
) -> (Entity<Workbench>, VisualTestContext) {
    let recent = recent.to_owned();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        set_vim_enabled(cx, vim);
        set_recent_file(cx, recent);
    });
    let launch = Launch {
        root: None,
        file: None,
    };
    let window = cx.add_window(move |window, cx| {
        window.activate_window();
        let theme = EditorTheme::default_dark(cx);
        Workbench::new(launch, theme, EditorSettings::default(), window, cx)
    });
    let workbench = window.root(cx).unwrap();
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    let cx = &mut vcx;
    cx.update(|window, cx| window.focus(&workbench.focus_handle(cx), cx));
    draw(cx);
    (workbench, vcx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

fn page(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<StartPageView> {
    workbench.read_with(cx, |workbench, _| workbench.start_page().clone())
}

/// Whether the active pane shows the Start page, by its model and by what
/// the last frame drew.
fn shows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    let shown = pane.read_with(cx, |pane, _| pane.shown_start_page().is_some());
    let drawn = cx.debug_bounds("start-page").is_some();
    assert_eq!(shown, drawn, "the pane draws the page it says it shows");
    shown
}

fn page_focused(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let page = page(workbench, cx);
    cx.update(|window, cx| page.read(cx).focus_handle().is_focused(window))
}

fn tab_count(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> usize {
    workbench.read_with(cx, |workbench, cx| {
        (workbench.panes(cx).iter())
            .map(|pane| pane.read(cx).tabs().len())
            .sum()
    })
}

fn ran(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let page = page(workbench, cx);
    page.read_with(cx, |page, _| {
        (page.ran.iter())
            .map(|run| run.command.as_str().to_owned())
            .collect()
    })
}

/// Types `key` where focus is: whether something took it.
fn press(key: &str, cx: &mut VisualTestContext) -> bool {
    let taken =
        cx.update(|window, cx| window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx));
    draw(cx);
    taken
}

fn run(id: &str, cx: &mut VisualTestContext) {
    let run = RunCommand(Invocation::of(id).unwrap());
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

#[gpui::test]
fn vim_adds_the_dashboard_keys(cx: &mut TestAppContext) {
    let model = StartPage::from(
        &RecentList::default(),
        Options {
            vim: true,
            clone: false,
            home: None,
        },
    );
    assert_eq!(
        keys(&model),
        [
            ("New File".to_owned(), 'n'),
            ("Open File…".to_owned(), 'f'),
            ("Open Folder…".to_owned(), 'p'),
            ("Find Text".to_owned(), 'g'),
            ("Recent Files…".to_owned(), 'r'),
            ("Open Settings".to_owned(), 's'),
            ("Quit".to_owned(), 'q'),
        ]
    );
    let row = |key| model.row(model.row_for_key(key).unwrap()).unwrap();
    assert_eq!(row('n').invocation.command.as_str(), "workbench.new_file");
    assert_eq!(row('q').invocation.command.as_str(), "app.quit");
    // The key's letter in the label, underlined.
    assert_eq!(row('n').key_in_label(), Some(0..1));
    assert_eq!(row('p').key_in_label(), Some(1..2));
    assert_eq!(row('s').key_in_label(), Some(5..6));

    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = window(cx, true, &dir.path().join("recent.toml"));
    let cx = &mut vcx;
    assert!(shows(&workbench, cx));
    assert!(page_focused(&workbench, cx));
    // Down and up move the selection; `j` and `k` too.
    let selected =
        |cx: &mut VisualTestContext| page(&workbench, cx).read_with(cx, |p, _| p.selected());
    assert!(press("down", cx));
    assert!(press("j", cx));
    assert_eq!(selected(cx), 2);
    assert!(press("k", cx));
    assert_eq!(selected(cx), 1);
    // A letter with no row goes on.
    assert!(!press("z", cx));
    assert!(ran(&workbench, cx).is_empty());

    // `n` runs New File: an untitled tab opens, and the page goes.
    assert!(press("n", cx), "the page takes its key");
    assert_eq!(ran(&workbench, cx), ["workbench.new_file"]);
    assert_eq!(tab_count(&workbench, cx), 1);
    assert!(!shows(&workbench, cx));
    // The editor has it now: the page's key types there.
    assert!(!page_focused(&workbench, cx));
    press("n", cx);
    assert_eq!(ran(&workbench, cx), ["workbench.new_file"]);
}

#[gpui::test]
fn turning_vim_off_takes_the_keys_away(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = window(cx, true, &dir.path().join("recent.toml"));
    let cx = &mut vcx;
    let model =
        |cx: &mut VisualTestContext| page(&workbench, cx).read_with(cx, |p, _| p.page().clone());
    assert!(!keys(&model(cx)).is_empty());
    cx.update(|_, cx| set_vim_enabled(cx, false));
    // Before the page is drawn again, a letter already goes on.
    assert!(!press("n", cx));
    let event = settings_event(true, true);
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    draw(cx);
    draw(cx);
    assert!(keys(&model(cx)).is_empty(), "read again without the keys");
    assert!(!press("q", cx));
    assert!(ran(&workbench, cx).is_empty());
    assert!(shows(&workbench, cx));
}

#[gpui::test]
fn without_vim_letters_go_on(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = window(cx, false, &dir.path().join("recent.toml"));
    let cx = &mut vcx;
    assert!(shows(&workbench, cx));
    assert!(page_focused(&workbench, cx));
    let model = page(&workbench, cx).read_with(cx, |page, _| page.page().clone());
    assert!(keys(&model).is_empty());
    for key in ["n", "f", "p", "g", "r", "s", "q", "j"] {
        assert!(!press(key, cx), "{key} isn't taken");
    }
    assert!(ran(&workbench, cx).is_empty());
    assert_eq!(tab_count(&workbench, cx), 0);
    // Enter runs the selected row, New File.
    assert!(press("enter", cx));
    assert_eq!(ran(&workbench, cx), ["workbench.new_file"]);
    assert_eq!(tab_count(&workbench, cx), 1);
}

#[gpui::test]
fn shows_only_with_no_tabs(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let recent_file = dir.path().join("recent.toml");
    let (_, files) = entries(dir.path(), 2);
    let (workbench, mut vcx) = window(cx, false, &recent_file);
    let cx = &mut vcx;
    assert!(
        shows(&workbench, cx),
        "a rootless window with no tab shows it"
    );
    assert!(page_focused(&workbench, cx));
    let loaded = |cx: &mut VisualTestContext| {
        let page = page(&workbench, cx);
        page.read_with(cx, |page, _| page.is_loaded().then(|| page.page().clone()))
    };
    let first = loaded(cx).expect("read");
    assert!(
        first.section(SectionKind::RecentFiles).is_none(),
        "nothing recent yet"
    );

    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    for (ix, file) in files.iter().enumerate() {
        cx.update(|window, cx| pane.update(cx, |pane, cx| pane.open(file.clone(), window, cx)));
        draw(cx);
        assert_eq!(tab_count(&workbench, cx), ix + 1);
        assert!(!shows(&workbench, cx), "never while a tab is open");
    }
    // A split's pane is empty only for a moment: the tab comes with it.
    run("pane.split_right", cx);
    assert!(tab_count(&workbench, cx) > 0);
    assert!(!shows(&workbench, cx));
    for _ in 0..8 {
        if tab_count(&workbench, cx) == 0 {
            break;
        }
        assert!(!shows(&workbench, cx), "never while a tab is open");
        run("workbench.close_tab", cx);
    }
    assert_eq!(tab_count(&workbench, cx), 0);
    assert!(shows(&workbench, cx), "back once the last tab closes");
    assert!(page_focused(&workbench, cx));
    // Read again: the files just closed are recent now, most recent first.
    let again = loaded(cx).expect("read");
    let section = again
        .section(SectionKind::RecentFiles)
        .expect("recent files");
    let mut listed: Vec<PathBuf> = (section.rows.iter())
        .filter_map(|row| invocation_path(&row.invocation))
        .collect();
    listed.sort();
    assert_eq!(listed, files);

    // A recent file's row opens it, and the page goes.
    let row = (again.rows())
        .position(|row| invocation_path(&row.invocation).as_ref() == Some(&files[0]))
        .unwrap();
    for _ in 0..row {
        press("down", cx);
    }
    assert!(press("enter", cx));
    assert_eq!(ran(&workbench, cx), [OPEN_RECENT_FILE]);
    assert_eq!(tab_count(&workbench, cx), 1);
    let open = pane.read_with(cx, |pane, _| pane.tabs()[0].path.clone());
    assert_eq!(open.as_ref(), Some(&files[0]));
    assert!(!shows(&workbench, cx));
}

#[gpui::test]
fn a_recent_folder_roots_the_window(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let recent_file = dir.path().join("recent.toml");
    let (folders, _) = entries(dir.path(), 1);
    let folder = fs::canonicalize(&folders[0]).unwrap();
    let list = RecentList {
        folders: vec![folder.clone()],
        files: Vec::new(),
    };
    list.save(&recent_file).unwrap();
    let (workbench, mut vcx) = window(cx, false, &recent_file);
    let cx = &mut vcx;
    let model = page(&workbench, cx).read_with(cx, |page, _| page.page().clone());
    let row = (model.rows())
        .position(|row| row.invocation.command.as_str() == OPEN_RECENT_FOLDER)
        .expect("the folder is listed");
    for _ in 0..row {
        press("down", cx);
    }
    assert!(press("enter", cx));
    let root = workbench.read_with(cx, |workbench, _| workbench.root().map(Path::to_owned));
    assert_eq!(root, Some(folder));
    // Still no tab: the page stays.
    assert!(shows(&workbench, cx));
}

#[gpui::test]
fn setup_wizard_keeps_the_page_inert(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = window(cx, true, &dir.path().join("recent.toml"));
    let cx = &mut vcx;
    assert!(shows(&workbench, cx));
    cx.update(|window, cx| workbench.update(cx, |workbench, cx| workbench.show_setup(window, cx)));
    draw(cx);
    let setup = workbench.read_with(cx, |workbench, _| workbench.setup().cloned());
    let setup = setup.expect("the sheet shows");
    assert!(shows(&workbench, cx), "under the sheet");
    assert!(!page_focused(&workbench, cx));
    assert!(cx.update(|window, cx| setup.read(cx).focus_handle().is_focused(window)));

    // Focus put on the page goes back to the sheet.
    let focus = page(&workbench, cx).read_with(cx, |page, _| page.focus_handle().clone());
    cx.update(|window, cx| window.focus(&focus, cx));
    draw(cx);
    assert!(!page_focused(&workbench, cx));

    // Its keys do nothing.
    for key in ["n", "q", "enter"] {
        press(key, cx);
    }
    assert!(ran(&workbench, cx).is_empty());
    assert_eq!(tab_count(&workbench, cx), 0);
    assert!(workbench.read_with(cx, |workbench, _| workbench.setup().is_some()));

    // Nor does a row clicked under the sheet: the sheet takes the mouse.
    if let Some(bounds) = cx.debug_bounds("start-page-row:New File") {
        cx.simulate_click(bounds.center(), gpui::Modifiers::none());
        draw(cx);
    }
    assert!(ran(&workbench, cx).is_empty());
    assert_eq!(tab_count(&workbench, cx), 0);

    // Skipped, the sheet goes and the page has focus. Held here, the sheet
    // would take focus back as it goes.
    drop(setup);
    press("escape", cx);
    assert!(workbench.read_with(cx, |workbench, _| workbench.setup().is_none()));
    assert!(shows(&workbench, cx));
    assert!(page_focused(&workbench, cx));
    assert!(press("n", cx));
    assert_eq!(ran(&workbench, cx), ["workbench.new_file"]);
    assert_eq!(tab_count(&workbench, cx), 1);
}

#[gpui::test]
fn a_click_runs_a_row(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = window(cx, false, &dir.path().join("recent.toml"));
    let cx = &mut vcx;
    let bounds = cx.debug_bounds("start-page-row:New File").expect("drawn");
    cx.simulate_click(bounds.center(), gpui::Modifiers::none());
    draw(cx);
    assert_eq!(ran(&workbench, cx), ["workbench.new_file"]);
    assert_eq!(tab_count(&workbench, cx), 1);
    assert!(!shows(&workbench, cx));
}

fn settings_event(start_page: bool, previous: bool) -> SettingsEvent {
    let with = |on| {
        let mut settings = Settings::default();
        settings.workbench.start_page = on;
        Arc::new(settings)
    };
    SettingsEvent {
        settings: with(start_page),
        previous: with(previous),
        diagnostics: Vec::new(),
    }
}

#[gpui::test]
fn setting_off_keeps_the_plain_empty_state(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (workbench, mut vcx) = window(cx, false, &dir.path().join("recent.toml"));
    let cx = &mut vcx;
    assert!(shows(&workbench, cx));
    assert!(cx.debug_bounds("pane-message:No open files").is_none());

    let event = settings_event(false, true);
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    draw(cx);
    assert!(!shows(&workbench, cx));
    assert!(cx.debug_bounds("pane-message:No open files").is_some());
    // The pane has its focus back, and keys go on.
    let pane = workbench.read_with(cx, |workbench, _| workbench.pane().clone());
    assert!(cx.update(|window, cx| pane.focus_handle(cx).is_focused(window)));
    assert!(!press("enter", cx));
    assert!(ran(&workbench, cx).is_empty());

    // On again, it's back.
    let event = settings_event(true, false);
    workbench.update(cx, |workbench, cx| workbench.reload_settings(&event, cx));
    draw(cx);
    assert!(shows(&workbench, cx));
    assert!(cx.debug_bounds("pane-message:No open files").is_none());
}
