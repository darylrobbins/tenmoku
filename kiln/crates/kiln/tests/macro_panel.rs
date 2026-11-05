// SPDX-License-Identifier: Apache-2.0

//! `#[kiln::panel]` and `#[kiln::status]`: a struct implementing
//! `kiln::View` is a panel in its dock of every workspace window
//! `kiln::app` opens, with no other wiring, toggled by its key, the
//! palette and the View menu, focus leaving it when hidden, its own
//! commands attached; `dock` and `order` place it, the rail following
//! order, then file and line; it redraws on what it watches; and a status
//! fn shows on the bar, its new text once the bar next draws.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use gpui::{
    App, Context, Entity, Focusable as _, Global, InteractiveElement, IntoElement,
    ParentElement as _, Pixels, TestAppContext, div,
};
use kiln::{
    Command as _, Cx, View, ViewCommands,
    command::{ContextStack, Invocation},
    platform::{commands::observe_invocations, menus},
    prelude::*,
    shell::{
        DockItem, DockPosition, DockView as _, PanelId, StatusItem, StatusSide, Workspace,
        command_palette::{COMMAND_PALETTE, commands_in},
    },
    testing::TestApp,
    ui::Icon,
};

/// The notes list: in the left dock, toggled on `secondary-b`.
#[kiln::panel(title = "Notes", icon = FileText, dock = Left, toggle_key = "secondary-b")]
#[derive(Default)]
struct Notes;

impl View for Notes {
    fn render(&mut self, _: &mut Cx<Self>) -> impl IntoElement {
        div().child("notes")
    }
}

#[kiln::command(context = "notes")]
impl Notes {
    /// Clear Notes
    #[command(key = "f5")]
    fn clear(&mut self, cx: &mut Cx<Self>) {
        cx.default_global::<Cleared>().0 += 1;
    }
}

/// Zoom Notes
///
/// A View menu command of order 0, declared between the panels of order 0,
/// Notes and Tags: its item goes between their toggles.
#[kiln::command(menu = "View")]
fn zoom_notes(_: &mut Workspace, _: &mut Cx<Workspace>) {}

/// How often `Clear Notes` ran.
#[derive(Default)]
struct Cleared(usize);

impl Global for Cleared {}

// Three panels in the right dock, declared out of their order: by
// `order`, then by file and line, Tags, Outline, then Search.

/// Outline
#[kiln::panel(dock = Right, order = 1, icon = ListTree)]
#[derive(Default)]
struct Outline;

#[kiln::panel(title = "Search", dock = Right, order = 1, icon = Search, id = "find.results")]
#[derive(Default)]
struct SearchResults {
    query: String,
}

#[kiln::panel(title = "Tags", dock = Right, icon = Package, new = Tags::new)]
struct Tags(Vec<String>);

impl Tags {
    fn new(_: &mut Cx<Self>) -> Self {
        Self(vec!["kiln".to_owned()], Default::default())
    }
}

macro_rules! plain_view {
    ($($view:ty),*) => {$(
        impl View for $view {
            fn render(&mut self, _: &mut Cx<Self>) -> impl IntoElement {
                div()
            }
        }
    )*};
}

plain_view!(Outline, SearchResults, Tags);

/// Archive
#[kiln::command]
fn archive(_: &mut Workspace, cx: &mut Cx<Workspace>) {
    cx.default_global::<Archived>().0 += 1;
}

/// How often `archive` ran.
#[derive(Default)]
struct Archived(usize);

impl Global for Archived {}

/// The outline's own handler of `Archive`, by hand: it blurs the window,
/// then lets the command go on to `archive`.
impl ViewCommands for Outline {
    const CONTEXT: &'static str = "outline";

    fn attach<E: InteractiveElement>(element: E, cx: &Context<Self>) -> E {
        element
            .key_context(Self::CONTEXT)
            .on_action(cx.listener(|_, _: &Archive, window, cx| {
                window.blur(cx);
                cx.propagate();
            }))
    }
}

/// The model `Watched` watches.
#[derive(Default)]
struct Count(usize);

/// A panel drawing `Count`, last seen as `seen`, and how many times it
/// drew.
#[kiln::panel(title = "Watched", order = 5, icon = Database)]
#[derive(Default)]
struct Watched {
    seen: usize,
    renders: usize,
}

impl View for Watched {
    fn render(&mut self, cx: &mut Cx<Self>) -> impl IntoElement {
        self.seen = cx.watch::<Count>().0;
        self.renders += 1;
        div().child(format!("{} notes", self.seen))
    }
}

