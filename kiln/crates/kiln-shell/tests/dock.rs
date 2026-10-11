// SPDX-License-Identifier: LicenseRef-Proprietary

//! A dock of registered panels: the first added shows, `show` and `toggle`
//! switch and hide it, a panel not always on the rail joins it once shown,
//! rail clicks switch or hide, a panel can ask for focus as it shows, the
//! rail's tooltips read the show command's key, and a panel added again
//! takes the old one's place.

use std::{cell::RefCell, marker::PhantomData, path::Path, rc::Rc, time::Duration};

use gpui::{
    AppContext as _, Context, Entity, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    Modifiers, ParentElement as _, Render, SharedString, Styled as _, TestAppContext,
    VisualTestContext, Window, div, px,
};
use kiln_core::{Keymap, KeymapLayer};
use kiln_platform::keys;
use kiln_shell::{
    Dock, DockItem, DockPosition, DockState, DockView as _, PanelId, dock::DockEvent,
};
use kiln_ui::{Icon, RAIL_WIDTH, UiTheme};

/// What sets one test panel apart from another.
trait Kind: 'static {
    const ID: PanelId;
    const ICON: Icon;
    const ON_RAIL: bool;
    const FOCUS_ON_SHOW: bool;
    const SHOW: Option<&'static str>;
}

/// Notes: always on the rail, focused as it shows, shown by `notes.show`.
struct Notes;

impl Kind for Notes {
    const ID: PanelId = PanelId("notes");
    const ICON: Icon = Icon::Files;
    const ON_RAIL: bool = true;
    const FOCUS_ON_SHOW: bool = true;
    const SHOW: Option<&'static str> = Some("notes.show");
}

/// A log: on the rail once it has shown.
struct Log;

impl Kind for Log {
    const ID: PanelId = PanelId("log");
    const ICON: Icon = Icon::ListTree;
    const ON_RAIL: bool = false;
    const FOCUS_ON_SHOW: bool = false;
    const SHOW: Option<&'static str> = None;
}

/// A focusable panel of kind `K`, with a generation to tell a panel made
/// anew from the old.
struct Panel<K: Kind> {
    generation: usize,
    focus: FocusHandle,
    kind: PhantomData<K>,
}

impl<K: Kind> Panel<K> {
    fn new(generation: usize, cx: &mut Context<Self>) -> Self {
        Self {
            generation,
            focus: cx.focus_handle(),
            kind: PhantomData,
        }
    }
}

impl<K: Kind> Render for Panel<K> {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().track_focus(&self.focus).size_full().child(K::ID.0)
    }
}

impl<K: Kind> Focusable for Panel<K> {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl<K: Kind> DockItem for Panel<K> {
    fn id(&self) -> PanelId {
        K::ID
    }

    fn title(&self, _: &gpui::App) -> SharedString {
        format!("The {}", K::ID.0).into()
    }

    fn icon(&self, _: &gpui::App) -> Icon {
        K::ICON
    }

    fn show_command(&self) -> Option<&'static str> {
        K::SHOW
    }

    fn always_on_rail(&self) -> bool {
        K::ON_RAIL
    }

    fn focus_on_show(&self) -> bool {
        K::FOCUS_ON_SHOW
    }
}

/// A focusable centre with the dock on its right.
struct Host {
    dock: Entity<Dock>,
    focus: FocusHandle,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("host")
            .size_full()
            .flex()
            .child(div().track_focus(&self.focus).flex_1().h_full())
            .child(self.dock.clone())
    }
}

struct Opened<'a> {
    host: Entity<Host>,
    dock: Entity<Dock>,
    notes: Entity<Panel<Notes>>,
    log: Entity<Panel<Log>>,
    events: Rc<RefCell<Vec<DockEvent>>>,
    cx: &'a mut VisualTestContext,
}

impl Opened<'_> {
    fn draw(&mut self) {
        self.cx.run_until_parked();
        self.cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    /// Whether the dock shows, and its active panel.
    fn state(&mut self) -> (bool, Option<PanelId>) {
        self.dock
            .read_with(self.cx, |dock, _| (dock.is_visible(), dock.active()))
    }

    fn click(&mut self, panel: PanelId) {
        let button = self.dock.read_with(self.cx, |dock, _| {
            dock.dock()
                .drawn()
                .rail_button_bounds(panel)
                .expect("drawn")
        });
        self.cx.simulate_click(button.center(), Modifiers::none());
        self.draw();
    }

    fn focused<V: Focusable>(&mut self, view: &Entity<V>) -> bool {
        self.cx
            .update(|window, cx| view.focus_handle(cx).contains_focused(window, cx))
    }

    fn icons(&mut self) -> Vec<Icon> {
        self.dock
            .read_with(self.cx, |dock, _| dock.dock().drawn().rail_icons())
    }
}

