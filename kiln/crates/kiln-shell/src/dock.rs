// SPDX-License-Identifier: Apache-2.0

//! A dock: panels beside the centre, one shown at a time under a title row
//! naming it, picked from an activity rail on the window's edge, which
//! stays when the dock hides. Switching keeps each panel's state.
//!
//! A panel is any view implementing [`DockItem`], registered with
//! [`DockState::add_panel`] and named by its [`PanelId`]. The rail lists
//! the panels in registration order, in two groups: the top one, and at
//! a side dock's foot the [`DockItem::on_rail_foot`] panels, then the
//! [`RailAction`]s. One that isn't [`DockItem::always_on_rail`] joins it
//! once it has shown. The selection's bar is on the rail's edge facing the
//! panel, and the host can badge a panel's button
//! ([`DockState::set_badge`]).
//!
//! The title row is the panel's one heading: its name in the UI heading
//! style, then the pane controls, right-aligned: the panel's own actions
//! ([`DockItem::actions`]), ⋯ for the dock's menu ([`DockState::set_menu`]),
//! and — to hide the dock ([`DockView::hide_clicked`]).
//!
//! [`DockState`] is the dock's state, for the view that draws it, a
//! [`DockView`]: [`Dock`] is kiln's, and a host can hold the state in a
//! view of its own to add to what a rail click does. Either way,
//! [`DockState::render`] draws it.
//!
//! A shown dock's inner edge, the one facing the centre, has a handle
//! that drags its size, between [`MIN_DOCK_WIDTH`] (or
//! [`MIN_DOCK_HEIGHT`] at the bottom) and what leaves the centre
//! [`MIN_PANE_WIDTH`] (or [`MIN_PANE_HEIGHT`]); pressed twice, it puts back
//! the size the dock was built with. A dock keeps a size for each axis,
//! its width at a side and its height at the bottom, so
//! [`DockState::set_position`] moving it between them finds each as it was
//! last left. A window too small for its size
//! draws it smaller, by layout: the dock's column shrinks to its least
//! before the centre, given its own least ([`MIN_PANE_WIDTH`],
//! [`MIN_PANE_HEIGHT`]) as the [`Shell`](crate::Shell) gives it, does. A
//! drag or [`DockState::max_size`] stops at what leaves the centre its
//! least, as [`track_centre`], which the shell draws in it, measured it.
//!
//! ```ignore
//! let dock = cx.new(|cx| {
//!     let mut dock = Dock::new(DockPosition::Left);
//!     dock.add_panel(outline.clone(), cx);
//!     dock
//! });
//! dock.update(cx, |dock, cx| dock.show(OUTLINE, window, cx));
//! ```

use std::{
    any::TypeId,
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
};

use gpui::{
    Anchor, AnyElement, AnyEntity, AnyView, App, AppContext as _, Bounds, Context, CursorStyle,
    DragMoveEvent, Entity, EntityId, EventEmitter, FocusHandle, Focusable, FontWeight, Global,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    ParentElement as _, Pixels, Point, Render, ScrollHandle, SharedString, Size,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, WindowId, anchored, canvas,
    deferred, div, prelude::FluentBuilder as _, px,
};
use kiln_command::{CommandId, ContextFrame, ContextStack, Invocation, Source, Value};
use kiln_core::{shortcut_label, shortcuts};
use kiln_l10n::tr;
use kiln_platform::{command_actions::dispatch_invocation, keys};
use kiln_ui::{
    ActivityRail, Badge, BadgeTone, Icon, IconButton, MenuEntry, MenuPanel, RAIL_WIDTH, RailButton,
    TAB_HEIGHT, Tooltip, UI_HEADING, ui_theme,
};

use crate::pane_group::{MIN_PANE_HEIGHT, MIN_PANE_WIDTH};

/// The narrowest a side dock's panel gets by dragging its edge.
pub const MIN_DOCK_WIDTH: Pixels = px(160.);
/// The shortest a bottom dock's panel gets by dragging its edge.
pub const MIN_DOCK_HEIGHT: Pixels = px(80.);
/// How far across the inner edge's handle is, centred on its 1 px border.
const HANDLE: f32 = 5.;

/// Where the centre that a window's docks surround was last laid out, and
/// the window's content size then.
#[derive(Clone, Copy)]
struct Centre {
    bounds: Bounds<Pixels>,
    viewport: Size<Pixels>,
}

/// Each window's [`Centre`], as [`track_centre`] last measured it.
#[derive(Default)]
struct Centres(HashMap<WindowId, Centre>);

impl Global for Centres {}

/// Measures the centre that the window's docks surround, for their
/// limits: an element to put in the centre's own, positioned
/// (`relative`), that fills it and draws nothing. The
/// [`Shell`](crate::Shell) draws one; in a window without one, a dock's
/// limit leaves the centre its least only as the window's size alone tells.
pub fn track_centre() -> impl IntoElement {
    let measure = |bounds, window: &mut Window, cx: &mut App| {
        let centre = Centre {
            bounds,
            viewport: window.viewport_size(),
        };
        let id = window.window_handle().window_id();
        cx.default_global::<Centres>().0.insert(id, centre);
    };
    canvas(measure, |_, _, _, _| {})
        .absolute()
        .top_0()
        .left_0()
        .size_full()
}

/// Forgets the centre of `window`, which has gone.
pub(crate) fn forget_centre(window: WindowId, cx: &mut App) {
    if cx
        .try_global::<Centres>()
        .is_some_and(|centres| centres.0.contains_key(&window))
    {
        cx.global_mut::<Centres>().0.remove(&window);
    }
}

/// A panel's identity among a dock's panels: `"project"`, `"search"`, …
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PanelId(pub &'static str);

/// Which edge of the window a dock is on. Its rail is on that edge, its
/// panel beside it towards the centre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DockPosition {
    Left,
    #[default]
    Right,
    /// Below the centre, above the status bar. Its rail's buttons sit in
    /// the title row.
    Bottom,
}

/// A panel in a dock.
pub trait DockItem: Render + Focusable + 'static {
    /// Distinct among the dock's panels.
    fn id(&self) -> PanelId;

    /// The title row's text, and its rail button's tooltip.
    fn title(&self, cx: &App) -> SharedString;

    /// Its rail button's icon.
    fn icon(&self, cx: &App) -> Icon;

    /// The command that shows it, whose key the rail's tooltip shows.
    fn show_command(&self) -> Option<&'static str> {
        None
    }

    /// On the rail from the start; `false` joins it once first shown.
    fn always_on_rail(&self) -> bool {
        true
    }

    /// Whether a panel that isn't [`DockItem::always_on_rail`] joins the
    /// rail once shown. `false` keeps it off the rail, shown by its
    /// command alone.
    fn joins_rail_on_show(&self) -> bool {
        true
    }

    /// Focused when shown, as a search panel focuses its input.
    fn focus_on_show(&self) -> bool {
        false
    }

    /// Whether its rail button is in the rail's bottom group, before the
    /// [`RailAction`]s, rather than its top one.
    fn on_rail_foot(&self) -> bool {
        false
    }

    /// Its own actions, at the right of the dock's title row, before ⋯ and
    /// —: none unless a panel has some.
    fn actions(&self, _cx: &App) -> Vec<PaneAction> {
        Vec::new()
    }

    /// Its own state, for a saved layout: [`DockState::save`] keeps it
    /// under the panel's id. None, saving nothing, unless a panel has
    /// state to keep, as a [`ViewContainer`](crate::ViewContainer) keeps
    /// its views' collapse.
    fn save_state(&self, _cx: &App) -> Option<Value> {
        None
    }

    /// Puts back what [`DockItem::save_state`] saved, as
    /// [`DockState::restore`] finds it under the panel's id. It does
    /// nothing unless a panel saves state.
    fn restore_state(&mut self, _saved: &Value, _cx: &mut Context<Self>) {}
}

