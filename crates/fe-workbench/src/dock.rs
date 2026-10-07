//! The right dock: one panel at a time, the project tree, project search or
//! the outline, under a title row naming it, and picked from the activity
//! rail on the window's right edge, which stays when the dock hides.
//! Switching keeps each panel's state.
//!
//! Its commands, and the search panel's, are those of [`init`]'s registry:
//! the dock's in the workbench's [`crate::KEY_CONTEXT`], the search options'
//! in the search panel's [`search_panel::KEY_CONTEXT`], and
//! `text_input.submit` in its input's. Their handlers only name an op: the
//! workbench, or the search panel, then does it with the window at hand.

use std::rc::Rc;

use fe_command::{
    Actor, CommandId, CommandTarget, ContextFrame, ContextPredicate, ContextStack, Dispatched,
    Effect, Registry, Value,
};
use fe_gpui::commands::RunCommand;
use fe_ui::{ActivityRail, Icon, RailButton, TAB_HEIGHT, TextInput, Tooltip, UiTheme};
use gpui::{
    App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable as _, Global,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Window, div,
};

use crate::{
    OutlinePanel, PANEL_WIDTH, ProjectPanel,
    search_panel::{self, SearchPanel},
};

/// A panel the dock shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockPanel {
    Project,
    Search,
    Outline,
}

impl DockPanel {
    /// The dock's panels, in the rail's order.
    pub const ALL: [DockPanel; 3] = [DockPanel::Project, DockPanel::Search, DockPanel::Outline];

    pub fn name(self) -> &'static str {
        match self {
            DockPanel::Project => "Project",
            DockPanel::Search => "Search",
            DockPanel::Outline => "Outline",
        }
    }

    /// The command that shows it.
    fn show_command(self) -> &'static str {
        match self {
            DockPanel::Project => "dock.show_project",
            DockPanel::Search => "search.show",
            DockPanel::Outline => "outline.show",
        }
    }

    /// The op of [`DockPanel::show_command`].
    fn show_op(self) -> DockOp {
        match self {
            DockPanel::Project => DockOp::ShowProject,
            DockPanel::Search => DockOp::ShowSearch,
            DockPanel::Outline => DockOp::ShowOutline,
        }
    }
}

/// Its rail button's icon.
pub(crate) fn panel_icon(panel: DockPanel) -> Icon {
    match panel {
        DockPanel::Project => Icon::Files,
        DockPanel::Search => Icon::Search,
        DockPanel::Outline => Icon::ListTree,
    }
}

/// A rail button was clicked; the workbench runs [`Dock::rail_op`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DockEvent {
    Rail(DockPanel),
}

pub struct Dock {
    project: Entity<ProjectPanel>,
    search: Entity<SearchPanel>,
    outline: Entity<OutlinePanel>,
    active: DockPanel,
    visible: bool,
    ui: UiTheme,
    /// The rail's buttons' laid-out bounds.
    #[cfg(test)]
    rail: gpui::ScrollHandle,
    /// The dock's column, if shown, then the rail.
    #[cfg(test)]
    layout: gpui::ScrollHandle,
    #[cfg(test)]
    ops: Vec<DockOp>,
    #[cfg(test)]
    last_tooltip: Rc<std::cell::RefCell<Option<String>>>,
    /// The icons the last frame's rail buttons drew, top to bottom.
    #[cfg(test)]
    rail_icons: Vec<Icon>,
}

impl Dock {
    /// Shows `project` first.
    pub fn new(
        project: Entity<ProjectPanel>,
        search: Entity<SearchPanel>,
        outline: Entity<OutlinePanel>,
        ui: UiTheme,
    ) -> Self {
        Self {
            project,
            search,
            outline,
            active: DockPanel::Project,
            visible: true,
            ui,
            #[cfg(test)]
            rail: gpui::ScrollHandle::new(),
            #[cfg(test)]
            layout: gpui::ScrollHandle::new(),
            #[cfg(test)]
            ops: Vec::new(),
            #[cfg(test)]
            last_tooltip: Rc::default(),
            #[cfg(test)]
            rail_icons: Vec::new(),
        }
    }

    pub fn project(&self) -> &Entity<ProjectPanel> {
        &self.project
    }

    pub fn search(&self) -> &Entity<SearchPanel> {
        &self.search
    }

    pub fn outline(&self) -> &Entity<OutlinePanel> {
        &self.outline
    }

