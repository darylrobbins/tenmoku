use gpui::{
    App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};

use crate::{ClickHandler, UiTheme};

/// Every tree row's height, so a uniform list can lay them out.
pub const ROW_HEIGHT: gpui::Pixels = px(22.);
const INDENT: gpui::Pixels = px(12.);

/// One row of a file tree: indented by depth, a chevron for a directory,
/// and its name, muted when ignored and in the error colour when it
/// couldn't be read.
#[derive(IntoElement)]
pub struct TreeRow {
    id: ElementId,
    label: SharedString,
    depth: usize,
    /// `Some(expanded)` for a directory.
    expanded: Option<bool>,
    ignored: bool,
    error: bool,
    selected: bool,
    theme: UiTheme,
    on_click: Option<ClickHandler>,
}

impl TreeRow {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>, theme: &UiTheme) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            depth: 0,
            expanded: None,
            ignored: false,
            error: false,
            selected: false,
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

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for TreeRow {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let color = if self.error {
            theme.error_text
        } else if self.ignored {
            theme.muted_text
        } else {
            theme.text
        };
        let chevron = match self.expanded {
            Some(true) => "▾",
            Some(false) => "▸",
            None => "",
        };
        div()
            .id(self.id)
            .h(ROW_HEIGHT)
            .w_full()
            .flex()
            .items_center()
            .pl(px(8.) + INDENT * self.depth as f32)
            .pr_2()
            .text_color(color)
            .when(self.selected, |row| row.bg(theme.row_selected))
            .hover(move |style| style.bg(theme.row_hover))
            .child(div().w(px(14.)).flex_none().child(chevron))
            .child(
                div()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(self.label),
            )
            .when_some(self.on_click, |row, on_click| row.on_click(on_click))
    }
}
