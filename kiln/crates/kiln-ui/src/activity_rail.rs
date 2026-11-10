// SPDX-License-Identifier: Apache-2.0

use gpui::{
    AnyView, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, RenderOnce, Role, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Toggled, Window, deferred, div, prelude::FluentBuilder as _, px, svg,
};

use crate::{ActiveTheme as _, BADGE_OVERHANG, BADGE_RING, Badge, ClickHandler, Icon, UiTheme};

/// The activity rail's width, and each of its buttons' width.
pub const RAIL_WIDTH: Pixels = px(42.);

/// A button's icon, square.
const ICON_SIZE: Pixels = px(16.);
/// The highlighted button's bar, on the rail's panel-facing edge.
const BAR_WIDTH: Pixels = px(2.);
/// The gap between the highlighted button's tinted tab and the rail's
/// outer edge, the window's.
const TAB_INSET: Pixels = px(4.);
/// The tinted tab's outer corners.
const TAB_RADIUS: Pixels = px(4.);
/// The space above the rail's first button and below its last.
const RAIL_PADDING: Pixels = px(4.);
/// How far a badge's outer edge, its ring's, sits inside its button's
/// right edge: clear of a left rail's selection bar.
const BADGE_RIGHT: Pixels = px(3.);
/// A count badge's fill's height and a dot's, inside their rings, as
/// kiln-ui's `Badge` draws them.
const BADGE_COUNT: Pixels = px(14.);
const BADGE_DOT: Pixels = px(6.);

type TooltipBuilder = Box<dyn Fn(&mut Window, &mut App) -> AnyView>;

/// A full-height vertical strip of [`RailButton`]s, a top group and a
/// bottom one, on the window's chrome, with a 1 px line on the edge facing
/// its panel: its right on the window's left edge
/// ([`ActivityRail::on_left`]), else its left.
#[derive(IntoElement)]
pub struct ActivityRail {
    id: ElementId,
    buttons: Vec<RailButton>,
    footer: Vec<RailButton>,
    track: Option<ScrollHandle>,
    on_left: bool,
}

impl ActivityRail {
    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(id: impl Into<ElementId>, _theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            buttons: Vec::new(),
            footer: Vec::new(),
            track: None,
            on_left: false,
        }
    }

    pub fn children(mut self, buttons: impl IntoIterator<Item = RailButton>) -> Self {
        self.buttons.extend(buttons);
        self
    }

    /// Buttons pinned to the rail's bottom, below `children`.
    pub fn footer(mut self, buttons: impl IntoIterator<Item = RailButton>) -> Self {
        self.footer.extend(buttons);
        self
    }

    /// Whether the rail is on the window's left edge, its panel to its
    /// right: its line and its buttons' selection are on its right edge.
    /// On the right edge, the default, they're on its left. Its buttons
    /// follow it.
    pub fn on_left(mut self, on_left: bool) -> Self {
        self.on_left = on_left;
        self
    }

    /// Tracks each button's laid-out bounds
    /// ([`ScrollHandle::bounds_for_item`]) in `handle`. With a footer, the
    /// item after the children is the gap that pushes it down, then the
    /// footer's buttons.
    pub fn track(mut self, handle: &ScrollHandle) -> Self {
        self.track = Some(handle.clone());
        self
    }
}

impl RenderOnce for ActivityRail {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let on_left = self.on_left;
        // Drawn under the buttons, so the highlight's bar covers it.
        let line = div()
            .absolute()
            .top_0()
            .map(|line| {
                if on_left {
                    line.right_0()
                } else {
                    line.left_0()
                }
            })
            .h_full()
            .w(px(1.))
            .bg(colors.border.default);
        let side = move |button: RailButton| button.on_left(on_left);
        div()
            .id(self.id)
            .relative()
            .flex_none()
            .w(RAIL_WIDTH)
            .h_full()
            .bg(colors.surface.chrome)
            .child(line)
            .child(
                div()
                    .id("buttons")
                    .flex()
                    .flex_col()
                    .size_full()
                    .py(RAIL_PADDING)
                    .when_some(self.track, |buttons, track| buttons.track_scroll(&track))
                    .children(self.buttons.into_iter().map(side))
                    .when(!self.footer.is_empty(), |buttons| {
                        buttons
                            .child(div().flex_1().min_h_0())
                            .children(self.footer.into_iter().map(side))
                    }),
            )
    }
}

/// One of the rail's buttons, [`RAIL_WIDTH`] by [`RailButton::HEIGHT`]: a
/// 16 px icon centred in it, in `list.icon`. Highlighted, the icon is in
/// `tab.active_text` on a tinted tab that runs to the rail's panel-facing
/// edge, with a 2 px `accent.base` bar on that edge. A [`Badge`] marks its
/// icon's bottom-right corner.
#[derive(IntoElement)]
pub struct RailButton {
    id: ElementId,
    icon: Icon,
    highlighted: bool,
    on_left: bool,
    name: Option<SharedString>,
    badge: Option<Badge>,
    on_click: Option<ClickHandler>,
    tooltip: Option<TooltipBuilder>,
}

