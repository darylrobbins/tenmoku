//! The right dock: one panel at a time, the project tree, project search or
//! the outline, picked from a strip of tabs at its top. Switching keeps each panel's
//! state, and the dock can be hidden.
//!
//! Its commands, and the search panel's, are those of [`init`]'s registry:
//! the dock's in the workbench's [`crate::KEY_CONTEXT`], the search options'
//! in the search panel's [`search_panel::KEY_CONTEXT`], and
//! `text_input.submit` in its input's. Their handlers only name an op: the
//! workbench, or the search panel, then does it with the window at hand.

use std::rc::Rc;

use fe_command::{Actor, CommandTarget, ContextPredicate, Dispatched, Effect, Registry};
use fe_gpui::commands::RunCommand;
use fe_ui::{TabBar, TextInput, UiTheme};
use gpui::{
    App, Context, Entity, FocusHandle, Focusable as _, Global, IntoElement, ParentElement as _,
    Render, ScrollHandle, Styled as _, Window, div,
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

pub struct Dock {
    project: Entity<ProjectPanel>,
    search: Entity<SearchPanel>,
    outline: Entity<OutlinePanel>,
    active: DockPanel,
    visible: bool,
    ui: UiTheme,
    tabs: ScrollHandle,
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
            tabs: ScrollHandle::new(),
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

    /// Does a dock op: showing search focuses its input, and hiding the
    /// search panel moves focus from it to `fallback`. Search options and
    /// submits, which the search panel does when focused, go on.
    pub(crate) fn run(
        &mut self,
        op: DockOp,
        fallback: &FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        match op {
            DockOp::ShowSearch => self.show_search(window, cx),
            DockOp::ShowProject => {
                self.show(DockPanel::Project, cx);
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

    /// Shows `panel` as its header tab does: as the commands that show it
    /// do, with the pane's focus target as the fallback.
    fn switch(&mut self, panel: DockPanel, window: &mut Window, cx: &mut Context<Self>) {
        if panel == DockPanel::Search {
            return self.show_search(window, cx);
        }
        self.show(panel, cx);
        let pane = self.search.read(cx).pane().upgrade();
        if let Some(pane) = pane {
            let fallback = pane.read(cx).focus_target(cx);
            self.leave_search(&fallback, window, cx);
        }
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

    /// Where the header's tab for `panel` was laid out in the last frame.
    #[cfg(test)]
    pub(crate) fn tab_bounds(&self, panel: DockPanel) -> Option<gpui::Bounds<gpui::Pixels>> {
        self.tabs.bounds_for_item(panel as usize)
    }
}

impl Render for Dock {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.visible {
            return div();
        }
        let tabs = [
            (DockPanel::Project, "Project"),
            (DockPanel::Search, "Search"),
            (DockPanel::Outline, "Outline"),
        ]
        .map(|(panel, label)| {
            fe_ui::Tab::new(("dock-tab", panel as usize), label, &self.ui)
                .active(panel == self.active)
                .on_click(cx.listener(move |dock, _, window, cx| dock.switch(panel, window, cx)))
        });
        let body = match self.active {
            DockPanel::Project => self.project.clone().into_any_element(),
            DockPanel::Search => self.search.clone().into_any_element(),
            DockPanel::Outline => self.outline.clone().into_any_element(),
        };
        div()
            .w(PANEL_WIDTH)
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .border_l_1()
                    .border_color(self.ui.border)
                    .child(
                        TabBar::new("dock-tabs", &self.ui)
                            .track_scroll(&self.tabs)
                            .children(tabs),
                    ),
            )
            .child(div().flex_1().min_h_0().flex().child(body))
    }
}

/// What a dock or search command asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DockOp {
    ShowSearch,
    ShowProject,
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