/// A registered panel: its entity, without its type.
trait PanelHandle {
    fn id(&self) -> PanelId;
    fn title(&self, cx: &App) -> SharedString;
    fn icon(&self, cx: &App) -> Icon;
    fn show_command(&self, cx: &App) -> Option<&'static str>;
    fn always_on_rail(&self, cx: &App) -> bool;
    fn joins_rail_on_show(&self, cx: &App) -> bool;
    fn focus_on_show(&self, cx: &App) -> bool;
    fn on_rail_foot(&self, cx: &App) -> bool;
    fn actions(&self, cx: &App) -> Vec<PaneAction>;
    fn focus_handle(&self, cx: &App) -> FocusHandle;
    fn save_state(&self, cx: &App) -> Option<Value>;
    fn restore_state(&self, saved: &Value, cx: &mut App);
    /// Calls `changed` as the panel notifies.
    fn observe(&self, changed: Box<dyn Fn(&mut App)>, cx: &mut App) -> Subscription;
    fn to_any_view(&self) -> AnyView;
    fn to_any_entity(&self) -> AnyEntity;
    fn type_id(&self) -> TypeId;
}

/// A panel's entity, and its id, read when it was added.
struct Panel<P: DockItem> {
    id: PanelId,
    entity: Entity<P>,
}

impl<P: DockItem> PanelHandle for Panel<P> {
    fn id(&self) -> PanelId {
        self.id
    }

    fn title(&self, cx: &App) -> SharedString {
        self.entity.read(cx).title(cx)
    }

    fn icon(&self, cx: &App) -> Icon {
        self.entity.read(cx).icon(cx)
    }

    fn show_command(&self, cx: &App) -> Option<&'static str> {
        self.entity.read(cx).show_command()
    }

    fn always_on_rail(&self, cx: &App) -> bool {
        self.entity.read(cx).always_on_rail()
    }

    fn joins_rail_on_show(&self, cx: &App) -> bool {
        self.entity.read(cx).joins_rail_on_show()
    }

    fn focus_on_show(&self, cx: &App) -> bool {
        self.entity.read(cx).focus_on_show()
    }

    fn on_rail_foot(&self, cx: &App) -> bool {
        self.entity.read(cx).on_rail_foot()
    }

    fn actions(&self, cx: &App) -> Vec<PaneAction> {
        self.entity.read(cx).actions(cx)
    }

    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.entity.focus_handle(cx)
    }

    fn save_state(&self, cx: &App) -> Option<Value> {
        self.entity.read(cx).save_state(cx)
    }

    fn restore_state(&self, saved: &Value, cx: &mut App) {
        (self.entity).update(cx, |panel, cx| panel.restore_state(saved, cx));
    }

    fn observe(&self, changed: Box<dyn Fn(&mut App)>, cx: &mut App) -> Subscription {
        cx.observe(&self.entity, move |_, cx| changed(cx))
    }

    fn to_any_view(&self) -> AnyView {
        self.entity.clone().into()
    }

    fn to_any_entity(&self) -> AnyEntity {
        self.entity.clone().into()
    }

    fn type_id(&self) -> TypeId {
        TypeId::of::<P>()
    }
}

/// A rail button that isn't one of the dock's panels: it runs `command`
/// when clicked and leaves the dock as it is. On a side dock's rail it is
/// pinned to the bottom, below the panel buttons; on a bottom dock's it
/// follows them.
#[derive(Clone, Debug)]
pub struct RailAction {
    pub id: &'static str,
    pub icon: Icon,
    /// Its tooltip's text.
    pub title: SharedString,
    /// The command a click runs, whose key the tooltip shows.
    pub command: &'static str,
}

/// One of a panel's own actions, in the dock's title row before ⋯ and —
/// ([`DockItem::actions`]): an icon button, named by its title, that runs
/// `run` when clicked.
#[derive(Clone)]
pub struct PaneAction {
    /// Distinct among the panel's actions: its button's `icon-button:<id>`
    /// selector.
    pub id: &'static str,
    pub icon: Icon,
    /// Its accessible name and tooltip.
    pub title: SharedString,
    /// The command it does, whose key its tooltip shows, if it has one.
    pub command: Option<&'static str>,
    pub run: ActionHandler,
}

/// What a [`PaneAction`]'s click runs.
pub type ActionHandler = Rc<dyn Fn(&mut Window, &mut App)>;

impl std::fmt::Debug for PaneAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (f.debug_struct("PaneAction"))
            .field("id", &self.id)
            .field("icon", &self.icon)
            .field("title", &self.title)
            .field("command", &self.command)
            .finish_non_exhaustive()
    }
}

/// An item of the dock's ⋯ menu ([`DockState::set_menu`]): it runs
/// `command`, in the focused context, when clicked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockMenuItem {
    pub title: SharedString,
    pub command: &'static str,
}

/// The key context of the open ⋯ menu, which has focus.
pub const MENU_KEY_CONTEXT: &str = "dock_menu";

/// The ids of the title row's own buttons, after the panel's actions:
/// `icon-button:dock-more` and `icon-button:dock-hide`.
pub const MORE_BUTTON: &str = "dock-more";
pub const HIDE_BUTTON: &str = "dock-hide";

/// A badge on a panel's rail button, which the host sets
/// ([`DockState::set_badge`]) as what it counts changes: a count, or a dot
/// with none, in a colour by its meaning, and the accessible name that says
/// what it means, such as "3 changed files".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RailBadge {
    pub count: Option<usize>,
    pub tone: BadgeTone,
    pub name: SharedString,
}

impl RailBadge {
    /// A count of `count`.
    pub fn count(count: usize, tone: BadgeTone, name: impl Into<SharedString>) -> Self {
        Self {
            count: Some(count),
            tone,
            name: name.into(),
        }
    }

    /// A dot.
    pub fn dot(tone: BadgeTone, name: impl Into<SharedString>) -> Self {
        Self {
            count: None,
            tone,
            name: name.into(),
        }
    }

    /// The badge drawn for it on the rail's `ix`th button.
    fn element(&self, ix: usize) -> Badge {
        let badge = match self.count {
            Some(count) => Badge::count(count),
            None => Badge::dot(),
        };
        badge
            .tone(self.tone)
            .name(("dock-rail-badge", ix), self.name.clone())
    }
}

/// What the last frame drew of a dock: where its rail, its buttons and its
/// column were laid out, the icons its buttons drew and the last tooltip
/// built. Cloning it shares it, so a host can read it without the dock.
#[derive(Clone, Default)]
pub struct DrawnDock {
    /// The rail's buttons.
    rail: ScrollHandle,
    /// The dock's column, if shown, then the rail.
    layout: ScrollHandle,
    /// The ids of the rail's drawn items, top to bottom, an action's by its
    /// own; a side rail's gap above its actions has an empty one.
    buttons: Rc<RefCell<Vec<PanelId>>>,
    icons: Rc<RefCell<Vec<Icon>>>,
    /// The ids of the buttons drawn highlighted.
    highlighted: Rc<RefCell<Vec<PanelId>>>,
    /// The badges drawn, by their buttons' ids, top to bottom.
    badges: Rc<RefCell<Vec<(PanelId, RailBadge)>>>,
    tooltip: Rc<RefCell<Option<String>>>,
    /// The title row's buttons, left to right: each one's id and
    /// accessible name.
    controls: Rc<RefCell<Vec<(String, SharedString)>>>,
    /// The ⋯ button, whose own press doesn't close its menu: it toggles it.
    more: ScrollHandle,
    /// The column's title row, its panel and its edge's handle.
    column: ScrollHandle,
    /// The handle's cursor, if drawn.
    cursor: Rc<Cell<Option<CursorStyle>>>,
    /// Whether the rail's buttons were drawn in the column's title row, as
    /// a shown bottom dock's are, rather than beside or below it.
    rail_in_title: Rc<Cell<bool>>,
    /// Whether the dock was built without a rail, which draws no rail at all.
    no_rail: Rc<Cell<bool>>,
    /// Whether the column was drawn: a dock without a rail draws nothing
    /// hidden, which leaves its layout as it was.
    column_drawn: Rc<Cell<bool>>,
}

