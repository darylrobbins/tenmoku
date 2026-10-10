// SPDX-License-Identifier: LicenseRef-Proprietary

use gpui::{
    App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px, svg,
};

use crate::{ClickHandler, Icon, UiTheme};

/// The height of a pane's toolbar, which holds the breadcrumbs.
pub const TOOLBAR_HEIGHT: Pixels = px(24.);

/// What a crumb stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrumbKind {
    /// A directory.
    Path,
    File,
    Symbol,
}

/// One segment of [`Breadcrumbs`].
pub struct Crumb {
    pub label: SharedString,
    pub icon: Option<Icon>,
    pub kind: CrumbKind,
    on_click: Option<ClickHandler>,
}

impl Crumb {
    pub fn new(label: impl Into<SharedString>, kind: CrumbKind) -> Self {
        Self {
            label: label.into(),
            icon: None,
            kind,
            on_click: None,
        }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
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

/// Crumbs in a row, a chevron between each two, filling the space left in
/// their row. Too narrow, directory crumbs shrink first, together, their
/// labels ending in an ellipsis, down to nothing if that's what it takes for
/// the file crumb to fit; the file and symbol crumbs keep their width and
/// are clipped at the right edge. Each crumb has the debug selector
/// `crumb:<index>:<label>`.
#[derive(IntoElement)]
pub struct Breadcrumbs {
    crumbs: Vec<Crumb>,
    theme: UiTheme,
}

impl Breadcrumbs {
    pub fn new(crumbs: Vec<Crumb>, theme: &UiTheme) -> Self {
        Self {
            crumbs,
            theme: *theme,
        }
    }
}

impl RenderOnce for Breadcrumbs {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let mut row = div()
            .flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .items_center()
            .overflow_hidden();
        for (ix, crumb) in self.crumbs.into_iter().enumerate() {
            if ix > 0 {
                row = row.child(
                    svg()
                        .path(Icon::ChevronRight.path())
                        .size(px(12.))
                        .flex_none()
                        .text_color(theme.muted_text),
                );
            }
            let shrinks = crumb.kind == CrumbKind::Path;
            let label = crumb.label.clone();
            let element = div()
                .id(ElementId::NamedInteger("crumb".into(), ix as u64))
                .debug_selector(|| format!("crumb:{ix}:{label}"))
                .flex()
                .items_center()
                .gap_1()
                .px_1()
                .h_full()
                .rounded_sm()
                .hover(move |style| style.bg(theme.row_hover))
                .map(|element| {
                    if shrinks {
                        element.flex_shrink(1.).min_w_0()
                    } else {
                        element.flex_none()
                    }
                })
                .when_some(crumb.icon, |element, icon| {
                    element.child(
                        svg()
                            .path(icon.path())
                            .size(px(14.))
                            .flex_none()
                            .text_color(theme.icon),
                    )
                })
                .child(
                    div()
                        .min_w_0()
                        .whitespace_nowrap()
                        .when(shrinks, |label| label.overflow_hidden().text_ellipsis())
                        .child(crumb.label),
                )
                .when_some(crumb.on_click, |element, on_click| {
                    element.on_click(on_click)
                });
            row = row.child(element);
        }
        row
    }
}
