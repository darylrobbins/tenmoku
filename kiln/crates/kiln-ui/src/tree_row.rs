// SPDX-License-Identifier: Apache-2.0

use std::f32::consts::FRAC_PI_2;

use gpui::{
    App, AppContext as _, ClickEvent, Context, Div, DragMoveEvent, ElementId, Hsla,
    InteractiveElement as _, IntoElement, MouseButton, ParentElement as _, Pixels, Point, Render,
    RenderOnce, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _,
    Transformation, Window, div, prelude::FluentBuilder as _, px, radians, svg,
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
/// row is its focused list's active descendant, unless the list selects
/// several and says which one its keys act on ([`TreeRow::active`]).
///
/// A row can be dragged ([`TreeRow::on_drag`]), drawn under the pointer as
/// a small ghost of its icon and name, take what's dropped on it
/// ([`TreeRow::on_drop`]), and be drawn as where a drop would go
/// ([`TreeRow::drop_target`]). Dragging is mouse-only: a list that offers
/// it should offer the same through its keys (cut and paste, say).
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
    /// Whether it's its list's active descendant, if not as `selected`
    /// says.
    active: Option<bool>,
    focused: bool,
    /// Its place among its siblings, from 1, and how many there are.
    in_set: Option<(usize, usize)>,
    /// What a screen reader says after the label, such as a git status.
    a11y_description: Option<SharedString>,
    on_click: Option<ClickHandler>,
    on_secondary_click: Option<SecondaryClickHandler>,
    /// Whether it's drawn as where a drop would go.
    drop_target: bool,
    /// What its drag carries is drawn as, if not its icon and label.
    ghost: Option<SharedString>,
    drag: DragHooks,
}

/// Drag and drop handlers, kept until the row's element is built.
type DragHooks = Vec<Box<dyn FnOnce(Stateful<Div>, Ghost) -> Stateful<Div>>>;

/// What a right click on a row runs, given where it was in the window.
type SecondaryClickHandler = Box<dyn Fn(Point<Pixels>, &mut Window, &mut App)>;

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

/// The alpha of [`TreeRow::drop_color`]'s tint: as a pane's drop overlay.
const DROP_ALPHA: f32 = 0.2;

/// What a dragged row is drawn as under the pointer: its icon and label in
/// a small box of the list's colours.
#[derive(Clone)]
struct Ghost {
    icon: Option<IconRef>,
    label: SharedString,
}

impl Render for Ghost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let (text, icon_tint) = (colors.text.primary, colors.list.icon);
        let label = self.label.clone();
        div()
            .debug_selector(move || [kiln_l10n::verbatim!("tree-ghost:"), &label].concat())
            .h(ROW_HEIGHT)
            .px(PADDING)
            .flex()
            .items_center()
            .rounded(px(4.))
            .bg(colors.list.row_selected)
            .text_color(text)
            .opacity(0.9)
            .when_some(self.icon.as_ref(), |ghost, icon| {
                ghost.child(
                    div()
                        .size(ICON_SIZE)
                        .flex_none()
                        .mr(ICON_GAP)
                        .child(icon.element(ICON_SIZE, icon_tint)),
                )
            })
            .child(div().whitespace_nowrap().child(self.label.clone()))
    }
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
            active: None,
            focused: false,
            in_set: None,
            a11y_description: None,
            on_click: None,
            on_secondary_click: None,
            drop_target: false,
            ghost: None,
            drag: Vec::new(),
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

    /// Whether it's the row its list's keys act on, its active descendant
    /// to a screen reader, in a list that selects several rows: one of
    /// them is. A row is active as it's [`Self::selected`] unless this
    /// says otherwise.
    pub fn active(mut self, active: bool) -> Self {
        self.active = Some(active);
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

    /// What a right click on the row runs, given where it was pressed in
    /// the window, as a context menu opens: on the press, which goes no
    /// further, so neither the list's own right click nor its focus on a
    /// press takes it. A left click is [`Self::on_click`]'s still.
    pub fn on_secondary_click(
        mut self,
        handler: impl Fn(Point<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_secondary_click = Some(Box::new(handler));
        self
    }

    /// Makes the row draggable: moving the pointer a few pixels with the
    /// button down starts a drag of `value`, drawn under the pointer as a
    /// small ghost of the row's icon and label, or of `ghost` in their
    /// place ("3 items", say, for the rows dragged with it), and runs
    /// `on_start` as it starts, the pointer where the window has it. A drag
    /// doesn't click the row.
    pub fn on_drag<T: 'static>(
        mut self,
        value: T,
        ghost: Option<SharedString>,
        on_start: impl Fn(&T, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.ghost = ghost;
        self.drag.push(Box::new(move |row, ghost| {
            row.on_drag(value, move |value, _, window, cx| {
                on_start(value, window, cx);
                cx.new(|_| ghost.clone())
            })
        }));
        self
    }

    /// What a `T` dropped on the row runs. The innermost element that takes
    /// a `T` takes the drop, so the row's list can take it elsewhere.
    pub fn on_drop<T: 'static>(
        mut self,
        handler: impl Fn(&T, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.drag.push(Box::new(|row, _| row.on_drop(handler)));
        self
    }

    /// What the pointer moving while a `T` is dragged runs, wherever it is:
    /// the event's bounds are the row's, to tell whether it's over the row.
    pub fn on_drag_move<T: 'static>(
        mut self,
        handler: impl Fn(&DragMoveEvent<T>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.drag.push(Box::new(|row, _| row.on_drag_move(handler)));
        self
    }

    /// Draws the row as where what's dragged would drop: tinted in
    /// [`TreeRow::drop_color`], selected or not. A list marks the folder
    /// row a drop goes in and the rows under it.
    pub fn drop_target(mut self, target: bool) -> Self {
        self.drop_target = target;
        self
    }

    /// Whether it's drawn as where a drop would go, for a host's tests.
    pub fn is_drop_target(&self) -> bool {
        self.drop_target
    }

    /// The tint of a row a drop would go in ([`TreeRow::drop_target`]):
    /// the theme's `state.drop_target`, at a low alpha, for a list to tint
    /// other drop areas (its root's row, say) alike.
    pub fn drop_color(cx: &App) -> Hsla {
        cx.theme().colors.state.drop_target.opacity(DROP_ALPHA)
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
        let ghost = Ghost {
            icon: self.ghost.is_none().then(|| self.icon.clone()).flatten(),
            label: self.ghost.clone().unwrap_or_else(|| self.label.clone()),
        };
        let drop_tint = self.drop_target.then(|| Self::drop_color(cx));
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
        // Over the selection's tint, so a selected row reads as the target.
        let drop_tint = drop_tint.map(|tint| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(BOX_INSET)
                .right(BOX_INSET)
                .rounded(px(4.))
                .bg(tint)
                .debug_selector(|| [kiln_l10n::verbatim!("tree-drop:"), label].concat())
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
        let active = self.active.unwrap_or(self.selected);
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
            .children(drop_tint)
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
            .when_some(self.on_secondary_click, |row, on_secondary_click| {
                row.on_mouse_down(MouseButton::Right, move |event, window, cx| {
                    cx.stop_propagation();
                    on_secondary_click(event.position, window, cx);
                })
            })
            // Honoured only while an ancestor, the list, has focus.
            .when(active, |row| row.aria_active_descendant());
        let row = (self.drag.into_iter()).fold(row, |row, hook| hook(row, ghost.clone()));
        node_with(row, window, cx, Role::TreeItem, self.label, a11y_states)
    }
}
