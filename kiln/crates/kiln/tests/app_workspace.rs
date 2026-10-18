// SPDX-License-Identifier: LicenseRef-Proprietary

//! `kiln::app`'s default window, headless: a `Workspace` with the app's
//! panels, status sources and item kinds, under kiln's default keys and
//! menus; its layout saved in the state directory and restored by the
//! next start; debug builds failing fast on the app's keymap; and
//! `.root(..)` still a `Root` window that saves nothing.

use std::{
    ffi::OsString,
    fs,
    io::ErrorKind,
    panic::{AssertUnwindSafe, catch_unwind},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use gpui::{
    AnyView, App, AppContext as _, Context, Entity, FocusHandle, Focusable, Global,
    InteractiveElement as _, IntoElement, Render, SharedString, Styled as _, Task, TestAppContext,
    VisualTestContext, Window, WindowAppearance, div, px,
};
use kiln::{
    AppBuilder, Root, Started,
    command::Value,
    core::{AppId, AppPaths, Os, PathStyle},
    platform::{config::batch_channel, menus},
    prelude::*,
    shell::{
        Direction, DockItem, DockPosition, DockView as _, ItemPane, PaneItem, PaneView as _,
        PanelId, StatusItem, StatusSide, Workspace, command_palette::COMMAND_PALETTE,
    },
    ui::{Appearance, Icon, system_appearance_changed},
};

const ID: &str = "dev.kiln.Notes";
const NAME: &str = "Kiln Notes";

mod notes {
    kiln::commands!(
        notes,
        [
            /// New Note
            NewNote,
        ]
    );
}

/// The notes `notes.new_note` made.
#[derive(Default)]
struct Made(usize);

impl Global for Made {}

/// A note, its name its saved state.
struct Note {
    name: SharedString,
    focus: FocusHandle,
}

impl Note {
    fn new(name: &str, cx: &mut Context<Self>) -> Self {
        Self {
            name: name.to_owned().into(),
            focus: cx.focus_handle(),
        }
    }
}

impl Render for Note {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full()
    }
}

impl Focusable for Note {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl PaneItem for Note {
    const KIND: &'static str = "test.note";

    fn tab_title(&self, _: &App) -> SharedString {
        self.name.clone()
    }

    fn split(&mut self, _: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Self>> {
        let name = self.name.clone();
        Some(cx.new(|cx| Note::new(&name, cx)))
    }

    fn serialize(&self, _: &App) -> Option<Value> {
        Some(Value::from(self.name.as_ref()))
    }

    fn restore(state: Value, _: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        let name = state.as_str().unwrap_or_default().to_owned();
        Task::ready(Some(cx.new(|cx| Note::new(&name, cx))))
    }
}

/// The notes list, in the left dock.
struct List(FocusHandle);

impl Render for List {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.0).size_full()
    }
}

impl Focusable for List {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.0.clone()
    }
}

impl DockItem for List {
    fn id(&self) -> PanelId {
        PanelId("notes.list")
    }

    fn title(&self, _: &App) -> SharedString {
        "Notes".into()
    }

    fn icon(&self, _: &App) -> Icon {
        Icon::Files
    }
}

fn count(_: &App) -> Option<StatusItem> {
    let item = StatusItem::new(
        "notes.count".to_owned(),
        StatusSide::Left,
        0,
        "2 notes".to_owned(),
    );
    Some(item)
}

