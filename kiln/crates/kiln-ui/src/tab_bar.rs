// SPDX-License-Identifier: Apache-2.0

use std::{rc::Rc, sync::Arc};

use gpui::{
    App, AppContext as _, ClickEvent, Div, ElementId, Entity, FocusHandle, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Point, Render, RenderOnce,
    ScrollHandle, SharedString, Stateful, StatefulInteractiveElement as _, StyleRefinement,
    Styled as _, Window, accesskit::Orientation, div, prelude::FluentBuilder as _, px,
};

use kiln_l10n::tr;

use crate::{
    ActiveTheme as _, ClickHandler, Icon, IconRef, Role, States, Tooltip, UiTheme, node, node_with,
};

/// A click handler shared by a tab's mouse and screen reader paths.
type Handler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// A screen reader's Click calls the handler directly, as a keyboard
/// click (`click_count` 1, so it doesn't pin): GPUI's fallback is a mouse
/// click at the node's centre, which misses hidden or offscreen nodes.
fn on_a11y_click<E: gpui::InteractiveElement + gpui::StatefulInteractiveElement>(
    element: E,
    handler: Option<Handler>,
) -> E {
    match handler {
        Some(handler) => element
            .on_a11y_action(gpui::accesskit::Action::Click, move |_, window, cx| {
                handler(&ClickEvent::default(), window, cx)
            }),
        None => element,
    }
}

/// The tab strip's height, which panels' tab strips and headers match
/// (`docs/design/design-system.md`, Measure).
pub const TAB_HEIGHT: gpui::Pixels = px(34.);
/// The accent line along the top of the focused pane's active tab.
const TAB_ACCENT_HEIGHT: Pixels = px(2.);
/// A tab's icon, square.
const TAB_ICON_SIZE: Pixels = px(14.);
/// The dirty dot in a pinned tab's corner, square.
const PIN_GLYPH_SIZE: Pixels = px(8.);
/// The close mark, and the box it's centred in, which a hover fills.
const CLOSE_ICON_SIZE: Pixels = px(12.);
const CLOSE_BOX_SIZE: Pixels = px(16.);

/// A row of [`Tab`]s that scrolls sideways when they don't fit, on the
/// window's chrome.
///
/// To a screen reader it is a horizontal `TabList` "Tabs", which macOS
/// shows as an `AXTabGroup`, and each tab one of its `Tab`s, a radio
/// button there, with the active one selected and its place among them.
#[derive(IntoElement)]
pub struct TabBar {
    id: ElementId,
    tabs: Vec<Tab>,
    scroll: Option<ScrollHandle>,
    focus: Option<FocusHandle>,
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
            focus: None,
            drag: Vec::new(),
        }
    }

    /// Marks the active tab with the accent line along its top while
    /// `focus`, the pane's, is focused or contains what is: only the
    /// focused pane's active tab has it.
    pub fn focus_within(mut self, focus: &FocusHandle) -> Self {
        self.focus = Some(focus.clone());
        self
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
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // Without a handle to follow, each tab keeps its own `focused`.
        let focused = (self.focus.as_ref()).map(|focus| focus.contains_focused(window, cx));
        let colors = &cx.theme().colors;
        let count = self.tabs.len();
        let tabs = (self.tabs.into_iter().enumerate()).map(|(ix, tab)| {
            let tab = tab.in_set(ix + 1, count);
            match focused {
                Some(focused) => tab.focused(focused),
                None => tab,
            }
        });
        let bar = div()
            .id(self.id)
            .flex()
            .flex_none()
            .h(TAB_HEIGHT)
            .w_full()
            .overflow_x_scroll()
            .bg(colors.surface.chrome)
            .border_b_1()
            .border_color(colors.border.default)
            .when_some(self.scroll, |bar, scroll| bar.track_scroll(&scroll))
            .map(|bar| apply(bar, self.drag))
            .children(tabs);
        node(bar, window, cx, Role::TabList, tr!("kiln-ui-tab-bar"))
            .aria_orientation(Orientation::Horizontal)
    }
}

