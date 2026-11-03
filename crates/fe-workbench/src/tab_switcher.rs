//! Switching tabs in most-recently-used order, as VS Code's ctrl-tab:
//! `workbench.tab_switcher_next` and `workbench.tab_switcher_previous`.
//!
//! Each pane keeps its tabs' order of use ([`Mru`]), noted as tabs are
//! activated and closed. Held down, ctrl-tab shows that order in a list in
//! the picker's place ([`TabSwitcher`]), over the active pane's tabs only,
//! selecting the tab used before this one; ctrl-shift-tab starts on the
//! last. Further presses move the selection, wrapping. Releasing ctrl shows
//! the selected tab, so a quick tap goes to the previous tab, and escape
//! closes the list, as does focus leaving it, without switching.
//!
//! The list isn't a [`crate::Picker`]: it has no query, and with vim on a
//! picker's escape goes to normal mode. Escape is read by the list's own key
//! listener, not a binding, since ctrl being held makes it `ctrl-escape`.
//!
//! The commands run without ctrl held, as from the menu, the palette or a
//! key bound without ctrl, have no release to wait for: they go straight to
//! the tab the list would select. So does a sole tab, there being no list to
//! choose from, as [`crate::Pane::cycle`] activates it.

use fe_ui::ActiveTheme as _;
use gpui::{
    AnyElement, App, AppContext as _, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement as _, IntoElement, KeyDownEvent, ModifiersChangedEvent,
    ParentElement as _, Render, ScrollStrategy, SharedString, Styled as _, Subscription,
    UniformListScrollHandle, WeakEntity, Window, div, prelude::FluentBuilder as _, px, svg,
    uniform_list,
};
use kiln_shell::{ModalId, ModalView, PaneView as _, picker::VISIBLE_ROWS};

use crate::{Pane, Workbench, pane::TabId};

/// The modal layer's id for the list.
const TAB_SWITCHER: ModalId = ModalId("tab_switcher");

/// The key context the list's element sets.
const KEY_CONTEXT: &str = "tab_switcher";

/// A pane's tabs that have been shown, the latest first.
#[derive(Debug, Default)]
pub(crate) struct Mru(Vec<TabId>);

impl Mru {
    /// Tab `id` was shown.
    pub(crate) fn activated(&mut self, id: TabId) {
        self.0.retain(|tab| *tab != id);
        self.0.insert(0, id);
    }

    /// Tab `id` came without the user seeing it, though it was noted as
    /// shown: it goes back among those never shown.
    pub(crate) fn forget(&mut self, id: TabId) {
        self.0.retain(|tab| *tab != id);
    }

    /// The pane's tabs are now `open`: any other closed or moved away.
    pub(crate) fn retain_open(&mut self, open: &[TabId]) {
        self.0.retain(|tab| open.contains(tab));
    }

    /// The tabs `open` in order of use: `active` first, then the latest
    /// shown, then those never shown (restored behind the active one, say)
    /// as they are in the strip.
    pub(crate) fn order(&self, open: &[TabId], active: Option<TabId>) -> Vec<TabId> {
        let mut order: Vec<TabId> = active.into_iter().collect();
        for id in self.0.iter().chain(open) {
            if !order.contains(id) {
                order.push(*id);
            }
        }
        order
    }
}

/// What a row shows of its tab.
struct Row {
    title: SharedString,
    description: Option<SharedString>,
    icon: Option<SharedString>,
}

/// The list over a pane's tabs, shown while ctrl is held.
pub(crate) struct TabSwitcher {
    pane: WeakEntity<Pane>,
    focus: FocusHandle,
    /// The pane's tabs in order of use, as last heard.
    order: Vec<TabId>,
    /// The selected index into `order`.
    selected: usize,
    scroll: UniformListScrollHandle,
    _subscriptions: [Subscription; 2],
}

impl EventEmitter<DismissEvent> for TabSwitcher {}

impl ModalView for TabSwitcher {}

impl Focusable for TabSwitcher {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl TabSwitcher {
    /// The list over `order`, `pane`'s tabs, with `selected` selected.
    fn new(
        pane: &Entity<Pane>,
        order: Vec<TabId>,
        selected: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        let scroll = UniformListScrollHandle::new();
        scroll.scroll_to_item(selected, ScrollStrategy::Nearest);
        Self {
            pane: pane.downgrade(),
            focus: cx.focus_handle(),
            order,
            selected,
            scroll,
            _subscriptions: [
                cx.observe(pane, Self::pane_changed),
                cx.observe_release(pane, |_, _, cx| cx.emit(DismissEvent)),
            ],
        }
    }

    /// Follows the pane's tabs: the selection stays on its tab, or, with
    /// that gone, at its place in the list. A list left empty goes.
    fn pane_changed(&mut self, pane: Entity<Pane>, cx: &mut Context<Self>) {
        let selected = self.order.get(self.selected).copied();
        self.order = pane.read(cx).tabs_by_recency();
        let Some(last) = self.order.len().checked_sub(1) else {
            return cx.emit(DismissEvent);
        };
        let kept = selected.and_then(|id| self.order.iter().position(|tab| *tab == id));
        self.selected = kept.unwrap_or(self.selected.min(last));
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Moves the selection a row on, or with `back` a row back, wrapping.
    fn step(&mut self, back: bool, cx: &mut Context<Self>) {
        let count = self.order.len();
        if count == 0 {
            return;
        }
        let by = if back { count - 1 } else { 1 };
        self.selected = (self.selected + by) % count;
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Shows the selected tab and goes.
    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let tab = self.order.get(self.selected).copied();
        if let (Some(pane), Some(tab)) = (self.pane.upgrade(), tab) {
            activate(&pane, tab, window, cx);
        }
        cx.emit(DismissEvent);
    }

    /// Ctrl's release is the commit. The window draws a pending list before
    /// it dispatches a modifier change, so even a release on the heels of
    /// the press finds this listener in place.
    fn on_modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.modifiers.control {
            self.commit(window, cx);
        }
    }

