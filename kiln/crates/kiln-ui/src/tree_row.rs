// SPDX-License-Identifier: Apache-2.0

use std::f32::consts::FRAC_PI_2;

use gpui::{
    App, ClickEvent, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Transformation,
    Window, div, prelude::FluentBuilder as _, px, radians, svg,
};

use crate::{
    ActiveTheme as _, ClickHandler, Icon, IconRef, ROW_HEIGHT, Role, States, Tokens, UiTheme,
    node_with,
};

/// How far each level of the tree is indented.
pub const TREE_INDENT: Pixels = px(14.);
/// The chevron column, reserved on every row so files line up with their
/// sibling folders.
pub const CHEVRON_WIDTH: Pixels = px(16.);
/// A row's icon, square.
pub const ICON_SIZE: Pixels = px(16.);

/// Left of the chevron column at depth 0.
pub(crate) const PADDING: Pixels = px(6.);
/// The selection box's inset from the list's sides.
const BOX_INSET: Pixels = px(4.);
const CHEVRON_GAP: Pixels = px(2.);
const ICON_GAP: Pixels = px(6.);
const BADGE_GAP: Pixels = px(6.);
const DETAIL_GAP: Pixels = px(6.);
/// The least width of the column at a row's right end that a badge is
/// centred in, and a [`TreeRow::dot`]: wide enough for one capital, so
/// letters and dots line up; a longer badge is as wide as its text.
pub const BADGE_COLUMN: Pixels = px(14.);
/// A [`TreeRow::dot`]'s diameter: the least a coloured mark reads at.
pub const DOT_SIZE: Pixels = px(6.);

/// One row of a file tree: indented by depth, with indent guides for its
/// ancestors, a chevron for a directory, an optional icon, its name, and
/// an optional dimmed detail right after the name, and an optional badge at
/// its right end, a letter or a dot centred in the [`BADGE_COLUMN`]. The
/// name is muted when ignored and in the error colour when it couldn't be
/// read, else in its label colour if it has one. A masked icon is in the
/// error colour with it, else in its icon colour if it has one, selected or
/// not; an ignored row's chevron and icon keep their full strength and
/// colour, so it reads as dim, not disabled.
///
/// To a screen reader it is a `TreeItem` (macOS: an `AXRow` of an
/// `AXOutline`), named by its label, at its depth's level, expanded or
/// collapsed if it's a directory, selected if it is, with its place among
/// its siblings ([`TreeRow::in_set`]) and a description
/// ([`TreeRow::a11y_description`]). Its list should be a `Tree`. A selected
/// row is its focused list's active descendant.
#[derive(IntoElement)]
pub struct TreeRow {
    id: ElementId,
    label: SharedString,
    depth: usize,
    /// `Some(expanded)` for a directory.
    expanded: Option<bool>,
    icon: Option<IconRef>,
    ignored: bool,
    error: bool,
    label_color: Option<Hsla>,
    icon_color: Option<Hsla>,
    /// A dimmed text right after the label, and its colour.
    detail: Option<(SharedString, Hsla)>,
    /// A short text after the label, and its colour.
    badge: Option<(SharedString, Hsla)>,
    /// Whether the badge is drawn as a dot rather than its text.
    dot: bool,
    selected: bool,
    focused: bool,
    /// Its place among its siblings, from 1, and how many there are.
    in_set: Option<(usize, usize)>,
    /// What a screen reader says after the label, such as a git status.
    a11y_description: Option<SharedString>,
    on_click: Option<ClickHandler>,
}

/// A row's colours for its state; `None` draws nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RowColors {
    pub background: Option<Hsla>,
    /// The [`SELECTION_EDGE`]-wide bar down the selection's left side.
    pub edge: Option<Hsla>,
    pub label: Hsla,
    /// A masked icon's tint; a coloured icon ignores it.
    pub icon: Hsla,
    /// The icon's opacity: 1, ignored or not.
    pub icon_opacity: f32,
    pub chevron: Hsla,
    pub badge: Option<Hsla>,
}

/// The width of the accent bar down the left of a selected row while its
/// list has focus.
pub(crate) const SELECTION_EDGE: Pixels = px(2.);

/// A selected row's (background, edge): without focus, the
/// `state.selected_inactive` tint alone; with focus, the `state.selected`
/// tint and an `accent.base` edge. Each through its `list.*` token, which
/// follows the state's unless a theme sets it.
pub(crate) fn selection_colors(focused: bool, c: &Tokens<Hsla>) -> (Hsla, Option<Hsla>) {
    if focused {
        (c.list.row_selected, Some(c.accent.base))
    } else {
        (c.list.row_selected_inactive, None)
    }
}

/// The edge of a selected row in a focused list: a [`SELECTION_EDGE`]-wide
/// bar down its left side.
pub(crate) fn selection_edge(color: Hsla) -> gpui::Div {
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .left_0()
        .w(SELECTION_EDGE)
        .rounded(SELECTION_EDGE / 2.)
        .bg(color)
}

