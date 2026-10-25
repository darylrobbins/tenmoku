// SPDX-License-Identifier: LicenseRef-Proprietary

use gpui::{
    App, ClickEvent, Div, ElementId, Entity, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Point, Render, RenderOnce, ScrollHandle, SharedString, Stateful,
    StatefulInteractiveElement as _, StyleRefinement, Styled as _, Window, div,
    prelude::FluentBuilder as _, px, svg,
};

use crate::{ActiveTheme as _, ClickHandler, Icon, UiTheme};

pub const TAB_HEIGHT: gpui::Pixels = px(30.);

/// A row of [`Tab`]s that scrolls sideways when they don't fit.
#[derive(IntoElement)]
pub struct TabBar {
    id: ElementId,
    tabs: Vec<Tab>,
    scroll: Option<ScrollHandle>,
    drag: DragHooks,
}

/// Drag and drop handlers, kept until the element they go on is built.
type DragHooks = Vec<Box<dyn FnOnce(Stateful<Div>) -> Stateful<Div>>>;

fn apply(element: Stateful<Div>, hooks: DragHooks) -> Stateful<Div> {
    hooks
        .into_iter()
        .fold(element, |element, hook| hook(element))
}

impl TabBar {
    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(id: impl Into<ElementId>, _theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            tabs: Vec::new(),
            scroll: None,
            drag: Vec::new(),
        }
    }

    /// Called when a `T` is dropped on the bar outside its tabs (or on a tab
    /// with no [`Tab::on_drop`] of its own), such as after the last tab.
    pub fn on_drop<T: 'static>(
        mut self,
        handler: impl Fn(&T, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.drag.push(Box::new(|bar| bar.on_drop(handler)));
        self
    }

    /// Restyles the bar while a `T` is dragged over it.
    pub fn drag_over<T: 'static>(
        mut self,
        style: impl Fn(StyleRefinement, &T, &mut Window, &mut App) -> StyleRefinement + 'static,
    ) -> Self {
        self.drag.push(Box::new(|bar| bar.drag_over(style)));
        self
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
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        div()
            .id(self.id)
            .flex()
            .flex_none()
            .h(TAB_HEIGHT)
            .w_full()
            .overflow_x_scroll()
            .bg(colors.surface.panel)
            .border_b_1()
            .border_color(colors.border.default)
            .when_some(self.scroll, |bar, scroll| bar.track_scroll(&scroll))
            .map(|bar| apply(bar, self.drag))
            .children(self.tabs)
    }
}

/// One tab: its icon, its label and, muted, its description, then a dot
/// if it has unsaved changes or else a close button. A preview tab's label
/// and description are in italics. An inactive tab's
/// close button shows only while the pointer is over the tab, as VS Code's
/// does; its dirty dot always shows.
#[derive(IntoElement)]
pub struct Tab {
    id: ElementId,
    icon: Option<Icon>,
    label: SharedString,
    description: Option<SharedString>,
    label_color: Option<Hsla>,
    dirty: bool,
    active: bool,
    preview: bool,
    on_click: Option<ClickHandler>,
    on_close: Option<ClickHandler>,
    on_middle_click: Option<ClickHandler>,
    drag: DragHooks,
}

