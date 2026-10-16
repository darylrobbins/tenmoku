// SPDX-License-Identifier: LicenseRef-Proprietary

//! The notes example headless: a note is added by its key, the File menu
//! and the palette, opening its tab, and the status counts them; the
//! sidebar toggles on its key and its rows open or activate their notes;
//! `[notes] font_size` reaches the open note's view; and notes and the
//! layout come back on a second start with the same paths.

#[path = "../examples/notes/note_view.rs"]
mod note_view;
#[path = "../examples/notes/notes.rs"]
mod notes;
#[path = "../examples/notes/sidebar.rs"]
mod sidebar;

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use gpui::{Entity, TestAppContext, VisualTestContext};
use kiln::{
    Contributions,
    core::{AppId, AppPaths, Os, PathStyle},
    model::{self, persist::DEBOUNCE},
    platform::{Capabilities, config::batch_channel},
    prelude::*,
    shell::{ItemPane, PaneView as _, Workspace, command_palette::COMMAND_PALETTE},
    testing::TestApp,
};

use note_view::NoteView;
use notes::Notes;

const ID: &str = "dev.kiln.Notes";

fn app() -> kiln::AppBuilder {
    kiln::app(ID, "Notes")
}

/// The active pane's tab titles, and the active one's index.
fn tabs(
    workspace: &Entity<Workspace<ItemPane>>,
    cx: &mut VisualTestContext,
) -> (Vec<String>, Option<usize>) {
    workspace.read_with(cx, |workspace, cx| {
        let pane = workspace.active_pane().read(cx).pane();
        let titles = pane
            .items()
            .iter()
            .map(|item| item.tab_title(cx).to_string());
        (titles.collect(), pane.active())
    })
}

/// What the status bar's sources say.
fn status(cx: &mut VisualTestContext) -> Vec<String> {
    cx.read(|cx| {
        let specs = Contributions::statuses(cx);
        specs
            .iter()
            .filter_map(|spec| (spec.item)(cx))
            .map(|item| item.text.to_string())
            .collect()
    })
}

fn active(workspace: &Entity<Workspace<ItemPane>>, cx: &mut VisualTestContext) -> Option<u64> {
    workspace.read_with(cx, |workspace, cx| workspace.active_state::<NoteView>(cx))
}

fn sidebar_shows(app: &mut TestApp) -> bool {
    app.cx().debug_bounds("note-row:1").is_some()
}

#[gpui::test]
fn a_note_is_added_by_its_key_the_file_menu_and_the_palette(cx: &mut TestAppContext) {
    let mut app = TestApp::open(app().restore_layout(false), cx);
    let workspace = app.workspace();
    assert_eq!(status(app.cx()), ["0 notes"]);

    app.press("secondary-n");
    assert_eq!(tabs(&workspace, app.cx()), (vec!["Note 1".into()], Some(0)));
    assert_eq!(status(app.cx()), ["1 note"]);

    app.menu("File", "New Note");
    assert_eq!(tabs(&workspace, app.cx()).0, ["Note 1", "Note 2"]);
    assert_eq!(status(app.cx()), ["2 notes"]);

    let shell = workspace.read_with(app.cx(), |workspace, _| workspace.shell().clone());
    app.press("secondary-shift-p");
    assert_eq!(
        shell.read_with(app.cx(), |shell, cx| shell.modal(cx)),
        Some(COMMAND_PALETTE)
    );
    app.cx().simulate_input("new note");
    app.press("enter");
    assert_eq!(
        tabs(&workspace, app.cx()),
        (
            vec!["Note 1".into(), "Note 2".into(), "Note 3".into()],
            Some(2)
        )
    );
    assert_eq!(status(app.cx()), ["3 notes"]);
}