/// What `count` shows.
#[derive(Default)]
struct NoteCount(usize);

impl Global for NoteCount {}

/// The note count, on the left of the bar.
#[kiln::status(side = Left)]
fn count(cx: &App) -> String {
    let count = cx.try_global::<NoteCount>().map_or(0, |count| count.0);
    format!("{count} notes")
}

/// Whether `syncing` shows.
#[derive(Default)]
struct Syncing(bool);

impl Global for Syncing {}

/// "Syncing", on the right of the bar, while syncing.
#[kiln::status(side = Right, order = 2, id = "sync.state")]
fn syncing(cx: &App) -> Option<String> {
    let syncing = cx.try_global::<Syncing>().is_some_and(|syncing| syncing.0);
    syncing.then(|| "Syncing".to_owned())
}

/// An item of its own, as it is.
#[kiln::status]
fn branch(_: &App) -> StatusItem {
    StatusItem::new("git.branch", StatusSide::Right, 0, "main")
}

fn open(cx: &mut TestAppContext) -> TestApp {
    TestApp::open(
        kiln::app("dev.kiln.MacroPanel", "Kiln Macro Panel").restore_layout(false),
        cx,
    )
}

/// The dock at `position`: whether it shows, its active panel, and its
/// panels in rail order.
fn dock(app: &mut TestApp, position: DockPosition) -> (bool, Option<PanelId>, Vec<PanelId>) {
    let workspace = app.workspace();
    app.cx().read(|cx| {
        let dock = workspace.read(cx).dock(position).expect("a dock");
        let dock = dock.read(cx).dock();
        (dock.is_visible(), dock.active(), dock.panel_ids())
    })
}

/// Whether the dock at `position` shows the panel `id`.
fn showing(app: &mut TestApp, position: DockPosition, id: &str) -> bool {
    let (visible, active, _) = dock(app, position);
    visible && active.is_some_and(|active| active.0 == id)
}

/// The panel `P` of the dock at `position`.
fn panel<P: DockItem>(app: &mut TestApp, position: DockPosition) -> Entity<P> {
    let workspace = app.workspace();
    app.cx().read(|cx| {
        let dock = workspace.read(cx).dock(position).expect("a dock");
        dock.read(cx).panel::<P>().expect("the panel")
    })
}

fn focus_in<P: DockItem>(app: &mut TestApp, panel: &Entity<P>) -> bool {
    app.cx()
        .update(|window, cx| panel.focus_handle(cx).contains_focused(window, cx))
}

fn focus<P: DockItem>(app: &mut TestApp, panel: &Entity<P>) {
    app.cx()
        .update(|window, cx| window.focus(&panel.focus_handle(cx), cx));
    app.draw();
    assert!(focus_in(app, panel));
}

/// Shows the panel `id` at `position`, by its toggle command, if it
/// doesn't show.
fn show(app: &mut TestApp, position: DockPosition, id: &str) {
    if !showing(app, position, id) {
        app.run_command(&format!("{id}.toggle"));
    }
    assert!(showing(app, position, id), "{id} shows");
}

fn rail_icons(app: &mut TestApp, position: DockPosition) -> Vec<Icon> {
    let workspace = app.workspace();
    app.cx().read(|cx| {
        let dock = workspace.read(cx).dock(position).expect("a dock");
        dock.read(cx).dock().drawn().rail_icons()
    })
}