impl DrawnDock {
    /// Where `panel`'s rail button, or the [`RailAction`] of that id, was
    /// laid out, if it was drawn.
    pub fn rail_button_bounds(&self, panel: PanelId) -> Option<Bounds<Pixels>> {
        let ix = self.buttons.borrow().iter().position(|id| *id == panel)?;
        self.rail.bounds_for_item(ix)
    }

    /// Where the rail was laid out: at a side, the column of buttons on
    /// the window's edge; at the bottom, the row of buttons, in the title
    /// row while the dock shows.
    pub fn rail_bounds(&self) -> Option<Bounds<Pixels>> {
        if self.no_rail.get() {
            return None;
        }
        if self.rail_in_title.get() {
            return self.column_bounds().map(|_| self.rail.bounds());
        }
        let count = self.layout.children_count();
        self.layout.bounds_for_item(count.checked_sub(1)?)
    }

    /// Where the dock's column, its title row then its panel, was laid
    /// out, if it showed.
    pub fn column_bounds(&self) -> Option<Bounds<Pixels>> {
        if self.no_rail.get() && !self.column_drawn.get() {
            return None;
        }
        // Beside or above the rail, unless the rail is in its title row.
        let children = if self.rail_in_title.get() || self.no_rail.get() {
            1
        } else {
            2
        };
        (self.layout.children_count() == children)
            .then(|| self.layout.bounds_for_item(0))
            .flatten()
    }

    /// The title row's buttons the last frame drew, left to right, by id
    /// and accessible name: the panel's actions, then [`MORE_BUTTON`] if the
    /// dock has a menu, then [`HIDE_BUTTON`].
    pub fn controls(&self) -> Vec<(String, SharedString)> {
        self.controls.borrow().clone()
    }

    /// Where the column's title row was laid out, if it showed.
    pub fn title_bounds(&self) -> Option<Bounds<Pixels>> {
        self.column_bounds()?;
        self.column.bounds_for_item(0)
    }

    /// The ids of the rail's buttons, panels' and actions', top to bottom,
    /// as [`DrawnDock::rail_icons`] lists their icons.
    pub fn rail_ids(&self) -> Vec<PanelId> {
        (self.buttons.borrow().iter())
            .filter(|id| !id.0.is_empty())
            .copied()
            .collect()
    }

    /// The icons the rail's buttons drew, top to bottom.
    pub fn rail_icons(&self) -> Vec<Icon> {
        self.icons.borrow().clone()
    }

    /// The ids of the rail's buttons drawn highlighted, panels' and actions'.
    pub fn rail_highlighted(&self) -> Vec<PanelId> {
        self.highlighted.borrow().clone()
    }

    /// The badges the rail's buttons drew, by their panels' ids, top to
    /// bottom.
    pub fn rail_badges(&self) -> Vec<(PanelId, RailBadge)> {
        self.badges.borrow().clone()
    }

    /// The text of the last tooltip a rail button built.
    pub fn last_tooltip(&self) -> Option<String> {
        self.tooltip.borrow().clone()
    }

    /// Where the handle on the column's inner edge was laid out, if the
    /// column showed.
    pub fn handle_bounds(&self) -> Option<Bounds<Pixels>> {
        self.column_bounds()?;
        self.column.bounds_for_item(2)
    }

    /// The cursor the handle shows, if the column showed.
    pub fn handle_cursor(&self) -> Option<CursorStyle> {
        self.cursor.get()
    }
}

/// A drag of the handle on a dock's inner edge, which the dock at that
/// position follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct EdgeDrag(DockPosition);

/// An edge's drag draws nothing at the pointer.
struct NoGhost;

impl Render for NoGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

/// A dock's panels, the one shown, and whether the dock shows: the state
/// a [`DockView`] holds and draws with [`DockState::render`].
pub struct DockState {
    position: DockPosition,
    panels: Vec<Box<dyn PanelHandle>>,
    active: Option<PanelId>,
    visible: bool,
    /// Panels not always on the rail that have shown, which puts them on
    /// it.
    joined: HashSet<PanelId>,
    /// The rail's actions, with whether each is highlighted.
    actions: Vec<(RailAction, bool)>,
    /// The badges on panels' rail buttons, as the host set them.
    badges: HashMap<PanelId, RailBadge>,
    /// The panel's width at a side.
    side_size: Pixels,
    /// The panel's height at the bottom.
    bottom_size: Pixels,
    /// The size it was built with, which a double-click on the edge puts
    /// back, at least the position's least.
    built: Pixels,
    /// Where the edge's drag started: the pointer, and the size drawn.
    press: Option<(Point<Pixels>, Pixels)>,
    /// The key contexts a rail button's tooltip looks its show command's
    /// key up in, outermost first.
    shortcut_contexts: Vec<SharedString>,
    /// Whether it draws no rail, at any position.
    rail_hidden: bool,
    /// The ⋯ menu's items, whether it's open and the item highlighted.
    menu: Vec<DockMenuItem>,
    menu_open: bool,
    menu_highlighted: Option<usize>,
    /// The open menu's focus, made the first time it opens, and what had
    /// focus before it, which it gives focus back to as it closes.
    menu_focus: Option<FocusHandle>,
    menu_return: Option<FocusHandle>,
    /// The command — runs, if not hiding the dock itself.
    hide_command: Option<&'static str>,
    drawn: DrawnDock,
}

impl DockState {
    /// An empty dock at `position`, shown, 240 px across. It draws in the
    /// active theme's colours ([`ui_theme`]), so it follows a theme change.
    pub fn new(position: DockPosition) -> Self {
        Self {
            position,
            panels: Vec::new(),
            active: None,
            visible: true,
            joined: HashSet::new(),
            actions: Vec::new(),
            badges: HashMap::new(),
            side_size: px(240.),
            bottom_size: px(240.),
            built: px(240.),
            press: None,
            shortcut_contexts: Vec::new(),
            rail_hidden: false,
            menu: Vec::new(),
            menu_open: false,
            menu_highlighted: None,
            menu_focus: None,
            menu_return: None,
            hide_command: None,
            drawn: DrawnDock::default(),
        }
    }

    /// Whether the dock draws no rail, at any position: no panel buttons and
    /// no actions, only the open panel's column. Its panels are shown by
    /// command or by another dock's [`RailAction`]. A rail is the default.
    pub fn hide_rail(&mut self, hidden: bool) {
        self.rail_hidden = hidden;
    }

    /// The items of the title row's ⋯ menu: with none, the row has no ⋯.
    pub fn set_menu(&mut self, items: Vec<DockMenuItem>) {
        if items.is_empty() {
            self.menu_open = false;
        }
        self.menu = items;
    }

    /// The ⋯ menu's items.
    pub fn menu(&self) -> &[DockMenuItem] {
        &self.menu
    }

    /// Whether the ⋯ menu is open.
    pub fn is_menu_open(&self) -> bool {
        self.menu_open
    }

    /// The ⋯ menu's highlighted item, while it's open.
    pub fn menu_highlighted(&self) -> Option<usize> {
        self.menu_highlighted.filter(|_| self.menu_open)
    }

    /// Whether the open ⋯ menu has focus, which its keys need.
    pub fn menu_focused(&self, window: &Window) -> bool {
        (self.menu_focus.as_ref()).is_some_and(|focus| self.menu_open && focus.is_focused(window))
    }

    /// Opens the ⋯ menu, if it has items, its first item highlighted and
    /// focus in it, for its keys: Up and Down move the highlight, Enter or
    /// Space runs the item, Escape closes it.
    pub fn open_menu(&mut self, window: &mut Window, cx: &mut App) {
        if self.menu.is_empty() {
            return;
        }
        self.menu_open = true;
        self.menu_highlighted = Some(0);
        let focus = self
            .menu_focus
            .get_or_insert_with(|| cx.focus_handle())
            .clone();
        self.menu_return = window.focused(cx).filter(|focused| *focused != focus);
        window.focus(&focus, cx);
    }