    /// The panel shown while the dock is visible.
    pub fn active(&self) -> DockPanel {
        self.active
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// The panel whose rail button is highlighted: the active one, while
    /// the dock shows.
    pub fn rail_highlighted(&self) -> Option<DockPanel> {
        self.visible.then_some(self.active)
    }

    /// What clicking `panel`'s rail button does: hides the dock if it's the
    /// highlighted one, else shows the dock with it, as its show command
    /// does.
    pub(crate) fn rail_op(&self, panel: DockPanel) -> DockOp {
        if self.rail_highlighted() == Some(panel) {
            DockOp::Toggle
        } else {
            panel.show_op()
        }
    }

    /// Does a dock op, for a key or a rail click: showing search focuses its
    /// input, and hiding the search panel moves focus from it to
    /// `fallback`. Search options and submits, which the search panel does
    /// when focused, go on.
    pub(crate) fn run(
        &mut self,
        op: DockOp,
        fallback: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        #[cfg(test)]
        self.ops.push(op);
        match op {
            DockOp::ShowSearch => self.show_search(window, cx),
            DockOp::ShowProject => {
                self.show(DockPanel::Project, cx);
                self.leave_search(fallback, window, cx);
            }
            DockOp::ShowOutline => {
                self.show(DockPanel::Outline, cx);
                self.leave_search(fallback, window, cx);
            }
            DockOp::Toggle => {
                self.visible = !self.visible;
                if !self.visible {
                    self.leave_search(fallback, window, cx);
                }
                cx.notify();
            }
            DockOp::ToggleRegex | DockOp::ToggleCase | DockOp::ToggleWholeWord | DockOp::Submit => {
                return false;
            }
        }
        true
    }

    /// Shows search, focusing its input.
    fn show_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.show(DockPanel::Search, cx);
        let input = self.search.read(cx).input().focus_handle(cx);
        window.focus(&input, cx);
    }

    /// Shows the dock with `panel`.
    pub fn show(&mut self, panel: DockPanel, cx: &mut Context<Self>) {
        self.active = panel;
        self.visible = true;
        cx.notify();
    }

    /// Focuses `fallback` if the search panel, now hidden, has focus.
    pub(crate) fn leave_search(&self, fallback: &FocusHandle, window: &mut Window, cx: &mut App) {
        let input = self.search.read(cx).input().focus_handle(cx);
        if input.contains_focused(window, cx) {
            window.focus(fallback, cx);
        }
    }

    fn rail_button(&mut self, panel: DockPanel, cx: &mut Context<Self>) -> RailButton {
        let ui = self.ui;
        let icon = panel_icon(panel);
        #[cfg(test)]
        self.rail_icons.push(icon);
        #[cfg(test)]
        let last_tooltip = self.last_tooltip.clone();
        RailButton::new(("dock-rail", panel as usize), icon, &self.ui)
            .highlighted(self.rail_highlighted() == Some(panel))
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(DockEvent::Rail(panel))))
            .tooltip(move |_, cx| {
                let tooltip = Tooltip::new(panel.name(), shortcut(panel.show_command(), cx), &ui);
                #[cfg(test)]
                last_tooltip.replace(Some(tooltip.text()));
                cx.new(|_| tooltip).into()
            })
    }

    /// The title row's text: the active panel's name.
    pub(crate) fn title(&self) -> SharedString {
        self.active.name().into()
    }
}

/// The first key sequence that runs `command` in the workbench's editor, in
/// macOS symbols, read from the keymap now so it follows reloads.
fn shortcut(command: &str, cx: &App) -> Option<String> {
    let command = CommandId::parse(command).ok()?;
    let stack: ContextStack = [crate::KEY_CONTEXT, fe_gpui::KEY_CONTEXT]
        .into_iter()
        .map(ContextFrame::new)
        .collect();
    let keymap = fe_gpui::current_keymap(cx);
    let shortcuts = fe_keymap::shortcuts(&keymap, &command, &Value::Null, &stack);
    shortcuts.first().map(fe_keymap::macos_label)
}

#[cfg(test)]
impl Dock {
    /// Where the rail's button for `panel` was laid out in the last frame.
    pub(crate) fn rail_button_bounds(
        &self,
        panel: DockPanel,
    ) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.rail.bounds_for_item(panel as usize)
    }

    /// Where the rail was laid out in the last frame.
    pub(crate) fn rail_bounds(&self) -> Option<gpui::Bounds<gpui::Pixels>> {
        let count = self.layout.children_count();
        self.layout.bounds_for_item(count.checked_sub(1)?)
    }

    /// Where the dock's column, its title row then its panel, was laid out
    /// in the last frame, if it showed.
    pub(crate) fn column_bounds(&self) -> Option<gpui::Bounds<gpui::Pixels>> {
        (self.layout.children_count() == 2)
            .then(|| self.layout.bounds_for_item(0))
            .flatten()
    }

    /// The icons the last frame's rail buttons drew, top to bottom.
    pub(crate) fn rail_icons(&self) -> &[Icon] {
        &self.rail_icons
    }

    /// Every op [`Dock::run`] was asked to do, oldest first.
    pub(crate) fn ops(&self) -> &[DockOp] {
        &self.ops
    }

    /// The text of the last tooltip built.
    pub(crate) fn last_tooltip(&self) -> Option<String> {
        self.last_tooltip.borrow().clone()
    }
}

