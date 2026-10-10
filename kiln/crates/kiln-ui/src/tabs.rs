// SPDX-License-Identifier: LicenseRef-Proprietary

//! [`Tabs`]: views behind a [`TabBar`], one shown at a time.

use gpui::{
    AnyView, Context, EventEmitter, IntoElement, ParentElement as _, Render, ScrollHandle,
    SharedString, Styled as _, Window, div,
};

use crate::{Tab, TabBar, ui_theme, v_flex};

/// What [`Tabs`] tells its subscribers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabsEvent {
    /// Tab `ix` was clicked and is now shown.
    Activated(usize),
    /// Tab `ix`'s close button was clicked. The tab stays until its owner
    /// removes it with [`Tabs::remove`], so it can ask first.
    CloseRequested(usize),
}

/// Labelled views behind a tab bar: clicking a tab shows its view and
/// emits [`TabsEvent::Activated`]. Tabs are added and removed by the
/// owner, which is told when one's close button is clicked.
pub struct Tabs {
    tabs: Vec<(SharedString, AnyView)>,
    active: usize,
    scroll: ScrollHandle,
}

impl EventEmitter<TabsEvent> for Tabs {}

impl Default for Tabs {
    fn default() -> Self {
        Self::new()
    }
}

impl Tabs {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            scroll: ScrollHandle::new(),
        }
    }

    /// Adds a tab, while building.
    pub fn with_tab(mut self, label: impl Into<SharedString>, view: impl Into<AnyView>) -> Self {
        self.tabs.push((label.into(), view.into()));
        self
    }

    /// Adds a tab at the end, without showing it.
    pub fn push(
        &mut self,
        label: impl Into<SharedString>,
        view: impl Into<AnyView>,
        cx: &mut Context<Self>,
    ) {
        self.tabs.push((label.into(), view.into()));
        cx.notify();
    }

    /// Removes tab `ix`, if there is one; the tab after it, or else the
    /// one before, is shown if it was.
    pub fn remove(&mut self, ix: usize, cx: &mut Context<Self>) -> Option<AnyView> {
        if ix >= self.tabs.len() {
            return None;
        }
        let (_, view) = self.tabs.remove(ix);
        if ix < self.active || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        cx.notify();
        Some(view)
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// The tab shown; 0 when there are none.
    pub fn active(&self) -> usize {
        self.active
    }

    /// Shows tab `ix` (clamped to the tabs) without emitting an event.
    pub fn set_active(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.active = ix.min(self.tabs.len().saturating_sub(1));
        self.scroll.scroll_to_item(self.active);
        cx.notify();
    }

    /// The tab bar's scroll handle, which knows each tab's bounds.
    pub fn scroll_handle(&self) -> &ScrollHandle {
        &self.scroll
    }
}

impl Render for Tabs {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ui_theme(cx);
        let tabs = self.tabs.iter().enumerate().map(|(ix, (label, _))| {
            Tab::new(("tab", ix), label.clone(), &theme)
                .active(ix == self.active)
                .on_click(cx.listener(move |tabs, _, _, cx| {
                    if ix < tabs.tabs.len() {
                        tabs.set_active(ix, cx);
                        cx.emit(TabsEvent::Activated(ix));
                    }
                }))
                .on_close(cx.listener(move |_, _, _, cx| cx.emit(TabsEvent::CloseRequested(ix))))
        });
        let bar = TabBar::new("tabs", &theme)
            .track_scroll(&self.scroll)
            .children(tabs);
        v_flex().size_full().child(bar).child(
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .children(self.tabs.get(self.active).map(|(_, view)| view.clone())),
        )
    }
}
