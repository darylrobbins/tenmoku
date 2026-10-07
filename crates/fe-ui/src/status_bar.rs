use gpui::{
    AnyElement, App, ClickEvent, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _,
    Window, div, prelude::FluentBuilder as _,
};

use crate::{ClickHandler, ROW_HEIGHT, UiTheme};

/// The status bar: a full-width row along the window's bottom edge, with a
/// 1 px line on top, its left children from the left edge and its right
/// children against the right edge.
#[derive(IntoElement)]
pub struct StatusBarFrame {
    left: Vec<AnyElement>,
    right: Vec<AnyElement>,
    theme: UiTheme,
}

impl StatusBarFrame {
    pub fn new(theme: &UiTheme) -> Self {
        Self {
            left: Vec::new(),
            right: Vec::new(),
            theme: *theme,
        }
    }

    pub fn left(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.left
            .extend(children.into_iter().map(IntoElement::into_any_element));
        self
    }

    pub fn right(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.right
            .extend(children.into_iter().map(IntoElement::into_any_element));
        self
    }
}

impl RenderOnce for StatusBarFrame {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let side = || div().flex().items_center().h_full();
        div()
            .debug_selector(|| "status-bar".into())
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .w_full()
            .h(ROW_HEIGHT)
            .overflow_hidden()
            .bg(self.theme.panel_background)
            .border_t_1()
            .border_color(self.theme.border)
            .text_color(self.theme.text)
            .child(side().min_w_0().overflow_hidden().children(self.left))
            .child(side().flex_none().children(self.right))
    }
}

/// One status bar item: its label, padded. With a click handler, it's
/// highlighted on hover under a pointer cursor.
#[derive(IntoElement)]
pub struct StatusEntry {
    id: SharedString,
    label: SharedString,
    theme: UiTheme,
    on_click: Option<ClickHandler>,
}

impl StatusEntry {
    /// `id` names the element; in tests its bounds are those of the debug
    /// selector `status-item:{id}`.
    pub fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        theme: &UiTheme,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            theme: *theme,
            on_click: None,
        }
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

impl RenderOnce for StatusEntry {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let id = self.id;
        div()
            .id(ElementId::Name(id.clone()))
            .debug_selector(|| format!("status-item:{id}"))
            .flex()
            .flex_none()
            .items_center()
            .h_full()
            .px_2()
            .whitespace_nowrap()
            .child(self.label)
            .when_some(self.on_click, |entry, on_click| {
                entry
                    .cursor_pointer()
                    .hover(move |style| style.bg(theme.row_hover))
                    .on_click(on_click)
            })
    }
}
