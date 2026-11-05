// SPDX-License-Identifier: Apache-2.0

use gpui::{
    App, ClickEvent, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    RenderOnce, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px, svg,
};

use crate::{ActiveTheme as _, ClickHandler, Icon, IconRef, UiTheme};

/// The height of a pane's toolbar, which holds the breadcrumbs.
pub const TOOLBAR_HEIGHT: Pixels = px(24.);
/// A crumb's icon, square.
const CRUMB_ICON_SIZE: Pixels = px(14.);

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
    pub icon: Option<IconRef>,
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

    /// A bundled [`Icon`] or an icon set's answer.
    pub fn icon(mut self, icon: impl Into<IconRef>) -> Self {
        self.icon = Some(icon.into());
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
}

impl Breadcrumbs {
    /// Its colours come from `cx.theme()` as it renders. `_theme` is
    /// ignored: deprecated, kept so callers that pass a [`UiTheme`] build.
    pub fn new(crumbs: Vec<Crumb>, _theme: &UiTheme) -> Self {
        Self { crumbs }
    }
}

impl RenderOnce for Breadcrumbs {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = &cx.theme().colors;
        let (hover, icon_color) = (colors.list.row_hover, colors.list.icon);
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
                        .text_color(colors.text.muted),
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
                .hover(move |style| style.bg(hover))
                .map(|element| {
                    if shrinks {
                        element.flex_shrink(1.).min_w_0()
                    } else {
                        element.flex_none()
                    }
                })
                .when_some(crumb.icon, |element, icon| {
                    let selector = format!("crumb-icon:{}:{}", crumb.label, icon.name());
                    element.child(
                        div()
                            .size(CRUMB_ICON_SIZE)
                            .flex_none()
                            .debug_selector(|| selector)
                            .child(icon.element(CRUMB_ICON_SIZE, icon_color)),
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