/// One tab: its icon, its label and, muted, its description, then a dot
/// if it has unsaved changes or else a close button. A preview tab's label
/// and description are in italics. An inactive tab's
/// close button shows only while the pointer is over the tab, as VS Code's
/// does; its dirty dot always shows. The pointer over a tab fills it with
/// the hover state; the active tab of the focused pane ([`Tab::focused`])
/// has a 2 px accent line along its top.
///
/// A pinned tab ([`Tab::pinned`]) is drawn compact: its icon alone,
/// [`Tab::PINNED_WIDTH`] wide, its dirty dot in its corner if it has one,
/// and its label and description in its tooltip. It has no close button; a
/// middle click still closes it.
///
/// A screen reader hears its label and description, then "modified" if
/// it has unsaved changes ([`Tab::a11y_label`]), and after that its other
/// states ([`Tab::a11y_description`]). Its close button is the `Button`
/// "Close" and its label.
#[derive(IntoElement)]
pub struct Tab {
    id: ElementId,
    icon: Option<IconRef>,
    label: SharedString,
    description: Option<SharedString>,
    label_color: Option<Hsla>,
    dirty: bool,
    active: bool,
    focused: bool,
    preview: bool,
    pinned: bool,
    /// Its place in its [`TabBar`], from 1, and how many tabs that has.
    in_set: Option<(usize, usize)>,
    on_click: Option<Handler>,
    on_close: Option<Handler>,
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
            focused: false,
            preview: false,
            pinned: false,
            in_set: None,
            on_click: None,
            on_close: None,
            on_middle_click: None,
            drag: Vec::new(),
        }
    }

    /// Drawn before the label, in the theme's file icon colour unless it's
    /// coloured: a file tab's file-type icon, say: a bundled [`Icon`] or an
    /// icon set's answer.
    pub fn icon(mut self, icon: Option<impl Into<IconRef>>) -> Self {
        self.icon = icon.map(Into::into);
        self
    }

    /// The bundled icon drawn before the label, if it is one.
    pub fn icon_kind(&self) -> Option<Icon> {
        self.icon.as_ref().and_then(IconRef::bundled)
    }

    /// The icon drawn before the label, if any.
    pub fn icon_ref(&self) -> Option<&IconRef> {
        self.icon.as_ref()
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

    /// Whether the tab's pane is the focused one, whose active tab has the
    /// accent line. [`TabBar::focus_within`] sets it for its tabs.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Whether the tab is drawn with the accent line along its top: the
    /// active tab of the focused pane.
    pub fn is_marked(&self) -> bool {
        self.active && self.focused
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

    /// The width of a pinned tab: its icon and the padding around it.
    pub const PINNED_WIDTH: Pixels = px(38.);

    /// Draws the tab compact, its icon alone at [`Tab::PINNED_WIDTH`], its
    /// label in its tooltip: a pinned tab, kept at the strip's start.
    pub fn pinned(mut self, pinned: bool) -> Self {
        self.pinned = pinned;
        self
    }

    /// Whether the tab is drawn pinned, compact.
    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    /// What a pinned tab's tooltip says: its label, then its description
    /// if it has one. `None` for a tab that isn't pinned, whose label shows.
    pub fn tooltip_text(&self) -> Option<SharedString> {
        self.pinned.then(|| match &self.description {
            Some(description) => tr!(
                "kiln-ui-tab-pinned-tooltip",
                label = &self.label,
                description = description
            ),
            None => self.label.clone(),
        })
    }

    /// Its place among the tabs of its strip, from 1, and how many there
    /// are, for a screen reader's "2 of 5". [`TabBar`] sets it for its tabs.
    pub fn in_set(mut self, position: usize, count: usize) -> Self {
        self.in_set = Some((position, count));
        self
    }

    /// What a screen reader names the tab: its label, then its description
    /// (the folder telling it apart) if it has one, then "modified" if it
    /// has unsaved changes. Said every time the tab is, unlike
    /// [`Tab::a11y_description`], which macOS gives as help.
    pub fn a11y_label(&self) -> SharedString {
        let named = match &self.description {
            Some(description) => tr!(
                "kiln-ui-tab-label-described",
                label = &self.label,
                description = description
            ),
            None => self.label.clone(),
        };
        match self.dirty {
            true => tr!("kiln-ui-tab-label-modified", label = &named),
            false => named,
        }
    }

    /// The tab's states as a screen reader describes them: "modified",
    /// "pinned" and "preview", as they apply, or `None`.
    pub fn a11y_description(&self) -> Option<SharedString> {
        let states: Vec<SharedString> = [
            (self.dirty, tr!("kiln-ui-tab-state-modified")),
            (self.pinned, tr!("kiln-ui-tab-state-pinned")),
            (self.preview, tr!("kiln-ui-tab-state-preview")),
        ]
        .into_iter()
        .filter_map(|(on, state)| on.then_some(state))
        .collect();
        (!states.is_empty()).then(|| states.join(", ").into())
    }

    /// The role, label and states a screen reader is given for the tab.
    fn a11y_states(&self) -> (SharedString, States) {
        let states = States {
            selected: Some(self.active),
            description: self.a11y_description(),
            position_in_set: self.in_set.map(|(position, _)| position),
            size_of_set: self.in_set.map(|(_, count)| count),
            ..States::default()
        };
        (self.a11y_label(), states)
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    /// Called when the close button is clicked, instead of
    /// [`Tab::on_click`].
    pub fn on_close(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_close = Some(Rc::new(handler));
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

impl Tab {
    /// Under the tab's content, the hover fill over its own; along its
    /// top, the accent line if it's marked ([`Tab::is_marked`]). Neither
    /// takes space.
    fn under_content(&self, cx: &App) -> Vec<Div> {
        let colors = &cx.theme().colors;
        let hover = colors.state.hover;
        let fill = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .group_hover(TAB_GROUP, move |style| style.bg(hover));
        let label = self.label.clone();
        let accent = self.is_marked().then(|| {
            div()
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h(TAB_ACCENT_HEIGHT)
                .bg(colors.accent.base)
                .debug_selector(|| format!("tab-accent:{label}"))
        });
        std::iter::once(fill).chain(accent).collect()
    }
}

impl RenderOnce for Tab {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (a11y_label, a11y_states) = self.a11y_states();
        if self.pinned {
            let tab = self.render_pinned(window, cx);
            return node_with(tab, window, cx, Role::Tab, a11y_label, a11y_states)
                .into_any_element();
        }
        let title_color = self.title_color(cx);
        let under = self.under_content(cx);
        // Owned: the close button's node borrows `cx` while it's in use.
        let colors = &cx.theme().colors.clone();
        let (hover, muted) = (colors.state.hover, colors.text.muted);
        let label = self.label.clone();
        // Hidden, it keeps its place, so the tab doesn't change width.
        let hidden_at_rest = !self.active && !self.dirty;
        let close_label = tr!("kiln-ui-tab-close", label = &self.label);
        // Its own id, not one under the tab's: a screen reader's node is
        // told apart from the other tabs' close buttons by it.
        let close_id = ElementId::NamedChild(Arc::new(self.id.clone()), "close".into());
        let close = div()
            .id(close_id)
            .size(CLOSE_BOX_SIZE)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded_sm()
            .hover(move |style| style.bg(hover))
            .child(
                // Only this content hides at rest: the wrapper stays
                // painted, so its click and screen reader listeners
                // register. The debug selectors sit below it, so their
                // bounds still appear only when the mark is shown.
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(hidden_at_rest, |mark| {
                        mark.invisible()
                            .group_hover(TAB_GROUP, |style| style.visible())
                    })
                    .map(|mark| {
                        if self.dirty {
                            mark.child(
                                div()
                                    .size(px(6.))
                                    .rounded_full()
                                    .bg(colors.state.dirty)
                                    .debug_selector(|| format!("tab-dirty:{label}")),
                            )
                        } else {
                            mark.child(
                                div()
                                    .size(CLOSE_ICON_SIZE)
                                    .flex_none()
                                    .debug_selector(|| format!("tab-close:{label}"))
                                    .child(
                                        IconRef::Bundled(Icon::X).element(CLOSE_ICON_SIZE, muted),
                                    ),
                            )
                        }
                    }),
            )
            .when_some(self.on_close.clone(), |close, on_close| {
                let on_a11y = on_close.clone();
                let close = close.on_click(move |event, window, cx| {
                    cx.stop_propagation();
                    on_close(event, window, cx);
                });
                on_a11y_click(close, Some(on_a11y))
            });
        let close = node(close, window, cx, Role::Button, close_label);
        let tab = div()
            .id(self.id)
            .group(TAB_GROUP)
            .relative()
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
            .children(under)
            .when_some(self.icon.clone(), |tab, icon| {
                tab.child(
                    div()
                        .size(TAB_ICON_SIZE)
                        .flex_none()
                        .debug_selector(|| format!("tab-icon:{}:{}", self.label, icon.name()))
                        .child(icon.element(TAB_ICON_SIZE, colors.list.icon)),
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
            .when_some(self.on_click, |tab, on_click| {
                let tab = tab.on_click({
                    let on_click = on_click.clone();
                    move |event, window, cx| on_click(event, window, cx)
                });
                on_a11y_click(tab, Some(on_click))
            })
            .when_some(self.on_middle_click, |tab, on_middle_click| {
                tab.on_aux_click(move |event, window, cx| {
                    if event.is_middle_click() {
                        on_middle_click(event, window, cx);
                    }
                })
            })
            .map(|tab| apply(tab, self.drag));
        node_with(tab, window, cx, Role::Tab, a11y_label, a11y_states).into_any_element()
    }
}

impl Tab {
    /// A pinned tab: its icon alone centred at [`Tab::PINNED_WIDTH`], its
    /// dirty dot if it has one in its top right corner, and its label in a
    /// tooltip. A tab with no icon of its own shows the pin.
    fn render_pinned(self, _: &mut Window, cx: &mut App) -> Stateful<Div> {
        let under = self.under_content(cx);
        let colors = &cx.theme().colors;
        let tip = self.tooltip_text().unwrap_or_else(|| self.label.clone());
        let label = self.label.clone();
        let icon = self.icon.clone().unwrap_or(IconRef::Bundled(Icon::Pin));
        let corner = self.dirty.then(|| {
            div()
                .absolute()
                .top(px(4.))
                .right(px(4.))
                .size(PIN_GLYPH_SIZE)
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .size(px(6.))
                        .rounded_full()
                        .bg(colors.state.dirty)
                        .debug_selector(|| format!("tab-dirty:{label}")),
                )
        });
        div()
            .id(self.id)
            .group(TAB_GROUP)
            .relative()
            .flex()
            .flex_none()
            .w(Self::PINNED_WIDTH)
            .h_full()
            .items_center()
            .justify_center()
            .border_r_1()
            .border_color(colors.border.default)
            .bg(if self.active {
                colors.tab.active
            } else {
                colors.tab.inactive
            })
            .children(under)
            .child(
                div()
                    .size(TAB_ICON_SIZE)
                    .flex_none()
                    .debug_selector(|| format!("tab-icon:{}:{}", self.label, icon.name()))
                    .child(icon.element(TAB_ICON_SIZE, colors.list.icon)),
            )
            .children(corner)
            .tooltip(move |_, cx| {
                let text = tip.clone();
                cx.new(|cx| PinnedTip {
                    tip: cx
                        .new(|cx| Tooltip::new(text.clone(), None::<&str>, &crate::ui_theme(cx))),
                    text,
                })
                .into()
            })
            .when_some(self.on_click, |tab, on_click| {
                let tab = tab.on_click({
                    let on_click = on_click.clone();
                    move |event, window, cx| on_click(event, window, cx)
                });
                on_a11y_click(tab, Some(on_click))
            })
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

/// A pinned tab's tooltip: its label, and its description if any, which
/// the tab no longer shows. Found in tests as `tab-tooltip:<text>`.
struct PinnedTip {
    tip: Entity<Tooltip>,
    text: SharedString,
}

impl Render for PinnedTip {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let text = self.text.clone();
        div()
            .debug_selector(move || format!("tab-tooltip:{text}"))
            .child(self.tip.clone())
    }
}