    /// Escape, with or without the ctrl that's held, goes without switching.
    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.stop_propagation();
            cx.emit(DismissEvent);
        }
    }

    /// The rows, in the list's order, as the tab strip draws their tabs:
    /// tabs of the same title are told apart by their folders.
    fn rows(&self, cx: &App) -> Vec<Row> {
        let Some(pane) = self.pane.upgrade() else {
            return Vec::new();
        };
        let state = pane.read(cx).pane();
        let tabs = state.tabs(cx);
        (self.order.iter())
            .filter_map(|id| {
                let ix = state.position(*id)?;
                let tab = tabs.get(ix)?;
                Some(Row {
                    title: state.items().get(ix)?.tab_title(cx),
                    description: tab.description_text().cloned(),
                    icon: tab.icon_ref().map(|icon| icon.path()),
                })
            })
            .collect()
    }

    fn render_rows(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let ui = fe_ui::ui_theme(cx);
        let icon_color = cx.theme().colors.list.icon;
        let rows = self.rows(cx);
        range
            .filter_map(|ix| {
                let row = rows.get(ix)?;
                let description = row.description.clone().map(|description| {
                    div()
                        .flex_none()
                        .text_color(ui.muted_text)
                        .child(description)
                });
                let icon = row.icon.clone().map(|icon| {
                    svg()
                        .path(icon)
                        .size(px(14.))
                        .flex_none()
                        .text_color(icon_color)
                });
                let title = row.title.clone();
                Some(
                    div()
                        .debug_selector(move || format!("tab-switcher-row:{title}"))
                        .h(fe_ui::ROW_HEIGHT)
                        .w_full()
                        .px_3()
                        .flex()
                        .items_center()
                        .gap_2()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_color(ui.text)
                        .when(ix == self.selected, |row| row.bg(ui.row_selected))
                        .children(icon)
                        .child(row.title.clone())
                        .children(description)
                        .into_any_element(),
                )
            })
            .collect()
    }
}

impl Render for TabSwitcher {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let ui = fe_ui::ui_theme(cx);
        let count = self.order.len();
        let list = uniform_list(
            "tab-switcher-rows",
            count,
            cx.processor(|switcher, range, _, cx| switcher.render_rows(range, cx)),
        )
        .track_scroll(&self.scroll)
        .h(fe_ui::ROW_HEIGHT * count.min(VISIBLE_ROWS) as f32);
        div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .on_modifiers_changed(cx.listener(Self::on_modifiers_changed))
            .on_key_down(cx.listener(Self::on_key_down))
            .absolute()
            .inset_0()
            .child(fe_ui::PaletteFrame::new(&ui).list(list))
    }
}

/// Shows `pane`'s tab `tab`, if it's still there.
fn activate(pane: &Entity<Pane>, tab: TabId, window: &mut Window, cx: &mut App) {
    pane.update(cx, |pane, cx| {
        if let Some(ix) = pane.pane().position(tab) {
            pane.activate_at(ix, window, cx);
            cx.notify();
        }
    });
}

impl Workbench {
    /// The list showing in the modal layer, if it is.
    pub(crate) fn tab_switcher(&self, cx: &App) -> Option<Entity<TabSwitcher>> {
        let layer = self.shell().read(cx).modals().read(cx);
        layer.modal().filter(|id| *id == TAB_SWITCHER)?;
        layer.modal_view()?.clone().downcast().ok()
    }

    /// Moves the list's selection, showing the list first if it's not up:
    /// see the module docs.
    pub(crate) fn switch_tab(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(switcher) = self.tab_switcher(cx) {
            switcher.update(cx, |switcher, cx| switcher.step(back, cx));
            return;
        }
        let pane = self.pane().clone();
        let order = pane.read(cx).tabs_by_recency();
        let Some(last) = order.len().checked_sub(1) else {
            return;
        };
        let selected = if back { last } else { last.min(1) };
        if last == 0 || !window.modifiers().control {
            return activate(&pane, order[selected], window, cx);
        }
        self.dismiss_palette(window, cx);
        let switcher = cx.new(|cx| TabSwitcher::new(&pane, order, selected, cx));
        let layer = self.shell().read(cx).modals().clone();
        layer.update(cx, |layer, cx| {
            layer.show_modal(TAB_SWITCHER, switcher, window, cx)
        });
    }
}

#[cfg(test)]
impl TabSwitcher {
    /// The rows' titles, in the list's order.
    pub(crate) fn titles(&self, cx: &App) -> Vec<String> {
        (self.rows(cx).into_iter())
            .map(|row| row.title.to_string())
            .collect()
    }

    /// The selected row's title.
    pub(crate) fn selected_title(&self, cx: &App) -> Option<String> {
        self.titles(cx).get(self.selected).cloned()
    }
}

#[cfg(test)]
#[path = "tab_switcher_tests.rs"]
mod tests;
