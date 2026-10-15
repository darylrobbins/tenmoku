// SPDX-License-Identifier: LicenseRef-Proprietary

//! `kiln::Module`s and `.contributions(..)`: a library crate's panels, item
//! kinds, status sources and commands, in each workspace window `kiln::app`
//! opens, each panel with a toggle command on its key, in the palette and
//! the View menu; panels by order, then in the order added; and, in a debug
//! build, a warning naming a module that contributes nothing.

use std::{
    ffi::OsString,
    fs,
    path::PathBuf,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use gpui::{
    App, AppContext as _, Context, Entity, FocusHandle, Focusable, Global, InteractiveElement as _,
    IntoElement, Render, SharedString, Styled as _, Task, TestAppContext, VisualTestContext,
    Window, div,
};
use kiln::{
    AppBuilder, CommandSpec, Contributions, Keys, MakePanel, PanelSpec, Started, StatusSpec,
    command::{ContextStack, Value},
    core::{AppId, AppPaths, Os, PathStyle},
    platform::{
        command_actions::{Command as _, register_command},
        config::batch_channel,
        menus,
    },
    prelude::*,
    shell::{
        DockItem, DockPosition, DockView as _, ItemPane, PaneItem, PaneView as _, PanelId,
        StatusItem, StatusSide, Workspace, command_palette::commands_in, layout,
    },
    ui::Icon,
};

const ID: &str = "dev.kiln.Modules";
const NAME: &str = "Kiln Modules";

/// The notes library crate's commands.
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
    const KIND: &'static str = "test.module_note";

    fn tab_title(&self, _: &App) -> SharedString {
        self.name.clone()
    }

    fn serialize(&self, _: &App) -> Option<Value> {
        Some(Value::from(self.name.as_ref()))
    }

    fn restore(state: Value, _: &mut Window, cx: &mut App) -> Task<Option<Entity<Self>>> {
        let name = state.as_str().unwrap_or_default().to_owned();
        Task::ready(Some(cx.new(|cx| Note::new(&name, cx))))
    }
}

/// A panel: its id, title and icon.
struct Panel {
    id: &'static str,
    title: &'static str,
    icon: Icon,
    focus: FocusHandle,
}

impl Panel {
    fn new(id: &'static str, title: &'static str, icon: Icon, cx: &mut App) -> Self {
        Self {
            id,
            title,
            icon,
            focus: cx.focus_handle(),
        }
    }
}

impl Render for Panel {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full()
    }
}

impl Focusable for Panel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DockItem for Panel {
    fn id(&self) -> PanelId {
        PanelId(self.id)
    }

    fn title(&self, _: &App) -> SharedString {
        self.title.into()
    }

    fn icon(&self, _: &App) -> Icon {
        self.icon
    }
}

const TOGGLE_KEY: &str = "secondary-alt-l";

fn count(_: &App) -> Option<StatusItem> {
    Some(StatusItem::new(
        "notes.count",
        StatusSide::Left,
        0,
        "2 notes",
    ))
}

/// What the notes crate contributes, by hand: the notes list in the left
/// dock, toggled on `TOGGLE_KEY`; the note kind; a note count on the
/// status bar; and `notes.new_note`, with its handler.
fn notes_contributions(cx: &mut App) {
    Contributions::add_panel(
        cx,
        PanelSpec {
            id: PanelId("notes.list"),
            title: "Notes".into(),
            icon: Icon::Files,
            position: DockPosition::Left,
            order: 0,
            toggle_key: TOGGLE_KEY.into(),
            make: |_, cx| {
                cx.new(|cx| Panel::new("notes.list", "Notes", Icon::Files, cx))
                    .into()
            },
        },
    );
    Contributions::add_item_kind(cx, layout::register_item::<Note>);
    Contributions::add_status(
        cx,
        StatusSpec {
            side: StatusSide::Left,
            order: 0,
            item: count,
        },
    );
    Contributions::add_command(cx, CommandSpec::new::<notes::NewNote>("New Note"));
    Contributions::add_model(cx, |cx| {
        cx.on_action(|_: &notes::NewNote, cx| cx.default_global::<Made>().0 += 1);
    });
}

