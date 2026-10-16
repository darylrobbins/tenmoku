// SPDX-License-Identifier: LicenseRef-Proprietary

//! A dock: panels beside the centre, one shown at a time under a title row
//! naming it, picked from an activity rail on the window's edge, which
//! stays when the dock hides. Switching keeps each panel's state.
//!
//! A panel is any view implementing [`DockItem`], registered with
//! [`DockState::add_panel`] and named by its [`PanelId`]. The rail lists
//! the panels in registration order; one that isn't
//! [`DockItem::always_on_rail`] joins it once it has shown.
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
    AnyElement, AnyEntity, AnyView, App, AppContext as _, Bounds, Context, CursorStyle,
    DragMoveEvent, Entity, EventEmitter, FocusHandle, Focusable, Global, InteractiveElement as _,
    IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Pixels, Point, Render,
    ScrollHandle, SharedString, Size, StatefulInteractiveElement as _, Styled as _, Window,
    WindowId, canvas, div, prelude::FluentBuilder as _, px,
};
use kiln_command::{CommandId, ContextFrame, ContextStack, Invocation, Source, Value};
use kiln_core::{shortcut_label, shortcuts};
use kiln_platform::{command_actions::dispatch_invocation, keys};
use kiln_ui::{ActivityRail, Icon, RAIL_WIDTH, RailButton, TAB_HEIGHT, Tooltip, ui_theme};

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

    /// Focused when shown, as a search panel focuses its input.
    fn focus_on_show(&self) -> bool {
        false
    }
}

/// A registered panel: its entity, without its type.
trait PanelHandle {
    fn id(&self) -> PanelId;
    fn title(&self, cx: &App) -> SharedString;
    fn icon(&self, cx: &App) -> Icon;
    fn show_command(&self, cx: &App) -> Option<&'static str>;
    fn always_on_rail(&self, cx: &App) -> bool;
    fn focus_on_show(&self, cx: &App) -> bool;
    fn focus_handle(&self, cx: &App) -> FocusHandle;
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

    fn focus_on_show(&self, cx: &App) -> bool {
        self.entity.read(cx).focus_on_show()
    }

    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.entity.focus_handle(cx)
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
    tooltip: Rc<RefCell<Option<String>>>,
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

    /// Where the column's title row was laid out, if it showed.
    pub fn title_bounds(&self) -> Option<Bounds<Pixels>> {
        self.column_bounds()?;
        self.column.bounds_for_item(0)
    }

    /// The icons the rail's buttons drew, top to bottom.
    pub fn rail_icons(&self) -> Vec<Icon> {
        self.icons.borrow().clone()
    }

    /// The ids of the rail's buttons drawn highlighted, panels' and actions'.
    pub fn rail_highlighted(&self) -> Vec<PanelId> {
        self.highlighted.borrow().clone()
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
            side_size: px(240.),
            bottom_size: px(240.),
            built: px(240.),
            press: None,
            shortcut_contexts: Vec::new(),
            rail_hidden: false,
            drawn: DrawnDock::default(),
        }
    }

    /// Whether the dock draws no rail, at any position: no panel buttons and
    /// no actions, only the open panel's column. Its panels are shown by
    /// command or by another dock's [`RailAction`]. A rail is the default.
    pub fn hide_rail(&mut self, hidden: bool) {
        self.rail_hidden = hidden;
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
        self.active = Some(panel);
        self.visible = true;
        self.joined.insert(panel);
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
    /// at its position, `"side_size"` and `"bottom_size"` at each.
    pub fn save(&self) -> Value {
        let size = |size: Pixels| Value::from(f64::from(f32::from(size)));
        let ids = |ids: Vec<PanelId>| Value::from_iter(ids.into_iter().map(|id| id.0));
        Value::from_iter([
            ("visible", Value::from(self.visible)),
            ("active", self.active.map_or(Value::Null, |id| id.0.into())),
            ("joined", ids(self.joined())),
            ("size", size(self.current_size())),
            ("side_size", size(self.side_size)),
            ("bottom_size", size(self.bottom_size)),
        ])
    }

    /// Puts back what [`DockState::save`] saved, as far as its panels are
    /// registered: whether it shows, its active panel (without focusing
    /// it), the panels that had joined the rail and its sizes, each at
    /// least its least: `"side_size"` and `"bottom_size"`, else, saved
    /// before a dock had both, `"size"` at its position.
    pub fn restore(&mut self, saved: &Value) {
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
        self.joined.extend(joined);
        if let Some(active) = self.active {
            self.joined.insert(active);
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
        let contexts = self.shortcut_contexts.clone();
        let last_tooltip = self.drawn.tooltip.clone();
        RailButton::new(("dock-rail", ix), icon, &ui_theme(cx))
            .highlighted(self.rail_highlighted() == Some(id))
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
        self.drawn.cursor.set(None);
        let (size, min) = (self.current_size(), self.min_size());
        self.drawn.no_rail.set(self.rail_hidden);
        self.drawn
            .column_drawn
            .set(self.visible && self.active.is_some());
        let shown: Vec<_> = (0..self.panels.len())
            .filter(|&ix| !self.rail_hidden && self.on_rail(self.panels[ix].id(), cx))
            .collect();
        let buttons: Vec<_> = shown
            .into_iter()
            .map(|ix| self.rail_button(ix, cx))
            .collect();
        let actions: Vec<_> = (self.actions.iter().enumerate())
            .filter(|_| !self.rail_hidden)
            .map(|(ix, (action, highlighted))| self.action_button(ix, action, *highlighted, cx))
            .collect();
        let active = self.active.and_then(|id| self.handle(id));
        let body = active
            .filter(|_| self.visible)
            .map(|panel| (panel.to_any_view(), panel.title(cx)));
        let ui = &ui_theme(cx);
        let title_row = |title: SharedString| {
            div()
                .flex_none()
                .flex()
                .items_center()
                .h(TAB_HEIGHT)
                .px_3()
                .bg(ui.panel_background)
                .border_b_1()
                .border_color(ui.border)
                .text_color(ui.muted_text)
                .child(title)
        };
        if self.position == DockPosition::Bottom {
            // The rail's buttons in a row before the title, the panel below;
            // hidden, the buttons alone.
            let row_height = if self.rail_hidden || buttons.is_empty() && actions.is_empty() {
                TAB_HEIGHT
            } else {
                // The buttons, then the row's bottom border.
                RAIL_WIDTH + px(1.)
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
            let Some((view, title)) = body else {
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
                .bg(ui.panel_background)
                .border_b_1()
                .border_color(ui.border)
                .text_color(ui.muted_text)
                .when(!self.rail_hidden, |row| row.child(rail))
                .child(div().px_3().child(title));
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
            .children(buttons)
            .footer(actions)
            .track(&self.drawn.rail);
        let column = body.map(|(view, title)| {
            let title = title_row(title).map(|row| {
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
}

/// What a [`Dock`] tells its host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockEvent {
    /// A rail click hid the panel that had focus: the host focuses
    /// something shown.
    HidFocus,
}

/// Kiln's dock view.
pub struct Dock {
    state: DockState,
}

impl Dock {
    pub fn new(position: DockPosition) -> Self {
        Self {
            state: DockState::new(position),
        }
    }

    /// Kiln's dock over `state`, as built.
    pub fn with_state(state: DockState) -> Self {
        Self { state }
    }

    /// See [`DockState::add_panel`].
    pub fn add_panel<P: DockItem>(&mut self, panel: Entity<P>, cx: &mut Context<Self>) {
        self.state.add_panel(panel, cx);
        cx.notify();
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
}

impl Render for Dock {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.state.render(cx)
    }
}