    /// Closes the ⋯ menu, giving focus back to what had it before, if the
    /// menu still has it.
    pub fn close_menu(&mut self, window: &mut Window, cx: &mut App) {
        let focused = self.menu_focused(window);
        self.menu_open = false;
        self.menu_highlighted = None;
        let back = self.menu_return.take();
        if focused && let Some(back) = back {
            window.focus(&back, cx);
        }
    }

    /// The open menu's answer to `key`: whether it took it. Running an
    /// item closes the menu first, so the command runs where focus was.
    fn menu_key(&mut self, key: &str, window: &mut Window, cx: &mut App) -> bool {
        let count = self.menu.len();
        let at = self.menu_highlighted.unwrap_or(0);
        match key {
            "up" => self.menu_highlighted = Some((at + count - 1) % count),
            "down" => self.menu_highlighted = Some((at + 1) % count),
            "home" => self.menu_highlighted = Some(0),
            "end" => self.menu_highlighted = Some(count - 1),
            "enter" | "space" => {
                let command = self.menu.get(at).map(|item| item.command);
                self.close_menu(window, cx);
                if let Some(command) = command {
                    window.defer(cx, move |window, cx| run_action(command, window, cx));
                }
            }
            "escape" => self.close_menu(window, cx),
            _ => return false,
        }
        true
    }

    /// Builds it with — running `command`, in the focused context, whose
    /// key its tooltip shows, rather than hiding the dock itself: for a
    /// host whose hide does more, such as moving focus.
    pub fn hide_command(mut self, command: &'static str) -> Self {
        self.hide_command = Some(command);
        self
    }

