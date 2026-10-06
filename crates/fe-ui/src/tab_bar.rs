use gpui::{
    App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _, Window,
    div, prelude::FluentBuilder as _, px,
};

use crate::{ClickHandler, UiTheme};

pub const TAB_HEIGHT: gpui::Pixels = px(30.);

/// A row of [`Tab`]s that scrolls sideways when they don't fit.
#[derive(IntoElement)]
pub struct TabBar {
    id: ElementId,
    tabs: Vec<Tab>,
    theme: UiTheme,
    scroll: Option<ScrollHandle>,
}

impl TabBar {
    pub fn new(id: impl Into<ElementId>, theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            tabs: Vec::new(),
            theme: *theme,
            scroll: None,
        }
    }

    pub fn children(mut self, tabs: impl IntoIterator<Item = Tab>) -> Self {
        self.tabs.extend(tabs);
        self
    }

    /// Tracks the bar's scroll offset and each tab's laid-out bounds
    /// ([`ScrollHandle::bounds_for_item`]) in `handle`.
    pub fn track_scroll(mut self, handle: &ScrollHandle) -> Self {
        self.scroll = Some(handle.clone());
        self
    }
}

impl RenderOnce for TabBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .h(TAB_HEIGHT)
            .w_full()
            .overflow_x_scroll()
            .bg(self.theme.panel_background)
            .border_b_1()
            .border_color(self.theme.border)
            .when_some(self.scroll, |bar, scroll| bar.track_scroll(&scroll))
            .children(self.tabs)
    }
}

/// One tab: its label, then a dot if it has unsaved changes or else a
/// close button.
#[derive(IntoElement)]
pub struct Tab {
    id: ElementId,
    label: SharedString,
    dirty: bool,
    active: bool,
    theme: UiTheme,
    on_click: Option<ClickHandler>,
    on_close: Option<ClickHandler>,
}

impl Tab {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            dirty: false,
            active: false,
            theme: *theme,
            on_click: None,
            on_close: None,
        }
    }

    pub fn dirty(mut self, dirty: bool) -> Self {
        self.dirty = dirty;
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Called when the close button is clicked, instead of
    /// [`Tab::on_click`].
    pub fn on_close(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_close = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for Tab {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let close = div()
            .id("close")
            .size(px(14.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded_sm()
            .hover(move |style| style.bg(theme.row_hover))
            .map(|close| {
                if self.dirty {
                    close.child(div().size(px(6.)).rounded_full().bg(theme.dirty_dot))
                } else {
                    close.text_color(theme.muted_text).child("×")
                }
            })
            .when_some(self.on_close, |close, on_close| {
                close.on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    on_close(event, window, cx);
                })
            });
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .h_full()
            .items_center()
            .gap_2()
            .pl_3()
            .pr_2()
            .border_r_1()
            .border_color(theme.border)
            .map(|tab| {
                if self.active {
                    tab.bg(theme.tab_active).text_color(theme.tab_active_text)
                } else {
                    tab.bg(theme.tab_inactive).text_color(theme.text)
                }
            })
            .child(div().whitespace_nowrap().child(self.label))
            .child(close)
            .when_some(self.on_click, |tab, on_click| tab.on_click(on_click))
    }
}