/// The notes crate's module.
fn notes_module(cx: &mut App) {
    notes_contributions(cx);
}

/// A directory under the system's temporary one, removed when dropped.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!(
            "kiln-modules-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(name);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// The app's directories under this one: `config` and `state`.
    fn paths(&self) -> AppPaths {
        let id = AppId::new(ID, "kiln-modules", NAME);
        let config: OsString = self.0.join("config").into();
        let state: OsString = self.0.join("state").into();
        AppPaths::resolve(&id, PathStyle::Native, Os::current(), |name| match name {
            "KILN_MODULES_CONFIG_DIR" => Some(config.clone()),
            "KILN_MODULES_STATE_DIR" => Some(state.clone()),
            _ => None,
        })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

/// An app on `paths` with no user files watched and nothing of its own: a
/// window staying when closed.
fn bare(paths: AppPaths) -> AppBuilder {
    let (_, closed) = batch_channel();
    kiln::app(ID, NAME)
        .paths(paths)
        .quit_on_last_window(false)
        .config(move |config| config.watch_with(closed))
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

/// The started app's windows, each a workspace, drawn.
fn open(app: AppBuilder, cx: &mut TestAppContext) -> Vec<(Entity<Workspace>, VisualTestContext)> {
    let started: Started = cx.update(|cx| app.start(cx));
    assert_eq!(started.diagnostics, []);
    let windows = started.windows.iter().map(|window| {
        let workspace = window.root::<Workspace>(cx).expect("a workspace");
        let mut cx = VisualTestContext::from_window(window.0, cx);
        draw(&mut cx);
        (workspace, cx)
    });
    windows.collect()
}

fn update(
    workspace: &Entity<Workspace>,
    cx: &mut VisualTestContext,
    f: impl FnOnce(&mut Workspace, &mut Window, &mut Context<Workspace>),
) {
    cx.update(|window, cx| workspace.update(cx, |workspace, cx| f(workspace, window, cx)));
    draw(cx);
}

/// The left dock: whether it shows, and its panels in rail order.
fn left_dock(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> (bool, Vec<PanelId>) {
    workspace.read_with(cx, |workspace, cx| {
        let dock = workspace.dock(DockPosition::Left).expect("a left dock");
        let dock = dock.read(cx).dock();
        (dock.is_visible(), dock.panel_ids())
    })
}

/// The titles of the items in the workspace's panes.
fn titles(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<String> {
    workspace.read_with(cx, |workspace, cx| {
        let panes: Vec<Entity<ItemPane>> = workspace.group().panes().into_iter().cloned().collect();
        let items = panes.iter().flat_map(|pane| pane.read(cx).pane().items());
        items.map(|item| item.tab_title(cx).to_string()).collect()
    })
}

/// The workspace's notes list: in its left dock, with a toggle command
/// that `TOGGLE_KEY` and the View menu run, which shows and hides it, focus
/// leaving it when hidden; and the note count drawn.
fn check_window(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
    let (shown, panels) = left_dock(workspace, cx);
    assert_eq!(panels, [PanelId("notes.list")]);
    assert!(cx.debug_bounds("status-item:notes.count").is_some());

    press(TOGGLE_KEY, cx);
    assert_eq!(left_dock(workspace, cx).0, !shown, "its key toggles it");
    if shown {
        press(TOGGLE_KEY, cx);
        assert!(left_dock(workspace, cx).0, "and back");
    }
    let list = workspace.read_with(cx, |workspace, cx| {
        let dock = workspace.dock(DockPosition::Left).unwrap();
        dock.read(cx).panel::<Panel>().expect("the notes list")
    });
    cx.update(|window, cx| window.focus(&list.focus_handle(cx), cx));
    press(TOGGLE_KEY, cx);
    assert!(!left_dock(workspace, cx).0, "and hides it");
    let focused = cx.update(|window, cx| list.focus_handle(cx).contains_focused(window, cx));
    assert!(!focused, "focus leaves the hidden panel");

    cx.update(|window, cx| {
        let bar = menus::installed(cx).expect("the menus are installed");
        let view = bar.menu("View").expect("a View menu");
        let item = (view.items.iter())
            .find(|item| item.id() == Some("notes.list.toggle"))
            .expect("the toggle in the View menu");
        assert_eq!(
            item.command().map(|command| command.title.as_str()),
            Some("Toggle Notes")
        );
        menus::run("View", "notes.list.toggle", window, cx);
    });
    draw(cx);
    assert!(left_dock(workspace, cx).0, "the menu item shows it");
    update(workspace, cx, |workspace, window, cx| {
        workspace.toggle_dock(DockPosition::Left, window, cx)
    });

    cx.update(|_, cx| {
        let palette = commands_in(&ContextStack::new(), cx);
        let toggle = palette
            .iter()
            .find(|entry| entry.id.as_str() == "notes.list.toggle")
            .expect("the toggle in the palette");
        assert_eq!(toggle.title, "Toggle Notes");
        assert!(
            (palette.iter()).any(|entry| entry.id == notes::NewNote::id()),
            "the module's command is in the palette"
        );
    });
}

/// The notes app `app` makes, started three times on the same
/// directories: twice at once, so that each opens a window of its own with
/// a note, then again, once both have closed, restoring both windows with
/// their notes, each with the module's panel, toggle and status item.
fn each_window_has_the_contributions(
    app: fn(AppBuilder) -> AppBuilder,
    first: &mut TestAppContext,
    second: &mut TestAppContext,
    third: &mut TestAppContext,
) {
    let dir = TempDir::new();
    let mut opened = Vec::new();
    for (name, cx) in [("a", first), ("b", second)] {
        let mut windows = open(app(bare(dir.paths())), cx);
        let (workspace, mut cx) = windows.remove(0);
        assert!(windows.is_empty(), "one fresh window");
        update(&workspace, &mut cx, |workspace, window, cx| {
            let note = cx.new(|cx| Note::new(name, cx));
            workspace.add_item(Box::new(note), window, cx);
        });
        check_window(&workspace, &mut cx);
        opened.push(cx);
    }
    // The windows close, keeping their entries.
    for cx in &mut opened {
        cx.update(|window, _| window.remove_window());
        cx.run_until_parked();
    }

    let mut windows = open(app(bare(dir.paths())), third);
    assert_eq!(windows.len(), 2, "both windows come back");
    let mut restored: Vec<String> = Vec::new();
    for (workspace, cx) in &mut windows {
        restored.extend(titles(workspace, cx));
        check_window(workspace, cx);
    }
    restored.sort();
    assert_eq!(restored, ["a", "b"], "the module's item kind restores");

    let (workspace, cx) = &mut windows[0];
    update(workspace, cx, |workspace, window, cx| {
        workspace.focus_active(window, cx);
    });
    cx.update(|window, cx| {
        window
            .run_command(kiln::command::Invocation::new(notes::NewNote::id()), cx)
            .unwrap();
    });
    draw(cx);
    assert_eq!(cx.update(|_, cx| cx.default_global::<Made>().0), 1);
}

#[gpui::test]
fn a_module_gives_each_workspace_window_its_panel_toggle_item_kind_and_status(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
    third: &mut TestAppContext,
) {
    each_window_has_the_contributions(|app| app.module(notes_module), first, second, third);
}

#[gpui::test]
fn contributions_inline_do_the_same(
    first: &mut TestAppContext,
    second: &mut TestAppContext,
    third: &mut TestAppContext,
) {
    each_window_has_the_contributions(
        |app| app.contributions(notes_contributions),
        first,
        second,
        third,
    );
}

/// A panel in the left dock, with no toggle key.
fn panel(id: &'static str, icon: Icon, order: i32, make: MakePanel) -> PanelSpec {
    PanelSpec {
        id: PanelId(id),
        title: id.into(),
        icon,
        position: DockPosition::Left,
        order,
        toggle_key: Keys::None,
        make,
    }
}

/// Three panels, out of order, the last two tying.
fn ordered_module(cx: &mut App) {
    let specs = [
        panel("late.one", Icon::Folder, 20, |_, cx| {
            cx.new(|cx| Panel::new("late.one", "late.one", Icon::Folder, cx))
                .into()
        }),
        panel("early", Icon::Files, 10, |_, cx| {
            cx.new(|cx| Panel::new("early", "early", Icon::Files, cx))
                .into()
        }),
        panel("late.two", Icon::Search, 20, |_, cx| {
            cx.new(|cx| Panel::new("late.two", "late.two", Icon::Search, cx))
                .into()
        }),
    ];
    for spec in specs {
        Contributions::add_panel(cx, spec);
    }
}

#[gpui::test]
fn panels_sort_by_order_then_registration_in_the_dock_and_its_rail(cx: &mut TestAppContext) {
    let dir = TempDir::new();
    let mut windows = open(bare(dir.paths()).module(ordered_module), cx);
    let (workspace, cx) = &mut windows[0];
    let expected = [PanelId("early"), PanelId("late.one"), PanelId("late.two")];
    assert_eq!(left_dock(workspace, cx).1, expected, "the dock's panels");
    let icons = workspace.read_with(cx, |workspace, cx| {
        let dock = workspace.dock(DockPosition::Left).unwrap();
        dock.read(cx).dock().drawn().rail_icons()
    });
    assert_eq!(icons, [Icon::Files, Icon::Folder, Icon::Search], "its rail");

    // The View menu's toggles go the same way.
    let toggles = cx.update(|_, cx| {
        let bar = menus::installed(cx).unwrap();
        let view = bar.menu("View").unwrap();
        let ids = view.items.iter().filter_map(|item| item.id());
        ids.filter(|id| id.starts_with("early") || id.starts_with("late"))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    });
    assert_eq!(
        toggles,
        ["early.toggle", "late.one.toggle", "late.two.toggle"]
    );
}

/// The warnings logged, as `level: message`.
fn logged() -> &'static Mutex<Vec<String>> {
    static LOGGED: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    LOGGED.get_or_init(|| Mutex::new(Vec::new()))
}

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            let line = format!("{}: {}", record.level(), record.args());
            logged().lock().unwrap().push(line);
        }
    }

    fn flush(&self) {}
}