/// A host with a right dock of notes, then a log, its centre focused, the
/// keymap binding `notes.show` in the host's context.
fn open(cx: &mut TestAppContext) -> Opened<'_> {
    cx.update(|cx| {
        let keymap = r#"
            [[bindings]]
            keys = "ctrl-shift-n"
            command = "notes.show"
            context = "host"
        "#;
        let (layer, diagnostics) = KeymapLayer::parse(Path::new("keymap.toml"), keymap, None);
        assert_eq!(diagnostics, []);
        keys::init(cx, Keymap::from_layers([layer.unwrap()]));
    });
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    let notes = Rc::new(RefCell::new(None));
    let log = Rc::new(RefCell::new(None));
    let (made_notes, made_log) = (notes.clone(), log.clone());
    let (host, cx) = cx.add_window_view(move |_, cx| {
        let notes = cx.new(|cx| Panel::<Notes>::new(0, cx));
        let log = cx.new(|cx| Panel::<Log>::new(0, cx));
        made_notes.replace(Some(notes.clone()));
        made_log.replace(Some(log.clone()));
        let dock = cx.new(|cx| {
            let state = DockState::new(DockPosition::Right, UiTheme::default_dark())
                .size(px(200.))
                .shortcut_contexts(["host"]);
            let mut dock = Dock::with_state(state);
            dock.add_panel(notes, cx);
            dock.add_panel(log, cx);
            dock
        });
        cx.subscribe(&dock, move |_, _, event, _| sink.borrow_mut().push(*event))
            .detach();
        Host {
            dock,
            focus: cx.focus_handle(),
        }
    });
    let dock = host.read_with(cx, |host, _| host.dock.clone());
    let focus = host.read_with(cx, |host, _| host.focus.clone());
    cx.update(|window, cx| window.focus(&focus, cx));
    let mut opened = Opened {
        host,
        dock,
        notes: notes.take().unwrap(),
        log: log.take().unwrap(),
        events,
        cx,
    };
    opened.draw();
    opened
}

#[gpui::test]
fn the_first_panel_shows_and_others_join_the_rail(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let notes = PanelId("notes");
    let log = PanelId("log");
    assert_eq!(s.state(), (true, Some(notes)));
    let found = s.dock.read_with(s.cx, |dock, _| {
        (dock.panel::<Panel<Notes>>(), dock.panel::<Panel<Log>>())
    });
    assert_eq!(found, (Some(s.notes.clone()), Some(s.log.clone())));
    let title = s.dock.read_with(s.cx, |dock, cx| dock.dock().title(cx));
    assert_eq!(title, Some("The notes".into()));
    // The log isn't on the rail until it has shown.
    assert_eq!(s.icons(), [Icon::Files]);

    // The window's right edge: the rail, then the column left of it.
    let (rail, column) = s.dock.read_with(s.cx, |dock, _| {
        let drawn = dock.dock().drawn();
        (drawn.rail_bounds().unwrap(), drawn.column_bounds().unwrap())
    });
    let window = s.cx.update(|window, _| window.viewport_size());
    assert_eq!(rail.right(), window.width);
    assert_eq!(rail.size.width, RAIL_WIDTH);
    assert_eq!(column.right(), rail.left());
    assert_eq!(column.size.width, px(200.));

    let dock = s.dock.clone();
    s.cx.update(|window, cx| {
        dock.update(cx, |dock, cx| assert!(dock.show(log, window, cx)));
    });
    s.draw();
    assert_eq!(s.state(), (true, Some(log)));
    assert_eq!(s.icons(), [Icon::Files, Icon::ListTree]);
    // Showing it doesn't focus it, as it doesn't ask to be.
    let log_panel = s.log.clone();
    assert!(!s.focused(&log_panel));

    // Hidden, the rail stays, and the log keeps its place on it.
    dock.update(s.cx, |dock, cx| dock.toggle(cx));
    s.draw();
    assert_eq!(s.state(), (false, Some(log)));
    let (hidden, column) = s.dock.read_with(s.cx, |dock, _| {
        let drawn = dock.dock().drawn();
        (drawn.rail_bounds(), drawn.column_bounds())
    });
    assert_eq!(hidden, Some(rail));
    assert_eq!(column, None);
    assert_eq!(s.icons(), [Icon::Files, Icon::ListTree]);

    // An unknown panel shows nothing.
    s.cx.update(|window, cx| {
        dock.update(cx, |dock, cx| {
            assert!(!dock.show(PanelId("nothing"), window, cx))
        });
    });
    assert_eq!(s.state(), (false, Some(log)));
}

