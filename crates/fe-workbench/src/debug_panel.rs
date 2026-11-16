//! The Debug tool window of the bottom panel (docs/decisions/r1b9dw3g0g-debugger.md
//! §5 "Panels", §13 "B3 design first"; `docs/design/design-system.md`,
//! Debugger, "The Debug tool window"; card D9): Call Stack, Watch and
//! Variables on its Variables tab, and the breakpoints and the adapter's
//! exception filters on its Breakpoints tab.
//!
//! - **Where.** One more tool window of the bottom dock, registered as Runs
//!   is ([`crate::dock`]). Its rail button (Lucide `bug`) joins the rail's
//!   foot right after Runs' the first time a session starts in the window
//!   ([`Dock::join_debug`](crate::Dock::join_debug)), and carries a hare's
//!   fur dot while a session is stopped. Each session shows the tool
//!   window once, as it launches, without taking focus; its first stop
//!   switches it to Variables unless a tab was picked since.
//!   `debug.show_panels` toggles it: shown and focused, or hidden, focus
//!   going back to the editor if it was in it.
//! - **Call Stack** ([`stop`]): the program's threads, the stopped one
//!   holding its frames (the first page the session fetched), the others
//!   fetching theirs when expanded. A frame row is the function in small
//!   code and its `file:line` faint at the right; frames with no source
//!   path, or that the adapter calls `subtle`, are faint. Selecting one
//!   opens its file at its line (the stop's own navigation, so it goes
//!   with the stop), bands a caller's row through D6's seam
//!   ([`Workbench::band_debug_frame`]), says "Frame K of M: function, line
//!   N", and re-scopes Variables and Watch. `debug.frame_up` and
//!   `debug.frame_down` select the caller and the callee.
//! - **Variables**: the selected frame's scopes, the first expanded, as a
//!   tree whose children are fetched when a node expands, once per stop:
//!   a node collapsed and expanded again in the same stop is drawn from
//!   what was fetched, and a node still expanded at the next stop fetches
//!   anew. Children show 100 at a time, with a "Show 100 more" row: an
//!   adapter that counts a node's `indexedVariables` past 100 is asked for
//!   them a page (`start`, `count`) at a time; one that doesn't is asked
//!   once and shown a page at a time.
//! - **Watch**: the expressions in the root's state file's `watches` list
//!   ([`BreakpointStore::watch_expressions`]), evaluated with context
//!   `watch` in the selected frame on each stop and each frame selection;
//!   an expandable result expands as a variable does. An Add watch input
//!   adds one; a row's ✕ removes it. They are the project's, so they are
//!   there after a restart.
//! - **Breakpoints** ([`breakpoints`]): the adapter's exception filters as
//!   checkboxes (re-sent with `setExceptionBreakpoints` on each toggle,
//!   [`Debugger::set_exception_filter`]), then every breakpoint grouped by
//!   file as Problems groups (file icon, path, count): a checkbox that
//!   enables it, its glyph, `line N` and the line's code. Selecting a row
//!   opens the file there.
//! - **Keys.** Each tree keeps a selected row: up and down move it, right
//!   expands and left collapses, enter acts on it (selects a frame, opens a
//!   breakpoint), space toggles a breakpoint's checkbox and delete removes
//!   a watch. Rows and controls have accessible names (the design's table).
//!
//! Its text is the catalogue's (`l10n/en-US/debug.ftl`).

use std::{collections::HashMap, path::PathBuf};

use fe_gpui::commands::RunCommand;
use fe_ui::{ActiveTheme as _, Icon, TextInput, TextInputEvent};
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyContext, KeyDownEvent, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    UniformListScrollHandle, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
};
use kiln_l10n::{text, tr, verbatim};
use kiln_shell::{DockView as _, PanelId};

use crate::{
    Workbench,
    debug_breakpoints::BreakpointStore,
    debug_session::{Debugger, DebuggerEvent, debugger_of},
};
use fe_dap::SessionState;

