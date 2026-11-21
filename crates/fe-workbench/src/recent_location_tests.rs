//! Remote folders in Open Recent and on the Start page: shown as
//! `app — neon`, local entries as before, and choosing one says it can't
//! be opened yet.

use std::{
    fs,
    path::{Path, PathBuf},
};

use fe_config::recent::{self, Entry, ProjectLocation, RecentList};
use fe_gpui::{EditorSettings, EditorTheme};
use gpui::{Entity, Focusable as _, Keystroke, TestAppContext, VisualTestContext};

use super::{remote_label, set_recent_file, tilde};
use crate::{
    Launch, Workbench,
    start_page_model::{OPEN_RECENT_FOLDER, Options, SectionKind, StartPage, invocation_path},
};

fn location(text: &str) -> ProjectLocation {
    ProjectLocation::parse(text).unwrap()
}

/// A rootless window keeping the recent list in `file`, active and drawn.
fn window(cx: &mut TestAppContext, file: &Path) -> (Entity<Workbench>, VisualTestContext) {
    let file = file.to_owned();
    cx.update(|cx| {
        fe_gpui::init(cx);
        crate::init(cx);
        set_recent_file(cx, file);
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

fn press(key: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx));
    draw(cx);
}

fn open_recent(cx: &mut VisualTestContext) {
    let run =
        fe_gpui::commands::RunCommand(fe_command::Invocation::of("workbench.open_recent").unwrap());
    cx.update(|window, cx| window.dispatch_action(Box::new(run), cx));
    draw(cx);
}

fn rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
    workbench.read_with(cx, |workbench, cx| {
        let picker = workbench.recent_picker().read(cx);
        assert!(picker.delegate.loaded(), "the list is read");
        picker.delegate.labels()
    })
}

/// The toasts shown: each message and detail.
fn toasts(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Vec<(String, Option<String>)> {
    let layer = workbench.read_with(cx, |workbench, _| workbench.toasts().clone());
    layer.read_with(cx, |layer, _| {
        (layer.queue().shown().iter())
            .map(|entry| {
                let message = kiln_l10n::strip_isolates(&entry.toast.message).into_owned();
                (message, entry.toast.detail.clone())
            })
            .collect()
    })
}

fn root(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<PathBuf> {
    workbench.read_with(cx, |workbench, _| workbench.root().map(Path::to_owned))
}

/// What choosing the remote folder `ssh://neon/home/me/src/app` shows.
fn cant_open_neon_app() -> (String, Option<String>) {
    (
        "Can't open folder ssh://neon/home/me/src/app".to_owned(),
        Some("remote folders aren't supported yet".to_owned()),
    )
}

#[test]
fn a_remote_folder_is_labelled_with_its_name_and_host() {
    assert_eq!(
        remote_label(&location("ssh://neon/home/me/src/app")).as_deref(),
        Some("app — neon")
    );
    assert_eq!(
        remote_label(&location("ssh://me@box:2222/srv/app/")).as_deref(),
        Some("app — me@box:2222")
    );
    assert_eq!(
        remote_label(&location("ssh://neon/")).as_deref(),
        Some("/ — neon")
    );
    assert_eq!(
        remote_label(&ProjectLocation::Local("/srv/app".into())),
        None
    );
}

#[gpui::test]
fn the_picker_shows_a_remote_folder_beside_local_ones(cx: &mut TestAppContext) {
    let (state, dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let file = state.path().join("recent.toml");
    let local = dir.path().join("app");
    fs::create_dir(&local).unwrap();
    let remote = location("ssh://neon/home/me/src/app");
    recent::record(&file, Entry::Folder(&local)).unwrap();
    recent::record(&file, Entry::Location(&remote)).unwrap();
    let (workbench, mut vcx) = window(cx, &file);
    let cx = &mut vcx;
    open_recent(cx);
    let home = std::env::home_dir();
    assert_eq!(
        rows(&workbench, cx),
        [
            ("app — neon".to_owned(), true),
            (tilde(&local, home.as_deref()), true),
        ]
    );
}

#[gpui::test]
fn choosing_a_remote_folder_in_the_picker_says_it_cant_be_opened(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let file = state.path().join("recent.toml");
    recent::record(
        &file,
        Entry::Location(&location("ssh://neon/home/me/src/app")),
    )
    .unwrap();
    let (workbench, mut vcx) = window(cx, &file);
    let cx = &mut vcx;
    open_recent(cx);
    press("enter", cx);
    assert_eq!(root(&workbench, cx), None, "the window keeps its root");
    assert_eq!(toasts(&workbench, cx), [cant_open_neon_app()]);
}

#[test]
fn the_start_page_shows_a_remote_folder_as_its_name_and_host() {
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("lib");
    fs::create_dir(&local).unwrap();
    let recent = RecentList {
        folders: vec![
            location("ssh://neon/home/me/src/app"),
            ProjectLocation::Local(local.clone()),
        ],
        files: Vec::new(),
    };
    let home = dir.path();
    let options = Options {
        home: Some(home),
        ..Options::default()
    };
    let page = StartPage::from(&recent, options);
    let section = page.section(SectionKind::RecentFolders).expect("listed");
    let shown: Vec<_> = (section.rows.iter())
        .map(|row| (row.label.resolve().to_string(), row.detail.clone()))
        .collect();
    assert_eq!(
        shown,
        [
            ("app — neon".to_owned(), Some("/home/me/src".to_owned())),
            ("lib".to_owned(), Some("~".to_owned())),
        ]
    );
    let commands: Vec<_> = (section.rows.iter())
        .map(|row| row.invocation.command.as_str())
        .collect();
    assert_eq!(commands, [OPEN_RECENT_FOLDER; 2]);
    assert_eq!(invocation_path(&section.rows[0].invocation), None);
    assert_eq!(invocation_path(&section.rows[1].invocation), Some(local));
}

#[gpui::test]
fn choosing_a_remote_folder_on_the_start_page_says_it_cant_be_opened(cx: &mut TestAppContext) {
    let state = tempfile::tempdir().unwrap();
    let file = state.path().join("recent.toml");
    recent::record(
        &file,
        Entry::Location(&location("ssh://neon/home/me/src/app")),
    )
    .unwrap();
    let (workbench, mut vcx) = window(cx, &file);
    let cx = &mut vcx;
    let page = workbench.read_with(cx, |workbench, _| workbench.start_page().clone());
    let model = page.read_with(cx, |page, _| page.page().clone());
    let row = (model.rows())
        .position(|row| row.invocation.command.as_str() == OPEN_RECENT_FOLDER)
        .expect("the remote folder is listed");
    for _ in 0..row {
        press("down", cx);
    }
    press("enter", cx);
    assert_eq!(root(&workbench, cx), None, "the window keeps its root");
    assert_eq!(toasts(&workbench, cx), [cant_open_neon_app()]);
}