impl Tab {
    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, _theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            icon: None,
            label: label.into(),
            description: None,
            label_color: None,
            dirty: false,
            active: false,
            preview: false,
            on_click: None,
            on_close: None,
            on_middle_click: None,
            drag: Vec::new(),
        }
    }

    /// Drawn before the label, in the theme's file icon colour: a file
    /// tab's file-type icon, say.
    pub fn icon(mut self, icon: Option<Icon>) -> Self {
        self.icon = icon;
        self
    }

    /// The icon drawn before the label, if any.
    pub fn icon_kind(&self) -> Option<Icon> {
        self.icon
    }

    /// Drawn after the label, at its size in the theme's muted text colour:
    /// what tells this tab apart from others with the same label, such as
    /// its file's folder.
    pub fn description(mut self, description: Option<SharedString>) -> Self {
        self.description = description;
        self
    }

    /// The description drawn after the label, if any.
    pub fn description_text(&self) -> Option<&SharedString> {
        self.description.as_ref()
    }

    /// Draws the title in `color`, active or not, in place of the theme's
    /// tab text colour. The dirty dot and close button keep theirs.
    pub fn label_color(mut self, color: Option<Hsla>) -> Self {
        self.label_color = color;
        self
    }

    /// The colour the title is drawn in: the label colour if set, else the
    /// theme's tab text colour for this tab's state.
    pub fn title_color(&self, cx: &App) -> Hsla {
        tab_label_color(self.active, self.label_color, cx)
    }

    /// Whether the tab shows its dirty dot.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn dirty(mut self, dirty: bool) -> Self {
        self.dirty = dirty;
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Draws the label and description in italics: a preview tab, which
    /// the next file opened for a look replaces, as VS Code's are.
    pub fn preview(mut self, preview: bool) -> Self {
        self.preview = preview;
        self
    }

    /// Whether the tab is drawn as a preview, its label in italics.
    pub fn is_preview(&self) -> bool {
        self.preview
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

    /// Called when the tab is clicked with the middle button, as VS Code
    /// closes a tab. The close button's own click doesn't reach it.
    pub fn on_middle_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_middle_click = Some(Box::new(handler));
        self
    }

    /// Makes the tab draggable: moving the pointer more than 2 px with the
    /// button down starts a drag of `value`, drawn as the view `ghost`
    /// makes, which follows the pointer.
    pub fn on_drag<T: 'static, W: Render>(
        mut self,
        value: T,
        ghost: impl Fn(&T, Point<Pixels>, &mut Window, &mut App) -> Entity<W> + 'static,
    ) -> Self {
        self.drag.push(Box::new(|tab| tab.on_drag(value, ghost)));
        self
    }

    /// Called when a `T` is dropped on the tab.
    pub fn on_drop<T: 'static>(
        mut self,
        handler: impl Fn(&T, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.drag.push(Box::new(|tab| tab.on_drop(handler)));
        self
    }

    /// Restyles the tab while a `T` is dragged over it.
    pub fn drag_over<T: 'static>(
        mut self,
        style: impl Fn(StyleRefinement, &T, &mut Window, &mut App) -> StyleRefinement + 'static,
    ) -> Self {
        self.drag.push(Box::new(|tab| tab.drag_over(style)));
        self
    }
}

/// The colour a tab's title is drawn in: `tint` if set, else the theme's
/// tab text colour for an active or inactive tab.
fn tab_label_color(active: bool, tint: Option<Hsla>, cx: &App) -> Hsla {
    let colors = &cx.theme().colors;
    tint.unwrap_or(if active {
        colors.tab.active_text
    } else {
        colors.text.primary
    })
}

/// The group an inactive tab's close button shows on hovering.
const TAB_GROUP: &str = "kiln-tab";

impl RenderOnce for Tab {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let title_color = self.title_color(cx);
        let colors = &cx.theme().colors;
        let (hover, muted) = (colors.list.row_hover, colors.text.muted);
        let label = self.label.clone();
        // Hidden, it keeps its place, so the tab doesn't change width.
        let hidden_at_rest = !self.active && !self.dirty;
        let close = div()
            .id("close")
            .size(px(14.))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded_sm()
            .hover(move |style| style.bg(hover))
            .when(hidden_at_rest, |close| {
                close
                    .invisible()
                    .group_hover(TAB_GROUP, |style| style.visible())
            })
            .map(|close| {
                if self.dirty {
                    close.child(
                        div()
                            .size(px(6.))
                            .rounded_full()
                            .bg(colors.state.dirty)
                            .debug_selector(|| format!("tab-dirty:{label}")),
                    )
                } else {
                    close.text_color(muted).child(
                        div()
                            .child("×")
                            .debug_selector(|| format!("tab-close:{label}")),
                    )
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
            .group(TAB_GROUP)
            .flex()
            .flex_none()
            .h_full()
            .items_center()
            .gap_2()
            .pl_3()
            .pr_2()
            .border_r_1()
            .border_color(colors.border.default)
            .map(|tab| {
                if self.active {
                    tab.bg(colors.tab.active)
                } else {
                    tab.bg(colors.tab.inactive)
                }
            })
            .when_some(self.icon, |tab, icon| {
                tab.child(
                    svg()
                        .path(icon.path())
                        .size(px(14.))
                        .flex_none()
                        .text_color(colors.list.icon)
                        .debug_selector(|| format!("tab-icon:{}:{}", self.label, icon.name())),
                )
            })
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap_1p5()
                    .whitespace_nowrap()
                    .when(self.preview, |label| label.italic())
                    .child(div().text_color(title_color).child(self.label))
                    .when_some(self.description, |label, description| {
                        label.child(div().text_color(muted).child(description))
                    }),
            )
            .child(close)
            .when_some(self.on_click, |tab, on_click| tab.on_click(on_click))
            .when_some(self.on_middle_click, |tab, on_middle_click| {
                tab.on_aux_click(move |event, window, cx| {
                    if event.is_middle_click() {
                        on_middle_click(event, window, cx);
                    }
                })
            })
            .map(|tab| apply(tab, self.drag))
    }
}