mod breakpoints;
mod rows;
mod stop;

#[cfg(test)]
pub(crate) use breakpoints::{BreakpointRow, CodeReadGate};
#[cfg(test)]
pub(crate) use rows::WATCH_MAX_HEIGHT;
pub(crate) use rows::{StackRow, ValueRow};
#[cfg(test)]
pub(crate) use stop::FramePageGate;
use stop::Stop;

/// The tool window's id in the bottom dock, and its rail button's.
pub(crate) const PANEL: &str = "debug";

/// The command that shows or hides it.
pub(crate) const SHOW_COMMAND: &str = "debug.show_panels";

/// Its rail button's and tab's icon.
pub(crate) const ICON: Icon = Icon::Bug;

/// Its key context, which the keymap's dock keys name, with [`VIM_FLAG`]
/// while vim is on.
pub(crate) const KEY_CONTEXT: &str = "debug_panel";
const VIM_FLAG: &str = "vim";

/// How many children a node shows at a time, and an adapter is asked for
/// in one page.
pub(crate) const PAGE: usize = 100;

/// The tool window's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tab {
    #[default]
    Variables,
    Breakpoints,
}

/// The parts the keys move between ([`DebugPanel::traverse`]): the row of
/// tabs, the trees and the Add watch input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Part {
    Tabs,
    Stack,
    Watch,
    WatchInput,
    Variables,
    Breakpoints,
}

impl Tab {
    /// Its parts, in the order tab moves through them.
    fn parts(self) -> &'static [Part] {
        match self {
            Tab::Variables => &[
                Part::Tabs,
                Part::Stack,
                Part::Watch,
                Part::WatchInput,
                Part::Variables,
            ],
            Tab::Breakpoints => &[Part::Tabs, Part::Breakpoints],
        }
    }

    /// The part it opens on: its first tree.
    fn first_tree(self) -> Part {
        match self {
            Tab::Variables => Part::Stack,
            Tab::Breakpoints => Part::Breakpoints,
        }
    }
}

/// A node of the Watch and Variables trees: its scope or watch, then each
/// variable's name down to it. Expansion follows it from stop to stop,
/// as variable references don't.
pub(crate) type NodePath = Vec<SharedString>;

/// The Debug tool window: see the [module](self).
pub(crate) struct DebugPanel {
    workbench: WeakEntity<Workbench>,
    debugger: WeakEntity<Debugger>,
    /// The root's breakpoints and watches.
    store: Option<Entity<BreakpointStore>>,
    tab: Tab,
    /// A tab was picked since the session started, so its first stop
    /// leaves it.
    picked: bool,
    /// The session the tool window was last shown for ([`Debugger::session_id`]).
    shown_for: Option<u64>,
    /// Counts frame selections, so a selection's deferred navigation and
    /// band are dropped once another selection follows.
    pub(crate) frame_selection: u64,
    focus: FocusHandle,
    /// The stop shown, while the program is stopped.
    stop: Option<Stop>,
    /// The nodes expanded or collapsed by the user; others are as built:
    /// the first scope expanded, the rest collapsed.
    expanded: HashMap<NodePath, bool>,
    /// The watch expressions, as the store keeps them.
    watches: Vec<String>,
    watch_input: Entity<TextInput>,
    /// The row each tree has selected, and the tree with the keys.
    selected: HashMap<Part, usize>,
    part: Part,
    /// The breakpoint lines' code, read from disk by file, until the store
    /// changes.
    code: breakpoints::CodeCache,
    stack_scroll: UniformListScrollHandle,
    /// The Watch tree's: its heading, its rows, then the Add watch row.
    pub(crate) watch_scroll: gpui::ScrollHandle,
    /// The window it's in, to move focus off a control a change hides.
    window: Option<gpui::AnyWindowHandle>,
    values_scroll: UniformListScrollHandle,
    breakpoints_scroll: UniformListScrollHandle,
    _store: Option<Subscription>,
    _subscriptions: Vec<Subscription>,
}