    /// The command — runs, if it was built with one.
    pub fn hide_command_id(&self) -> Option<&'static str> {
        self.hide_command
    }

    /// Builds it without a rail: see [`DockState::hide_rail`].
    pub fn without_rail(mut self) -> Self {
        self.rail_hidden = true;
        self
    }

    /// Builds it with its panel `size` across, its width at a side and its
    /// height at the bottom, each at least its least: the size a
    /// double-click on its edge puts back.
    pub fn size(mut self, size: Pixels) -> Self {
        self.side_size = size.max(MIN_DOCK_WIDTH);
        self.bottom_size = size.max(MIN_DOCK_HEIGHT);
        self.built = size;
        self
    }

    /// The panel's width, or its height in a bottom dock, as last set: a
    /// window too small for it draws it smaller. (Not `size()`, the
    /// builder's name.)
    pub fn current_size(&self) -> Pixels {
        match self.position {
            DockPosition::Bottom => self.bottom_size,
            DockPosition::Left | DockPosition::Right => self.side_size,
        }
    }

    /// The panel's width at a side, wherever the dock is.
    pub fn side_size(&self) -> Pixels {
        self.side_size
    }

    /// The panel's height at the bottom, wherever the dock is.
    pub fn bottom_size(&self) -> Pixels {
        self.bottom_size
    }

    /// Sets the panel's width, or its height in a bottom dock, at least
    /// [`MIN_DOCK_WIDTH`] or [`MIN_DOCK_HEIGHT`]. The other is left as it is.
    pub fn set_size(&mut self, size: Pixels) {
        let size = size.max(self.min_size());
        *self.size_mut() = size;
    }

    fn size_mut(&mut self) -> &mut Pixels {
        match self.position {
            DockPosition::Bottom => &mut self.bottom_size,
            DockPosition::Left | DockPosition::Right => &mut self.side_size,
        }
    }

    /// The least size: [`MIN_DOCK_WIDTH`], or [`MIN_DOCK_HEIGHT`] at the
    /// bottom.
    pub fn min_size(&self) -> Pixels {
        match self.position {
            DockPosition::Bottom => MIN_DOCK_HEIGHT,
            DockPosition::Left | DockPosition::Right => MIN_DOCK_WIDTH,
        }
    }

    /// The most size there's room for in `window`: what leaves the centre
    /// [`MIN_PANE_WIDTH`] or [`MIN_PANE_HEIGHT`], beside the other docks
    /// and rows as they are, from the centre as [`track_centre`] last
    /// measured it and the column as last laid out, counting the window's
    /// change of size since. Before the centre is measured, the window's
    /// content size less the rail and the centre's least. Never less than
    /// [`DockState::min_size`].
    pub fn max_size(&self, window: &Window, cx: &App) -> Pixels {
        let (along, least): (fn(Size<Pixels>) -> Pixels, _) = match self.position {
            DockPosition::Bottom => (|size| size.height, MIN_PANE_HEIGHT),
            DockPosition::Left | DockPosition::Right => (|size| size.width, MIN_PANE_WIDTH),
        };
        let viewport = window.viewport_size();
        let centre = cx
            .try_global::<Centres>()
            .and_then(|centres| centres.0.get(&window.window_handle().window_id()).copied());
        let max = match centre {
            Some(centre) => {
                let column = self.drawn.column_bounds();
                let column = column.map_or(Pixels::ZERO, |column| along(column.size));
                let grown = along(viewport) - along(centre.viewport);
                along(centre.bounds.size) + column + grown - least
            }
            None => {
                // A rail in the title row takes nothing beside the column.
                let rail = match self.drawn.rail_in_title.get() || self.rail_hidden {
                    true => Pixels::ZERO,
                    false => (self.drawn.rail_bounds()).map_or(RAIL_WIDTH, |rail| along(rail.size)),
                };
                along(viewport) - rail - least
            }
        };
        max.max(self.min_size())
    }

    /// The size the column was drawn at, if it showed: the size, or less in
    /// a window too small for it.
    fn drawn_size(&self) -> Option<Pixels> {
        let column = self.drawn.column_bounds()?;
        Some(match self.position {
            DockPosition::Bottom => column.size.height,
            DockPosition::Left | DockPosition::Right => column.size.width,
        })
    }

    /// The key contexts, outermost first, that a rail button's tooltip
    /// looks its panel's show command up in: the app's root context and
    /// whatever is usually focused inside it. With none, tooltips show
    /// titles only.
    pub fn shortcut_contexts(
        mut self,
        contexts: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        self.shortcut_contexts = contexts.into_iter().map(Into::into).collect();
        self
    }

    pub fn position(&self) -> DockPosition {
        self.position
    }

    /// Moves the dock to `position`'s edge: it draws there from the next
    /// frame, a vertical rail at a side, its buttons in a row at the
    /// bottom, at the size it last had there, keeping its panels, active
    /// panel, visibility and rail actions. The host puts its view at that
    /// position ([`Shell::set_dock`](crate::Shell::set_dock)).
    pub fn set_position(&mut self, position: DockPosition) {
        if position != self.position {
            self.position = position;
            self.press = None;
        }
    }

    /// Adds `panel` at the end of the rail, or, if a panel with its id is
    /// registered, puts it in that one's place, which keeps the dock's
    /// state: for a panel made anew, as a project tree is for a new folder.
    /// The first panel added is the active one.
    pub fn add_panel<P: DockItem>(&mut self, panel: Entity<P>, cx: &App) {
        let id = panel.read(cx).id();
        let handle = Box::new(Panel { id, entity: panel });
        match self.panels.iter().position(|panel| panel.id() == id) {
            Some(ix) => self.panels[ix] = handle,
            None => self.panels.push(handle),
        }
        self.active.get_or_insert(id);
    }

    /// Adds `action` to the rail's actions, or, if one has its id, puts it
    /// in that one's place, highlighted as that was.
    pub fn add_action(&mut self, action: RailAction) {
        match self
            .actions
            .iter_mut()
            .find(|(have, _)| have.id == action.id)
        {
            Some((have, _)) => *have = action,
            None => self.actions.push((action, false)),
        }
    }

    /// Draws the action `id` highlighted, or not. The host calls it as what
    /// the action's command toggles changes.
    pub fn set_action_highlighted(&mut self, id: &str, highlighted: bool) {
        if let Some((_, was)) = self.actions.iter_mut().find(|(action, _)| action.id == id) {
            *was = highlighted;
        }
    }

    /// Puts `badge` on `panel`'s rail button, or takes its badge off. The
    /// host sets it as what the badge counts changes, and redraws the dock.
    pub fn set_badge(&mut self, panel: PanelId, badge: Option<RailBadge>) {
        match badge {
            Some(badge) => self.badges.insert(panel, badge),
            None => self.badges.remove(&panel),
        };
    }

    /// The badge on `panel`'s rail button, as set.
    pub fn badge(&self, panel: PanelId) -> Option<&RailBadge> {
        self.badges.get(&panel)
    }

    /// The registered panel of type `P`, if any.
    pub fn panel<P: DockItem>(&self) -> Option<Entity<P>> {
        let panel = self
            .panels
            .iter()
            .find(|panel| panel.type_id() == TypeId::of::<P>())?;
        panel.to_any_entity().downcast().ok()
    }

    /// The registered panels' ids, in rail order.
    pub fn panel_ids(&self) -> Vec<PanelId> {
        self.panels.iter().map(|panel| panel.id()).collect()
    }

    /// The panel shown while the dock is visible: the last shown, else the
    /// first added.
    pub fn active(&self) -> Option<PanelId> {
        self.active
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// The panel whose rail button is highlighted: the active one, while
    /// the dock shows.
    pub fn rail_highlighted(&self) -> Option<PanelId> {
        self.active.filter(|_| self.visible)
    }

    /// The active panel's title, for the title row.
    pub fn title(&self, cx: &App) -> Option<SharedString> {
        Some(self.handle(self.active?)?.title(cx))
    }

    /// Whether `panel` has a button on the rail.
    pub fn on_rail(&self, panel: PanelId, cx: &App) -> bool {
        self.handle(panel)
            .is_some_and(|handle| handle.always_on_rail(cx) || self.joined.contains(&panel))
    }

    /// Shows the dock with `panel`, focusing it if it's
    /// [`DockItem::focus_on_show`]. False, doing nothing, if no such panel
    /// is registered.
    pub fn show(&mut self, panel: PanelId, window: &mut Window, cx: &mut App) -> bool {
        let Some(handle) = self.handle(panel) else {
            return false;
        };
        let focus = handle.focus_on_show(cx).then(|| handle.focus_handle(cx));
        let joins = handle.joins_rail_on_show(cx);
        self.active = Some(panel);
        self.visible = true;
        if joins {
            self.joined.insert(panel);
        }
        if let Some(focus) = focus {
            window.focus(&focus, cx);
        }
        true
    }

    /// Shows the dock if it's hidden, else hides it. Focus on a panel the
    /// dock hides stays there: see [`DockState::hidden_panel_focused`].
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Shows or hides the dock, keeping its active panel.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// What a click on `panel`'s rail button does: hides the dock if it's
    /// the highlighted panel, else shows the dock with it.
    pub fn click_rail(&mut self, panel: PanelId, window: &mut Window, cx: &mut App) {
        if self.rail_highlighted() == Some(panel) {
            self.toggle();
        } else {
            self.show(panel, window, cx);
        }
    }

    /// Whether a panel that isn't showing, the dock being hidden or showing
    /// another, has focus: its host then focuses something shown, such as
    /// the active pane item.
    pub fn hidden_panel_focused(&self, window: &Window, cx: &App) -> bool {
        self.panels.iter().any(|panel| {
            self.rail_highlighted() != Some(panel.id())
                && panel.focus_handle(cx).contains_focused(window, cx)
        })
    }

    /// The panels not always on the rail that have joined it, by having
    /// shown, in rail order.
    pub fn joined(&self) -> Vec<PanelId> {
        (self.panels.iter())
            .map(|panel| panel.id())
            .filter(|id| self.joined.contains(id))
            .collect()
    }

    /// The dock's state, for a saved layout: whether it shows, its active
    /// panel, the panels that have joined the rail and its size, `"size"`
    /// at its position, `"side_size"` and `"bottom_size"` at each; and,
    /// if any panel saves state of its own ([`DockItem::save_state`]),
    /// `"panels"`, each one's by its id.
    pub fn save(&self, cx: &App) -> Value {
        let size = |size: Pixels| Value::from(f64::from(f32::from(size)));
        let ids = |ids: Vec<PanelId>| Value::from_iter(ids.into_iter().map(|id| id.0));
        let mut saved = Value::from_iter([
            ("visible", Value::from(self.visible)),
            ("active", self.active.map_or(Value::Null, |id| id.0.into())),
            ("joined", ids(self.joined())),
            ("size", size(self.current_size())),
            ("side_size", size(self.side_size)),
            ("bottom_size", size(self.bottom_size)),
        ]);
        let panels: serde_json::Map<_, _> = (self.panels.iter())
            .filter_map(|panel| Some((panel.id().0.to_owned(), panel.save_state(cx)?)))
            .collect();
        // Without, it's as a dock saved before panels had state.
        if !panels.is_empty() {
            saved["panels"] = Value::Object(panels);
        }
        saved
    }

    /// Puts back what [`DockState::save`] saved, as far as its panels are
    /// registered: whether it shows, its active panel (without focusing
    /// it), the panels that had joined the rail and its sizes, each at
    /// least its least: `"side_size"` and `"bottom_size"`, else, saved
    /// before a dock had both, `"size"` at its position. Each registered
    /// panel saved under `"panels"` gets its state back
    /// ([`DockItem::restore_state`]); the others are left as they are.
    pub fn restore(&mut self, saved: &Value, cx: &mut App) {
        let size = |key| {
            saved
                .get(key)
                .and_then(Value::as_f64)
                .map(|size| px(size as f32))
        };
        if let Some(size) = size("size") {
            self.set_size(size);
        }
        if let Some(size) = size("side_size") {
            self.side_size = size.max(MIN_DOCK_WIDTH);
        }
        if let Some(size) = size("bottom_size") {
            self.bottom_size = size.max(MIN_DOCK_HEIGHT);
        }
        let registered = |name: &str| {
            (self.panels.iter())
                .map(|panel| panel.id())
                .find(|id| id.0 == name)
        };
        if let Some(visible) = saved.get("visible").and_then(Value::as_bool) {
            self.visible = visible;
        }
        if let Some(active) = saved
            .get("active")
            .and_then(Value::as_str)
            .and_then(registered)
        {
            self.active = Some(active);
        }
        let joined: Vec<PanelId> = (saved.get("joined").and_then(Value::as_array))
            .into_iter()
            .flatten()
            .filter_map(|id| registered(id.as_str()?))
            .collect();
        let joins =
            |id: PanelId| (self.handle(id)).is_none_or(|handle| handle.joins_rail_on_show(cx));
        let joined: Vec<PanelId> = joined.into_iter().filter(|&id| joins(id)).collect();
        let active = self.active.filter(|&id| joins(id));
        self.joined.extend(joined);
        if let Some(active) = active {
            self.joined.insert(active);
        }
        let panels = saved.get("panels").and_then(Value::as_object);
        for panel in &self.panels {
            if let Some(state) = panels.and_then(|panels| panels.get(panel.id().0)) {
                panel.restore_state(state, cx);
            }
        }
    }

    /// What the last frame drew.
    pub fn drawn(&self) -> &DrawnDock {
        &self.drawn
    }

    /// A press on the edge's handle: the start of a drag from the size
    /// drawn, or, pressed twice, puts back the size the dock was built
    /// with and starts the drag from that: the last frame drew the size
    /// it replaces.
    fn press_edge(&mut self, event: &MouseDownEvent) {
        let from = if event.click_count == 2 {
            let built = self.built.max(self.min_size());
            *self.size_mut() = built;
            built
        } else {
            self.drawn_size().unwrap_or(self.current_size())
        };
        self.press = Some((event.position, from));
    }

    /// Sizes the dock to follow the edge's drag, the pointer now at `to`,
    /// within the limits.
    fn drag_edge(&mut self, to: Point<Pixels>, window: &Window, cx: &App) {
        let Some((at, pressed)) = self.press else {
            return;
        };
        let grown = match self.position {
            DockPosition::Right => at.x - to.x,
            DockPosition::Left => to.x - at.x,
            DockPosition::Bottom => at.y - to.y,
        };
        let max = self.max_size(window, cx);
        *self.size_mut() = (pressed + grown).min(max).max(self.min_size());
    }

    /// The handle on the column's inner edge, over its border and a
    /// little either side: pressing it starts a drag (twice puts back the
    /// built size), and moving draws nothing but the cursor.
    fn edge_handle<V: DockView>(&self, cx: &mut Context<V>) -> impl IntoElement {
        // From inside the column, whose border is its first pixel.
        let offset = px(-(HANDLE - 1.) / 2.);
        let cursor = match self.position {
            DockPosition::Bottom => CursorStyle::ResizeUpDown,
            DockPosition::Left | DockPosition::Right => CursorStyle::ResizeLeftRight,
        };
        self.drawn.cursor.set(Some(cursor));
        div()
            .id("dock-edge")
            .absolute()
            // Over the centre beside it, for presses, but not for drops.
            .when(!cx.has_active_drag(), |handle| handle.occlude())
            .map(|handle| match self.position {
                DockPosition::Right => handle.top_0().bottom_0().left(offset).w(px(HANDLE)),
                DockPosition::Left => handle.top_0().bottom_0().right(offset).w(px(HANDLE)),
                DockPosition::Bottom => handle.left_0().right_0().top(offset).h(px(HANDLE)),
            })
            .cursor(cursor)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, event: &MouseDownEvent, _, cx| {
                    view.dock_mut().press_edge(event);
                    cx.notify();
                }),
            )
            .on_drag(EdgeDrag(self.position), |_, _, _, cx| cx.new(|_| NoGhost))
    }

    /// Follows the edge's drag, on the dock's outermost element.
    fn on_edge_drag<V: DockView>(
        &self,
        cx: &mut Context<V>,
    ) -> impl Fn(&DragMoveEvent<EdgeDrag>, &mut Window, &mut App) + 'static {
        let position = self.position;
        cx.listener(move |view, event: &DragMoveEvent<EdgeDrag>, window, cx| {
            if *event.drag(cx) == EdgeDrag(position) {
                view.dock_mut().drag_edge(event.event.position, window, cx);
                cx.notify();
            }
        })
    }

    fn handle(&self, panel: PanelId) -> Option<&dyn PanelHandle> {
        self.panels
            .iter()
            .find(|handle| handle.id() == panel)
            .map(|handle| &**handle)
    }

    /// The first key sequence that runs `command` in the dock's shortcut
    /// contexts, as this platform prints it, read from the keymap now so
    /// it follows reloads.
    fn shortcut(contexts: &[SharedString], command: &str, cx: &App) -> Option<String> {
        if contexts.is_empty() {
            return None;
        }
        let command = CommandId::parse(command).ok()?;
        let stack: ContextStack = contexts
            .iter()
            .map(|context| ContextFrame::new(context.as_ref()))
            .collect();
        let keymap = keys::current_keymap(cx);
        let found = shortcuts(&keymap, &command, &Value::Null, &stack);
        found.first().map(shortcut_label)
    }

    /// A title-row button, `id`, named `name`: its tooltip shows the key
    /// of `command`, if it has one, looked up as it shows.
    fn control(
        &self,
        id: &str,
        icon: Icon,
        name: SharedString,
        command: Option<&'static str>,
        cx: &App,
    ) -> IconButton {
        self.drawn
            .controls
            .borrow_mut()
            .push((id.to_owned(), name.clone()));
        let contexts = self.shortcut_contexts.clone();
        let last_tooltip = self.drawn.tooltip.clone();
        let title = name.clone();
        IconButton::new(SharedString::from(id.to_owned()), icon, &ui_theme(cx))
            .name(name)
            .tooltip(move |_, cx| {
                let shortcut = command.and_then(|command| Self::shortcut(&contexts, command, cx));
                let tooltip = Tooltip::new(title.clone(), shortcut, &ui_theme(cx));
                last_tooltip.replace(Some(tooltip.text()));
                cx.new(|_| tooltip).into()
            })
    }

    /// The title row's pane controls, right-aligned: `panel`'s actions, ⋯
    /// if the dock has a menu, with the menu under it while open, and —.
    fn controls<V: DockView>(&self, panel: &dyn PanelHandle, cx: &mut Context<V>) -> AnyElement {
        self.drawn.controls.borrow_mut().clear();
        let actions: Vec<_> = (panel.actions(cx).into_iter())
            .map(|action| {
                let run = action.run.clone();
                self.control(action.id, action.icon, action.title, action.command, cx)
                    .on_click(move |_, window, cx| run(window, cx))
            })
            .collect();
        let more = (!self.menu.is_empty()).then(|| {
            let button = self
                .control(
                    MORE_BUTTON,
                    Icon::Ellipsis,
                    tr!("kiln-shell-dock-more"),
                    None,
                    cx,
                )
                .on_click(cx.listener(|view: &mut V, _, window, cx| {
                    match view.dock().is_menu_open() {
                        true => view.dock_mut().close_menu(window, cx),
                        false => view.dock_mut().open_menu(window, cx),
                    }
                    cx.notify();
                }));
            let menu = self.menu_open.then(|| self.menu_panel(cx));
            div()
                .id("dock-more")
                .relative()
                .track_scroll(&self.drawn.more)
                .child(button)
                .children(menu)
        });
        let hide = self
            .control(
                HIDE_BUTTON,
                Icon::Minus,
                tr!("kiln-shell-dock-hide"),
                self.hide_command,
                cx,
            )
            .on_click(cx.listener(|view: &mut V, _, window, cx| view.hide_clicked(window, cx)));
        div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.))
            .children(actions)
            .children(more)
            .child(hide)
            .into_any_element()
    }

    /// The open ⋯ menu, under its button's right end, over the window and
    /// kept inside it: a click on an item runs its command, a press outside
    /// closes it, and it takes the keys [`DockState::open_menu`] lists.
    fn menu_panel<V: DockView>(&self, cx: &mut Context<V>) -> AnyElement {
        let entries = (self.menu.iter())
            .map(|item| MenuEntry::Item {
                title: item.title.clone(),
                shortcut: Self::shortcut(&self.shortcut_contexts, item.command, cx)
                    .map(SharedString::from),
            })
            .collect();
        let commands: Vec<_> = self.menu.iter().map(|item| item.command).collect();
        let (clicked, hovered) = (cx.weak_entity(), cx.weak_entity());
        let more = self.drawn.more.clone();
        let panel = MenuPanel::new(entries, &ui_theme(cx))
            .highlighted(self.menu_highlighted)
            .on_click(move |ix, window, cx| {
                (clicked.update(cx, |view, cx| {
                    view.dock_mut().close_menu(window, cx);
                    cx.notify();
                }))
                .ok();
                if let Some(&command) = commands.get(ix) {
                    window.defer(cx, move |window, cx| run_action(command, window, cx));
                }
            })
            .on_hover(move |ix, _, cx| {
                (hovered.update(cx, |view, cx| {
                    if view.dock().menu_highlighted != Some(ix) {
                        view.dock_mut().menu_highlighted = Some(ix);
                        cx.notify();
                    }
                }))
                .ok();
            });
        let mut menu = div()
            .debug_selector(|| "dock-menu".into())
            .key_context(MENU_KEY_CONTEXT)
            .on_key_down(
                cx.listener(|view: &mut V, event: &KeyDownEvent, window, cx| {
                    let keystroke = &event.keystroke;
                    if !keystroke.modifiers.modified()
                        && view.dock_mut().menu_key(&keystroke.key, window, cx)
                    {
                        cx.stop_propagation();
                        cx.notify();
                    }
                }),
            )
            .on_mouse_down_out(cx.listener(
                move |view: &mut V, event: &MouseDownEvent, window, cx| {
                    if !more.bounds().contains(&event.position) {
                        view.dock_mut().close_menu(window, cx);
                        cx.notify();
                    }
                },
            ))
            .child(panel);
        if let Some(focus) = &self.menu_focus {
            menu = menu.track_focus(focus);
        }
        // Its top right under the button's bottom right, shifted to stay in
        // the window, as a narrow sidebar on the left would push it out.
        let below = self.drawn.more.bounds().bottom_right() + gpui::point(px(0.), px(2.));
        let menu = anchored()
            .position(below)
            .anchor(Anchor::TopRight)
            .snap_to_window_with_margin(px(4.))
            .child(menu);
        deferred(menu).with_priority(1).into_any_element()
    }

    /// The title row's heading: `title` in the UI heading style.
    fn heading(title: SharedString, cx: &App) -> gpui::Div {
        let ui = ui_theme(cx);
        div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(UI_HEADING.size)
            .line_height(UI_HEADING.line_height)
            .font_weight(FontWeight(f32::from(UI_HEADING.weight)))
            .text_color(ui.text)
            .child(title)
    }

    /// `action`'s button: a click runs its command as a key would, from
    /// the mouse.
    fn action_button<V: DockView>(
        &self,
        ix: usize,
        action: &RailAction,
        highlighted: bool,
        cx: &mut Context<V>,
    ) -> RailButton {
        self.drawn.buttons.borrow_mut().push(PanelId(action.id));
        self.drawn.icons.borrow_mut().push(action.icon);
        if highlighted {
            self.drawn.highlighted.borrow_mut().push(PanelId(action.id));
        }
        let (title, command) = (action.title.clone(), action.command);
        let contexts = self.shortcut_contexts.clone();
        let last_tooltip = self.drawn.tooltip.clone();
        RailButton::new(("dock-rail-action", ix), action.icon, &ui_theme(cx))
            .name(action.title.clone())
            .highlighted(highlighted)
            .on_click(move |_, window, cx| run_action(command, window, cx))
            .tooltip(move |_, cx| {
                let shortcut = Self::shortcut(&contexts, command, cx);
                let tooltip = Tooltip::new(title.clone(), shortcut, &ui_theme(cx));
                last_tooltip.replace(Some(tooltip.text()));
                cx.new(|_| tooltip).into()
            })
    }

    fn rail_button<V: DockView>(&self, ix: usize, cx: &mut Context<V>) -> RailButton {
        let panel = &self.panels[ix];
        let (id, icon) = (panel.id(), panel.icon(cx));
        let (title, command) = (panel.title(cx), panel.show_command(cx));
        self.drawn.buttons.borrow_mut().push(id);
        self.drawn.icons.borrow_mut().push(icon);
        if self.rail_highlighted() == Some(id) {
            self.drawn.highlighted.borrow_mut().push(id);
        }
        let badge = self.badges.get(&id).map(|badge| {
            self.drawn.badges.borrow_mut().push((id, badge.clone()));
            badge.element(ix)
        });
        let contexts = self.shortcut_contexts.clone();
        let last_tooltip = self.drawn.tooltip.clone();
        RailButton::new(("dock-rail", ix), icon, &ui_theme(cx))
            .name(title.clone())
            .highlighted(self.rail_highlighted() == Some(id))
            .when_some(badge, RailButton::badge)
            .on_click(cx.listener(move |view, _, window, cx| view.rail_clicked(id, window, cx)))
            .tooltip(move |_, cx| {
                let shortcut = command.and_then(|command| Self::shortcut(&contexts, command, cx));
                let tooltip = Tooltip::new(title.clone(), shortcut, &ui_theme(cx));
                last_tooltip.replace(Some(tooltip.text()));
                cx.new(|_| tooltip).into()
            })
    }

    /// Draws the dock for `V`, whose [`DockView::rail_clicked`] a rail
    /// button's click runs: the panel's column, unless hidden, with its
    /// edge's handle, and the rail. Shown, it shrinks to its least in a
    /// window too small for it.
    pub fn render<V: DockView>(&self, cx: &mut Context<V>) -> AnyElement {
        self.drawn.buttons.borrow_mut().clear();
        self.drawn.icons.borrow_mut().clear();
        self.drawn.highlighted.borrow_mut().clear();
        self.drawn.badges.borrow_mut().clear();
        self.drawn.cursor.set(None);
        let (size, min) = (self.current_size(), self.min_size());
        self.drawn.no_rail.set(self.rail_hidden);
        self.drawn
            .column_drawn
            .set(self.visible && self.active.is_some());
        let (foot, top): (Vec<_>, Vec<_>) = (0..self.panels.len())
            .filter(|&ix| !self.rail_hidden && self.on_rail(self.panels[ix].id(), cx))
            .partition(|&ix| self.panels[ix].on_rail_foot(cx));
        let buttons: Vec<_> = (top.into_iter())
            .map(|ix| self.rail_button(ix, cx))
            .collect();
        // The bottom group: its panels, then the actions.
        let mut actions: Vec<_> = (foot.into_iter())
            .map(|ix| self.rail_button(ix, cx))
            .collect();
        actions.extend(
            (self.actions.iter().enumerate())
                .filter(|_| !self.rail_hidden)
                .map(|(ix, (action, highlighted))| {
                    self.action_button(ix, action, *highlighted, cx)
                }),
        );
        let active = self.active.and_then(|id| self.handle(id));
        if active.is_none() || !self.visible {
            self.drawn.controls.borrow_mut().clear();
        }
        let body = active.filter(|_| self.visible).map(|panel| {
            let heading = Self::heading(panel.title(cx), cx);
            (panel.to_any_view(), heading, self.controls(panel, cx))
        });
        let ui = &ui_theme(cx);
        // The panel's one heading, then its pane controls.
        let title_row = |heading, controls| {
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .h(TAB_HEIGHT)
                .pl_3()
                .pr_1()
                .bg(ui.panel_background)
                .border_b_1()
                .border_color(ui.border)
                .child(heading)
                .child(controls)
        };
        if self.position == DockPosition::Bottom {
            // The rail's buttons in a row before the title, the panel below;
            // hidden, the buttons alone.
            let row_height = if self.rail_hidden || buttons.is_empty() && actions.is_empty() {
                TAB_HEIGHT
            } else {
                // The buttons, then the row's bottom border.
                RailButton::HEIGHT + px(1.)
            };
            self.drawn.rail_in_title.set(body.is_some());
            let rail = div()
                .id("dock-bottom-rail")
                .flex_none()
                .flex()
                .track_scroll(&self.drawn.rail)
                .children(buttons)
                .children(actions);
            let dock = div()
                .id("dock-bottom")
                .on_drag_move(self.on_edge_drag(cx))
                .w_full()
                .flex()
                .flex_col()
                .track_scroll(&self.drawn.layout);
            let Some((view, heading, controls)) = body else {
                return match self.rail_hidden {
                    true => dock.flex_none().into_any_element(),
                    false => dock.flex_none().child(rail).into_any_element(),
                };
            };
            let title = div()
                .flex_none()
                .flex()
                .items_center()
                .h(row_height)
                .pr_1()
                .gap_2()
                .bg(ui.panel_background)
                .border_b_1()
                .border_color(ui.border)
                .when(!self.rail_hidden, |row| row.child(rail))
                .child(div().flex_1().min_w_0().flex().pl_3().child(heading))
                .child(controls);
            let column = div()
                .id("dock-bottom-column")
                .relative()
                .w_full()
                .h(size)
                .flex_shrink(1.)
                .min_h(min)
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(ui.border)
                .track_scroll(&self.drawn.column)
                .child(title)
                .child(div().flex_1().min_h_0().flex().child(view))
                .child(self.edge_handle(cx));
            return dock
                .flex_shrink(1.)
                .min_h(min)
                .child(column)
                .into_any_element();
        }
        self.drawn.rail_in_title.set(false);
        let left = self.position == DockPosition::Left;
        if !actions.is_empty() {
            // The gap that pushes the actions down is an item of its own.
            self.drawn
                .buttons
                .borrow_mut()
                .insert(buttons.len(), PanelId(""));
        }
        let rail = ActivityRail::new("dock-rail", ui)
            .on_left(left)
            .children(buttons)
            .footer(actions)
            .track(&self.drawn.rail);
        let column = body.map(|(view, heading, controls)| {
            let title = title_row(heading, controls).map(|row| {
                if left {
                    row.border_r_1()
                } else {
                    row.border_l_1()
                }
            });
            div()
                .id("dock-column")
                .relative()
                .w(size)
                .h_full()
                .flex_shrink(1.)
                .min_w(min)
                .flex()
                .flex_col()
                .track_scroll(&self.drawn.column)
                .child(title)
                .child(div().flex_1().min_h_0().flex().child(view))
                .child(self.edge_handle(cx))
        });
        let id = if left { "dock-left" } else { "dock" };
        let shown = column.is_some();
        div()
            .id(id)
            .on_drag_move(self.on_edge_drag(cx))
            .h_full()
            .map(|dock| match shown {
                true => dock
                    .flex_shrink(1.)
                    .min_w(min + if self.rail_hidden { px(0.) } else { RAIL_WIDTH }),
                false => dock.flex_none(),
            })
            .flex()
            // The rail on the window's edge: drawn last, it's laid out
            // last, which `DrawnDock::rail_bounds` relies on.
            .when(left, |dock| dock.flex_row_reverse())
            .track_scroll(&self.drawn.layout)
            .children(column)
            .when(!self.rail_hidden, |dock| dock.child(rail))
            .into_any_element()
    }
}