#[gpui::test]
fn a_panel_shows_in_its_dock_toggled_by_its_key_the_palette_and_the_view_menu(
    cx: &mut TestAppContext,
) {
    let mut app = open(cx);
    let left = DockPosition::Left;
    let id = "macro_panel.notes";
    let (_, _, panels) = dock(&mut app, left);
    assert_eq!(panels, [PanelId(id), PanelId("macro_panel.watched")]);
    let notes = panel::<Notes>(&mut app, left);
    app.cx().read(|cx| {
        let notes = notes.read(cx);
        assert_eq!(notes.title(cx), "Notes");
        assert_eq!(notes.icon(cx), Icon::FileText);
        assert_eq!(notes.show_command(), Some("macro_panel.notes.toggle"));
    });

    // Its key toggles it.
    let shown = showing(&mut app, left, id);
    app.press("secondary-b");
    assert_eq!(showing(&mut app, left, id), !shown, "its key toggles it");
    if shown {
        app.press("secondary-b");
    }
    assert!(showing(&mut app, left, id), "and back");
    assert!(
        rail_icons(&mut app, left).contains(&Icon::FileText),
        "its rail button"
    );

    // Its own commands hold inside it.
    focus(&mut app, &notes);
    app.press("f5");
    assert_eq!(app.cx().read(|cx| cx.global::<Cleared>().0), 1);

    // Hiding it, its key takes focus out of it.
    app.press("secondary-b");
    assert!(!showing(&mut app, left, id), "its key hides it");
    assert!(!focus_in(&mut app, &notes), "focus leaves the hidden panel");

    // The palette lists it, and runs it.
    let toggle = app.cx().read(|cx| {
        let palette = commands_in(&ContextStack::new(), cx);
        let toggle = palette
            .iter()
            .find(|entry| entry.id.as_str() == "macro_panel.notes.toggle");
        toggle.map(|entry| entry.title.clone())
    });
    assert_eq!(toggle.as_deref(), Some("Toggle Notes"));
    let shell = app
        .workspace()
        .read_with(app.cx(), |workspace, _| workspace.shell().clone());
    app.press("secondary-shift-p");
    assert_eq!(
        shell.read_with(app.cx(), |shell, cx| shell.modal(cx)),
        Some(COMMAND_PALETTE)
    );
    app.cx().simulate_input("toggle notes");
    app.press("enter");
    assert_eq!(shell.read_with(app.cx(), |shell, cx| shell.modal(cx)), None);
    assert!(showing(&mut app, left, id), "the palette shows it");

    // View > Toggle Notes, the same.
    let item = app.cx().read(|cx| {
        let bar = menus::installed(cx).expect("the menus are installed");
        let view = bar.menu("View").expect("a View menu");
        let item = (view.items.iter()).find(|item| item.id() == Some("macro_panel.notes.toggle"));
        item.and_then(|item| item.command().map(|command| command.title.clone()))
    });
    assert_eq!(item.as_deref(), Some("Toggle Notes"));
    focus(&mut app, &notes);
    app.menu("View", "Toggle Notes");
    assert!(!showing(&mut app, left, id), "the menu item hides it");
    assert!(!focus_in(&mut app, &notes), "focus leaves the hidden panel");
    app.menu("View", "Toggle Notes");
    assert!(showing(&mut app, left, id), "and shows it");
}