impl DebugPanel {
    pub(crate) fn new(workbench: WeakEntity<Workbench>, cx: &mut Context<Self>) -> Self {
        let ui = fe_ui::ui_theme(cx);
        let watch_input = cx.new(|cx| {
            TextInput::new(text!("workbench-debug-panel-add-watch"), &ui, cx)
                .name(tr!("workbench-debug-panel-add-watch"))
                .bordered()
                .height(fe_ui::height::CONTROL)
        });
        let submitted = cx.subscribe(&watch_input, |panel, input, event, cx| {
            if *event == TextInputEvent::Submitted {
                let expression = input.read(cx).text().trim().to_owned();
                input.update(cx, |input, cx| input.clear(cx));
                panel.add_watch(expression, cx);
            }
        });
        Self {
            workbench,
            debugger: WeakEntity::new_invalid(),
            store: None,
            tab: Tab::Variables,
            picked: false,
            shown_for: None,
            frame_selection: 0,
            focus: cx.focus_handle(),
            stop: None,
            expanded: HashMap::new(),
            watches: Vec::new(),
            watch_input,
            selected: HashMap::new(),
            part: Part::Stack,
            code: breakpoints::CodeCache::default(),
            stack_scroll: UniformListScrollHandle::new(),
            watch_scroll: gpui::ScrollHandle::new(),
            window: None,
            values_scroll: UniformListScrollHandle::new(),
            breakpoints_scroll: UniformListScrollHandle::new(),
            _store: None,
            _subscriptions: vec![submitted],
        }
    }