#[gpui::test]
fn the_sidebar_toggles_on_its_key_and_a_row_opens_or_activates_its_note(cx: &mut TestAppContext) {
    let mut app = TestApp::open(app().restore_layout(false), cx);
    let workspace = app.workspace();
    app.press("secondary-n");
    app.press("secondary-n");
    assert_eq!(active(&workspace, app.cx()), Some(2));

    let shown = sidebar_shows(&mut app);
    app.press("secondary-b");
    assert_eq!(sidebar_shows(&mut app), !shown, "its key toggles it");
    app.press("secondary-b");
    assert_eq!(sidebar_shows(&mut app), shown);
    if !shown {
        app.press("secondary-b");
    }
    assert!(sidebar_shows(&mut app), "the sidebar lists its notes");

    // A row activates the note, its tab already open.
    let row = app.cx().debug_bounds("note-row:1").expect("a row");
    app.cx()
        .simulate_click(row.center(), gpui::Modifiers::none());
    app.draw();
    assert_eq!(active(&workspace, app.cx()), Some(1));
    assert_eq!(
        tabs(&workspace, app.cx()),
        (vec!["Note 1".into(), "Note 2".into()], Some(0))
    );

    // And opens the note whose tab was closed.
    workspace.update_in(app.cx(), |workspace, window, cx| {
        let pane = workspace.active_pane().clone();
        pane.update(cx, |pane, cx| pane.close_active_item(window, cx));
    });
    app.draw();
    assert_eq!(tabs(&workspace, app.cx()).0, ["Note 2"]);
    let row = app.cx().debug_bounds("note-row:1").expect("a row");
    app.cx()
        .simulate_click(row.center(), gpui::Modifiers::none());
    app.draw();
    assert_eq!(
        tabs(&workspace, app.cx()),
        (vec!["Note 2".into(), "Note 1".into()], Some(1))
    );
}

/// The text size of the open note's body.
fn body_size(app: &mut TestApp) -> f32 {
    let note = app.active_item::<NoteView>().expect("a note is open");
    app.cx()
        .read(|cx| f32::from(note.read(cx).body.read(cx).font_size()))
}

#[gpui::test]
fn a_font_size_in_settings_reaches_the_open_notes_view(cx: &mut TestAppContext) {
    let mut app = TestApp::open(app().restore_layout(false), cx);
    app.press("secondary-n");
    assert_eq!(body_size(&mut app), 14.0, "the default");
    app.write_user_file("settings.toml", "[notes]\nfont_size = 20\n");
    assert_eq!(body_size(&mut app), 20.0, "the file's");
}

/// A directory of its own under the target's, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let dir =
            Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("notes-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn paths(&self) -> AppPaths {
        let home: OsString = self.0.clone().into();
        AppPaths::resolve(
            &AppId::new(ID, "notes", "Notes"),
            PathStyle::Native,
            Os::current(),
            |name| match name {
                "HOME" | "APPDATA" | "LOCALAPPDATA" => Some(home.clone()),
                _ => None,
            },
        )
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// The app on `dir`'s paths, its first window drawn.
fn start(
    dir: &TempDir,
    cx: &mut TestAppContext,
) -> (Entity<Workspace<ItemPane>>, VisualTestContext) {
    let (_sender, batches) = batch_channel();
    let config_dir = dir.0.join("config");
    let builder = app().paths(dir.paths()).config(move |config| {
        config
            .user_files_in(config_dir)
            .watch_with(batches)
            .capabilities(Capabilities::for_os(Os::Linux))
    });
    let started = cx.update(|cx| builder.start(cx));
    let workspace = started
        .window
        .root::<Workspace<ItemPane>>(cx)
        .expect("a workspace");
    let mut cx = VisualTestContext::from_window(started.window.0, cx);
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
    (workspace, cx)
}

#[gpui::test]
fn notes_and_layout_come_back_on_a_second_start_with_the_same_paths(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
) {
    let dir = TempDir::new();
    let (workspace, mut cx) = start(&dir, first);
    cx.simulate_keystrokes("ctrl-n");
    cx.simulate_input("hello");
    cx.simulate_keystrokes("ctrl-n ctrl-b");
    cx.run_until_parked();
    assert_eq!(tabs(&workspace, &mut cx).0, ["Note 1", "Note 2"]);
    cx.executor().advance_clock(DEBOUNCE * 2);
    cx.run_until_parked();
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();

    let (workspace, mut cx) = start(&dir, second);
    assert_eq!(
        tabs(&workspace, &mut cx),
        (vec!["Note 1".into(), "Note 2".into()], Some(1))
    );
    assert_eq!(status(&mut cx), ["2 notes"]);
    cx.update(|_, cx| {
        assert_eq!(
            model::read::<Notes>(cx).get(1),
            Some(&(1, "hello".to_owned()))
        )
    });
    assert!(
        cx.debug_bounds("note-row:1").is_none(),
        "the sidebar stays hidden, as it was left"
    );
}