/// A directory under the system's temporary one, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        // A pid is reused, so a directory a past process leaked can bear
        // this name: only ever open one just created.
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        loop {
            let name = format!(
                "kiln-app-workspace-{}-{nanos}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            );
            let dir = std::env::temp_dir().join(name);
            match fs::create_dir(&dir) {
                Ok(()) => return Self(dir),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => panic!("creating {}: {error}", dir.display()),
            }
        }
    }

    /// The app's directories under this one: `config` and `state`.
    fn paths(&self) -> AppPaths {
        let id = AppId::new(ID, "kiln-notes", NAME);
        let config: OsString = self.0.join("config").into();
        let state: OsString = self.0.join("state").into();
        AppPaths::resolve(&id, PathStyle::Native, Os::current(), |name| match name {
            "KILN_NOTES_CONFIG_DIR" => Some(config.clone()),
            "KILN_NOTES_STATE_DIR" => Some(state.clone()),
            _ => None,
        })
    }

    fn layouts(&self) -> PathBuf {
        self.0.join("state").join("layouts")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// The files in `dir`, none if it's missing.
fn files(dir: &Path) -> Vec<PathBuf> {
    let entries = fs::read_dir(dir).into_iter().flatten();
    entries.map(|entry| entry.unwrap().path()).collect()
}

/// The notes app on `paths`, with no user files watched, its window
/// staying when closed: a note kind, the list in the left dock and a note
/// count on the status bar, and no note opened.
fn bare(paths: AppPaths) -> AppBuilder {
    let (_, closed) = batch_channel();
    kiln::app(ID, NAME)
        .paths(paths)
        .commands(|cx| {
            notes::register(cx);
            cx.on_action(|_: &notes::NewNote, cx| cx.default_global::<Made>().0 += 1);
        })
        .quit_on_last_window(false)
        .item::<Note>()
        .panel(DockPosition::Left, |_, cx| {
            cx.new(|cx| List(cx.focus_handle()))
        })
        .status(count)
        .config(move |config| config.watch_with(closed))
}

/// The notes app, with `a` opened in a fresh window.
fn notes(paths: AppPaths) -> AppBuilder {
    bare(paths).workspace(|workspace, window, cx| {
        let a = cx.new(|cx| Note::new("a", cx));
        workspace.add_item(Box::new(a), window, cx);
    })
}

/// The started app's first window, a workspace, drawn.
fn open(
    app: AppBuilder,
    cx: &mut TestAppContext,
) -> (Started, Entity<Workspace>, VisualTestContext) {
    let started = cx.update(|cx| app.start(cx));
    let workspace = started.window.root::<Workspace>(cx).expect("a workspace");
    let mut cx = VisualTestContext::from_window(started.window.0, cx);
    draw(&mut cx);
    (started, workspace, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    cx.run_until_parked();
}

/// Presses `keys`, with this platform's `secondary`, then draws a frame.
fn press(keys: &str, cx: &mut VisualTestContext) {
    let secondary = if Os::current() == Os::MacOs {
        "cmd"
    } else {
        "ctrl"
    };
    cx.simulate_keystrokes(&keys.replace("secondary", secondary));
    draw(cx);
}

fn panes(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<Entity<ItemPane>> {
    workspace.read_with(cx, |workspace, _| {
        workspace.group().panes().into_iter().cloned().collect()
    })
}

fn titles(pane: &Entity<ItemPane>, cx: &mut VisualTestContext) -> Vec<String> {
    pane.read_with(cx, |pane, cx| {
        let items = pane.pane().items().iter();
        items.map(|item| item.tab_title(cx).to_string()).collect()
    })
}

/// The left dock: whether it shows, and its panel.
fn left_dock(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> (bool, Option<PanelId>) {
    workspace.read_with(cx, |workspace, cx| {
        let dock = workspace.dock(DockPosition::Left).expect("a left dock");
        let dock = dock.read(cx).dock();
        (dock.is_visible(), dock.active())
    })
}

fn update(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
    f: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>),
) {
    cx.update(|window, cx| workspace.update(cx, |workspace, cx| f(workspace, window, cx)));
    draw(cx);
}

#[gpui::test]
fn the_window_is_a_workspace_with_the_panel_status_palette_and_default_menus(
    app_cx: &mut TestAppContext,
) {
    let dir = TempDir::new();
    let (started, workspace, mut cx) = open(notes(dir.paths()), app_cx);
    let cx = &mut cx;
    assert_eq!(started.diagnostics, []);
    assert_eq!(started.windows, [started.window]);
    assert_eq!(cx.update(|_, cx| cx.windows().len()), 1);

    assert_eq!(left_dock(&workspace, cx).1, Some(PanelId("notes.list")));
    assert!(cx.debug_bounds("status-item:notes.count").is_some());
    let [pane] = &panes(&workspace, cx)[..] else {
        panic!("one pane")
    };
    assert_eq!(titles(pane, cx), ["a"]);

    let shell = workspace.read_with(cx, |workspace, _| workspace.shell().clone());
    assert_eq!(shell.read_with(cx, |shell, _| shell.title().clone()), "a");
    palette_keys_work(&workspace, cx);

    // With no menu table, kiln's menus.
    let names = cx.update(|_, cx| {
        let bar = menus::installed(cx).expect("the menus are installed");
        bar.menus
            .iter()
            .map(|menu| menu.name.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(names, [NAME, "View", "Window"]);
    app_cx.quit();
}

/// kiln's keys: `secondary-shift-p` opens the palette, `escape` closes
/// it, and `enter` runs the command picked.
fn palette_keys_work(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
    let shell = workspace.read_with(cx, |workspace, _| workspace.shell().clone());
    let modal = |cx: &mut VisualTestContext| shell.read_with(cx, |shell, cx| shell.modal(cx));
    let made = cx.update(|_, cx| cx.default_global::<Made>().0);
    press("secondary-shift-p", cx);
    assert_eq!(modal(cx), Some(COMMAND_PALETTE));
    press("escape", cx);
    assert_eq!(modal(cx), None);
    press("secondary-shift-p", cx);
    cx.simulate_input("new note");
    press("enter", cx);
    assert_eq!(modal(cx), None);
    assert_eq!(cx.update(|_, cx| cx.default_global::<Made>().0), made + 1);
}

#[gpui::test]
fn an_empty_workspace_takes_kilns_keys(app_cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let (_, workspace, mut cx) = open(bare(dir.paths()), app_cx);
    let cx = &mut cx;
    let [pane] = &panes(&workspace, cx)[..] else {
        panic!("one pane")
    };
    assert_eq!(titles(pane, cx), Vec::<String>::new());
    let shell = workspace.read_with(cx, |workspace, _| workspace.shell().clone());
    assert_eq!(shell.read_with(cx, |shell, _| shell.title().clone()), NAME);
    palette_keys_work(&workspace, cx);
    app_cx.quit();
}

#[gpui::test]
fn the_theme_follows_the_windows_appearance(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    // The test platform is light: as if the system had been dark until the
    // window opened, which tells its observers its appearance.
    cx.update(|cx| {
        bare(dir.paths()).start(cx);
        system_appearance_changed(cx, WindowAppearance::Dark);
    });
    cx.run_until_parked();
    assert_eq!(cx.read(|cx| cx.theme().appearance), Appearance::Light);
    cx.quit();
}

#[gpui::test]
fn the_key_context_and_dock_size_are_the_apps(app_cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let app = notes(dir.paths())
        .key_context("notes")
        .dock_size(DockPosition::Left, px(321.));
    let (_, workspace, cx) = open(app, app_cx);
    let size = workspace.read_with(&cx, |workspace, cx| {
        let dock = workspace.dock(DockPosition::Left).unwrap().read(cx).dock();
        f32::from(dock.current_size())
    });
    assert_eq!(size, 321.);
    let shell = workspace.read_with(&cx, |workspace, _| workspace.shell().clone());
    assert_eq!(
        shell.read_with(&cx, |shell, _| shell.key_context().clone()),
        "notes"
    );
    app_cx.quit();
}

#[gpui::test]
fn the_layout_is_saved_and_the_next_start_restores_it(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
) {
    let dir = TempDir::new();
    let (_, workspace, mut cx) = open(notes(dir.paths()), first);
    let cx = &mut cx;
    update(&workspace, cx, |workspace, window, cx| {
        workspace.split(Direction::Right, window, cx);
        let b = cx.new(|cx| Note::new("b", cx));
        workspace.add_item(Box::new(b), window, cx);
        workspace.toggle_dock(DockPosition::Left, window, cx);
    });
    assert!(!left_dock(&workspace, cx).0);
    assert!(!files(&dir.layouts()).is_empty(), "saved as it changed");

    // The last window closes, keeping its entry.
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();

    let (started, workspace, mut cx) = open(notes(dir.paths()), second);
    let cx = &mut cx;
    assert_eq!(started.windows.len(), 1);
    let panes = panes(&workspace, cx);
    assert_eq!(panes.len(), 2);
    assert_eq!(titles(&panes[0], cx), ["a"]);
    assert_eq!(titles(&panes[1], cx), ["a", "b"]);
    let active = workspace.read_with(cx, |workspace, _| workspace.active_pane().clone());
    assert_eq!(active, panes[1]);
    assert_eq!(
        left_dock(&workspace, cx),
        (false, Some(PanelId("notes.list")))
    );
    first.quit();
    second.quit();
}

#[gpui::test]
fn without_restore_layout_a_fresh_window_saves_nothing(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
) {
    let dir = TempDir::new();
    let (_, workspace, mut cx) = open(notes(dir.paths()), first);
    let cx = &mut cx;
    update(&workspace, cx, |workspace, window, cx| {
        workspace.split(Direction::Right, window, cx);
    });
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    let saved: Vec<_> = (files(&dir.layouts()).iter())
        .map(|file| (file.clone(), fs::read(file).unwrap()))
        .collect();

    let app = notes(dir.paths()).restore_layout(false);
    let (_, workspace, mut cx) = open(app, second);
    let cx = &mut cx;
    let [pane] = &panes(&workspace, cx)[..] else {
        panic!("one pane")
    };
    assert_eq!(titles(pane, cx), ["a"]);
    update(&workspace, cx, |workspace, window, cx| {
        workspace.split(Direction::Down, window, cx);
    });
    cx.update(|window, _| window.remove_window());
    cx.run_until_parked();
    let now: Vec<_> = (files(&dir.layouts()).iter())
        .map(|file| (file.clone(), fs::read(file).unwrap()))
        .collect();
    assert_eq!(now, saved);
    first.quit();
    second.quit();
}

const UNKNOWN: &str = "[[bindings]]\nkeys = \"secondary-n\"\ncommand = \"notes.no_such\"\n";

/// What `start` panicked with, if it did.
fn start_panic(app: AppBuilder, cx: &mut TestAppContext) -> Option<String> {
    let started = catch_unwind(AssertUnwindSafe(|| {
        cx.update(|cx| {
            app.start(cx);
        })
    }));
    let payload = started.err()?;
    Some(match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => (*payload.downcast::<&str>().unwrap()).to_owned(),
    })
}

#[gpui::test]
fn in_a_debug_build_an_unknown_command_in_the_keymap_panics_at_start(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let app = notes(dir.paths()).keymap("notes-keymap.toml", UNKNOWN);
    let message = start_panic(app, cx);
    if cfg!(debug_assertions) {
        let message = message.expect("start panics");
        assert!(message.contains("notes-keymap.toml"), "{message}");
        assert!(message.contains("notes.no_such"), "{message}");
    } else {
        assert_eq!(message, None);
    }
    cx.quit();
}

#[gpui::test]
fn fail_fast_off_returns_the_diagnostic(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let app = notes(dir.paths())
        .keymap("notes-keymap.toml", UNKNOWN)
        .config(|config| config.fail_fast(false));
    let started = cx.update(|cx| app.start(cx));
    let [diagnostic] = &started.diagnostics[..] else {
        panic!("one diagnostic: {:?}", started.diagnostics)
    };
    assert_eq!(diagnostic.file, Path::new("notes-keymap.toml"));
    cx.quit();
}

struct View;

impl Render for View {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

#[gpui::test]
fn root_opens_a_root_window_that_saves_no_layout(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let app = notes(dir.paths()).root(|_, cx| AnyView::from(cx.new(|_| View)));
    let started = cx.update(|cx| app.start(cx));
    cx.run_until_parked();
    assert!(started.window.root::<Root>(cx).is_some());
    assert_eq!(started.window.root::<Workspace>(cx), None);
    started
        .window
        .update(cx, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    assert_eq!(files(&dir.layouts()), Vec::<PathBuf>::new());
    cx.quit();
}

#[gpui::test]
fn with_neither_a_root_nor_an_item_start_panics_naming_both(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    // `start` runs `init`, and so the config service, before it checks for a
    // window; take changes from our own channel so the real watcher thread
    // never wakes the test scheduler.
    let (_, closed) = batch_channel();
    let app = kiln::app(ID, NAME)
        .paths(dir.paths())
        .status(count)
        .config(move |config| config.watch_with(closed));
    let message = start_panic(app, cx).expect("start panics");
    assert!(message.contains(".root(..)"), "{message}");
    assert!(message.contains(".item::<T>()"), "{message}");
    cx.quit();
}