/// Runs `command` for a clicked [`RailAction`], in the focused context.
fn run_action(command: &str, window: &mut Window, cx: &mut App) {
    let Ok(invocation) = Invocation::of(command) else {
        return;
    };
    let context = keys::context_stack(&window.context_stack(), cx);
    let invocation = invocation.source(Source::Mouse).context(context);
    if let Err(error) = dispatch_invocation(invocation, window, cx) {
        log::warn!("{error}");
    }
}

/// A view that holds a [`DockState`] and draws it.
pub trait DockView: Render + Sized {
    fn dock(&self) -> &DockState;

    fn dock_mut(&mut self) -> &mut DockState;

    /// `panel`'s rail button was clicked: by default, as
    /// [`DockState::click_rail`] says.
    fn rail_clicked(&mut self, panel: PanelId, window: &mut Window, cx: &mut Context<Self>) {
        self.dock_mut().click_rail(panel, window, cx);
        cx.notify();
    }

    /// The title row's — was clicked: by default, runs the dock's
    /// [`DockState::hide_command`] once the click's update is over, or
    /// else hides the dock.
    fn hide_clicked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dock_mut().close_menu(window, cx);
        match self.dock().hide_command_id() {
            // Once this update is over: the command may update the view.
            Some(command) => window.defer(cx, move |window, cx| run_action(command, window, cx)),
            None => self.dock_mut().set_visible(false),
        }
        cx.notify();
    }
}