    /// Shows `tab`, as a click on it does: picked, so a stop leaves it.
    pub(crate) fn pick_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.picked = true;
        self.tab = tab;
        self.part = tab.first_tree();
        cx.notify();
    }

    /// Notes that the session `id` began: the first time for it, its stop
    /// shows Variables again, and the caller shows the tool window. Whether
    /// it's new: an emulated restart launches its adapter again in the same
    /// session.
    fn session_began(&mut self, id: u64) -> bool {
        if self.shown_for == Some(id) {
            return false;
        }
        self.shown_for = Some(id);
        self.picked = false;
        true
    }

    /// The tab shown, and the part with the keys.
    #[cfg(test)]
    pub(crate) fn tab_and_part(&self) -> (Tab, Part) {
        (self.tab, self.part)
    }

    /// The watch expressions.
    #[cfg(test)]
    pub(crate) fn watches(&self) -> &[String] {
        &self.watches
    }

    /// The input that adds a watch.
    #[cfg(test)]
    pub(crate) fn watch_input(&self) -> &Entity<TextInput> {
        &self.watch_input
    }

    fn debugger(&self) -> Option<Entity<Debugger>> {
        self.debugger.upgrade()
    }

    /// Follows the window's debugger from now on.
    fn attach(
        &mut self,
        debugger: &Entity<Debugger>,
        window: gpui::AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        self.debugger = debugger.downgrade();
        self.window = Some(window);
        let events = cx.subscribe(debugger, |panel, _, event: &DebuggerEvent, cx| {
            panel.debugger_event(event, cx)
        });
        self._subscriptions.push(events);
    }

    /// Follows the store of `root`, or none, and its watches.
    fn follow_store(&mut self, store: Option<Entity<BreakpointStore>>, cx: &mut Context<Self>) {
        if self.store.as_ref().map(Entity::entity_id) == store.as_ref().map(Entity::entity_id) {
            return;
        }
        self._store = store.as_ref().map(|store| {
            cx.observe(store, |panel, store, cx| {
                panel.code.clear();
                let watches = store.read(cx).watch_expressions();
                if watches != panel.watches {
                    panel.watches = watches;
                    panel.evaluate_watches(cx);
                }
                cx.notify();
            })
        });
        self.watches = (store.as_ref())
            .map(|store| store.read(cx).watch_expressions())
            .unwrap_or_default();
        self.store = store;
        self.code.clear();
        cx.notify();
    }

    fn debugger_event(&mut self, event: &DebuggerEvent, cx: &mut Context<Self>) {
        match event {
            DebuggerEvent::Stopped(location) => {
                self.stopped(location, cx);
                // Not picked: the keys go with the tab, to its first tree.
                if !self.picked && self.tab != Tab::Variables {
                    self.tab = Tab::Variables;
                    if !Tab::Variables.parts().contains(&self.part) {
                        self.part = Tab::Variables.first_tree();
                    }
                }
            }
            DebuggerEvent::State(_) => {
                // A stop that's over, or one at a new epoch whose frames
                // aren't in yet, goes; its Stopped event brings the next.
                let current = (self.debugger())
                    .and_then(|debugger| Some(debugger.read(cx).stop_location()?.epoch));
                if self.stop.as_ref().map(|stop| stop.epoch) != current {
                    self.stop = None;
                }
            }
            DebuggerEvent::Breakpoints(_) => {}
            DebuggerEvent::Starting(_) | DebuggerEvent::StartGivenUp | DebuggerEvent::Output(_) => {
                return;
            }
        }
        self.leave_hidden_part(cx);
        cx.notify();
    }

    /// Adds the watch `expression`, kept in the root's state file, and
    /// evaluates it if stopped. Nothing for a blank one.
    pub(crate) fn add_watch(&mut self, expression: String, cx: &mut Context<Self>) {
        if expression.is_empty() {
            return;
        }
        self.watches.push(expression);
        self.save_watches(cx);
        self.evaluate_watches(cx);
        cx.notify();
    }

    /// Removes the `ix`th watch.
    pub(crate) fn remove_watch(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix >= self.watches.len() {
            return;
        }
        self.watches.remove(ix);
        self.save_watches(cx);
        cx.notify();
    }

    fn save_watches(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.store.clone() else {
            return;
        };
        let watches = self.watches.clone();
        store.update(cx, |store, cx| store.set_watch_expressions(watches, cx));
    }

    /// Whether `path` is expanded: as the user left it, else the first
    /// scope only.
    pub(crate) fn is_expanded(&self, path: &NodePath, default: bool) -> bool {
        self.expanded.get(path).copied().unwrap_or(default)
    }

    /// Runs `update` on the workbench once this update is over: the panel
    /// is often updated from inside the workbench's.
    fn with_workbench(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        update: impl FnOnce(&mut Workbench, &mut Window, &mut Context<Workbench>) + 'static,
    ) {
        let workbench = self.workbench.clone();
        window.defer(cx, move |window, cx| {
            workbench
                .update(cx, |workbench, cx| update(workbench, window, cx))
                .ok();
        });
    }

    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let plain = !m.modified();
        if keystroke.key == "tab" && !m.control && !m.alt && !m.platform {
            self.traverse(if m.shift { -1 } else { 1 }, window, cx);
            cx.stop_propagation();
            return cx.notify();
        }
        // The Add watch input types its keys; enter adds the watch, escape
        // leaves it for the Watch tree.
        if self.watch_input.focus_handle(cx).is_focused(window) {
            match keystroke.key.as_str() {
                "enter" if plain => self.watch_input.update(cx, |input, cx| input.submit(cx)),
                "escape" if plain => self.focus_part(Part::Watch, window, cx),
                _ => return,
            }
            cx.stop_propagation();
            return cx.notify();
        }
        if !plain {
            return;
        }
        if !self.visible_parts().contains(&self.part) {
            self.part = Part::Tabs;
        }
        let handled = match (self.part, keystroke.key.as_str()) {
            (Part::Tabs, "left") => self.cycle_tab(-1, cx),
            (Part::Tabs, "right") => self.cycle_tab(1, cx),
            (_, "up") => self.move_selection(-1, cx),
            (_, "down") => self.move_selection(1, cx),
            (_, "right") => self.expand_selected(true, cx),
            (_, "left") => self.expand_selected(false, cx),
            (_, "enter") => self.act_on_selected(window, cx),
            (_, "space") => self.toggle_selected(cx),
            (_, "delete" | "backspace") => self.delete_selected(cx),
            _ => false,
        };
        if handled {
            cx.stop_propagation();
            cx.notify();
        }
    }

    /// The parts the tab shown draws now: Variables' trees and its Add
    /// watch input only while stopped, its empty state else.
    fn visible_parts(&self) -> &'static [Part] {
        match (self.tab, self.stop.is_some()) {
            (Tab::Variables, false) => &[Part::Tabs],
            (tab, _) => tab.parts(),
        }
    }

    /// Moves the keys and focus from the Add watch input to the row of tabs
    /// if it isn't drawn any more (the program ran on, or the session
    /// ended). A tree that isn't drawn keeps the keys for the next stop to
    /// give back; a key pressed before then goes to the tabs.
    fn leave_hidden_part(&mut self, cx: &mut Context<Self>) {
        if self.part != Part::WatchInput || self.visible_parts().contains(&self.part) {
            return;
        }
        self.part = Part::Tabs;
        let Some(window) = self.window else {
            return;
        };
        let (focus, typing) = (self.focus.clone(), self.watch_input.focus_handle(cx));
        cx.defer(move |cx| {
            let moved = window.update(cx, |_, window, cx| {
                if typing.is_focused(window) {
                    window.focus(&focus, cx);
                }
            });
            moved.ok();
        });
    }

    /// Tab (`by` 1) or shift-tab (-1): the keys go to the next or last part
    /// the tab shown draws, round from the row of tabs to its last tree.
    pub(crate) fn traverse(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let parts = self.visible_parts();
        let at = parts
            .iter()
            .position(|part| *part == self.part)
            .unwrap_or(0) as isize;
        let next = parts[(at + by).rem_euclid(parts.len() as isize) as usize];
        self.focus_part(next, window, cx);
    }

    /// Gives `part` the keys: the input its focus, a tree the panel's, its
    /// first row selected if none is.
    fn focus_part(&mut self, part: Part, window: &mut Window, cx: &mut Context<Self>) {
        self.part = part;
        if part == Part::WatchInput {
            // The Add watch row, last in Watch's scrolled list.
            self.watch_scroll
                .scroll_to_item(self.watch_rows().0.len() + 1);
            window.focus(&self.watch_input.focus_handle(cx), cx);
            return;
        }
        if part != Part::Tabs && !self.selected.contains_key(&part) && self.part_len(part, cx) > 0 {
            self.selected.insert(part, 0);
        }
        window.focus(&self.focus, cx);
    }

    /// Left and right on the row of tabs: the other tab shows, as a click
    /// shows it, the keys staying on the row.
    fn cycle_tab(&mut self, by: isize, cx: &mut Context<Self>) -> bool {
        let tabs = [Tab::Variables, Tab::Breakpoints];
        let at = tabs.iter().position(|tab| *tab == self.tab).unwrap_or(0) as isize;
        let next = tabs[(at + by).rem_euclid(tabs.len() as isize) as usize];
        self.pick_tab(next, cx);
        self.part = Part::Tabs;
        true
    }

    /// The Add watch input's editing commands (`text_input.backspace` and
    /// the like, which the input leaves to its host); others go on.
    pub(super) fn on_input_command(
        &mut self,
        run: &RunCommand,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match crate::picker::op(run, cx).and_then(crate::picker::PaletteOp::edit) {
            Some(edit) => self.watch_input.update(cx, edit),
            None => cx.propagate(),
        }
    }

    /// The rows of the tree with the keys.
    fn part_len(&self, part: Part, cx: &App) -> usize {
        match part {
            Part::Stack => self.stack_rows(cx).len(),
            Part::Watch => self.watch_rows().0.len(),
            Part::Variables => self.variable_rows().0.len(),
            Part::Breakpoints => self.breakpoint_rows(cx).len(),
            Part::Tabs => 2,
            Part::WatchInput => 0,
        }
    }

    fn move_selection(&mut self, by: isize, cx: &mut Context<Self>) -> bool {
        let len = self.part_len(self.part, cx);
        if len == 0 {
            return false;
        }
        let at = self.selected.get(&self.part).copied();
        let next = match at {
            None => 0,
            Some(at) => at.saturating_add_signed(by).min(len - 1),
        };
        self.selected.insert(self.part, next);
        if self.part == Part::Watch {
            // After Watch's heading.
            self.watch_scroll.scroll_to_item(next + 1);
        }
        let scroll = match self.part {
            Part::Stack => Some(&self.stack_scroll),
            Part::Variables => Some(&self.values_scroll),
            Part::Breakpoints => Some(&self.breakpoints_scroll),
            Part::Watch | Part::Tabs | Part::WatchInput => None,
        };
        if let Some(scroll) = scroll {
            scroll.scroll_to_item(next, gpui::ScrollStrategy::Top);
        }
        true
    }

    fn selected_in(&self, part: Part) -> Option<usize> {
        self.selected.get(&part).copied()
    }

    fn expand_selected(&mut self, expand: bool, cx: &mut Context<Self>) -> bool {
        let Some(ix) = self.selected_in(self.part) else {
            return false;
        };
        match self.part {
            Part::Stack => match self.stack_rows(cx).get(ix) {
                Some(StackRow::Thread { id, expanded, .. }) if *expanded != expand => {
                    self.toggle_thread(*id, cx);
                    true
                }
                _ => false,
            },
            Part::Watch | Part::Variables => {
                let rows = match self.part {
                    Part::Watch => self.watch_rows().0,
                    _ => self.variable_rows().0,
                };
                match rows.get(ix) {
                    Some(row) if row.expandable() && row.expanded() != expand => {
                        self.toggle_node(row, cx);
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn act_on_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(ix) = self.selected_in(self.part) else {
            return false;
        };
        match self.part {
            Part::Stack => {
                self.click_stack_row(ix, window, cx);
                true
            }
            Part::Watch | Part::Variables => {
                let rows = match self.part {
                    Part::Watch => self.watch_rows().0,
                    _ => self.variable_rows().0,
                };
                match rows.get(ix) {
                    Some(row @ ValueRow::More { .. }) => {
                        let row = row.clone();
                        self.show_more(&row, cx);
                        true
                    }
                    Some(row) if row.expandable() => {
                        let row = row.clone();
                        self.toggle_node(&row, cx);
                        true
                    }
                    _ => false,
                }
            }
            Part::Breakpoints => {
                self.click_breakpoint_row(ix, window, cx);
                true
            }
            Part::Tabs | Part::WatchInput => false,
        }
    }

    fn toggle_selected(&mut self, cx: &mut Context<Self>) -> bool {
        match (self.part, self.selected_in(Part::Breakpoints)) {
            (Part::Breakpoints, Some(ix)) => self.toggle_breakpoint_row(ix, cx),
            _ => false,
        }
    }

    fn delete_selected(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(ix) = self
            .selected_in(Part::Watch)
            .filter(|_| self.part == Part::Watch)
        else {
            return false;
        };
        match self.watch_rows().0.get(ix) {
            Some(ValueRow::Watch { index, .. }) => {
                let index = *index;
                self.remove_watch(index, cx);
                true
            }
            _ => false,
        }
    }

    /// Selects row `ix` of `part`, giving it the keys, as a click does.
    fn select_row(&mut self, part: Part, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.part = part;
        self.selected.insert(part, ix);
        window.focus(&self.focus, cx);
        cx.notify();
    }
}

impl Focusable for DebugPanel {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The selectors tests find the parts by.
pub(crate) const TAB_ID: &str = "debug-panel-tab";
pub(crate) const STACK_ID: &str = "debug-panel-call-stack";
pub(crate) const WATCH_ID: &str = "debug-panel-watch";
pub(crate) const VARIABLES_ID: &str = "debug-panel-variables";
pub(crate) const BREAKPOINTS_ID: &str = "debug-panel-breakpoints";
pub(crate) const EMPTY_SELECTOR: &str = "debug-panel-empty";

impl Render for DebugPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = fe_ui::ui_theme(cx);
        let c = cx.theme().colors;
        let mut context = KeyContext::default();
        context.add(KEY_CONTEXT);
        if fe_gpui::vim_enabled(cx) {
            context.add(VIM_FLAG);
        }
        let tabs = [
            (Tab::Variables, tr!("workbench-debug-panel-tab-variables")),
            (
                Tab::Breakpoints,
                tr!("workbench-debug-panel-tab-breakpoints"),
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(ix, (tab, label))| {
            let shown = self.tab == tab;
            let cursor = shown && self.part == Part::Tabs && self.focus.is_focused(window);
            let element = div()
                .id(gpui::ElementId::NamedInteger(TAB_ID.into(), ix as u64))
                .h_full()
                .px_2()
                .flex()
                .items_center()
                .cursor_pointer()
                .text_color(if shown { c.text.primary } else { c.text.muted })
                .when(shown, |tab| tab.border_b_2().border_color(c.accent.base))
                .when(cursor, |tab| {
                    tab.rounded(fe_ui::radius::CONTROL).bg(ui.row_hover)
                })
                .hover(move |tab| tab.text_color(c.text.primary))
                .on_click(cx.listener(move |panel, _, _, cx| panel.pick_tab(tab, cx)))
                .child(label.clone());
            let states = fe_ui::States {
                selected: Some(shown),
                ..Default::default()
            };
            fe_ui::node_with(element, window, cx, fe_ui::Role::Tab, label, states)
        })
        .collect::<Vec<_>>();
        let header = div()
            .flex_none()
            .h(fe_ui::height::CONTROL)
            .px_1()
            .flex()
            .items_center()
            .gap_1()
            .border_b_1()
            .border_color(c.border.subtle)
            .text_size(fe_ui::UI_SMALL.size)
            .children(tabs);
        let body: AnyElement = match self.tab {
            Tab::Variables => self.render_variables_tab(window, cx),
            Tab::Breakpoints => self.render_breakpoints_tab(window, cx),
        };
        div()
            .id(verbatim!("debug-panel"))
            .key_context(context)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(ui.panel_background)
            .text_color(ui.text)
            .text_size(px(12.))
            .child(header)
            .child(div().flex_1().min_h_0().flex().child(body))
    }
}

impl DebugPanel {
    /// An empty state, centred, in muted body text, with at most one
    /// button.
    fn empty(
        &self,
        message: SharedString,
        button: Option<(kiln_l10n::Text, &'static str)>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = cx.theme().colors;
        div()
            .debug_selector(|| EMPTY_SELECTOR.to_owned())
            .flex_1()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .text_size(fe_ui::UI_BODY.size)
            .text_color(c.text.muted)
            .child(message)
            .when_some(button, |empty, (label, command)| {
                empty.child(
                    fe_ui::Button::new(verbatim!("debug-panel-empty-button"), label)
                        .small()
                        .on_click(move |_, window, cx| {
                            kiln_shell::dock::run_action(command, window, cx)
                        }),
                )
            })
            .into_any_element()
    }

    /// The empty state the Variables tab shows instead of its columns, if
    /// it does: no session, or one running.
    fn variables_empty(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.stop.is_some() {
            return None;
        }
        let state = self.debugger().and_then(|debugger| {
            let debugger = debugger.read(cx);
            debugger.is_active().then(|| debugger.state())
        });
        let empty = match state {
            None => self.empty(
                tr!("workbench-debug-panel-not-debugging"),
                Some((text!("workbench-debug-panel-start"), "debug.start")),
                cx,
            ),
            Some(Some(SessionState::Running)) => self.empty(
                tr!("workbench-debug-panel-running"),
                Some((text!("workbench-debug-toolbar-pause"), "debug.pause")),
                cx,
            ),
            Some(_) => self.empty(tr!("workbench-debug-panel-waiting"), None, cx),
        };
        Some(empty)
    }
}

impl Workbench {
    /// The Debug tool window.
    #[cfg(test)]
    pub(crate) fn debug_panel(&self) -> &Entity<DebugPanel> {
        &self.debug_panel
    }

    /// `debug.show_panels`: shows the Debug tool window in the bottom dock,
    /// focused, or hides it if the dock shows it, focus going back to the
    /// editor if it was in it.
    pub(crate) fn toggle_debug_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self.terminal_dock().read(cx).dock().rail_highlighted();
        if shown == Some(PanelId(PANEL)) {
            let focused = (self.debug_panel.focus_handle(cx)).contains_focused(window, cx);
            self.hide_bottom_panel(cx);
            if focused {
                window.focus(&self.focus_handle(cx), cx);
            }
            return;
        }
        self.dock().update(cx, |dock, cx| dock.join_debug(cx));
        self.show_bottom_panel(PanelId(PANEL), window, cx);
        window.focus(&self.debug_panel.focus_handle(cx), cx);
    }

    /// `debug.frame_up` (`by` 1, the caller) and `debug.frame_down` (-1,
    /// the callee): moves the call stack's selected frame.
    pub(crate) fn debug_move_frame(
        &mut self,
        by: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        (self.debug_panel.clone()).update(cx, |panel, cx| panel.move_frame(by, window, cx));
    }

    /// A session of the window's debugger began, or changed state: the
    /// rail's Debug button joins on the first, the tool window shows once
    /// per session, and the button's dot follows its stops.
    fn follow_debugger(
        &mut self,
        event: &DebuggerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let DebuggerEvent::State(state) = event else {
            return;
        };
        let session = (self.debugger(cx)).and_then(|debugger| debugger.read(cx).session_id());
        let began = *state == SessionState::Launching
            && session.is_some_and(|id| {
                (self.debug_panel.clone()).update(cx, |panel, _| panel.session_began(id))
            });
        if began {
            self.dock().update(cx, |dock, cx| dock.join_debug(cx));
            // Shown without focus: the stop takes the caret to its line.
            self.show_bottom_panel(PanelId(PANEL), window, cx);
        }
        let stopped = *state == SessionState::Stopped;
        self.dock()
            .update(cx, |dock, cx| dock.set_debug_stopped(stopped, cx));
    }
}

/// Hands each window's Debug tool window its debugger and its root's store,
/// and has the workbench follow the debugger's sessions. After
/// `debug_session::init`, whose hook makes the debugger.
pub(crate) fn init(cx: &mut App) {
    cx.observe_new(|workbench: &mut Workbench, window, cx| {
        let Some(window) = window else {
            return;
        };
        let Some(debugger) = debugger_of(&cx.entity(), cx) else {
            return;
        };
        let panel = workbench.debug_panel.clone();
        let handle = window.window_handle();
        panel.update(cx, |panel, cx| panel.attach(&debugger, handle, cx));
        cx.subscribe_in(&debugger, window, |workbench, _, event, window, cx| {
            workbench.follow_debugger(event, window, cx)
        })
        .detach();
        let follow_root = |workbench: &mut Workbench, cx: &mut Context<Workbench>| {
            let store = workbench.breakpoint_store(cx);
            let panel = workbench.debug_panel.clone();
            panel.update(cx, |panel, cx| panel.follow_store(store, cx));
        };
        follow_root(workbench, cx);
        cx.subscribe_self(move |workbench, _: &crate::RootChanged, cx| follow_root(workbench, cx))
            .detach();
    })
    .detach();
}

/// The file a frame is in, if its source has a path.
pub(crate) fn frame_path(frame: &fe_dap::protocol::StackFrame) -> Option<PathBuf> {
    (frame.source.as_ref())
        .and_then(|source| source.path.as_deref())
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}
