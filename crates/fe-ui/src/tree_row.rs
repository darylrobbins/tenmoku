use std::f32::consts::FRAC_PI_2;

use gpui::{
    App, ClickEvent, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Transformation,
    Window, div, prelude::FluentBuilder as _, px, radians, svg,
};

use crate::{ClickHandler, Icon, ROW_HEIGHT, UiTheme};

/// How far each level of the tree is indented.
pub const TREE_INDENT: Pixels = px(14.);
/// The chevron column, reserved on every row so files line up with their
/// sibling folders.
pub const CHEVRON_WIDTH: Pixels = px(16.);
/// A row's icon, square.
pub const ICON_SIZE: Pixels = px(16.);

/// Left of the chevron column at depth 0.
const PADDING: Pixels = px(6.);
/// The selection box's inset from the list's sides.
const BOX_INSET: Pixels = px(4.);
const CHEVRON_GAP: Pixels = px(2.);
const ICON_GAP: Pixels = px(6.);

/// One row of a file tree: indented by depth, with indent guides for its
/// ancestors, a chevron for a directory, an optional icon, and its name,
/// muted when ignored and in the error colour when it couldn't be read.
#[derive(IntoElement)]
pub struct TreeRow {
    id: ElementId,
    label: SharedString,
    depth: usize,
    /// `Some(expanded)` for a directory.
    expanded: Option<bool>,
    icon: Option<Icon>,
    ignored: bool,
    error: bool,
    selected: bool,
    focused: bool,
    theme: UiTheme,
    on_click: Option<ClickHandler>,
}

/// A row's colours for its state; `None` draws nothing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RowColors {
    pub background: Option<Hsla>,
    pub border: Option<Hsla>,
    pub label: Hsla,
    pub icon: Hsla,
    pub chevron: Hsla,
}

impl TreeRow {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            depth: 0,
            expanded: None,
            icon: None,
            ignored: false,
            error: false,
            selected: false,
            focused: false,
            theme: *theme,
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

    /// The icon before the label.
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
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

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Whether the list has focus, which outlines the selected row.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }

    pub(crate) fn colors(&self) -> RowColors {
        let theme = &self.theme;
        let (background, border) = match (self.selected, self.focused) {
            (true, true) => (Some(theme.row_selected), Some(theme.row_focus_border)),
            (true, false) => (Some(theme.row_selected_inactive), None),
            (false, _) => (None, None),
        };
        let (label, icon, chevron) = if self.error {
            (theme.error_text, theme.error_text, theme.chevron)
        } else if self.ignored {
            (theme.muted_text, theme.muted_text, theme.muted_text)
        } else if self.expanded.is_some() {
            (theme.text, theme.folder_icon, theme.chevron)
        } else {
            (theme.text, theme.icon, theme.chevron)
        };
        RowColors {
            background,
            border,
            label,
            icon,
            chevron,
        }
    }
}

impl RenderOnce for TreeRow {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let colors = self.colors();
        let hover = self.theme.row_hover;
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
            .when_some(colors.border, |selection, border| {
                selection.border_1().border_color(border)
            });
        // Each ancestor's line runs through the centre of its chevron.
        let guides = (0..self.depth).map(|level| {
            div()
                .absolute()
                .top_0()
                .h(ROW_HEIGHT)
                .left(PADDING + TREE_INDENT * level as f32 + CHEVRON_WIDTH / 2. - px(0.5))
                .w(px(1.))
                .bg(self.theme.indent_guide)
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
            svg()
                .path(icon.path())
                .size(ICON_SIZE)
                .flex_none()
                .mr(ICON_GAP)
                .text_color(colors.icon)
                .debug_selector(|| format!("tree-icon:{label}:{}", icon.name()))
        });
        div()
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
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_color(colors.label)
                    .child(self.label.clone()),
            )
            .when_some(self.on_click, |row, on_click| row.on_click(on_click))
    }
}