/// What a [`Dock`] tells its host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockEvent {
    /// A rail click hid the panel that had focus: the host focuses
    /// something shown.
    HidFocus,
}

/// A registered panel the dock hears.
struct Watched {
    entity: EntityId,
    /// Its state as last heard ([`DockItem::save_state`]).
    saved: Option<Value>,
    _changed: Subscription,
}

/// Kiln's dock view.
pub struct Dock {
    state: DockState,
    /// The panels it hears, by id.
    watched: HashMap<PanelId, Watched>,
}

impl Dock {
    pub fn new(position: DockPosition) -> Self {
        Self::with_state(DockState::new(position))
    }

    /// Kiln's dock over `state`, as built.
    pub fn with_state(state: DockState) -> Self {
        Self {
            state,
            watched: HashMap::new(),
        }
    }

    /// See [`DockState::add_panel`]. The dock notifies as a panel's own
    /// state ([`DockItem::save_state`]) changes, so an observer that saves
    /// the dock's state, as [`Workspace`](crate::Workspace)'s layout does,
    /// saves the panel's. It hears the panels its state was built with, or
    /// that were added through [`DockView::dock_mut`], from when it next
    /// draws, and notifies then.
    pub fn add_panel<P: DockItem>(&mut self, panel: Entity<P>, cx: &mut Context<Self>) {
        self.state.add_panel(panel, cx);
        self.watch_panels(cx);
        cx.notify();
    }

