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
//! ```ignore
//! let dock = cx.new(|cx| {
//!     let mut dock = Dock::new(DockPosition::Left, ui);
//!     dock.add_panel(outline.clone(), cx);
//!     dock
//! });
//! dock.update(cx, |dock, cx| dock.show(OUTLINE, window, cx));
//! ```

use std::{any::TypeId, cell::RefCell, collections::HashSet, rc::Rc};

use gpui::{
    AnyElement, AnyEntity, AnyView, App, AppContext as _, Bounds, Context, Entity, EventEmitter,
    FocusHandle, Focusable, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    Render, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use kiln_command::{CommandId, ContextFrame, ContextStack, Value};
use kiln_core::{shortcut_label, shortcuts};
use kiln_platform::keys;
use kiln_ui::{ActivityRail, Icon, RailButton, TAB_HEIGHT, Tooltip, UiTheme, ui_theme};

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

/// What the last frame drew of a dock: where its rail, its buttons and its
/// column were laid out, the icons its buttons drew and the last tooltip
/// built. Cloning it shares it, so a host can read it without the dock.
#[derive(Clone, Default)]
pub struct DrawnDock {
    /// The rail's buttons.
    rail: ScrollHandle,
    /// The dock's column, if shown, then the rail.
    layout: ScrollHandle,
    /// The ids of the buttons drawn, top to bottom.
    buttons: Rc<RefCell<Vec<PanelId>>>,
    icons: Rc<RefCell<Vec<Icon>>>,
    tooltip: Rc<RefCell<Option<String>>>,
}

impl DrawnDock {
    /// Where `panel`'s rail button was laid out, if it was drawn.
    pub fn rail_button_bounds(&self, panel: PanelId) -> Option<Bounds<Pixels>> {
        let ix = self.buttons.borrow().iter().position(|id| *id == panel)?;
        self.rail.bounds_for_item(ix)
    }

    /// Where the rail was laid out.
    pub fn rail_bounds(&self) -> Option<Bounds<Pixels>> {
        let count = self.layout.children_count();
        self.layout.bounds_for_item(count.checked_sub(1)?)
    }

    /// Where the dock's column, its title row then its panel, was laid
    /// out, if it showed.
    pub fn column_bounds(&self) -> Option<Bounds<Pixels>> {
        (self.layout.children_count() == 2)
            .then(|| self.layout.bounds_for_item(0))
            .flatten()
    }

    /// The icons the rail's buttons drew, top to bottom.
    pub fn rail_icons(&self) -> Vec<Icon> {
        self.icons.borrow().clone()
    }

    /// The text of the last tooltip a rail button built.
    pub fn last_tooltip(&self) -> Option<String> {
        self.tooltip.borrow().clone()
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
    /// The panel's width, or its height at the bottom.
    size: Pixels,
    /// The key contexts a rail button's tooltip looks its show command's
    /// key up in, outermost first.
    shortcut_contexts: Vec<SharedString>,
    drawn: DrawnDock,
}

impl DockState {
    /// An empty dock at `position`, shown, 240 px across. It draws in the
    /// active theme's colours ([`ui_theme`]), so it follows a theme change;
    /// `_ui` is unused, left for callers until a later API cleanup.
    pub fn new(position: DockPosition, _ui: UiTheme) -> Self {
        Self {
            position,
            panels: Vec::new(),
            active: None,
            visible: true,
            joined: HashSet::new(),
            size: px(240.),
            shortcut_contexts: Vec::new(),
            drawn: DrawnDock::default(),
        }
    }

    /// The panel's width, or its height in a bottom dock.
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
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
    /// panel and the panels that have joined the rail.
    pub fn save(&self) -> Value {
        let ids = |ids: Vec<PanelId>| Value::from_iter(ids.into_iter().map(|id| id.0));
        Value::from_iter([
            ("visible", Value::from(self.visible)),
            ("active", self.active.map_or(Value::Null, |id| id.0.into())),
            ("joined", ids(self.joined())),
        ])
    }

    /// Puts back what [`DockState::save`] saved, as far as its panels are
    /// registered: whether it shows, its active panel (without focusing
    /// it) and the panels that had joined the rail.
    pub fn restore(&mut self, saved: &Value) {
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

    fn rail_button<V: DockView>(&self, ix: usize, cx: &mut Context<V>) -> RailButton {
        let panel = &self.panels[ix];
        let (id, icon) = (panel.id(), panel.icon(cx));
        let (title, command) = (panel.title(cx), panel.show_command(cx));
        self.drawn.buttons.borrow_mut().push(id);
        self.drawn.icons.borrow_mut().push(icon);
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
    /// button's click runs: the panel's column, unless hidden, and the
    /// rail.
    pub fn render<V: DockView>(&self, cx: &mut Context<V>) -> AnyElement {
        self.drawn.buttons.borrow_mut().clear();
        self.drawn.icons.borrow_mut().clear();
        let shown: Vec<_> = (0..self.panels.len())
            .filter(|&ix| self.on_rail(self.panels[ix].id(), cx))
            .collect();
        let buttons: Vec<_> = shown
            .into_iter()
            .map(|ix| self.rail_button(ix, cx))
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
            // The rail's buttons in a row before the title, the panel below.
            let rail = div()
                .id("dock-bottom-rail")
                .flex_none()
                .flex()
                .track_scroll(&self.drawn.rail)
                .children(buttons);
            let column = body.map(|(view, title)| {
                div()
                    .w_full()
                    .h(self.size)
                    .flex_none()
                    .flex()
                    .flex_col()
                    .border_t_1()
                    .border_color(ui.border)
                    .child(title_row(title))
                    .child(div().flex_1().min_h_0().flex().child(view))
            });
            return div()
                .id("dock-bottom")
                .w_full()
                .flex_none()
                .flex()
                .flex_col()
                .track_scroll(&self.drawn.layout)
                .children(column)
                .child(rail)
                .into_any_element();
        }
        let left = self.position == DockPosition::Left;
        let rail = ActivityRail::new("dock-rail", ui)
            .children(buttons)
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
                .w(self.size)
                .h_full()
                .flex_none()
                .flex()
                .flex_col()
                .child(title)
                .child(div().flex_1().min_h_0().flex().child(view))
        });
        let id = if left { "dock-left" } else { "dock" };
        div()
            .id(id)
            .h_full()
            .flex_none()
            .flex()
            // The rail on the window's edge: drawn last, it's laid out
            // last, which `DrawnDock::rail_bounds` relies on.
            .when(left, |dock| dock.flex_row_reverse())
            .track_scroll(&self.drawn.layout)
            .children(column)
            .child(rail)
            .into_any_element()
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
    pub fn new(position: DockPosition, ui: UiTheme) -> Self {
        Self {
            state: DockState::new(position, ui),
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