#[gpui::test]
fn rail_clicks_switch_and_hide(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let (notes, log) = (PanelId("notes"), PanelId("log"));
    let dock = s.dock.clone();
    s.cx.update(|window, cx| {
        dock.update(cx, |dock, cx| dock.show(log, window, cx));
    });
    s.draw();

    // Notes asks for focus as it shows.
    s.click(notes);
    assert_eq!(s.state(), (true, Some(notes)));
    let notes_panel = s.notes.clone();
    assert!(s.focused(&notes_panel));
    assert_eq!(*s.events.borrow(), []);

    // The highlighted panel's button hides the dock, and says it hid the
    // panel that had focus.
    s.click(notes);
    assert_eq!(s.state(), (false, Some(notes)));
    assert_eq!(*s.events.borrow(), [DockEvent::HidFocus]);
    let highlighted = s
        .dock
        .read_with(s.cx, |dock, _| dock.dock().rail_highlighted());
    assert_eq!(highlighted, None);
    let centre = s.host.read_with(s.cx, |host, _| host.focus.clone());
    s.cx.update(|window, cx| window.focus(&centre, cx));

    // Any button shows it again.
    s.click(log);
    assert_eq!(s.state(), (true, Some(log)));
    assert_eq!(s.events.borrow().len(), 1);
    let hidden_focused =
        s.cx.update(|window, cx| dock.read(cx).dock().hidden_panel_focused(window, cx));
    assert!(!hidden_focused);
}

#[gpui::test]
fn tooltips_show_the_title_and_key(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let hover = |s: &mut Opened, panel: PanelId| {
        let button = s.dock.read_with(s.cx, |dock, _| {
            dock.dock()
                .drawn()
                .rail_button_bounds(panel)
                .expect("drawn")
        });
        s.cx.simulate_mouse_move(button.center(), None, Modifiers::none());
        s.cx.executor().advance_clock(Duration::from_millis(600));
        s.draw();
        s.dock
            .read_with(s.cx, |dock, _| dock.dock().drawn().last_tooltip())
    };
    // The key, as this platform prints it, after the title.
    let notes = hover(&mut s, PanelId("notes"));
    assert!(
        notes
            .as_deref()
            .is_some_and(|text| text.starts_with("The notes ") && text.ends_with('N')),
        "{notes:?}"
    );
    let dock = s.dock.clone();
    s.cx.update(|window, cx| {
        dock.update(cx, |dock, cx| dock.show(PanelId("log"), window, cx));
    });
    s.draw();
    s.cx.simulate_mouse_move(gpui::point(px(5.), px(5.)), None, Modifiers::none());
    s.draw();
    // No show command: the title alone.
    assert_eq!(hover(&mut s, PanelId("log")), Some("The log".to_owned()));
}

#[gpui::test]
fn a_panel_added_again_takes_the_old_ones_place(cx: &mut TestAppContext) {
    let mut s = open(cx);
    let dock = s.dock.clone();
    s.cx.update(|window, cx| {
        dock.update(cx, |dock, cx| dock.show(PanelId("log"), window, cx));
    });
    let fresh = s.cx.new(|cx| Panel::<Notes>::new(1, cx));
    dock.update(s.cx, |dock, cx| dock.add_panel(fresh.clone(), cx));
    s.draw();
    let (ids, found, state) = s.dock.read_with(s.cx, |dock, _| {
        let notes = dock.panel::<Panel<Notes>>();
        (dock.dock().panel_ids(), notes, dock.active())
    });
    assert_eq!(ids, [PanelId("notes"), PanelId("log")]);
    assert_eq!(found, Some(fresh.clone()));
    assert_ne!(found, Some(s.notes.clone()));
    // The dock as it was.
    assert_eq!(state, Some(PanelId("log")));
    assert_eq!(fresh.read_with(s.cx, |panel, _| panel.generation), 1);
}

#[gpui::test]
fn a_left_dock_has_its_rail_on_the_left(cx: &mut TestAppContext) {
    let (host, cx) = cx.add_window_view(|_, cx| {
        let notes = cx.new(|cx| Panel::<Notes>::new(0, cx));
        let dock = cx.new(|cx| {
            let mut dock = Dock::new(DockPosition::Left, UiTheme::default_dark());
            dock.add_panel(notes, cx);
            dock
        });
        Host {
            dock,
            focus: cx.focus_handle(),
        }
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    let dock = host.read_with(cx, |host, _| host.dock.clone());
    let (rail, column) = dock.read_with(cx, |dock, _| {
        let drawn = dock.dock().drawn();
        (drawn.rail_bounds().unwrap(), drawn.column_bounds().unwrap())
    });
    assert_eq!(
        dock.read_with(cx, |dock, _| dock.dock().position()),
        DockPosition::Left
    );
    // In this host the dock is on the right of the centre, but its rail is
    // on its own left edge, the panel beside it.
    assert_eq!(column.left(), rail.right());
    assert_eq!(column.size.width, px(240.));
}