fn capture_logs() {
    static LOGGER: Logger = Logger;
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Warn);
    }
}

fn empty_module(_: &mut App) {}

fn record_command(cx: &mut App) {
    // Registered, but not contributed.
    register_command::<notes::NewNote>(cx, Default::default());
}

#[gpui::test]
fn a_module_that_contributes_nothing_logs_a_warning_naming_it(cx: &mut TestAppContext) {
    capture_logs();
    let dir = TempDir::new();
    let app = bare(dir.paths())
        .module(notes_module)
        .module(empty_module)
        .module(record_command)
        .contributions(|_| {});
    let _windows = open(app, cx);
    let warnings: Vec<String> = (logged().lock().unwrap().iter())
        .filter(|line| line.contains("contributes nothing"))
        .cloned()
        .collect();
    let named = |name: &str| warnings.iter().any(|line| line.contains(name));
    assert_eq!(
        named("modules::empty_module"),
        cfg!(debug_assertions),
        "{warnings:?}"
    );
    assert_eq!(
        named("modules::record_command"),
        cfg!(debug_assertions),
        "{warnings:?}"
    );
    assert!(!named("notes_module"), "{warnings:?}");
    let expected = if cfg!(debug_assertions) { 2 } else { 0 };
    assert_eq!(
        warnings.len(),
        expected,
        "the inline form is no module: {warnings:?}"
    );
}