impl TreeRow {
    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, _theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            depth: 0,
            expanded: None,
            icon: None,
            ignored: false,
            error: false,
            label_color: None,
            icon_color: None,
            detail: None,
            badge: None,
            dot: false,
            selected: false,
            focused: false,
            in_set: None,
            a11y_description: None,
            on_click: None,
        }
    }

    /// How deep in the tree it is: 0 for the root's children.
    pub fn depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    /// Marks it a directory, showing whether it's expanded.
    pub fn directory(mut self, expanded: bool) -> Self {
        self.expanded = Some(expanded);
        self
    }

    /// The icon before the label: a bundled [`Icon`] or an icon set's
    /// answer.
    pub fn icon(mut self, icon: impl Into<IconRef>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn ignored(mut self, ignored: bool) -> Self {
        self.ignored = ignored;
        self
    }

    pub fn error(mut self, error: bool) -> Self {
        self.error = error;
        self
    }

    /// The label's colour, not the icon's or the chevron's, unless the row
    /// is an error's (the error colour wins) or ignored (muted wins).
    pub fn label_color(mut self, color: Hsla) -> Self {
        self.label_color = Some(color);
        self
    }

    /// A masked icon's tint, in place of the list's icon colour, selected
    /// or not, unless the row is an error's (the error colour wins) or
    /// ignored (its icon keeps the list's colour). A coloured or tinted
    /// icon ignores it.
    pub fn icon_color(mut self, color: Hsla) -> Self {
        self.icon_color = Some(color);
        self
    }

    /// What [`Self::icon_color`] set, for a host's tests to see what it
    /// built.
    pub fn icon_tint(&self) -> Option<Hsla> {
        self.icon_color
    }

    /// A short text drawn in `color` at the row's right end; a long label
    /// ellipsizes before it.
    pub fn badge(mut self, text: impl Into<SharedString>, color: Hsla) -> Self {
        self.badge = Some((text.into(), color));
        self.dot = false;
        self
    }

    /// A [`DOT_SIZE`] dot in `color` at the row's right end, centred where
    /// a [`TreeRow::badge`]'s letter is: a folder's mark for the changes
    /// beneath it. [`TreeRow::status`] reports it as the badge `•`.
    pub fn dot(mut self, color: Hsla) -> Self {
        self.badge = Some(("•".into(), color));
        self.dot = true;
        self
    }

    /// A text drawn in `color` right after the label, as a symbol's
    /// signature after its name. It ellipsizes before the badge and gives
    /// way first: the label keeps its whole text while the row has room
    /// for it. An empty text draws nothing.
    pub fn detail(mut self, text: impl Into<SharedString>, color: Hsla) -> Self {
        let text = text.into();
        self.detail = (!text.is_empty()).then_some((text, color));
        self
    }

    /// What [`Self::detail`] set, for a host's tests to see what it built.
    pub fn detail_text(&self) -> Option<(&SharedString, Hsla)> {
        self.detail.as_ref().map(|(text, color)| (text, *color))
    }

    /// What [`Self::label_color`] and [`Self::badge`] set, for a host's
    /// tests to see what it built.
    pub fn status(&self) -> (Option<Hsla>, Option<(&SharedString, Hsla)>) {
        let badge = self.badge.as_ref().map(|(text, color)| (text, *color));
        (self.label_color, badge)
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Whether the list has focus, which marks the selected row with an
    /// accent edge.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Its place among its siblings in the tree's model, from 1, and how
    /// many siblings there are, for a screen reader's "2 of 5": the
    /// model's, since a virtual list draws only the rows in view.
    pub fn in_set(mut self, position: usize, count: usize) -> Self {
        self.in_set = Some((position, count));
        self
    }

    /// What a screen reader says after the row's name, as a word: its git
    /// status, say ("modified"), which its badge shows as a letter.
    pub fn a11y_description(mut self, description: impl Into<SharedString>) -> Self {
        let description = description.into();
        self.a11y_description = (!description.is_empty()).then_some(description);
        self
    }

    /// The states a screen reader is given for the row: its level (its
    /// depth, from 1), whether it's expanded (a directory) and selected,
    /// its place among its siblings and its description.
    pub fn a11y_states(&self) -> States {
        States {
            selected: Some(self.selected),
            expanded: self.expanded,
            description: self.a11y_description.clone(),
            level: Some(self.depth + 1),
            position_in_set: self.in_set.map(|(position, _)| position),
            size_of_set: self.in_set.map(|(_, count)| count),
            ..States::default()
        }
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    /// Its colours in `colors`, the active theme's tokens.
    pub(crate) fn colors(&self, colors: &Tokens<Hsla>) -> RowColors {
        let (list, text) = (&colors.list, &colors.text);
        let (background, edge) = match self.selected {
            true => {
                let (background, edge) = selection_colors(self.focused, colors);
                (Some(background), edge)
            }
            false => (None, None),
        };
        let error = colors.status.error;
        let (label, icon, chevron) = if self.error {
            (error, error, list.chevron)
        } else if self.ignored {
            // Dim, not disabled: the label alone is muted.
            let icon = match self.expanded {
                Some(_) => list.folder_icon,
                None => list.icon,
            };
            (text.muted, icon, list.chevron)
        } else if self.expanded.is_some() {
            let label = self.label_color.unwrap_or(text.primary);
            let icon = self.icon_color.unwrap_or(list.folder_icon);
            (label, icon, list.chevron)
        } else {
            let label = self.label_color.unwrap_or(text.primary);
            (label, self.icon_color.unwrap_or(list.icon), list.chevron)
        };
        let icon_opacity = 1.;
        RowColors {
            background,
            edge,
            label,
            icon,
            icon_opacity,
            chevron,
            badge: self.badge.as_ref().map(|(_, color)| *color),
        }
    }
}

impl RenderOnce for TreeRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let a11y_states = self.a11y_states();
        let tokens = &cx.theme().colors;
        let colors = self.colors(tokens);
        let (hover, indent_guide) = (tokens.list.row_hover, tokens.list.indent_guide);
        let label = &self.label;
        // Hover is styled here, so the box needs an ID: GPUI keeps hover state
        // and asks for a redraw on entry and exit only for identified
        // elements. Scoped under the row's ID, it's stable across frames.
        let selection = div()
            .id("selection")
            .absolute()
            .top_0()
            .bottom_0()
            .left(BOX_INSET)
            .right(BOX_INSET)
            .rounded(px(4.))
            .map(|selection| match colors.background {
                Some(background) => selection.bg(background),
                None => selection.hover(move |style| style.bg(hover)),
            })
            .when_some(colors.edge, |selection, edge| {
                selection
                    .child(selection_edge(edge).debug_selector(|| format!("tree-row-edge:{label}")))
            });
        // Each ancestor's line runs through the centre of its chevron.
        let guides = (0..self.depth).map(|level| {
            div()
                .absolute()
                .top_0()
                .h(ROW_HEIGHT)
                .left(PADDING + TREE_INDENT * level as f32 + CHEVRON_WIDTH / 2. - px(0.5))
                .w(px(1.))
                .bg(indent_guide)
                .debug_selector(|| format!("tree-guide:{label}:{level}"))
        });
        let chevron = match self.expanded {
            Some(expanded) => svg()
                .path(Icon::ChevronRight.path())
                .size(CHEVRON_WIDTH)
                .when(expanded, |chevron| {
                    chevron.with_transformation(Transformation::rotate(radians(FRAC_PI_2)))
                })
                .text_color(colors.chevron)
                .debug_selector(|| {
                    let state = if expanded { "open" } else { "closed" };
                    format!("tree-chevron:{label}:{state}")
                })
                .into_any_element(),
            None => div().into_any_element(),
        };
        let icon = self.icon.map(|icon| {
            div()
                .size(ICON_SIZE)
                .flex_none()
                .mr(ICON_GAP)
                .opacity(colors.icon_opacity)
                .debug_selector(|| format!("tree-icon:{label}:{}", icon.name()))
                .child(icon.element(ICON_SIZE, colors.icon))
        });
        // With a detail, the label takes its text's width and shrinks
        // only when that's wider than the row; the detail, from no width,
        // grows into what's left and ellipsizes.
        let has_detail = self.detail.is_some();
        let detail = self.detail.map(|(text, color)| {
            div()
                .flex_1()
                .min_w_0()
                .ml(DETAIL_GAP)
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(color)
                .debug_selector(|| format!("tree-detail:{label}"))
                .child(text)
        });
        let dot = self.dot;
        let badge = self.badge.zip(colors.badge).map(|((text, _), color)| {
            let column = div()
                .flex_none()
                .ml(BADGE_GAP)
                .min_w(BADGE_COLUMN)
                .flex()
                .justify_center()
                .items_center()
                .whitespace_nowrap();
            match dot {
                true => column.child(
                    div()
                        .size(DOT_SIZE)
                        .flex_none()
                        .rounded_full()
                        .bg(color)
                        .debug_selector(|| format!("tree-dot:{label}")),
                ),
                false => column
                    .text_color(color)
                    .debug_selector(|| format!("tree-badge:{label}"))
                    .child(text),
            }
        });
        let selected = self.selected;
        let row = div()
            .id(self.id)
            .relative()
            .h(ROW_HEIGHT)
            .w_full()
            .flex()
            .items_center()
            .pl(PADDING + TREE_INDENT * self.depth as f32)
            .pr(BOX_INSET + px(4.))
            .child(selection)
            .children(guides)
            .child(
                div()
                    .size(CHEVRON_WIDTH)
                    .flex_none()
                    .mr(CHEVRON_GAP)
                    .child(chevron),
            )
            .children(icon)
            .child(
                div()
                    .map(|label| if has_detail { label } else { label.flex_1() })
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(colors.label)
                    .debug_selector(|| format!("tree-label:{label}"))
                    .child(self.label.clone()),
            )
            .children(detail)
            .children(badge)
            .when_some(self.on_click, |row, on_click| row.on_click(on_click))
            // Honoured only while an ancestor, the list, has focus.
            .when(selected, |row| row.aria_active_descendant());
        node_with(row, window, cx, Role::TreeItem, self.label, a11y_states)
    }
}