impl RailButton {
    /// A button's height.
    pub const HEIGHT: Pixels = px(32.);

    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(id: impl Into<ElementId>, icon: Icon, _theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            icon,
            highlighted: false,
            on_left: false,
            name: None,
            badge: None,
            on_click: None,
            tooltip: None,
        }
    }

    pub fn highlighted(mut self, highlighted: bool) -> Self {
        self.highlighted = highlighted;
        self
    }

    /// Whether its rail is on the window's left edge, which puts its
    /// selection's bar on its right edge: [`ActivityRail::on_left`] sets it
    /// for the rail's buttons.
    pub fn on_left(mut self, on_left: bool) -> Self {
        self.on_left = on_left;
        self
    }

    /// Its accessible name, such as the panel it shows.
    pub fn name(mut self, name: impl Into<SharedString>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// A badge on its icon's bottom-right corner, its ring in the surface
    /// under it.
    pub fn badge(mut self, badge: Badge) -> Self {
        self.badge = Some(badge);
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Shown on hover, built when it shows: see [`crate::Tooltip`].
    pub fn tooltip(mut self, build: impl Fn(&mut Window, &mut App) -> AnyView + 'static) -> Self {
        self.tooltip = Some(Box::new(build));
        self
    }
}

impl RenderOnce for RailButton {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let color = if self.highlighted {
            colors.tab.active_text
        } else {
            colors.list.icon
        };
        let hover = colors.list.row_hover;
        let on_left = self.on_left;
        let tint = colors.state.selected;
        let icon_id = self.id.clone();
        // From the outer edge's inset to the panel-facing edge, the bar on
        // that edge.
        let tab = self.highlighted.then(|| {
            div()
                .debug_selector(|| "rail-selection".into())
                .absolute()
                .top_0()
                .h_full()
                .w(RAIL_WIDTH - TAB_INSET)
                .bg(tint)
                .map(|tab| match on_left {
                    true => tab.right_0().rounded_l(TAB_RADIUS),
                    false => tab.left_0().rounded_r(TAB_RADIUS),
                })
        });
        let bar = self.highlighted.then(|| {
            div()
                .debug_selector(|| "rail-bar".into())
                .absolute()
                .top_0()
                .map(|bar| if on_left { bar.right_0() } else { bar.left_0() })
                .h_full()
                .w(BAR_WIDTH)
                .bg(colors.accent.base)
        });
        // The ring is the colour under the badge: the chrome, tinted if
        // highlighted.
        let surface = match self.highlighted {
            true => colors.surface.chrome.blend(tint),
            false => colors.surface.chrome,
        };
        // Its fill's top on the icon's bottom, its right edge
        // `BADGE_RIGHT` inside the button's: below the glyph, never over
        // it, its ring alone touching the icon's box. A count's runs into
        // the space above the next button's icon, so it paints after the
        // rail's buttons. `Badge` puts itself on its host's bottom-right
        // corner, `BADGE_OVERHANG` past it.
        let badge = self.badge.map(|badge| {
            let fill = if badge.text().is_some() {
                BADGE_COUNT
            } else {
                BADGE_DOT
            };
            let top = (Self::HEIGHT + ICON_SIZE) / 2. - BADGE_RING;
            let bottom = top + fill + BADGE_RING * 2.;
            let host = div()
                .absolute()
                .top_0()
                .left_0()
                .w(RAIL_WIDTH - BADGE_RIGHT - BADGE_OVERHANG)
                .h(bottom - BADGE_OVERHANG)
                .child(badge.surface(surface));
            deferred(host)
        });
        div()
            .id(self.id)
            .role(Role::Button)
            .aria_toggled(Toggled::from(self.highlighted))
            .when_some(self.name, |button, name| button.aria_label(name))
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .w(RAIL_WIDTH)
            .h(Self::HEIGHT)
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .children(tab)
            .child(
                div()
                    .debug_selector(|| format!("rail-icon:{icon_id}"))
                    .flex_none()
                    .size(ICON_SIZE)
                    .child(
                        svg()
                            .path(self.icon.path())
                            .size(ICON_SIZE)
                            .text_color(color),
                    ),
            )
            .children(bar)
            .children(badge)
            .when_some(self.on_click, |button, on_click| button.on_click(on_click))
            .when_some(self.tooltip, |button, tooltip| {
                button.tooltip(move |window, cx| tooltip(window, cx))
            })
    }
}