#[gpui::test]
fn dock_and_order_place_a_panel_the_rail_by_order_then_file_and_line(cx: &mut TestAppContext) {
    let mut app = open(cx);
    let right = DockPosition::Right;
    let (_, _, panels) = dock(&mut app, right);
    assert_eq!(
        panels,
        [
            PanelId("macro_panel.tags"),
            PanelId("macro_panel.outline"),
            PanelId("find.results"),
        ]
    );
    show(&mut app, right, "macro_panel.outline");
    assert_eq!(
        rail_icons(&mut app, right),
        [Icon::Package, Icon::ListTree, Icon::Search],
        "the rail"
    );

    // The View menu's toggles go the same way, by their order, then file
    // and line, among the commands placed there; the title falls back to
    // the doc line; `id` names the panel and its toggle.
    let items = app.cx().read(|cx| {
        let bar = menus::installed(cx).unwrap();
        let view = bar.menu("View").unwrap();
        let items = view.items.iter().filter_map(|item| item.command());
        // This test's, kiln's own View items left out.
        let ours = items.filter(|command| {
            let id = command.command.as_str();
            id.starts_with("macro_panel.") || id.starts_with("find.")
        });
        ours.map(|command| command.title.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        items,
        [
            "Toggle Notes",
            "Zoom Notes",
            "Toggle Tags",
            "Toggle Outline",
            "Toggle Search",
            "Toggle Watched",
        ]
    );
    show(&mut app, right, "find.results");
    show(&mut app, right, "macro_panel.tags");

    // Built from `Default`, or by `new`.
    let tags = panel::<Tags>(&mut app, right);
    assert_eq!(app.cx().read(|cx| tags.read(cx).0.clone()), ["kiln"]);
    let results = panel::<SearchResults>(&mut app, right);
    assert_eq!(app.cx().read(|cx| results.read(cx).query.clone()), "");
}

#[gpui::test]
fn a_panel_redraws_when_a_model_it_watches_changes(cx: &mut TestAppContext) {
    let mut app = open(cx);
    let left = DockPosition::Left;
    show(&mut app, left, "macro_panel.watched");
    let watched = panel::<Watched>(&mut app, left);
    let seen = |app: &mut TestApp| app.cx().read(|cx| watched.read(cx).seen);
    assert_eq!(seen(&mut app), 0);

    // Notified, which asks its window for a frame, when a model it
    // watches changes, and only then.
    let notified = Rc::new(Cell::new(0));
    let _observing = app.cx().update(|_, cx| {
        let notified = notified.clone();
        cx.observe(&watched, move |_, _| notified.set(notified.get() + 1))
    });
    app.cx()
        .update(|_, cx| kiln::model::update(cx, |count: &mut NoteCount, _| count.0 = 1));
    app.draw();
    assert_eq!(notified.get(), 0, "not on another model's change");

    // `update_model` from another view's `Cx`.
    let notes = panel::<Notes>(&mut app, left);
    app.cx().update(|window, cx| {
        notes.update(cx, |_, cx| {
            Cx { window, cx }.update_model(|count: &mut Count, _| count.0 = 3);
        })
    });
    assert_eq!(notified.get(), 1, "notified");
    app.draw();
    assert_eq!(seen(&mut app), 3, "it redraws");
    // Watched once, however often it drew.
    assert!(app.cx().read(|cx| watched.read(cx).renders) > 1);
    assert_eq!(app.cx().read(kiln::model::watchers::<Count>), 1);
}

/// The IDs of the invocations observed from now on.
fn observe(app: &mut TestApp) -> Rc<RefCell<Vec<String>>> {
    let observed = Rc::new(RefCell::new(Vec::new()));
    let record = observed.clone();
    app.cx().update(|_, cx| {
        observe_invocations(cx, move |invocation| {
            record.borrow_mut().push(invocation.command.to_string());
        });
    });
    observed
}

fn archived(app: &mut TestApp) -> usize {
    app.cx()
        .read(|cx| cx.try_global::<Archived>().map_or(0, |archived| archived.0))
}

#[gpui::test]
fn observers_hear_of_a_command_once_whatever_focus_does_before_its_handler_runs(
    cx: &mut TestAppContext,
) {
    let mut app = open(cx);
    show(&mut app, DockPosition::Right, "macro_panel.outline");
    show(&mut app, DockPosition::Left, "macro_panel.watched");
    let outline = panel::<Outline>(&mut app, DockPosition::Right);
    let watched = panel::<Watched>(&mut app, DockPosition::Left);
    let observed = observe(&mut app);

    // The outline handles it too, so its dispatch tells them; the
    // outline's listener blurs the window before it goes on to `archive`.
    focus(&mut app, &outline);
    app.run_command("macro_panel.archive");
    assert_eq!(archived(&mut app), 1);
    assert!(!focus_in(&mut app, &outline), "the listener blurred it");
    assert_eq!(*observed.borrow(), ["macro_panel.archive"]);

    // No view on the focus path has it, so `archive`'s handler tells
    // them, though focus moves into the outline before it runs.
    focus(&mut app, &watched);
    app.cx().update(|window, cx| {
        window
            .run_command(Invocation::new(Archive::id()), cx)
            .unwrap();
        window.focus(&outline.focus_handle(cx), cx);
    });
    app.draw();
    assert_eq!(archived(&mut app), 2);
    assert_eq!(
        *observed.borrow(),
        ["macro_panel.archive", "macro_panel.archive"]
    );
}

/// Where the bar drew the item `id`, if it did.
fn drawn(app: &mut TestApp, id: &str) -> Option<gpui::Bounds<Pixels>> {
    app.cx()
        .debug_bounds(Box::leak(format!("status-item:{id}").into_boxed_str()))
}

#[gpui::test]
fn a_status_fn_shows_on_the_bar_with_its_new_text_when_the_bar_next_draws(cx: &mut TestAppContext) {
    let mut app = open(cx);
    let width = app.cx().update(|window, _| window.viewport_size().width);
    let before = drawn(&mut app, "macro_panel.count").expect("the count shows");
    assert!(before.origin.x < width / 2., "on the left");
    let branch = drawn(&mut app, "git.branch").expect("an item of its own shows");
    assert!(branch.origin.x > width / 2., "on its own side");
    assert!(
        drawn(&mut app, "sync.state").is_none(),
        "None shows nothing"
    );

    // Changed: the bar shows it as it next draws, when the workspace
    // does.
    app.cx().update(|_, cx| {
        cx.set_global(NoteCount(1_234_567_890));
        cx.set_global(Syncing(true));
    });
    let workspace = app.workspace();
    app.cx()
        .update(|_, cx| workspace.update(cx, |_, cx| cx.notify()));
    app.draw();
    let after = drawn(&mut app, "macro_panel.count").expect("the count shows");
    assert!(
        after.size.width > before.size.width,
        "the new, longer text: {after:?} after {before:?}"
    );
    let syncing = drawn(&mut app, "sync.state").expect("syncing shows");
    assert!(syncing.origin.x > width / 2., "on the right");
}