    /// Hears each registered panel it doesn't yet, as the entity it is
    /// now, and no longer those that went. True if it heard one anew.
    fn watch_panels(&mut self, cx: &mut Context<Self>) -> bool {
        let panels = &self.state.panels;
        (self.watched).retain(|id, _| panels.iter().any(|panel| panel.id() == *id));
        let mut added = false;
        for panel in panels {
            let (id, entity) = (panel.id(), panel.to_any_entity().entity_id());
            if self
                .watched
                .get(&id)
                .is_some_and(|watched| watched.entity == entity)
            {
                continue;
            }
            let dock = cx.weak_entity();
            let changed = Box::new(move |cx: &mut App| {
                (dock.update(cx, |dock, cx| dock.panel_changed(id, cx))).ok();
            });
            let watched = Watched {
                entity,
                saved: panel.save_state(cx),
                _changed: panel.observe(changed, cx),
            };
            self.watched.insert(id, watched);
            added = true;
        }
        added
    }

    /// `panel` notified: the dock notifies if its state changed.
    fn panel_changed(&mut self, panel: PanelId, cx: &mut Context<Self>) {
        let (Some(handle), Some(watched)) =
            (self.state.handle(panel), self.watched.get_mut(&panel))
        else {
            return;
        };
        let saved = handle.save_state(cx);
        if saved != watched.saved {
            watched.saved = saved;
            cx.notify();
        }
    }

    /// See [`DockState::show`].
    pub fn show(&mut self, panel: PanelId, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let shown = self.state.show(panel, window, cx);
        cx.notify();
        shown
    }

    /// See [`DockState::add_action`].
    pub fn add_action(&mut self, action: RailAction, cx: &mut Context<Self>) {
        self.state.add_action(action);
        cx.notify();
    }

    /// See [`DockState::set_action_highlighted`].
    pub fn set_action_highlighted(&mut self, id: &str, highlighted: bool, cx: &mut Context<Self>) {
        self.state.set_action_highlighted(id, highlighted);
        cx.notify();
    }

    /// See [`DockState::toggle`].
    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        self.state.toggle();
        cx.notify();
    }

    pub fn active(&self) -> Option<PanelId> {
        self.state.active()
    }

    pub fn is_visible(&self) -> bool {
        self.state.is_visible()
    }

    /// See [`DockState::panel`].
    pub fn panel<P: DockItem>(&self) -> Option<Entity<P>> {
        self.state.panel()
    }
}

impl EventEmitter<DockEvent> for Dock {}

impl DockView for Dock {
    fn dock(&self) -> &DockState {
        &self.state
    }

    fn dock_mut(&mut self) -> &mut DockState {
        &mut self.state
    }

    fn rail_clicked(&mut self, panel: PanelId, window: &mut Window, cx: &mut Context<Self>) {
        self.state.click_rail(panel, window, cx);
        if self.state.hidden_panel_focused(window, cx) {
            cx.emit(DockEvent::HidFocus);
        }
        cx.notify();
    }

    /// Hides it as [`DockView::hide_clicked`] does, then tells the host if
    /// the panel it hid had focus.
    fn hide_clicked(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.close_menu(window, cx);
        match self.state.hide_command_id() {
            Some(command) => window.defer(cx, move |window, cx| run_action(command, window, cx)),
            None => self.state.set_visible(false),
        }
        if self.state.hidden_panel_focused(window, cx) {
            cx.emit(DockEvent::HidFocus);
        }
        cx.notify();
    }
}

impl Render for Dock {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Panels registered without the dock: what changed in them before
        // it heard them is saved as it notifies.
        if self.watch_panels(cx) {
            let dock = cx.weak_entity();
            cx.defer(move |cx| _ = dock.update(cx, |_, cx| cx.notify()));
        }
        self.state.render(cx)
    }
}