impl EventEmitter<DockEvent> for Dock {}

impl Render for Dock {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        self.rail_icons.clear();
        let buttons: Vec<_> = DockPanel::ALL
            .into_iter()
            .map(|panel| self.rail_button(panel, cx))
            .collect();
        let rail = ActivityRail::new("dock-rail", &self.ui).children(buttons);
        #[cfg(test)]
        let rail = rail.track(&self.rail);
        let column = self.visible.then(|| {
            let body = match self.active {
                DockPanel::Project => self.project.clone().into_any_element(),
                DockPanel::Search => self.search.clone().into_any_element(),
                DockPanel::Outline => self.outline.clone().into_any_element(),
            };
            let title = div()
                .flex_none()
                .flex()
                .items_center()
                .h(TAB_HEIGHT)
                .px_3()
                .bg(self.ui.panel_background)
                .border_l_1()
                .border_b_1()
                .border_color(self.ui.border)
                .text_color(self.ui.muted_text)
                .child(self.title());
            div()
                .w(PANEL_WIDTH)
                .h_full()
                .flex_none()
                .flex()
                .flex_col()
                .child(title)
                .child(div().flex_1().min_h_0().flex().child(body))
        });
        let root = div().id("dock").h_full().flex_none().flex();
        #[cfg(test)]
        let root = gpui::StatefulInteractiveElement::track_scroll(root, &self.layout);
        root.children(column).child(rail)
    }
}

/// What a dock or search command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DockOp {
    ShowSearch,
    ShowProject,
    /// `outline.show`, which the outline panel's registry has.
    ShowOutline,
    Toggle,
    ToggleRegex,
    ToggleCase,
    ToggleWholeWord,
    /// Enter in the search panel's input: [`TextInput::submit`].
    Submit,
}

/// The [`CommandTarget`] of dock commands: the op to do.
pub(crate) struct DockCommands;

impl CommandTarget for DockCommands {
    type Tx = ();
    type Target<'a> = Option<DockOp>;
}

struct Commands(Rc<Registry<DockCommands>>);

impl Global for Commands {}

/// Registers the dock and search commands.
pub(crate) fn init(cx: &mut App) {
    cx.set_global(Commands(Rc::new(registry())));
}

pub(crate) fn registry() -> Registry<DockCommands> {
    use DockOp::*;
    let workbench = crate::KEY_CONTEXT;
    let search = search_panel::KEY_CONTEXT;
    let commands = [
        ("search.show", "Find in Files", workbench, ShowSearch),
        ("dock.show_project", "Show Project", workbench, ShowProject),
        ("dock.toggle", "Toggle Dock", workbench, Toggle),
        ("search.toggle_regex", "Toggle Regex", search, ToggleRegex),
        (
            "search.toggle_case",
            "Toggle Match Case",
            search,
            ToggleCase,
        ),
        (
            "search.toggle_whole_word",
            "Toggle Whole Word",
            search,
            ToggleWholeWord,
        ),
        (
            "text_input.submit",
            "Submit",
            TextInput::KEY_CONTEXT,
            Submit,
        ),
    ];
    let mut registry = Registry::new();
    for (id, title, context, op) in commands {
        registry
            .register(id, title, move |target: &mut Option<DockOp>, _: &Actor| {
                *target = Some(op);
                Ok(Dispatched::new(Effect::Notify))
            })
            .expect("dock command IDs are valid and distinct")
            .context(ContextPredicate::identifier(context));
    }
    registry
}

/// The op of `run`, if it's a dock or search command that applies in its
/// context.
pub(crate) fn op(run: &RunCommand, cx: &App) -> Option<DockOp> {
    let registry = cx.try_global::<Commands>()?.0.clone();
    registry.get(&run.0.command)?;
    let mut op = None;
    if let Err(error) = registry.dispatch(&mut op, &run.0) {
        log::warn!("{error}");
    }
    op
}
